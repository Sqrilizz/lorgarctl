# Maintainer review

## Additional improvements considered

The requested phases exposed several useful follow-up investments beyond the
original checklist:

1. Archive and decode report descriptors with captures, so framing can be checked
   against the actual firmware instead of inferred from packet payloads.
2. Add a non-mutating `doctor` command covering physical interfaces, permissions,
   configuration, daemon reachability and foreground detection.
3. Add distribution packages with consistent binary, service, udev and completion
   installation; avoid installations containing mismatched old and new binaries.
4. Add CI across stable Rust and multiple Linux distributions with native HID and
   D-Bus dependencies, plus artifact provenance and release checksums.
5. Support explicit multi-mouse identities and multiple simultaneous sessions;
   empty device serials mean topology/identity changes need careful policy.
6. Add native compositor integrations for KDE/GNOME Wayland and dependable Wine
   process identification rather than guessing from XWayland or window titles.
7. Establish licensing files matching the existing workspace's MIT OR Apache-2.0
   metadata; the original README references an absent LICENSE file.

**Implemented choices: 1 and 2.** Descriptor provenance directly prevented a
real protocol mistake: interpreting configuration payload byte 0 as a report ID.
It also exposed another unknown feature-report path without sending anything to
it. Tests verify configuration and vendor layouts, truncated descriptors and
invalid sizes. Raw descriptors remain available alongside decoded layouts.

`doctor` is valuable because actual enumeration included virtual batteries with
the same VID/PID, permission failures, an existing root UPower process and a
Wayland session. It reports evidence without changing hardware settings, so
future reports can distinguish these conditions quickly. Its real X11 probe was
validated with `tools/x11-fixture.c` under Xvfb; native KDE Wayland explicitly
reports a disabled backend. No compositor support is inferred from DISPLAY.

The application lockfile is also now tracked. udev no longer grants everyone
HID/UHID writes; the UHID bridge is separately opt-in. Legacy UHID owns its fd
with RAII, checks write failures, and exits on SIGTERM. The tray releases its
initial HID handle and handles termination. The Plasma widget no longer starts
with a fabricated 83% value or a developer-specific executable path.

## Deliberate compatibility changes

- Existing commands and shorthand syntax remain. Control dispatch shares the
  typed execution path rather than repeated per-command raw packet construction.
- Unknown state fields are now `Option`/JSON null. Rust callers must account for
  optional charging and active-stage readback instead of relying on guessed data.
- c600/c601 remain supported IDs, but PID alone no longer chooses a connection
  mode or indicates charging. Multiple candidate mice require explicit selection.
- Direct HID clients hold a session lock; use `--daemon` when lorgard owns HID.
  Legacy installed binaries do not honor the new lock and must be managed by
  their owner. Existing services were not stopped or overwritten during work.
- Legacy tray/UHID integrations remain direct-only. Their old systemd service
  files are separate from the new user daemon and should not be enabled together.
- Configuration commands preserve extension fields and refuse unsafe edits to
  future versions. A failed hardware write may leave earlier profile settings
  applied; neither rollback nor readback is invented.

## Hardware and desktop validation limits

Physical c601 enumeration, feature state queries, both input interfaces,
descriptor retrieval, raw captures and daemon IPC were exercised. Query-only
experiments found reproducible `0x0f` association; data bytes remain unknown.
Later queries produced another vendor payload without a state response; the
daemon represented this as a timeout while preserving HID presence.

No c600 hardware, physical unplug/replug, physical DPI-button toggle, RGB visual
behavior or `0x27` experiment was independently verified in this session.
Reconnect, DPI-event handling, failure propagation and profile command ordering
are covered with mock transports/sessions. No hardware settings were changed:
the requested restoration preferences were unavailable, and existing legacy
writers also limit controlled setting experiments. Protocol confidence was not
raised to fill these gaps.

The real X11 backend resolved `x11-fixture` under Xvfb; a temporary configuration
with empty profiles selected `fixture` through the running daemon. Empty profiles
tested routing without changing mouse settings. KDE/GNOME Wayland support remains
explicitly unavailable, and X11 probing rejects a Wayland session.

