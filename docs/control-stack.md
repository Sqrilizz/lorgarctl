# LORGAR Linux control stack

## Architecture

The existing workspace has been extended, not replaced. `lorgar-core` retains
its public device and builder helpers, now alongside typed `Command`,
`FeaturePacket` and `InputPacket` representations. `Msa10w::from_transport` and
`MockTransport` support testing without HID. The `Provider`/`Session` traits
make the reconnect state machine independently testable.

`protocol` describes only documented commands. `descriptor` reads HID report
layouts without interpreting unknown commands. `monitor` owns a configuration
handle and the ff11 handle belonging to the same physical USB parent.
`profiles`/`storage` own configuration; `capture` owns evidence; `ipc` owns
versioned local messages. `foreground` is isolated from the protocol layer.
`lorgard` combines these components; `lorgarctl` retains direct mode and the
original command spellings, tray and UHID integration. `lorgar-app` is a native
GTK4 frontend; background IPC/config work runs off the UI thread. It uses the
same typed profile and daemon API, not a second hardware implementation.

The optional `kernel/hid-lorgar` module adds Linux `power_supply` integration and
root-owned sysfs controls. Its state cache is evidence-based: battery comes only
from a valid 64-byte `0x82` response, DPI stage only from a successful setter or
the confirmed `05 0b SS 00 00` event, and other settings only from successful
setters. Missing readback is `ENODATA`, not a factory-default guess. Charging is
always unknown because no charging field has been confirmed.

The daemon selects one mouse. When multiple configuration devices exist,
selection is explicit through `--device`; c600 is not silently preferred over
c601. Unknown usage-page metadata permits the old interface-2 fallback, but a
conflicting known usage page never does. Virtual batteries are not controllers.
Explicit hidraw paths are validated before opening. A pinned path stays pinned;
use automatic discovery if hidraw numbering changes across reconnects.

## Build and user installation

Use a current stable Rust toolchain. Native dependencies on Debian/Ubuntu:

```sh
sudo apt install build-essential pkg-config libudev-dev libdbus-1-dev libgtk-4-dev x11-utils
cargo build --workspace --release --locked
install -Dm755 target/release/lorgarctl ~/.local/bin/lorgarctl
install -Dm755 target/release/lorgard ~/.local/bin/lorgard
install -Dm755 target/release/lorgar-app ~/.local/bin/lorgar-app
install -Dm644 desktop/io.github.Sqrilizz.LorgarControl.desktop ~/.local/share/applications/io.github.Sqrilizz.LorgarControl.desktop
```

Keep `~/.local/bin` in PATH. The lockfile is now included for reproducible
application dependencies. The GTK app requires GTK 4.8+ and Rust 1.92+; the
workspace was verified with Rust 1.97. Build only `-p lorgarctl -p lorgard` to
omit GTK on headless machines. The baseline README's 1.75 claim was not
supported by its evolving dependencies.

Install the supplied mouse rule (the legacy filename is supported):

```sh
sudo install -m644 udev/99-lorgar-msa10w.rules /etc/udev/rules.d/99-lorgar-msa10w.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw
sudo udevadm trigger --subsystem-match=input
```

Reconnect the mouse/receiver, then run `lorgarctl doctor`. The rule grants the
active local login session access through `uaccess`, not world-writable HID
access. Its explicit builtin also handles the legacy rule's position after
systemd's seat rule. Headless/SSH users without a local active seat need an
administrator-managed group ACL; running the daemon as root is unnecessary.
The input rule retains the MSA10W extra-button keyboard capability while also
marking that composite interface as part of the mouse. This prevents UPower and
Plasma from giving the peripheral battery a keyboard icon.

The optional legacy `upower` command additionally needs `/dev/uhid` access.
Only if using that integration, install `udev/70-lorgar-uhid-optional.rules`.
This permission allows virtual HID creation generally and is intentionally not
part of normal mouse control. UPower desktop recognition is not guaranteed by
successfully creating a virtual battery device.

```sh
install -Dm644 systemd/lorgard.service ~/.config/systemd/user/lorgard.service
systemctl --user daemon-reload
systemctl --user enable --now lorgard
lorgarctl --daemon status
```

Stop previously installed tray/UPower/battery-polling services when switching to
`lorgard`; older installed binaries do not participate in the new ownership
lock. Neither root services nor other users are controlled by a per-user lock.
Do not run competing hardware writers during reverse engineering.
This repository does not automatically stop, install or enable system services.

