# Waveshare RP2040-Zero Mouse Jiggler & Egui Configuration Tool

A high-performance USB-HID Mouse Jiggler firmware for the **Waveshare RP2040-Zero** with a cross-platform companion desktop GUI written in Rust using **egui**.

## Features

- **RP2040 USB Composite Device**: Emulates a standard USB-HID mouse alongside a USB CDC-ACM virtual serial port for real-time control.
- **Configurable Motion Engines**:
  - **Random**: Pseudo-random cursor movements up to 400 px displacement, with an optional **Return to Home (RTH)** timer (in seconds) that tracks cumulative movement and returns the cursor to its exact starting coordinates, plus an optional **Click Home** left-click upon returning home.
  - **Gentle Nudge**: Alternating 2-pixel micro-steps with immediate return to origin.
  - **Circle**: Smooth 16-point circular trajectory with configurable **Diameter** (up to 400 px) and an optional **Click Home** left-click after each completed circular orbit.
- **Dynamic Interval & Scale**: Controllable movement interval (in milliseconds) and movement scale (Displacement / Diameter up to 400 pixels).
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

- **15-Second Boot Grace Period**: When plugged in (or whenever you tap the hardware **RESET** button), the board enters a **15-second grace period** where mouse movements are paused. This gives you plenty of calm, stationary mouse time to launch and interact with the app.
- **Instant Auto-Connect**: The desktop GUI automatically detects and connects to the RP2040 serial port (`usbmodem...`) on startup.
- **Safe Configuration Mode**: Once connected to the GUI, the RP2040 firmware detects the configuration session and **keeps mouse jiggling paused** so you can comfortably move your mouse and configure options without interference. If the app disconnects or closes, the jiggler resumes normal operation automatically.
- **10-Second Test Button**: Click **"Test Jiggle (10s Demo)"** to temporarily run your selected motion settings for 10 seconds to preview the behavior before saving.
- **Apply & Save**: Click **Apply Settings** to send updated period/scale/mode, or **Save To Flash** to store them permanently.
