#!/usr/bin/env bash
# Print one line of LORGAR state for correlating a physical change with the
# bytes the device reports. Built for controlled experiments: change exactly
# one thing (plug the cable, flip the receiver, move the mouse) and watch what
# byte 7, byte 8 and the vendor events do.
#
# Usage:
#   tools/probe-state.sh              # single sample
#   tools/probe-state.sh 5 12         # every 5 seconds, 12 samples
#
# Columns: time  pid  byte7  byte8  vendor events
#
# Byte 7 is the confirmed battery percentage. Byte 8 is a mode flag and the
# vendor events are UNDECODED. Do not infer charging from the USB product ID.
# See protocol/msa10w.md.
# Deliberately not 'set -e': a missing device, a transient query failure or an
# empty jq result must degrade to a printed line, never abort a long run.
set -uo pipefail

interval="${1:-0}"
count="${2:-1}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

bin="$root/target/debug/lorgarctl"
[ -x "$bin" ] || bin="$root/target/release/lorgarctl"
if [ ! -x "$bin" ]; then
    echo "probe-state: build lorgarctl first: cargo build -p lorgarctl" >&2
    exit 1
fi

state_field() {
    local json_file="$1" index="$2"
    jq -r --argjson i "$index" 'select(.type == "packet" and .direction == "input"
        and (.raw | length) == 64) | .raw[$i]' "$json_file" | tail -1
}

sample() {
    local work bin_file json_file pid byte7 byte8 events
    work="$(mktemp -d)"
    bin_file="$work/cap.bin"
    json_file="$work/cap.json"

    if ! "$bin" capture "$bin_file" --duration 2 --query-state >/dev/null 2>&1 ||
        ! "$bin" inspect-capture "$bin_file" --json >"$json_file" 2>/dev/null; then
        rm -rf "$work"
        printf '%s  query-failed\n' "$(date +%H:%M:%S)"
        return
    fi

    pid="$(lsusb 2>/dev/null | grep -o '093a:c6[0-9a-f][0-9a-f]' | head -1 | cut -d: -f2 || true)"
    byte7="$(state_field "$json_file" 7 || true)"
    byte8="$(state_field "$json_file" 8 || true)"
    events="$(jq -r 'select(.type == "packet" and .direction == "input"
        and (.raw | length) == 5) | .raw | map(tostring) | join(" ")' "$json_file" |
        sort -u | tr '\n' ';' || true)"
    rm -rf "$work"

    if [ -z "$byte7" ]; then
        printf '%s  %s  no-64-byte-state-report\n' "$(date +%H:%M:%S)" "${pid:-none}"
        return
    fi
    printf '%s  %s  byte7=%-3s byte8=%-3s events=%s\n' \
        "$(date +%H:%M:%S)" "${pid:-none}" "$byte7" "${byte8:-?}" "${events:-none}"
}

for ((i = 0; i < count; i++)); do
    sample || printf '%s  sample-error\n' "$(date +%H:%M:%S)"
    if [ "$i" -lt "$((count - 1))" ]; then
        sleep "$interval"
    fi
done
