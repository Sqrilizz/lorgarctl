# LORGAR MSA10W Linux Client & Protocol Driver (`lorgarctl`)

[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg?style=for-the-badge)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Linux-green.svg?style=for-the-badge&logo=linux)](https://kernel.org)

Native open-source Linux client, CLI controller, and reverse-engineered HID protocol library for the **LORGAR MSA10W** wireless gaming mouse (VID: `093a`, PID: `c600` / `c601`).

<p align="center">
  <img src="LORGAR_Advanced_MSA10W_Wireless_Gaming_MouseBlack__3-1524438479.webp" alt="LORGAR MSA10W Wireless Gaming Mouse" width="700">
</p>

---

## Overview & CLI Control Hub

`lorgarctl` implements the documented subset of MSA10W controls without requiring
the Windows application. It is a user-space control stack, not a replacement for
the kernel's normal mouse input driver. Unknown protocol fields remain raw.
The old dashboard screenshot below is historical: undocumented lighting,
polling, charging and active-stage readback are no longer presented as facts.

The workspace now includes the native GTK4 `lorgar-app`, `lorgard`, reconnecting event monitoring, persistent
profiles, Unix socket control, X11 application rules and lossless capture tools.
See [the control-stack guide](docs/control-stack.md), [baseline audit](AUDIT.md)
and [maintainer review](docs/maintainer-review.md) for architecture and limits.

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
│   ├── lorgarctl/               # CLI application and legacy integrations
│   ├── lorgard/                 # User daemon, cached state and profile switching
│   └── lorgar-app/              # Native GTK4 settings, profiles and application rules
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
sudo pacman -S base-devel pkgconf hidapi dbus gtk4

# Ubuntu / Debian
sudo apt install build-essential pkg-config libudev-dev libhidapi-dev libdbus-1-dev libgtk-4-dev

# Fedora
sudo dnf install gcc pkg-config systemd-devel hidapi-devel dbus-devel gtk4-devel
```

### 2. Build Release Binary

```bash
cargo build --workspace --release --locked
```

Executables: `target/release/lorgar-app`, `lorgarctl`, and `lorgard`.
The native app needs GTK 4.8+ and Rust 1.92+ to build. For a headless build,
use `cargo build --release -p lorgarctl -p lorgard` without GTK dependencies.

### 3. Native settings app

```bash
./target/release/lorgar-app
```

Edit eight DPI stages, active stage, polling rate, lighting mode/color, saved
profiles and reconnect/application preferences. The app connects to `lorgard`
or starts a temporary sibling daemon, which it stops on normal exit. An already
running daemon is left alone. Save and Apply are separate; displayed drafts are
not fabricated hardware readback. Offline profile editing works too.

The overview uses the product photo. An unfinished 3D experiment is available
only with `lorgar-app --experimental-3d`: drag to rotate, focus and scroll
to zoom, or use the keyboard and visible controls. The mesh is a visual
approximation based on the product photo, not a measured CAD model. Download
the [OBJ](assets/msa10w/msa10w.obj) with its [materials](assets/msa10w/msa10w.mtl),
or export it with `lorgar-app --export-model NEW_DIRECTORY`.

See [app installation and behavior](docs/control-stack.md#native-settings-app)
for details. Manual controls work on X11 and Wayland; automatic foreground
switching currently supports X11 only.

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

Display device presence and battery status. Unknown readback fields are explicit:

```bash
lorgarctl status
# Aliases:
lorgarctl info
lorgarctl hub
```

### 2. Battery Query & KDE Plasma Panel Integration

Read the current battery percentage:

```bash
lorgarctl battery
```

Output short string for **KDE Plasma Panel Widgets**:

```bash
lorgarctl battery --short
# Output: 🖱️ 83%
```

Install **Native KDE Plasma 6 Applet (`LORGAR Battery`)**:

```bash
mkdir -p ~/.local/share/plasma/plasmoids/
cp -r plasmoid/org.lorgar.battery ~/.local/share/plasma/plasmoids/
```

Output JSON payload for Waybar, Polybar, or KDE scripts:

```bash
lorgarctl battery --json
# Output: {"percentage":83,"icon":"🖱️","text":"83%","tooltip":"LORGAR MSA10W: 83%","class":"good"}
```

Send desktop notification if battery is low (<= 20%):

```bash
lorgarctl battery --notify --threshold 20
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

## New control-stack commands

```bash
lorgarctl monitor --json
lorgarctl profile create gaming
lorgarctl profile set gaming --dpi-stage 0=800 --dpi-stage 1=1600
lorgarctl profile set gaming --polling 1000 --rgb static --color ff5500
lorgarctl profile apply gaming
lorgarctl profile list
lorgarctl profile show gaming
lorgarctl profile delete gaming

lorgard --foreground auto
lorgarctl --daemon status --json
lorgarctl --daemon profile apply gaming
lorgarctl --daemon monitor --json

lorgarctl capture before.bin --duration 10
lorgarctl capture after.bin --duration 10 --query-state
lorgarctl inspect-capture after.bin --json
lorgarctl diff-captures before.bin after.bin --json
lorgarctl doctor --json
lorgarctl completions bash
```

Direct clients hold exclusive ownership while running; use `--daemon` while
`lorgard` is running. Stop legacy tray/UPower instances before switching to the
daemon. The original tray, UPower, debug and short control spellings remain.
Native KDE/GNOME Wayland foreground detection is not implemented; `auto`
disables application switching there. The tested X11 backend uses EWMH and
`xprop`; it explicitly refuses XWayland-only detection in a Wayland session.

Profiles live in `$XDG_CONFIG_HOME/lorgar/config.json` (default
`~/.config/lorgar/config.json`). See the guide for versioning, application rules,
systemd installation, non-root permissions, IPC and verification instructions.

## Protocol Reference

For full reverse-engineered HID command specifications, report structures, and opcode matrices, see [`protocol/msa10w.md`](protocol/msa10w.md).

---

## License

This project is licensed under the [MIT License](LICENSE).
