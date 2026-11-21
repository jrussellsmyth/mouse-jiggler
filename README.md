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

### 3. Flashing to RP2040-Zero

Put the board into bootloader mode by holding down the **BOOT** button while pressing/releasing **RESET** (or while plugging it into USB). The volume `RPI-RP2` will appear.

**Method A: Drag-and-drop or command line copy (Recommended on macOS)**
```bash
# macOS:
cp target/thumbv6m-none-eabi/release/firmware.uf2 /Volumes/RPI-RP2/

# Linux:
cp target/thumbv6m-none-eabi/release/firmware.uf2 /media/$USER/RPI-RP2/

# Windows (Command Prompt / PowerShell, assuming D: drive):
copy target\thumbv6m-none-eabi\release\firmware.uf2 D:\
```
*Note: As soon as the `.uf2` file finishes copying, the board automatically reboots and runs the firmware, detaching the `RPI-RP2` drive.*

**Method B: Direct deploy via elf2uf2-rs**
```bash
elf2uf2-rs -d target/thumbv6m-none-eabi/release/firmware
```
*(Note: On macOS, `elf2uf2-rs -d` can occasionally hang waiting for the OS volume mount lock. If that happens, press `Ctrl+C` and use Method A).*

### 4. Running the Desktop GUI

Run the cross-platform GUI application:
```bash
cargo run --package desktop-gui --release
```

Select the serial port corresponding to your RP2040-Zero (e.g., `/dev/cu.usbmodem...` on macOS or `COMx` on Windows) and click **Connect**.

- **Safe Configuration Mode**: Once connected to the GUI, the RP2040 firmware detects the configuration session and **automatically pauses mouse jiggling** so you can comfortably move your mouse and configure options without interference. If the app disconnects or closes, the jiggler resumes normal operation automatically.
- **10-Second Test Button**: Click **"Test Jiggle (10s Demo)"** to temporarily run your selected motion settings for 10 seconds to preview the behavior before saving.
- **Apply & Save**: Click **Apply Settings** to send updated period/scale/mode, or **Save To Flash** to store them permanently.