If `hid-lorgar` is installed, its battery query interval is 60 seconds. Every
valid state response, including one requested by `lorgard` at its normal 30-second
cadence, postpones the module's next query. Kernel-originated writes are serialized
with the device's 20 ms cooldown. Sysfs setters require administrator access;
desktop controls continue through the single user daemon and the hidraw `uaccess`
rule.

## Native settings app

Run `lorgar-app`. It provides overview, sensitivity, lighting, profiles and
automatic-profile pages with keyboard-accessible native controls. The layout
and purple accents were informed by [LORGAR Platform](https://lorgar.com/platform),
but this is an independent Linux frontend, not the official software.

The app connects to the user daemon. If none is available it starts `lorgard`
beside its executable (or from PATH), and stops only that owned child on normal
exit. Use the user service for persistent monitoring. `--no-start-daemon` disables
auto-start; `--socket` attaches to a specific existing socket. `--config` overrides
profile storage. Without that override the app follows the daemon's reported
config path; an explicit mismatch is an error.

Unchecked stages and “Leave unchanged” fields generate no writes. The example
DPI values are editable drafts, never claimed readback. “Save profile” writes
configuration only. “Apply to mouse” submits the entire validated draft in one
IPC request; clean saved profiles also become the last profile for optional
reconnect restoration. Unsaved drafts are not persisted as a reconnect target.
An Apply success means feature writes succeeded, not hardware readback or an
atomic hardware transaction. Existing application rules may later select another
profile. No macros, button remapping, brightness, per-stage RGB or other unknown
hardware commands are exposed as working features.

Profiles remain editable offline. Save detects concurrent changes to the same
profile; it does not overwrite CLI edits silently. Reconnect reloads the file;
reset or reload the profile before retrying a conflicted edit. Unsaved profile
and application-preference changes trigger a close confirmation, and unrelated
profile saves preserve in-progress application rules. Referenced profiles cannot
be deleted. Future config versions stay read-only.

Manual settings work on native Wayland and X11. Automatic application switching
is X11-only as described below; the app clearly displays this limitation.

The overview shows the product photo by default. The unfinished 3D MSA10W mesh
is available only with `--experimental-3d`, not as the normal app presentation. Drag to orbit;
focus the preview and scroll to zoom, or use the visible Zoom/Top/Reset controls.
Arrow keys rotate, +/- zoom, and Home resets. It renders only on changes and
does not continuously animate or require a browser, network model service or
OpenGL context. Keyboard controls and no automatic motion follow the UI/UX
accessibility guidance. The small indicator can preview the draft color; this
is not device readback or a simulation of hardware lighting effects. Export the
mesh using `lorgar-app --export-model NEW_DIRECTORY`; see [model provenance and
limits](../assets/README.md). No new hardware commands are introduced.

## CLI and output

Normal commands remain `status` (`info`, `hub`), `devices`, `battery`, `dpi`,
`polling`, `rgb`, `debug`, `tray` and `upower`, including old shorthand arguments.
`tray`'s historical `daemon` alias still means the tray, not `lorgard`.
Raw debug commands and legacy tray/UHID integrations stay direct-only.

`--json` works on controls, status, devices, profiles, diagnostics and capture
analysis. `monitor --json` emits one compact JSON event per line. Debug logging
uses stderr. Unknown fields are explicit; successful writes report
`hardware_readback_verified: false`. A profile application means its feature
writes succeeded, not that unknown state fields verified every setting.

Exit codes: 0 success, 1 other I/O/transport failure, 2 invalid input/config,
3 mouse/configuration interface absent, 4 permission denied, 5 device/config
lock busy, 6 state query timeout. Daemon errors preserve these codes. Clap
argument errors use code 2 and stderr. Normal runtime errors have no backtrace.

```sh
lorgarctl completions bash > ~/.local/share/bash-completion/completions/lorgarctl
lorgarctl completions zsh
lorgarctl completions fish
```

Create the destination directory first. Elvish and PowerShell are also supported
by the completion generator. Run each subcommand's `--help` for arguments.

## Profiles and configuration

Default path: `$XDG_CONFIG_HOME/lorgar/config.json`, falling back to
`~/.config/lorgar/config.json`. `lorgarctl profile path` prints it. Both binaries
accept `--config`; when overriding the daemon path, use the same CLI path for
profile edits. `--daemon profile apply` reads the daemon's configuration.

```sh
lorgarctl profile create gaming
lorgarctl profile set gaming --dpi-stage 0=800
lorgarctl profile set gaming --dpi-stage 1=1600 --active-stage 1
lorgarctl profile set gaming --polling 1000 --rgb static --color ff5500
lorgarctl profile show gaming --json
lorgarctl profile apply gaming
lorgarctl profile list
lorgarctl profile delete gaming
```

Omitted settings stay unchanged. All commands in a profile are validated before
the first hardware write. A mid-application disconnect reports partial progress;
there is no invented hardware transaction or rollback of unknown previous values.
The last successfully sent profile is remembered. Restore is opt-in through
`restore_on_reconnect` or `lorgard --restore-last-profile`.

Version 1 example:

```json
{
  "version": 1,
  "profiles": {
    "desktop": { "polling": 500 },
    "competitive": { "dpi_stages": { "0": 800 }, "active_stage": 0, "polling": 1000 },
    "genshin": { "polling": 1000, "rgb": "static", "color": "ff5500" }
  },
  "last_profile": "desktop",
  "restore_on_reconnect": false,
  "applications": {
    "default": "desktop",
    "rules": [
      { "process": "cs2", "profile": "competitive" },
      { "process": "genshinimpact.exe", "profile": "genshin" }
    ]
  }
}
```

Unknown fields at config/profile/application/rule levels survive edits. Missing
version or version 0 migrates to 1; future versions are never overwritten or
applied. Future files whose known fields still decode can be inspected read-only.
Invalid files are errors, never silently reset. Writers lock before read/modify/
save, write a private temporary file in the same directory, fsync it, rename
atomically and fsync the directory. A crash leaves the previous or complete new
configuration, not a truncated destination. An abandoned temporary file may
remain after SIGKILL but is not used as configuration. Concurrent edits return a
busy error rather than overwrite another writer. Profile names cannot escape
the configuration namespace.

The daemon reloads once a second. Invalid reloads keep the last valid in-memory
configuration and produce a warning. Deleting profiles referenced by application
rules is rejected. Rules are exact, case-sensitive executable basenames; first
match wins, then the default. Failure to detect focus does not mean desktop
focus: it preserves the current profile and reports the detection error.

## Foreground detection limits

The implemented backend is X11 EWMH: `_NET_ACTIVE_WINDOW`, `_NET_WM_PID` and
`WM_CLIENT_MACHINE`, then `/proc/PID/exe`. It uses `xprop` with bounded child
lifetimes and checks that the process belongs to the local host. A synthetic
EWMH client under Xvfb was used to exercise the real backend. It requires an
EWMH window manager and applications supplying those properties.

Wine applications only match `.exe` names if that is the actual executable
basename exposed through `/proc`; no reliable Windows-process inference is
claimed. Inspect `lorgarctl doctor`'s `active_process` before writing a rule.

There is no generic KDE/GNOME Wayland foreground backend in this release.
`--foreground auto` disables switching on Wayland, including this development
machine's KDE session. `--foreground x11` refuses a Wayland session to avoid
reporting XWayland's stale focus as global focus. `--foreground none` disables
application switching everywhere. This limitation does not affect manual
profiles, the daemon, monitoring or standard mouse input. A compositor-specific
backend can implement `ForegroundBackend` without changing HID/protocol code.

## Monitoring, caching and IPC

`lorgarctl monitor` keeps running across USB removal and reattachment. Retry
discovery occurs at most once per second; the main loop sleeps between bounded
reads. It reads both paired interfaces and preserves unknown vendor events and
other report IDs. Known DPI events are marked high confidence, not confirmed.
Device reconnect clears observed battery/stage and applied-profile caches.
Battery includes an observation timestamp and is invalidated on query timeout.
PID alone never indicates charging; receiver presence does not prove an awake
or connected wireless mouse. A timeout is distinct from USB disconnection.

`lorgard` owns its HID session. Use `lorgarctl --daemon ...` while it runs;
direct mode intentionally returns busy. SIGINT/SIGTERM releases resources and
removes the socket. The socket lives at `$XDG_RUNTIME_DIR/lorgar/lorgard.sock`,
or an owned private `/tmp/lorgar-UID/lorgar` fallback if XDG_RUNTIME_DIR is absent.
Runtime directories must be owned and 0700, and the socket is 0600. Stale sockets
are removed only under a daemon lock; unrelated files are never replaced.

IPC version 1 uses a four-byte little-endian JSON byte length followed by UTF-8
JSON. Frames are bounded to 1 MiB and requests have a total 200 ms read deadline.
A connection handles one request/response. Example request:

```json
{"version":1,"action":"execute","command":{"command":"set_polling","rate":"Hz1000"}}
```

Actions: `status`, `devices`, `execute`, `apply_profile` (`name`), `apply_draft`
(`profile`, validated in full before writes), `events`
(`after` sequence number), and `reload`. Only typed confirmed commands are
accepted; unknown raw writes stay in the explicit direct debug utility.
Responses contain version, success/error/code, optional state/config path, devices and
events. The ring retains 2048 events and returns up to 16 per page. `gap: true`
reports overwritten history or a reset cursor, allowing slow consumers to
detect missing evidence. Streaming CLI monitoring pages this endpoint; capture
stops with an explicit error on a gap. Daemon process restarts require reconnecting
the CLI; HID unplug/replug does not.

## Captures and investigations

```sh
lorgarctl capture before.bin --duration 10 --label passive
lorgarctl capture after.bin --duration 10 --query-state --query-interval-ms 1000
lorgarctl inspect-capture after.bin --json > after.jsonl
lorgarctl diff-captures before.bin after.bin --json
```

Capture is passive by default. `--query-state` only sends confirmed `0x82`
queries. With `--daemon`, capture observes the daemon's normal query traffic;
it is not a passive USB bus tap. Physical interactions/settings must be recorded
separately, and speculative feature writes are not generated automatically.

Binary format: `LORGAR`, byte `01`, newline; then length-prefixed JSON header
and timestamped event records (u32 little-endian lengths). Each raw packet is
an exact byte vector with direction and VID/PID/interface/usage/path metadata.
Input report IDs are preserved; configuration input has no report ID. Own
feature-output buffers contain HIDAPI's synthetic leading zero. The header
archives raw descriptors plus decoded report layouts; it does not imply support
for commands carried by those layouts. Files are created exclusively, mode 0600,
and flushed to disk per event. Existing captures are never overwritten.

`inspect-capture` and `diff-captures` accept binary captures or their JSONL
exports. Unknown bytes are never discarded. Malformed/truncated records produce
an error, not a silently successful partial analysis. Analysis is bounded to
256 MiB per input and 128 KiB per record; use finite captures for long experiments.
Timestamps are host read times, not bus timestamps. Kernel queue overflow and
traffic from other applications cannot be reconstructed by this user-space tool.

Diff groups by VID/PID/interface/usage/direction/framing/length, compares ordinal
packets, reports offsets and old/new values, full-packet repetitions and multiset
differences. Insertions may shift ordinal alignment; counts are reported
independently. Counter/sum8/xor8 candidates require at least four varying samples
and remain explicitly unconfirmed. A correlation is never promoted to a command.

See [protocol investigation](../protocol/msa10w.md#6-linux-investigation-2026-09-05)
for actual results, confidence and remaining experiments. No `0x27` reports were
sent. The repeated `0x0f` association is evidence, not a decoded field definition.

## Verification

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --workspace --release
xvfb-run -a cargo test -p lorgar-app ui_smoke -- --ignored
make -C kernel clean
make -C kernel W=1
bash -n kernel/reload.sh
make -C kernel live-check
```

Tests cover confirmed vectors, malformed/unknown packets, descriptor framing,
mock HID failures, reconnect timing, cache invalidation, partial applications,
profile migration/serialization, atomic replacement, locking, CLI workflows,
IPC limits and a real disconnected daemon process with clean SIGTERM shutdown.
The optional display test exercises GTK navigation, profile load/save requests,
DPI/polling/RGB editing, invalid color handling, resets and unsaved application
rules without sending hardware commands. It was run, not merely added. The app
was also visually inspected under Xvfb. The recorded hardware exports are
regression inputs. See the maintainer
review for the final validation record and deliberately unverified hardware paths.

Primary desktop references: [EWMH application properties](https://specifications.freedesktop.org/wm/latest/ar01s05.html)
and [systemd seat ACL rules](https://github.com/systemd/systemd/blob/main/rules.d/73-seat-late.rules.in).
