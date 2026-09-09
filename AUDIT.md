# Baseline audit (2026-09-05)

Inspected all Rust/C source, manifests/build script, protocol reference, README,
udev rules, systemd services and Plasma widget. Existing repository retained.

## Baseline checks

- `cargo build --workspace`: passed.
- `cargo test --workspace`: passed, 11 core tests, no CLI tests.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  failed on manual parity check in CLI hex parser.

## Architecture

Two crates: lorgar-core owns discovery, protocol helpers and a boxed HID
transport; lorgarctl owns command parsing, presentation, tray and a C UHID
battery bridge. Transport injection exists as a trait but no public device
constructor or mock. No persistent configuration, event reader, daemon IPC,
capture format or foreground application backend exists.

## Findings before implementation

1. Color and debug hex parsers slice UTF-8 at byte offsets and can panic.
2. IPC lock acquisition uses `expect`, blocks indefinitely and uses a shared
   world-accessible temporary pathname. Read handles are not exclusively owned.
3. Queries retry arbitrary feature writes five times and accept unrelated input
   with a loose nonzero-byte test; errors and timeouts lose their cause.
4. State bytes 2, 4 and 8 are decoded without supporting documentation.
   PID c600 is treated as proof of charging/wired operation. Neither PID alone
   establishes charging or radio link state. Battery values are silently clamped.
5. Interface #2 fallback ignores a conflicting known usage page. Explicit paths
   can open unrelated devices and fabricate LORGAR metadata. HID collections
   sharing a path are enumerated multiple times. Local UHID batteries even share
   the VID/PID; interface validation is essential.
6. Errors discard permission/disconnection causes. Status hides query failures.
7. Repeated packet/report construction occurs throughout CLI and transport.
8. No reconnect state machine, cache freshness, event parsing or bounded IPC.
9. udev grants everyone write access to matching hidraw and all UHID devices;
   rules are duplicated. System service/widget contain a developer home path.
10. UHID fd lacks Rust RAII; update and short-write failures are ignored. Tray
    and UPower loops do not shut down gracefully and can present stale state.
11. Protocol reference contradicts itself: 0x21 and 0x82 appear both confirmed
    and hypothetical. Only battery offset is documented in the state response.
    Vendor reports are described as 4 bytes but examples include a fifth report
    ID byte. Unknown fields must remain raw.
12. Cargo.lock is ignored despite shipping applications; no CI quality gate.
    README overstates full hardware control and names missing license files.

## Hardware availability

Read-only enumeration found a physical 093a:c601 device with configuration
usage page ff10 on interface 2 and vendor page ff11 on interface 1. Two local
virtual battery devices also use 093a:c601. Enumeration alone does not verify
permissions, firmware behavior or any command interpretation.

## Implementation direction

Keep confirmed command bytes and existing CLI spellings. Add typed protocol
models, lossless input parsing, transport injection, reconnect monitoring,
versioned atomic profiles and separate application/IPC modules. Keep protocol
unknowns explicit. Use private user runtime files and bounded operations.
Hardware evidence and final validation are recorded separately.