The guide documents capture bounds, ordinal diff alignment, possible kernel
queue loss, partial writes and receiver-vs-radio ambiguity. None is hidden by a
successful CLI exit or a guessed protocol label.

The added native GTK4 app uses the shared profile model and a single daemon
request for draft application. The UI/UX skill informed clear Save/Apply
separation, keyboard focus, offline states and unsaved-change protection.
The official LORGAR Platform lighting UI was reviewed as a visual reference,
without copying unsupported controls into this project. GTK interaction tests
and visual checks ran under Xvfb; live physical setter behavior remains untested.

## Quality gate

Run the four commands in [the guide](control-stack.md#verification). The suite
includes actual CLI subprocess workflows and a daemon subprocess, bad IPC
versions/oversized frames/stalled clients, clean socket removal, stored hardware
evidence, protocol vectors, mocks, profile migration and atomic storage.
Final verification on 2026-09-05: formatting, strict workspace clippy, all 44
non-display tests and the optimized workspace build passed. The separately run
GTK display test also passed (45 tests exercised in total). Desktop-file and
udev-rule validation passed. The release app started its own daemon, read the
physical receiver's cached status, and cleanly stopped/reaped that child and
removed its socket when the window closed. No system service was installed or
replaced. Hardware setting changes were not used as a substitute for mock tests.

2026-09-09 model update: the native overview now renders a procedural 3D mesh
with depth buffering, smooth normals, orbit/zoom controls and OBJ/MTL export.
The renderer only redraws on change and uses a bounded software buffer, so the
preview adds no OpenGL context or background animation loop. Tests check camera
bounds, finite geometry, changed renders, generated-asset consistency and
headless export without overwriting. All 47 non-display tests, the GTK display
test, strict clippy, formatting and release build passed. The model was visually
inspected from default and dragged side views under Xvfb. It remains an explicitly
approximate visual reconstruction; protocol and hardware confidence are unchanged.

## Kernel driver hardening, 2026-10-01

`hid-lorgar` 2.0 replaces inferred defaults with evidence-backed state. Battery is
invalid until a recognized 64-byte response arrives; charging remains unknown;
link state distinguishes `unknown`, `online`, and query `unresponsive`; DPI,
polling, and lighting reads return `ENODATA` until observed or successfully sent.
The supported DPI bound is aligned with the confirmed userspace contract.

The physical DPI parser now consumes report ID, event code, and stage from bytes
0, 1, and 2 and updates the shared configuration state across HID interfaces.
Protocol parsing lives in a shared header exercised by a strict host regression
test. HID event callbacks only update spinlock-protected state; sysfs, power-supply,
and uevent notification runs on a workqueue. Feature writes are mutex-serialized
with a 20 ms cooldown, and asynchronous state queries become online only after a
valid response before their deadline.

The module queries every 60 seconds. A valid state response generated by the
30-second user daemon cadence postpones the kernel query, preventing duplicate
periodic traffic during normal daemon ownership. Sysfs setters remain root-owned;
the removed udev rule did not and could not grant per-attribute write ACLs.

`make check` validates shell syntax, selects the target kernel's Clang/GCC mode,
builds with `W=1`, runs the host protocol tests, and inspects the module metadata.
Rust formatting, strict clippy, all workspace tests, release build, and GTK smoke
also pass. Version 2.0.0 was loaded on the physical USB interfaces; that run
exposed and confirmed the virtual-UHID bind bug fixed in 2.0.1. A later live
capture established a second valid state prefix, `02 01`, carrying an 82% battery
value; 2.0.2 accepts both observed response shapes.

Version 2.0.2 was then built, loaded, and bound to all three physical receiver
interfaces. The live verifier reported `online` with a present battery, the Rust
CLI returned 82%, and UPower exported the native power-supply device at 82%.
After applying the input classification rule, Plasma displayed the device as a
mouse instead of choosing its composite keyboard interface. Receiver re-enumeration
was exercised during reloads. No claim is made yet for a physical DPI press or a
post-DKMS reboot.
