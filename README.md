# LORGAR MSA10W Linux Client & Protocol Driver (`lorgarctl`)

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg?style=for-the-badge)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Linux-green.svg?style=for-the-badge&logo=linux)](https://kernel.org)

Native open-source Linux client, CLI controller, and reverse-engineered HID protocol library for the **LORGAR MSA10W** wireless gaming mouse (VID: `093a`, PID: `c600` / `c601`).

<p align="center">
  <img src="LORGAR_Advanced_MSA10W_Wireless_Gaming_MouseBlack__3-1524438479.webp" alt="LORGAR MSA10W Wireless Gaming Mouse" width="700">
</p>

---

## Overview & CLI Control Hub

`lorgarctl` provides full hardware control over the LORGAR MSA10W mouse on Linux without requiring proprietary Windows software or WINE. It includes an interactive **CLI Control Dashboard** that displays real-time battery status, connection details, active DPI stage, USB polling rate, and RGB lighting modes.

<p align="center">
  <img src="Screenshot_20260819_183825.png" alt="lorgarctl status CLI Control Dashboard" width="750">
</p>

---

## Workspace Architecture

The project is structured as a modular Rust workspace:

```text
lorgar-linux/
├── Cargo.toml
├── LORGAR_Advanced_MSA10W_Wireless_Gaming_MouseBlack__3-1524438479.webp
├── Screenshot_20260819_183825.png
├── crates/
│   ├── lorgar-core/             # Pure Rust HID protocol & device discovery library
│   └── lorgarctl/               # High-performance CLI application
├── protocol/
│   └── msa10w.md                # Reverse-engineered protocol specification
├── udev/
│   └── 99-lorgar-msa10w.rules   # Non-root udev permission rules
└── README.md
```

---

## Key Features

- 🔋 **Live Battery & Status Monitoring**: Real-time battery percentage query via Feature Report `0x82` with visual battery bar.
- 🎯 **DPI Stage Configuration (`0x26`, `0x21`)**: Custom DPI resolutions (100..12000 DPI) for stages 0..7 and active stage switching.
- ⚡ **USB Polling Rate Control (`0x20`)**: Hardware polling interval selection (125 Hz, 250 Hz, 500 Hz, 1000 Hz).
- 🌈 **RGB Lighting Modes & Color (`0x22`, `0x23`)**: Full support for lighting effects (`static`, `breathing`, `color-cycle`, `tapping`, `wave`) and custom 24-bit static RGB colors.
- 🔍 **Dynamic Device Auto-Discovery**: Automatically scans HID interfaces (`Usage Page: 0xff10` / Interface #2) without needing manual `/dev/hidrawX` paths.
- 🛠️ **Reverse-Engineering & Debug Utilities**: Raw 8-byte payload sending and packet inspection using `--debug`.

---

## Installation & Building

### 1. System Dependencies (Linux)

Ensure Rust and HIDAPI native build tools are installed:

```bash
# Arch Linux / CachyOS
sudo pacman -S base-devel pkgconf hidapi

# Ubuntu / Debian
sudo apt install build-essential pkg-config libudev-dev libhidapi-dev

# Fedora
sudo dnf install gcc pkg-config systemd-devel hidapi-devel
```

### 2. Build Release Binary

```bash
cargo build --release
```

The compiled executable will be built at `./target/release/lorgarctl`.

---

## Non-Root Permissions (`udev` Setup)

By default, Linux `/dev/hidraw*` device nodes require root access. To allow running `lorgarctl` as a normal user:

```bash
sudo cp udev/99-lorgar-msa10w.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger
```

---

## Usage Guide

### 1. Control Dashboard & Hub

Display the interactive status card with battery status, active stage, polling rate, and RGB mode:

```bash
lorgarctl status
# Aliases:
lorgarctl info
lorgarctl hub
```

### 2. Battery Query

Read the current battery percentage:

```bash
lorgarctl battery
```

### 3. DPI Stage & Resolution Settings

Set DPI value for a specific stage (stage: 0..7, DPI: 100..12000):

```bash
# Set DPI Stage 1 to 1400 DPI
lorgarctl dpi set 1 1400

# Set DPI Stage 0 to 800 DPI
lorgarctl dpi set 0 800
```

Switch active DPI stage:

```bash
lorgarctl dpi active 1
```

### 4. USB Polling Rate Settings

Set report frequency (125 Hz, 250 Hz, 500 Hz, 1000 Hz):

```bash
lorgarctl polling set 1000
lorgarctl polling set 500
lorgarctl polling set 250
lorgarctl polling set 125
```

### 5. RGB Lighting Effects & Colors

Change RGB mode (`static`, `breathing`, `color-cycle`, `tapping`, `wave`):

```bash
lorgarctl rgb mode static
lorgarctl rgb mode breathing
lorgarctl rgb mode color-cycle
lorgarctl rgb mode tapping
lorgarctl rgb mode wave
```

Set static RGB color (`RRGGBB` or `#RRGGBB` format):

```bash
lorgarctl rgb color ff0000
lorgarctl rgb color 00ff00
lorgarctl rgb color 0000ff
lorgarctl rgb color "#a28466"
```

### 6. Debug & Packet Inspection

Enable detailed packet output:

```bash
lorgarctl --debug polling set 1000
```

Send custom 8-byte feature report (Reverse-Engineering utility):

```bash
lorgarctl debug send-feature 20 00 00 01 00 00 00 00
```

---

## Protocol Reference

For full reverse-engineered HID command specifications, report structures, and opcode matrices, see [`protocol/msa10w.md`](protocol/msa10w.md).

---

## License

This project is licensed under the [MIT License](LICENSE).