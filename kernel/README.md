# hid-lorgar

Optional native Linux HID driver for the LORGAR MSA10W (`093a:c600` wired,
`093a:c601` wireless receiver).

The module keeps normal mouse input working and adds a `power_supply` device plus
sysfs controls for the confirmed protocol commands. It does not decode charging,
active DPI, polling, or lighting from the 64-byte state response. Those values are
unknown until this driver observes a confirmed DPI event or successfully sends the
corresponding command.

## Build and reload

The running kernel's headers and a kernel-compatible compiler are required.

```bash
make
./reload.sh
```

`reload.sh` validates administrator access before unloading the existing module.
It compiles first, reloads the module, rebinds matching interfaces, and prints the
recent kernel log. Reloading requires an interactive `sudo` session unless it is
run as root.

For a persistent DKMS installation on CachyOS/Arch:

```bash
./install-dkms.sh
```

The installer installs the `dkms` package when needed, registers version 2.0.2,
builds it for the running kernel, installs the udev and modules-load rules, and
disables obsolete tray/UHID battery services. Kernel headers must already match
the running kernel. Reboot once after installation to prove automatic loading.

To remove the DKMS installation:

```bash
sudo dkms remove -m hid-lorgar -v 2.0.2 --all
sudo rm -f /etc/modules-load.d/hid-lorgar.conf
sudo rm -f /etc/udev/rules.d/99-lorgar-msa10w.rules
sudo rm -rf /usr/src/hid-lorgar-2.0.2
sudo depmod -a
```

## Runtime contract

The configuration interface exposes attributes below
`/sys/bus/hid/drivers/hid-lorgar/*/`:

- `hardware_status`: `unknown`, `online`, or `unresponsive`. `online` requires a
  valid state response or a confirmed physical DPI event. A successful USB write
  alone never proves that the wireless mouse responded.
- `dpi_stage`: last stage successfully selected by this driver or observed through
  event `05 0b SS 00 00`.
- `dpi`: last DPI successfully written for the active known stage.
- `polling_rate`: last polling rate successfully written by this driver.
- `led_mode`: last lighting mode successfully written by this driver.
- `led_color`: last lighting color successfully written by this driver.

Reading an unobserved or unwritten setting returns `ENODATA`. This is intentional:
the MSA10W state response has no confirmed readback fields for those settings.
Writable sysfs controls remain administrator-owned; desktop user applications use
the `lorgard`/hidraw path granted through the supplied udev rule.

Examples:

```bash
cat /sys/bus/hid/drivers/hid-lorgar/*/hardware_status
cat /sys/class/power_supply/lorgar_mouse_battery_*/capacity

echo 1 | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/dpi_stage
echo 1400 | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/dpi
echo 1000 | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/polling_rate
echo 1 | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/led_mode
echo 00ffff | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/led_color
```

The battery starts absent with no fabricated percentage. A confirmed `0x82` state
response makes it present and publishes byte 7 as capacity. Charging status remains
`Unknown` for both USB product IDs because the protocol does not provide a confirmed
charging field.

The kernel query interval is 60 seconds. A state response caused by `lorgard` also
refreshes the kernel cache and postpones the kernel's next query, so the normal
30-second daemon cadence remains the active owner instead of creating duplicate
traffic. All kernel-originated feature reports are serialized with a 20 ms hardware
cooldown.

## Verification

```bash
make clean
make W=1
modinfo ./hid-lorgar.ko
bash -n reload.sh
make check
make live-check
```

After an administrator reload, verify `dmesg`, `hardware_status`, power-supply
`present`, `capacity`, and `status`. Physical DPI synchronization requires pressing
the hardware DPI button and observing `dpi_stage`; automated builds cannot prove
that physical path.
