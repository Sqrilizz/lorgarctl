# hid-lorgar: Native Linux Kernel Driver for LORGAR MSA10W

Native Linux kernel driver (`hid-lorgar.ko`) for the **LORGAR MSA10W** gaming mouse (VID: `0x093a`, PID: `0xc600` Wired / `0xc601` Wireless).

## Features

- **Native Linux Power Supply Integration (`power_supply`)**:
  - Exposes the mouse battery directly to the Linux kernel (`/sys/class/power_supply/lorgar_mouse_battery_*`).
  - Native integration with **UPower**, **KDE Plasma Battery & Brightness**, and **GNOME Power Settings**.
  - Internal mutex-protected queries prevent hardware timing collisions and freezes.
- **Hardware Link State (`hardware_status`)**:
  - `online`: Mouse is awake, transmitting packets and responding to RF queries.
  - `asleep`: Mouse has shut off its RF radio and LEDs for power saving.
- **Direct DPI Resolution Control (`dpi`)**:
  - Read or set the exact DPI resolution (from `100` to `26000`) for the current active stage without any proprietary Windows software!
- **Eco / Power-Save Mode (`power_save`)**:
  - `1`: Automatically switches the mouse to 500 Hz and turns off all RGB LEDs, extending battery life to **80–100+ hours**.
  - `0`: Restores full gaming performance (1000 Hz).
- **RGB Lighting Controls**:
  - `led_mode`: `1` (Static), `2` (Breathing), `3` (ColorCycle), `4` (Tapping), `5` (Wave).
  - `led_color`: Hex color `RRGGBB` (`000000` = LEDs completely OFF).
- **Physical DPI Button Sync**:
  - Catches hardware Report `0x05` events (`0x0b` DPI toggle) and broadcasts `sysfs_notify` uevents.

---

## Quick Reload & Test

```bash
cd kernel
./reload.sh
```

### Sysfs Controls Usage (`/sys/bus/hid/drivers/hid-lorgar/*/`):

```bash
# Check hardware status (online / asleep)
cat /sys/bus/hid/drivers/hid-lorgar/*/hardware_status

# Check current active DPI resolution
cat /sys/bus/hid/drivers/hid-lorgar/*/dpi

# Set current DPI to 1200
echo 1200 | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/dpi

# Turn on Eco power saving (LEDs off + 500Hz for 80+ hours battery life)
echo 1 | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/power_save

# Restore 1000Hz gaming mode
echo 0 | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/power_save

# Change RGB color (e.g. Neon Cyan)
echo 00ffff | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/led_color
```
