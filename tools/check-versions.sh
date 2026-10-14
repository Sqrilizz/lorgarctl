#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

dkms_ver="$(sed -n 's/^PACKAGE_VERSION="\([^"]*\)"/\1/p' kernel/dkms.conf)"
mod_ver="$(sed -n 's/^MODULE_VERSION("\([^"]*\)").*/\1/p' kernel/hid-lorgar.c)"
ws_ver="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"

fail=0
if [ -z "$dkms_ver" ] || [ -z "$mod_ver" ]; then
  echo "check-versions: could not parse kernel versions (dkms='$dkms_ver' module='$mod_ver')" >&2
  fail=1
elif [ "$dkms_ver" != "$mod_ver" ]; then
  echo "check-versions: kernel version drift: dkms.conf=$dkms_ver MODULE_VERSION=$mod_ver" >&2
  fail=1
fi

if ! grep -q "Kernel module (\`hid-lorgar\`) | $dkms_ver" VERSIONS.md; then
  echo "check-versions: VERSIONS.md does not list kernel $dkms_ver" >&2
  fail=1
fi
if ! grep -q "Rust workspace.*| $ws_ver" VERSIONS.md; then
  echo "check-versions: VERSIONS.md does not list Rust workspace $ws_ver" >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then exit 1; fi
echo "check-versions: ok (rust=$ws_ver kernel=$dkms_ver)"
