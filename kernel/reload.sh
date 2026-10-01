#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

echo "=== [1/4] Compiling hid-lorgar module ==="
make

if [ "$EUID" -eq 0 ]; then
    RUN=()
else
    sudo -v
    RUN=(sudo)
fi

echo "=== [2/4] Removing previous instance (if loaded) ==="
if grep -q '^hid_lorgar ' /proc/modules; then
    "${RUN[@]}" rmmod hid_lorgar
fi

echo "=== [3/4] Inserting hid-lorgar module ==="
"${RUN[@]}" insmod hid-lorgar.ko

echo "=== [4/4] Rebinding mouse to hid-lorgar ==="
shopt -s nullglob
devices=(/sys/bus/hid/devices/*093A:C60*)
for dev in "${devices[@]}"; do
    name="${dev##*/}"
    path="$(readlink -f "$dev")"
    parent="$(dirname "$path")"
    if ! grep -qx 'DEVTYPE=usb_interface' "$parent/uevent" 2>/dev/null; then
        echo "Skipping non-USB HID device $name"
        continue
    fi
    if [ -L "$dev/driver" ]; then
        driver="$(basename "$(readlink -f "$dev/driver")")"
    else
        driver="unbound"
    fi
    if [ "$driver" = "hid-lorgar" ]; then
        continue
    fi
    if [ "$driver" != "hid-generic" ] && [ "$driver" != "unbound" ]; then
        echo "Refusing to replace unexpected driver $driver for $name" >&2
        exit 1
    fi
    echo "Binding $name to hid-lorgar..."
    if [ "$driver" = "hid-generic" ]; then
        printf '%s\n' "$name" | "${RUN[@]}" tee /sys/bus/hid/drivers/hid-generic/unbind >/dev/null
    fi
    if ! printf '%s\n' "$name" | "${RUN[@]}" tee /sys/bus/hid/drivers/hid-lorgar/bind >/dev/null; then
        if [ "$driver" = "hid-generic" ]; then
            printf '%s\n' "$name" | "${RUN[@]}" tee /sys/bus/hid/drivers/hid-generic/bind >/dev/null
            echo "Restored $name to hid-generic after bind failure" >&2
        fi
        exit 1
    fi
done

echo ""
echo "=== Done! Status: ==="
"${RUN[@]}" dmesg | tail -n 15 | grep -iE "lorgar|hid" || true
echo ""
echo "Power supply devices:"
ls -ld /sys/class/power_supply/lorgar* 2>/dev/null || echo "No lorgar power_supply found yet"
