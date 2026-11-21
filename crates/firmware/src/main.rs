#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

pub mod jiggler;

#[cfg(not(target_arch = "arm"))]
fn main() {
    let mut engine = jiggler::JigglerEngine::new(common::JiggleConfig::default());
    engine.on_jiggle_start(0);
    let _ = engine.update(1000, false);
    let _ = engine.config();
    let _ = engine.total_jiggles();
    engine.set_config(common::JiggleConfig::default());
}

#[cfg(target_arch = "arm")]
use common::{parse_command, serialize_response, Command, JiggleConfig, JiggleMode, Response};
#[cfg(target_arch = "arm")]
use cortex_m_rt::entry;
#[cfg(target_arch = "arm")]
use heapless::String;
#[cfg(target_arch = "arm")]
use panic_halt as _;
#[cfg(target_arch = "arm")]
use rp2040_boot2 as _;
#[cfg(target_arch = "arm")]
use rp2040_hal::{
    clocks::init_clocks_and_plls, pac, rom_data, sio::Sio, timer::Timer, usb::UsbBus,
    watchdog::Watchdog,
};
#[cfg(target_arch = "arm")]
use usb_device::{
    class_prelude::UsbBusAllocator,
    prelude::{UsbDeviceBuilder, UsbDeviceState, UsbVidPid},
};
#[cfg(target_arch = "arm")]
use usbd_hid::{
    descriptor::{generator_prelude::*, MouseReport},
    hid_class::HIDClass,
};
#[cfg(target_arch = "arm")]
use usbd_serial::SerialPort;

#[cfg(target_arch = "arm")]
#[link_section = ".boot2"]
#[no_mangle]
#[used]
pub static BOOT2: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

// Flash storage parameters for persistent settings
// RP2040 flash base in memory space is 0x10000000.
// On the RP2040-Zero (2MB flash), offset 0x001F0000 (XIP 0x101F0000) is the last 64KB sector,
// well beyond our firmware size (~50KB) and safely aligned to a 4096-byte sector boundary.
#[cfg(target_arch = "arm")]
const FLASH_CONFIG_OFFSET: u32 = 0x001F_0000;
#[cfg(target_arch = "arm")]
const FLASH_CONFIG_ADDR: *const u32 = (0x1000_0000 + FLASH_CONFIG_OFFSET) as *const u32;
#[cfg(target_arch = "arm")]
const CONFIG_MAGIC: u32 = 0x4A49_4749; // 'JIGI'

#[cfg(target_arch = "arm")]
#[repr(C, align(4))]
struct Aligned256([u8; 256]);

#[cfg(target_arch = "arm")]
fn load_saved_config() -> Option<JiggleConfig> {
    unsafe {
        let w0 = core::ptr::read_volatile(FLASH_CONFIG_ADDR);
        if w0 != CONFIG_MAGIC {
            return None;
        }

        let w1 = core::ptr::read_volatile(FLASH_CONFIG_ADDR.add(1));
        let w2 = core::ptr::read_volatile(FLASH_CONFIG_ADDR.add(2));
        let w3 = core::ptr::read_volatile(FLASH_CONFIG_ADDR.add(3));
        let w4 = core::ptr::read_volatile(FLASH_CONFIG_ADDR.add(4));
        let w5 = core::ptr::read_volatile(FLASH_CONFIG_ADDR.add(5));
        let w6 = core::ptr::read_volatile(FLASH_CONFIG_ADDR.add(6));
        let w7 = core::ptr::read_volatile(FLASH_CONFIG_ADDR.add(7));
        let w8 = core::ptr::read_volatile(FLASH_CONFIG_ADDR.add(8));

        if (w0 ^ w1 ^ w2 ^ w3 ^ w4 ^ w5 ^ w6 ^ w7) != w8 {
            return None;
        }

        let mode = match w2 {
            1 => JiggleMode::Random,
            2 => JiggleMode::GentleNudge,
            3 => JiggleMode::Circle,
            _ => return None,
        };

        let mut cfg = JiggleConfig {
            enabled: w1 != 0,
            mode,
            period_ms: w3,
            scale_px: w4.clamp(1, 400) as u16,
            return_to_home: w5 != 0,
            rth_period_secs: w6,
            click_home: w7 != 0,
        };
        cfg.sanitize();
        Some(cfg)
    }
}

