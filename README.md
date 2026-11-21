# Waveshare RP2040-Zero Mouse Jiggler & Egui Configuration Tool

A high-performance USB-HID Mouse Jiggler firmware for the **Waveshare RP2040-Zero** with a cross-platform companion desktop GUI written in Rust using **egui**.

## Features

- **RP2040 USB Composite Device**: Emulates a standard USB-HID mouse alongside a USB CDC-ACM virtual serial port for real-time control.
- **Configurable Motion Engines**:
  - **Random Wander**: Subtle, natural-looking pseudo-random cursor movements with automatic return steps to avoid screen drifting.
  - **Gentle Nudge**: Alternating 1-2 pixel micro-steps.
  - **Circle**: Circular trajectory.
  - **Off**: Pause jiggling while maintaining device connection.
- **Dynamic Frequency & Scale**: Controllable movement period (interval in milliseconds) and movement scale (displacement in pixels).
- **Cross-Platform GUI Controller**: Built with `egui` and `eframe` for macOS, Windows, and Linux. Automatically enumerates serial ports, shows device uptime & jiggle telemetry, and syncs settings.

---

## Workspace Structure

- `crates/common`: `no_std`-compatible shared protocol, commands, and serial framing.
- `crates/firmware`: `#![no_std]` RP2040 firmware targeting `thumbv6m-none-eabi`.
- `crates/desktop-gui`: `std` desktop GUI application using `egui`/`eframe` and `serialport`.

---

## Building and Flashing

### 1. Prerequisites

Install the Rust embedded target for Cortex-M0+ (RP2040):
```bash
rustup target add thumbv6m-none-eabi
```

Install the UF2 conversion tool:
```bash
cargo install elf2uf2-rs --locked
```

### 2. Building Firmware

Build the release binary:
```bash
cargo build --release --package firmware --target thumbv6m-none-eabi
```

Convert to `.uf2` for the RP2040 bootloader:
```bash
elf2uf2-rs target/thumbv6m-none-eabi/release/firmware target/thumbv6m-none-eabi/release/firmware.uf2
```

To flash directly to a connected RP2040-Zero in bootloader mode (`BOOT` + `RESET`):
```bash
elf2uf2-rs -d target/thumbv6m-none-eabi/release/firmware
```

### 3. Running the Desktop GUI

Run the cross-platform GUI application:
```bash
cargo run --package desktop-gui --release
```

Select the serial port corresponding to your RP2040-Zero (e.g., `/dev/cu.usbmodem...` on macOS or `COMx` on Windows), click **Connect**, adjust the sliders, and click **Apply Settings**.
