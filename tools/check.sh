#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

run() {
    printf '\n[%s]\n' "$*"
    "$@"
}

run cargo fmt --all -- --check
run cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
run cargo test --workspace --locked
run bash tools/check-versions.sh

run cargo build --workspace --release --locked

if [ -d "/lib/modules/$(uname -r)/build" ]; then
    run make -C kernel check
    checkpatch="/lib/modules/$(uname -r)/build/scripts/checkpatch.pl"
    if [ -x "$checkpatch" ] && ! git diff --quiet -- kernel/hid-lorgar.c kernel/hid-lorgar-protocol.h; then
        printf '\n[kernel checkpatch]\n'
        git diff -- kernel/hid-lorgar.c kernel/hid-lorgar-protocol.h |
            "$checkpatch" --no-tree -
    elif [ ! -x "$checkpatch" ]; then
        printf '\n[SKIP] kernel checkpatch unavailable\n'
    fi
else
    printf '\n[SKIP] kernel check: matching headers unavailable\n'
fi

if command -v udevadm >/dev/null 2>&1; then
    run udevadm verify udev/99-lorgar-msa10w.rules udev/70-lorgar-uhid-optional.rules
else
    printf '\n[SKIP] udev validation: udevadm unavailable\n'
fi

run bash -n kernel/install-dkms.sh kernel/reload.sh kernel/verify-live.sh tools/probe-state.sh tools/check-versions.sh tools/generate-completions.sh

if command -v shellcheck >/dev/null 2>&1; then
    run shellcheck kernel/install-dkms.sh kernel/reload.sh kernel/verify-live.sh tools/check.sh tools/check-versions.sh tools/generate-completions.sh tools/probe-state.sh
else
    printf '\n[SKIP] shellcheck unavailable\n'
fi

run git diff --check
