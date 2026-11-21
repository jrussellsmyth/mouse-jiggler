#![no_std]
#![no_main]

mod jiggler;

use common::{parse_command, serialize_response, Command, JiggleConfig, JiggleMode, Response};
use cortex_m_rt::entry;
use heapless::String;
use panic_halt as _;
use rp2040_boot2 as _;
use rp2040_hal::{
    clocks::init_clocks_and_plls, pac, rom_data, sio::Sio, timer::Timer, usb::UsbBus,
    watchdog::Watchdog,
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

// Flash storage parameters for persistent settings
// RP2040 flash base in memory space is 0x10000000.
// On the RP2040-Zero (2MB flash), offset 0x001F0000 (XIP 0x101F0000) is the last 64KB sector,
// well beyond our firmware size (~50KB) and safely aligned to a 4096-byte sector boundary.
const FLASH_CONFIG_OFFSET: u32 = 0x001F_0000;
const FLASH_CONFIG_ADDR: *const u8 = (0x1000_0000 + FLASH_CONFIG_OFFSET) as *const u8;
const CONFIG_MAGIC: u32 = 0x4A494747; // 'JIGG'

fn load_saved_config() -> Option<JiggleConfig> {
    unsafe {
        let magic = core::ptr::read_unaligned(FLASH_CONFIG_ADDR as *const u32);
        if magic != CONFIG_MAGIC {
            return None;
        }

        let enabled_byte = core::ptr::read(FLASH_CONFIG_ADDR.add(4));
        let mode_byte = core::ptr::read(FLASH_CONFIG_ADDR.add(5));
        let period_ms = core::ptr::read_unaligned(FLASH_CONFIG_ADDR.add(6) as *const u32);
        let scale_px = core::ptr::read(FLASH_CONFIG_ADDR.add(10)) as i8;

        let mode = match mode_byte {
            0 => JiggleMode::Off,
            1 => JiggleMode::Random,
            2 => JiggleMode::GentleNudge,
            3 => JiggleMode::Circle,
            _ => return None,
        };

        let mut cfg = JiggleConfig {
            enabled: enabled_byte != 0,
            mode,
            period_ms,
            scale_px,
        };
        cfg.sanitize();
        Some(cfg)
    }
}

fn save_config_to_flash(cfg: &JiggleConfig) -> bool {
    let mut page_buf = [0xFFu8; 256];
    let magic_bytes = CONFIG_MAGIC.to_le_bytes();
    page_buf[0..4].copy_from_slice(&magic_bytes);
    page_buf[4] = if cfg.enabled { 1 } else { 0 };
    page_buf[5] = match cfg.mode {
        JiggleMode::Off => 0,
        JiggleMode::Random => 1,
        JiggleMode::GentleNudge => 2,
        JiggleMode::Circle => 3,
    };
    let period_bytes = cfg.period_ms.to_le_bytes();
    page_buf[6..10].copy_from_slice(&period_bytes);
    page_buf[10] = cfg.scale_px as u8;

    cortex_m::interrupt::free(|_| unsafe {
        rom_data::connect_internal_flash();
        rom_data::flash_exit_xip();
        // Erase 4096-byte sector at FLASH_CONFIG_OFFSET
        rom_data::flash_range_erase(FLASH_CONFIG_OFFSET, 4096, 65536, 0xD8);
        // Program 256-byte page
        rom_data::flash_range_program(FLASH_CONFIG_OFFSET, page_buf.as_ptr(), 256);
        rom_data::flash_flush_cache();
        rom_data::flash_enter_cmd_xip();
    });

    true
}

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

    let initial_config = load_saved_config().unwrap_or_default();
    let mut engine = jiggler::JigglerEngine::new(initial_config);
    let mut line_buf: String<128> = String::new();

    // Configuration session management:
    // Boot starts with a 15-second grace period where auto-jiggling is paused,
    // giving you time to launch or connect the configuration app without your mouse jumping around.
    // If the GUI config app is connected, jiggling remains paused.
    // If no host serial heartbeat/command is heard after the grace period, normal auto-jiggling starts.
    const BOOT_GRACE_PERIOD_MS: u32 = 15_000;
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

                        let in_boot_grace_period = now_ms < BOOT_GRACE_PERIOD_MS;
                        let is_paused = (host_connected || in_boot_grace_period) && now_ms >= test_until_ms;

                        let resp = match parse_command(&line_buf) {
                            Ok(Command::GetStatus) => Response::Status {
                                config: *engine.config(),
                                total_jiggles: engine.total_jiggles(),
                                uptime_secs: (now_us / 1_000_000) as u32,
                                paused: is_paused,
                                testing: now_ms < test_until_ms,
                            },
                            Ok(Command::SetConfig(new_cfg)) => {
                                engine.set_config(new_cfg);
                                Response::Ok
                            }
                            Ok(Command::SaveConfig) => {
                                if save_config_to_flash(engine.config()) {
                                    Response::Ok
                                } else {
                                    Response::Error(common::make_string("flash write failed"))
                                }
                            }
                            Ok(Command::ResetDefaults) => {
                                let default_cfg = JiggleConfig::default();
                                engine.set_config(default_cfg);
                                save_config_to_flash(&default_cfg);
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

        let in_boot_grace_period = now_ms < BOOT_GRACE_PERIOD_MS;
        let is_testing = now_ms < test_until_ms;
        let should_jiggle = (!host_connected && !in_boot_grace_period) || is_testing;

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
