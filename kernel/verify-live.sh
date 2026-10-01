#!/usr/bin/env bash
set -euo pipefail

expected_version="2.0.2"
loaded_version="$(cat /sys/module/hid_lorgar/version 2>/dev/null || true)"
if [ "$loaded_version" != "$expected_version" ]; then
    echo "Expected hid_lorgar $expected_version, loaded ${loaded_version:-none}" >&2
    exit 1
fi

shopt -s nullglob
driver_devices=(/sys/bus/hid/drivers/hid-lorgar/*:*)
config_devices=()
for device in "${driver_devices[@]}"; do
    path="$(readlink -f "$device")"
    parent="$(dirname "$path")"
    if ! grep -qx 'DEVTYPE=usb_interface' "$parent/uevent" 2>/dev/null; then
        echo "Non-USB HID device is bound to hid-lorgar: ${device##*/}" >&2
        exit 1
    fi
    if [ -f "$device/hardware_status" ]; then
        config_devices+=("$device")
    fi
done
if [ "${#config_devices[@]}" -ne 1 ]; then
    echo "Expected one hid-lorgar configuration interface, found ${#config_devices[@]}" >&2
    exit 1
fi

device="${config_devices[0]}"
for attribute in hardware_status dpi dpi_stage polling_rate led_mode led_color; do
    if [ ! -e "$device/$attribute" ]; then
        echo "Missing sysfs attribute: $attribute" >&2
        exit 1
    fi
done

status="$(<"$device/hardware_status")"
case "$status" in
    unknown|online|unresponsive) ;;
    *) echo "Invalid hardware_status: $status" >&2; exit 1 ;;
esac

supplies=(/sys/class/power_supply/lorgar_mouse_battery_*)
if [ "${#supplies[@]}" -ne 1 ]; then
    echo "Expected one LORGAR power supply, found ${#supplies[@]}" >&2
    exit 1
fi

supply="${supplies[0]}"
present="$(<"$supply/present")"
if [ "$present" != 0 ] && [ "$present" != 1 ]; then
    echo "Invalid battery present value: $present" >&2
    exit 1
fi
if [ "$(<"$supply/status")" != "Unknown" ]; then
    echo "Charging status must remain Unknown" >&2
    exit 1
fi
if [ "$present" = 1 ]; then
    capacity="$(<"$supply/capacity")"
    if ! [[ "$capacity" =~ ^[0-9]+$ ]] || [ "$capacity" -gt 100 ]; then
        echo "Invalid battery capacity: $capacity" >&2
        exit 1
    fi
fi

printf 'hid_lorgar=%s status=%s battery_present=%s\n' \
    "$loaded_version" "$status" "$present"