/// Runs entirely from SRAM (`.data`) so no instruction is ever fetched from XIP Flash
/// while XIP mode is disabled during flash erase and programming.
#[cfg(target_arch = "arm")]
#[link_section = ".data"]
#[inline(never)]
unsafe fn write_flash_in_ram(
    connect_internal_flash: unsafe extern "C" fn(),
    flash_exit_xip: unsafe extern "C" fn(),
    flash_range_erase: unsafe extern "C" fn(u32, usize, u32, u8),
    flash_range_program: unsafe extern "C" fn(u32, *const u8, usize),
    flash_flush_cache: unsafe extern "C" fn(),
    boot2_fn: unsafe extern "C" fn(),
    offset: u32,
    page_ptr: *const u8,
) {
    connect_internal_flash();
    flash_exit_xip();
    flash_range_erase(offset, 4096, 65536, 0xD8);
    flash_range_program(offset, page_ptr, 256);
    flash_flush_cache();
    boot2_fn();
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

#[cfg(target_arch = "arm")]
fn save_config_to_flash(cfg: &JiggleConfig) -> bool {
    let mut page = Aligned256([0xFFu8; 256]);
    let w0 = CONFIG_MAGIC;
    let w1: u32 = if cfg.enabled { 1 } else { 0 };
    let w2: u32 = match cfg.mode {
        JiggleMode::Random => 1,
        JiggleMode::GentleNudge => 2,
        JiggleMode::Circle => 3,
    };
    let w3: u32 = cfg.period_ms;
    let w4: u32 = cfg.scale_px as u32;
    let w5: u32 = if cfg.return_to_home { 1 } else { 0 };
    let w6: u32 = cfg.rth_period_secs;
    let w7: u32 = if cfg.click_home { 1 } else { 0 };
    let w8: u32 = w0 ^ w1 ^ w2 ^ w3 ^ w4 ^ w5 ^ w6 ^ w7;

    page.0[0..4].copy_from_slice(&w0.to_le_bytes());
    page.0[4..8].copy_from_slice(&w1.to_le_bytes());
    page.0[8..12].copy_from_slice(&w2.to_le_bytes());
    page.0[12..16].copy_from_slice(&w3.to_le_bytes());
    page.0[16..20].copy_from_slice(&w4.to_le_bytes());
    page.0[20..24].copy_from_slice(&w5.to_le_bytes());
    page.0[24..28].copy_from_slice(&w6.to_le_bytes());
    page.0[28..32].copy_from_slice(&w7.to_le_bytes());
    page.0[32..36].copy_from_slice(&w8.to_le_bytes());

    // Copy boot2 into 4-byte aligned SRAM buffer and resolve all ROM function pointers
    // BEFORE exiting XIP mode.
    let mut boot2_ram = Aligned256([0u8; 256]);
    boot2_ram.0.copy_from_slice(&BOOT2);

    let connect_internal_flash = rom_data::connect_internal_flash::ptr();
    let flash_exit_xip = rom_data::flash_exit_xip::ptr();
    let flash_range_erase = rom_data::flash_range_erase::ptr();
    let flash_range_program = rom_data::flash_range_program::ptr();
    let flash_flush_cache = rom_data::flash_flush_cache::ptr();
    let boot2_fn: unsafe extern "C" fn() =
        unsafe { core::mem::transmute((boot2_ram.0.as_ptr() as usize) | 1) };

    cortex_m::interrupt::free(|_| unsafe {
        write_flash_in_ram(
            connect_internal_flash,
            flash_exit_xip,
            flash_range_erase,
            flash_range_program,
            flash_flush_cache,
            boot2_fn,
            FLASH_CONFIG_OFFSET,
            page.0.as_ptr(),
        );
    });

    load_saved_config().is_some()
}

#[cfg(target_arch = "arm")]
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

    let mut usb_serial = SerialPort::new_with_store(&usb_bus, [0u8; 512], [0u8; 512]);
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
    let mut was_jiggling = false;

    loop {
        let _ = usb_dev.poll(&mut [&mut usb_serial, &mut usb_hid]);
        let _ = usb_serial.flush();

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
                            Ok(Command::GetStatus) => {
                                let in_boot_grace_period = now_ms < BOOT_GRACE_PERIOD_MS;
                                let is_paused = (host_connected || in_boot_grace_period)
                                    && now_ms >= test_until_ms;
                                Response::Status {
                                    config: *engine.config(),
                                    total_jiggles: engine.total_jiggles(),
                                    uptime_secs: (now_us / 1_000_000) as u32,
                                    paused: is_paused,
                                    testing: now_ms < test_until_ms,
                                }
                            }
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
                                engine.on_jiggle_start(now_ms);
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
                        if matches!(resp, Response::Ok) {
                            let in_boot_grace_period = now_ms < BOOT_GRACE_PERIOD_MS;
                            let is_paused =
                                (host_connected || in_boot_grace_period) && now_ms >= test_until_ms;
                            let status_resp = Response::Status {
                                config: *engine.config(),
                                total_jiggles: engine.total_jiggles(),
                                uptime_secs: (now_us / 1_000_000) as u32,
                                paused: is_paused,
                                testing: now_ms < test_until_ms,
                            };
                            let status_str = serialize_response(&status_resp);
                            let _ = usb_serial.write(status_str.as_bytes());
                        }
                        let _ = usb_serial.flush();
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

        if should_jiggle && !was_jiggling {
            engine.on_jiggle_start(now_ms);
        }
        was_jiggling = should_jiggle;

        if usb_dev.state() == UsbDeviceState::Configured && should_jiggle {
            if let Some(action) = engine.update(now_ms, is_testing) {
                let report = MouseReport {
                    buttons: action.buttons,
                    x: action.dx,
                    y: action.dy,
                    wheel: 0,
                    pan: 0,
                };
                let _ = usb_hid.push_input(&report);
            }
        }
    }
}
