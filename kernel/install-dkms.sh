#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$DIR")"
MODULE="hid-lorgar"
VERSION="$(sed -n 's/^PACKAGE_VERSION="\([^"]*\)"/\1/p' "$DIR/dkms.conf")"
KERNEL="$(uname -r)"
SOURCE_DIR="/usr/src/${MODULE}-${VERSION}"

if [ -z "$VERSION" ]; then
    echo "Cannot read PACKAGE_VERSION from dkms.conf" >&2
    exit 1
fi

if [ ! -d "/lib/modules/$KERNEL/build" ]; then
    echo "Missing headers for kernel $KERNEL" >&2
    exit 1
fi

if [ "$EUID" -eq 0 ]; then
    RUN=()
else
    sudo -v
    RUN=(sudo)
fi

if ! command -v dkms >/dev/null 2>&1; then
    echo "Install DKMS with the system package manager and rerun this script" >&2
    exit 1
fi

status="$("${RUN[@]}" dkms status -m "$MODULE" -v "$VERSION" 2>/dev/null || true)"
if [ -n "$status" ]; then
    for file in Makefile dkms.conf hid-lorgar.c hid-lorgar-protocol.h; do
        if ! "${RUN[@]}" cmp -s "$DIR/$file" "$SOURCE_DIR/$file"; then
            echo "$MODULE $VERSION is already registered with different source; bump PACKAGE_VERSION" >&2
            exit 1
        fi
    done
else
    "${RUN[@]}" install -d -m 0755 "$SOURCE_DIR"
    for file in Makefile dkms.conf hid-lorgar.c hid-lorgar-protocol.h; do
        "${RUN[@]}" install -m 0644 "$DIR/$file" "$SOURCE_DIR/$file"
    done
    "${RUN[@]}" dkms add -m "$MODULE" -v "$VERSION"
fi

kernel_status="$(printf '%s\n' "$status" | grep -F ", $KERNEL," || true)"
if ! grep -q ': installed' <<<"$kernel_status"; then
    if ! grep -q ': built' <<<"$kernel_status"; then
        "${RUN[@]}" dkms build -m "$MODULE" -v "$VERSION" -k "$KERNEL"
    fi
    "${RUN[@]}" dkms install -m "$MODULE" -v "$VERSION" -k "$KERNEL"
fi
"${RUN[@]}" install -m 0644 "$PROJECT_DIR/udev/99-lorgar-msa10w.rules" /etc/udev/rules.d/99-lorgar-msa10w.rules
"${RUN[@]}" install -m 0644 "$DIR/hid-lorgar.conf" /etc/modules-load.d/hid-lorgar.conf
"${RUN[@]}" udevadm control --reload-rules
"${RUN[@]}" udevadm trigger --subsystem-match=hidraw --action=change
"${RUN[@]}" udevadm trigger --subsystem-match=input --action=change
"${RUN[@]}" udevadm settle

"${RUN[@]}" systemctl daemon-reload
"${RUN[@]}" modprobe hid-lorgar

for service in lorgarctl-upower.service lorgarctl-tray.service lorgarctl-battery.timer; do
    if systemctl --user is-active --quiet "$service" 2>/dev/null; then
        echo "Warning: conflicting user service is active: $service" >&2
    fi
done
if "${RUN[@]}" systemctl is-active --quiet lorgarctl-upower.service 2>/dev/null; then
    echo "Warning: conflicting system service is active: lorgarctl-upower.service" >&2
fi

installed_version="$(modinfo -F version hid_lorgar 2>/dev/null || true)"
installed_path="$(modinfo -F filename hid_lorgar 2>/dev/null || true)"
if [ "$installed_version" != "$VERSION" ] || [ -z "$installed_path" ]; then
    echo "Installed module verification failed: version=${installed_version:-none} path=${installed_path:-none}" >&2
    exit 1
fi

printf 'DKMS %s %s installed for %s\nmodule=%s\n' \
    "$MODULE" "$VERSION" "$KERNEL" "$installed_path"
