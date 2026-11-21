#![no_std]
#![no_main]

mod jiggler;

use common::{parse_command, serialize_response, Command, JiggleConfig, Response};
use cortex_m_rt::entry;
use heapless::String;
use panic_halt as _;
use rp2040_boot2 as _;
use rp2040_hal::{
    clocks::init_clocks_and_plls, pac, sio::Sio, timer::Timer, usb::UsbBus, watchdog::Watchdog,
};
use usb_device::{
    class_prelude::UsbBusAllocator,
    prelude::{UsbDeviceBuilder, UsbDeviceState, UsbVidPid},
};
use usbd_hid::{
    descriptor::{generator_prelude::*, MouseReport},
    hid_class::HIDClass,
};
use usbd_serial::SerialPort;

#[link_section = ".boot2"]
#[no_mangle]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

#[entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let _core = pac::CorePeripherals::take().unwrap();

    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let _sio = Sio::new(pac.SIO);

    let external_xtal_freq_hz = 12_000_000u32;
    let clocks = init_clocks_and_plls(
        external_xtal_freq_hz,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    let timer = Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let usb_bus = UsbBusAllocator::new(UsbBus::new(
        pac.USBCTRL_REGS,
        pac.USBCTRL_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));

    let mut usb_serial = SerialPort::new(&usb_bus);
    let mut usb_hid = HIDClass::new(&usb_bus, MouseReport::desc(), 10);

    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .strings(&[usb_device::device::StringDescriptors::default()
            .manufacturer("Waveshare")
            .product("RP2040 Mouse Jiggler")
            .serial_number("WAVE-RP2040-001")])
        .unwrap()
        .device_class(0xEF)
        .device_sub_class(0x02)
        .device_protocol(0x01)
        .composite_with_iads()
        .build();

    let mut engine = jiggler::JigglerEngine::new(JiggleConfig::default());
    let mut line_buf: String<128> = String::new();

    // Configuration session management:
    // If the GUI config app is connected, pause continuous jiggling.
    // If no host serial heartbeat/command is heard for 5 seconds, resume auto-jiggling.
    let mut host_connected = false;
    let mut last_host_contact_ms: u32 = 0;
    let mut test_until_ms: u32 = 0;

    loop {
        let _ = usb_dev.poll(&mut [&mut usb_serial, &mut usb_hid]);

        let now_us = timer.get_counter().ticks();
        let now_ms = (now_us / 1000) as u32;

        let mut read_buf = [0u8; 64];
        if let Ok(count) = usb_serial.read(&mut read_buf) {
            for &b in &read_buf[..count] {
                if b == b'\n' || b == b'\r' {
                    if !line_buf.is_empty() {
                        host_connected = true;
                        last_host_contact_ms = now_ms;

                        let resp = match parse_command(&line_buf) {
                            Ok(Command::GetStatus) => Response::Status {
                                config: *engine.config(),
                                total_jiggles: engine.total_jiggles(),
                                uptime_secs: (now_us / 1_000_000) as u32,
                                paused: host_connected && now_ms >= test_until_ms,
                                testing: now_ms < test_until_ms,
                            },
                            Ok(Command::SetConfig(new_cfg)) => {
                                engine.set_config(new_cfg);
                                Response::Ok
                            }
                            Ok(Command::SaveConfig) => Response::Ok,
                            Ok(Command::ResetDefaults) => {
                                engine.set_config(JiggleConfig::default());
                                Response::Ok
                            }
                            Ok(Command::StartTest { duration_secs }) => {
                                test_until_ms =
                                    now_ms.saturating_add(duration_secs.saturating_mul(1000));
                                Response::Ok
                            }
                            Ok(Command::Pause) => {
                                host_connected = true;
                                Response::Ok
                            }
                            Ok(Command::Resume) => {
                                host_connected = false;
                                test_until_ms = 0;
                                Response::Ok
                            }
                            Err(err) => Response::Error(err),
                        };
                        let resp_str = serialize_response(&resp);
                        let _ = usb_serial.write(resp_str.as_bytes());
                        line_buf.clear();
                    }
                } else if line_buf.len() < line_buf.capacity() {
                    let _ = line_buf.push(b as char);
                }
            }
        }

        // Host timeout: if no serial communication for 5 seconds, resume auto-jiggling
        if host_connected && now_ms.wrapping_sub(last_host_contact_ms) > 5000 {
            host_connected = false;
        }

        let is_testing = now_ms < test_until_ms;
        let should_jiggle = !host_connected || is_testing;

        if usb_dev.state() == UsbDeviceState::Configured && should_jiggle {
            if let Some((dx, dy)) = engine.update(now_ms) {
                let report = MouseReport {
                    buttons: 0,
                    x: dx,
                    y: dy,
                    wheel: 0,
                    pan: 0,
                };
                let _ = usb_hid.push_input(&report);
            }
        }
    }
}
