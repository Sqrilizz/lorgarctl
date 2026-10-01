# hid-lorgar: Native Linux Kernel Driver for LORGAR MSA10W

Native out-of-tree Linux kernel driver (`hid-lorgar.ko`) for the **LORGAR MSA10W** gaming mouse (VID: `0x093a`, PID: `0xc600` Wired / `0xc601` Wireless).

## Features

- **Native Linux Power Supply Integration (`power_supply`)**:
  - Automatically exposes the mouse battery to the Linux kernel.
  - Native integration with **UPower**, **KDE Plasma Battery & Brightness**, and **GNOME Power Settings**.
  - Periodic hardware queries (every 60 seconds) with mutex locking to eliminate hardware cooldown collisions.
- **Sysfs Controls** (`/sys/bus/hid/drivers/hid-lorgar/<device>/`):
  - `dpi_stage`: Read/write active DPI stage index (`0`..`7`).
  - `polling_rate`: Read/write USB polling rate in Hz (`125`, `250`, `500`, `1000`).
  - `led_mode`: Read/write RGB lighting mode (`1` = Static, `2` = Breathing, `3` = ColorCycle, `4` = Tapping, `5` = Wave).
  - `led_color`: Read/write RGB color in hexadecimal (e.g., `ff0000` for Red).
- **Physical DPI Button Sync**:
  - Catches hardware Report `0x05` events (`0x0b` DPI toggle) and broadcasts `sysfs_notify` / uevents to the desktop environment.

---

## 1. Quick Test (Without Rebooting)

### Build the module
```bash
cd kernel
make
```

### Load the module
```bash
sudo insmod hid-lorgar.ko
```

### Switch mouse from `hid-generic` to `hid-lorgar` (live hot-rebind)
You do not need to unplug the USB dongle:
```bash
# Unbind from generic driver
for dev in /sys/bus/hid/drivers/hid-generic/*093A:C60*; do
    [ -e "$dev" ] && echo $(basename "$dev") | sudo tee /sys/bus/hid/drivers/hid-generic/unbind
done

# Bind to hid-lorgar
for dev in /sys/bus/hid/devices/*093A:C60*; do
    [ -e "$dev" ] && echo $(basename "$dev") | sudo tee /sys/bus/hid/drivers/hid-lorgar/bind
done
```

### Verify dmesg
```bash
sudo dmesg | grep -i lorgar
```

### Check Battery in UPower / System
```bash
upower -e | grep -i mouse
upower -i $(upower -e | grep -i mouse)
cat /sys/class/power_supply/lorgar_mouse_battery_*/capacity
```

### Check & Change DPI / Polling Rate via Sysfs
```bash
# Check current active DPI stage
cat /sys/bus/hid/drivers/hid-lorgar/*/dpi_stage

# Switch to DPI stage 2
echo 2 | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/dpi_stage

# Check polling rate
cat /sys/bus/hid/drivers/hid-lorgar/*/polling_rate

# Switch polling rate to 500 Hz
echo 500 | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/polling_rate
```

---

## 2. Permanent Installation (DKMS)

To ensure `hid-lorgar` automatically rebuilds on future kernel updates:

```bash
sudo mkdir -p /usr/src/hid-lorgar-1.0.0
sudo cp -r * /usr/src/hid-lorgar-1.0.0/
sudo dkms add -m hid-lorgar -v 1.0.0
sudo dkms build -m hid-lorgar -v 1.0.0
sudo dkms install -m hid-lorgar -v 1.0.0
```

To uninstall:
```bash
sudo dkms remove -m hid-lorgar -v 1.0.0 --all
sudo rm -rf /usr/src/hid-lorgar-1.0.0
```
