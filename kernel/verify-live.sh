#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
expected_version="$(sed -n 's/^PACKAGE_VERSION="\([^"]*\)"/\1/p' "$DIR/dkms.conf")"
loaded_version="$(cat /sys/module/hid_lorgar/version 2>/dev/null || true)"

diagnose() {
    local built_version installed_version
    built_version="$(modinfo -F version "$DIR/hid-lorgar.ko" 2>/dev/null || true)"
    installed_version="$(modinfo -F version hid_lorgar 2>/dev/null || true)"
    printf 'expected=%s built=%s loaded=%s installed=%s\n' \
        "${expected_version:-none}" "${built_version:-none}" \
        "${loaded_version:-none}" "${installed_version:-none}" >&2
    for path in /sys/bus/hid/drivers/hid-lorgar/*:*; do
        [ -e "$path" ] || continue
        printf 'bound=%s\n' "${path##*/}" >&2
        if [ -r "$path/hardware_status" ]; then
            printf 'hardware_status=%s\n' "$(<"$path/hardware_status")" >&2
        fi
    done
    journalctl -k -b --no-pager 2>/dev/null |
        grep -iE 'lorgar|state query' | tail -n 12 >&2 || true
}

fail() {
    echo "$1" >&2
    diagnose
    exit 1
}

if [ -z "$expected_version" ]; then
    fail "Cannot read expected version from dkms.conf"
fi
if [ "$loaded_version" != "$expected_version" ]; then
    fail "Expected hid_lorgar $expected_version, loaded ${loaded_version:-none}"
fi

shopt -s nullglob
driver_devices=(/sys/bus/hid/drivers/hid-lorgar/*:*)
config_devices=()
for device in "${driver_devices[@]}"; do
    path="$(readlink -f "$device")"
    parent="$(dirname "$path")"
    if ! grep -qx 'DEVTYPE=usb_interface' "$parent/uevent" 2>/dev/null; then
        fail "Non-USB HID device is bound to hid-lorgar: ${device##*/}"
    fi
    if [ -f "$device/hardware_status" ]; then
        config_devices+=("$device")
    fi
done
if [ "${#config_devices[@]}" -ne 1 ]; then
    fail "Expected one hid-lorgar configuration interface, found ${#config_devices[@]}"
fi

device="${config_devices[0]}"
for attribute in hardware_status input_seen dpi dpi_stage polling_rate led_mode led_color battery_indicator; do
    if [ ! -e "$device/$attribute" ]; then
        fail "Missing sysfs attribute: $attribute"
    fi
done

input_seen="$(<"$device/input_seen")"
if [ "$input_seen" != 0 ] && [ "$input_seen" != 1 ]; then
    fail "Invalid input_seen value: $input_seen"
fi

indicator="$(<"$device/battery_indicator")"
if [ "$indicator" != 0 ] && [ "$indicator" != 1 ]; then
    fail "Invalid battery_indicator value: $indicator"
fi

status="$(<"$device/hardware_status")"
case "$status" in
    unknown|online|unresponsive) ;;
    *) fail "Invalid hardware_status: $status" ;;
esac

supplies=(/sys/class/power_supply/lorgar_mouse_battery_*)
if [ "${#supplies[@]}" -ne 1 ]; then
    fail "Expected one LORGAR power supply, found ${#supplies[@]}"
fi

supply="${supplies[0]}"
present="$(<"$supply/present")"
if [ "$present" != 0 ] && [ "$present" != 1 ]; then
    fail "Invalid battery present value: $present"
fi
if [ "$(<"$supply/status")" != "Unknown" ]; then
    fail "Charging status must remain Unknown"
fi
if [ "$present" = 1 ]; then
    capacity="$(<"$supply/capacity")"
    if ! [[ "$capacity" =~ ^[0-9]+$ ]] || [ "$capacity" -gt 100 ]; then
        fail "Invalid battery capacity: $capacity"
    fi
fi

printf 'hid_lorgar=%s status=%s battery_present=%s\n' \
    "$loaded_version" "$status" "$present"
