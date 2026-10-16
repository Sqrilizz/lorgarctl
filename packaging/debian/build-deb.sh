#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-$(git describe --tags --always | sed 's/^v//')}"
PKG_NAME="lorgarctl_${VERSION}_amd64"
BUILD_DIR="/tmp/${PKG_NAME}"

echo "Building Debian package for version: ${VERSION}"

rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR/DEBIAN"
mkdir -p "$BUILD_DIR/usr/bin"
mkdir -p "$BUILD_DIR/usr/lib/udev/rules.d"
mkdir -p "$BUILD_DIR/usr/lib/systemd/user"
mkdir -p "$BUILD_DIR/usr/share/bash-completion/completions"
mkdir -p "$BUILD_DIR/usr/share/zsh/site-functions"
mkdir -p "$BUILD_DIR/usr/share/fish/vendor_completions.d"
mkdir -p "$BUILD_DIR/usr/share/plasma/plasmoids"
mkdir -p "$BUILD_DIR/usr/share/doc/lorgarctl"

# Binaries
cp target/release/lorgarctl "$BUILD_DIR/usr/bin/"
cp target/release/lorgard "$BUILD_DIR/usr/bin/"
chmod 755 "$BUILD_DIR/usr/bin/"*

# Systemd user service
cp systemd/lorgard.service "$BUILD_DIR/usr/lib/systemd/user/"

# Udev rules
cp udev/99-lorgar-msa10w.rules "$BUILD_DIR/usr/lib/udev/rules.d/"
cp udev/70-lorgar-uhid-optional.rules "$BUILD_DIR/usr/lib/udev/rules.d/"

# Shell completions
if [ -d target/completions ]; then
    cp target/completions/lorgarctl.bash "$BUILD_DIR/usr/share/bash-completion/completions/lorgarctl"
    cp target/completions/_lorgarctl "$BUILD_DIR/usr/share/zsh/site-functions/_lorgarctl"
    cp target/completions/lorgarctl.fish "$BUILD_DIR/usr/share/fish/vendor_completions.d/lorgarctl.fish"
fi

# KDE Plasma 6 widget
cp -r plasmoid/org.lorgar.battery "$BUILD_DIR/usr/share/plasma/plasmoids/"

# DKMS module sources
DKMS_VER="$(sed -n 's/^PACKAGE_VERSION="\([^"]*\)"/\1/p' kernel/dkms.conf)"
DKMS_DEST="$BUILD_DIR/usr/src/hid-lorgar-${DKMS_VER}"
mkdir -p "$DKMS_DEST"
cp kernel/*.c kernel/*.h kernel/Makefile kernel/dkms.conf "$DKMS_DEST/"

# Docs & Licenses
cp README.md LICENSE-MIT LICENSE-APACHE "$BUILD_DIR/usr/share/doc/lorgarctl/"

# Control file & maintainer scripts
sed "s/@@VERSION@@/${VERSION}/g" packaging/debian/DEBIAN/control.template > "$BUILD_DIR/DEBIAN/control"
cp packaging/debian/DEBIAN/postinst "$BUILD_DIR/DEBIAN/postinst"
cp packaging/debian/DEBIAN/prerm "$BUILD_DIR/DEBIAN/prerm"
chmod 755 "$BUILD_DIR/DEBIAN/postinst" "$BUILD_DIR/DEBIAN/prerm"

# Build deb package using dpkg-deb
mkdir -p target/debian
dpkg-deb --build --root-owner-group "$BUILD_DIR" "target/debian/${PKG_NAME}.deb"
sha256sum "target/debian/${PKG_NAME}.deb" > "target/debian/${PKG_NAME}.deb.sha256"

echo "Package created: target/debian/${PKG_NAME}.deb"
