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

The installer requires `dkms`, registers version 2.3.4, builds it for the
running kernel, installs the udev and modules-load rules, and warns when
obsolete tray/UHID battery services are still active. Kernel headers must
already match the running kernel. Reboot once after installation to prove
automatic loading.

To remove the DKMS installation:

```bash
sudo dkms remove -m hid-lorgar -v 2.3.4 --all
sudo rm -f /etc/modules-load.d/hid-lorgar.conf
sudo rm -f /etc/udev/rules.d/99-lorgar-msa10w.rules
sudo rm -rf /usr/src/hid-lorgar-2.3.4
sudo depmod -a
```

## Runtime contract

The configuration interface exposes attributes below
`/sys/bus/hid/drivers/hid-lorgar/*/`:

- `hardware_status`: `unknown`, `online`, or `unresponsive`. `online` requires a
  valid state response. A successful USB write or unrelated input report never
  proves that the state query succeeded. Two consecutive query
  timeouts are required before the status becomes `unresponsive`, because a
  single missed 2-second window is routinely caused by user-space polling the
  same device rather than by a lost mouse; verified on 2026-10-07, where
  `lorgarctl battery` polling every two seconds held the kernel status at
  `unresponsive` while the kernel capacity still tracked the same 50% the
  user-space client read.
- `input_seen`: `1` after this bind has received physical mouse input, otherwise
  `0`. It is intentionally separate from `hardware_status`.
- `dpi_stage`: last stage successfully selected by this driver or observed through
  event `05 0b SS 00 00`.
- `dpi`: last DPI successfully written for the active known stage.
- `polling_rate`: last polling rate successfully written by this driver.
- `led_mode`: last lighting mode successfully written by this driver.
- `led_color`: last lighting color successfully written by this driver.
- `battery_indicator`: optional battery palette for all eight top DPI indicator
  stages. `0-20%` is red, `21-50%` orange, `51-80%` yellow and `81-100%` green.
  It is disabled by default because the device provides no color readback or
  rollback. Writing `1` explicitly overwrites all eight stage colors once and
  updates them only when a later battery response crosses a band. It never
  switches the active DPI stage and does not run a periodic pulse.

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
echo 1 | sudo tee /sys/bus/hid/drivers/hid-lorgar/*/battery_indicator
```

The battery starts absent with no fabricated percentage. Its power-supply device
is registered only after a confirmed `0x82` state response publishes byte 7 as
capacity, so UPower never observes an empty battery during driver startup.
Charging status remains `Unknown` for both USB product IDs because the protocol
does not provide a confirmed charging field.

The kernel query interval is 60 seconds. A state response caused by `lorgard` also
refreshes the kernel cache and postpones the kernel's next query, so the normal
30-second daemon cadence remains the active owner instead of creating duplicate
traffic. At startup the driver makes at most three query attempts because the
receiver can ignore the first request immediately after a rebind. All
kernel-originated feature reports are serialized with a 20 ms hardware cooldown.

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
