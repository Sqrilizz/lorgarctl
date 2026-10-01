#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

echo "=== [1/4] Compiling hid-lorgar module ==="
make LLVM=1

echo "=== [2/4] Removing previous instance (if loaded) ==="
if lsmod | grep -q "^hid_lorgar"; then
    sudo rmmod hid_lorgar || true
fi

echo "=== [3/4] Inserting hid-lorgar module ==="
sudo insmod hid-lorgar.ko

echo "=== [4/4] Rebinding mouse to hid-lorgar ==="
for dev in /sys/bus/hid/drivers/hid-generic/*093A:C60*; do
    if [ -e "$dev" ]; then
        echo "Unbinding $(basename "$dev") from hid-generic..."
        echo $(basename "$dev") | sudo tee /sys/bus/hid/drivers/hid-generic/unbind >/dev/null || true
    fi
done

for dev in /sys/bus/hid/devices/*093A:C60*; do
    if [ -e "$dev" ]; then
        echo "Binding $(basename "$dev") to hid-lorgar..."
        echo $(basename "$dev") | sudo tee /sys/bus/hid/drivers/hid-lorgar/bind >/dev/null || true
    fi
done

echo ""
echo "=== Done! Status: ==="
sudo dmesg | tail -n 15 | grep -iE "lorgar|hid" || true
echo ""
echo "Power supply devices:"
ls -ld /sys/class/power_supply/lorgar* 2>/dev/null || echo "No lorgar power_supply found yet"
