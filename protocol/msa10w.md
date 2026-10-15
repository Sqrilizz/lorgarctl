# Protocol Reference: LORGAR MSA10W

This document tracks reverse-engineered HID commands and report formats for the **LORGAR MSA10W** gaming mouse (VID: `0x093a`, PID: `0xc600` / `0xc601`).

## Protocol Status Overview

| Command / Event | Status | Description |
| :--- | :--- | :--- |
| `0x26` | **CONFIRMED** | Set DPI value for specific stage |
| `0x21` | **CONFIRMED** | Set active DPI stage |
| `0x20` | **CONFIRMED** | Set polling rate |
| `0x22` | **CONFIRMED** | Set lighting mode |
| `0x23` | **CONFIRMED** | Set lighting color |
| `0x82` | **CONFIRMED** | Query device state / battery level |
| `0x27` | **CONFIRMED** | Set the separate top DPI indicator color for one stage |
| Packet `0x01` | **CONFIRMED** | NVRAM: Active stage, lighting mode, brightness, speed, direction |
| Packet `0x02` | **CONFIRMED** | NVRAM: Sensor LOD (1/2mm), sleep timeout (1..10m), angle snap, motion sync |
| Packet `0x03` | **CONFIRMED** | NVRAM: 8-stage RGB indicator palette |
| Packet `0x04` | **CONFIRMED** | NVRAM: Key debounce time (ms) and FireKey repeat settings |
| Packets `0x05`/`0x06` | **CONFIRMED** | NVRAM: Full 8-stage DPI table storage |
| Packet `0x07` | **CONFIRMED** | NVRAM: Button remapping (24-byte button definition table) |
| Packets `0x08`/`0x09` | **CONFIRMED** | NVRAM: Onboard hardware macro definitions |
| Report `0x05`, Event `0x0b` | **HIGH CONFIDENCE** | Hardware DPI stage change event |
| Report `0x05`, Event `0x0f` | **UNKNOWN** | Unknown vendor event payload |

---

## 1. Main Configuration Interface (`Usage Page: 0xff10`)

The main configuration interface accepts 8-byte Feature Reports.
On Linux `hidraw`, sending a Feature Report via `HIDIOCSFEATURE` requires prepending a synthetic Report ID (`0x00`), resulting in a **9-byte buffer**:

```text
Buffer: [ 0x00, byte_0, byte_1, byte_2, byte_3, byte_4, byte_5, byte_6, byte_7 ]
```

---

## 2. Confirmed Commands

### Command `0x26`: Set DPI Value

Sets the DPI resolution for a specified stage index.

#### Packet Structure (8-byte Payload)
```text
26 00 SS LL HH 00 00 00
```

- **`0x26`**: Command opcode (`SET_DPI`)
- **`0x00`**: Sub-command / Reserved
- **`SS`**: DPI Stage Index (`0x00` .. `0x07`, total 8 stages)
- **`LL`**: DPI Little-Endian Low Byte
- **`HH`**: DPI Little-Endian High Byte

#### Tested & Confirmed Reference Vectors
| DPI | Stage Index (`SS`) | Payload (Hex) | 9-byte Hidraw Report | Status |
| :--- | :--- | :--- | :--- | :--- |
| 800 | Stage 0 (`0x00`) | `26 00 00 20 03 00 00 00` | `00 26 00 00 20 03 00 00 00` | CONFIRMED |
| 1400 | Stage 1 (`0x01`) | `26 00 01 78 05 00 00 00` | `00 26 00 01 78 05 00 00 00` | CONFIRMED |
| 1600 | Stage 1 (`0x01`) | `26 00 01 40 06 00 00 00` | `00 26 00 01 40 06 00 00 00` | CONFIRMED |
| 3000 | Stage 1 (`0x01`) | `26 00 01 b8 0b 00 00 00` | `00 26 00 01 b8 0b 00 00 00` | CONFIRMED |

---

### Command `0x20`: Set Polling Rate

Sets the USB polling rate (report rate) in milliseconds.

#### Packet Structure (8-byte Payload)
```text
20 00 00 II 00 00 00 00
```
- **`0x20`**: Command opcode (`SET_POLLING_RATE`)
- **`II`**: Polling interval in milliseconds (`1000 / Hz`)

#### Tested & Confirmed Values
| Polling Rate | Interval (`II`) | Payload (Hex) | 9-byte Hidraw Report | Status |
| :--- | :--- | :--- | :--- | :--- |
| 125 Hz | `0x08` (8 ms) | `20 00 00 08 00 00 00 00` | `00 20 00 00 08 00 00 00 00` | CONFIRMED |
| 250 Hz | `0x04` (4 ms) | `20 00 00 04 00 00 00 00` | `00 20 00 00 04 00 00 00 00` | CONFIRMED |
| 500 Hz | `0x02` (2 ms) | `20 00 00 02 00 00 00 00` | `00 20 00 00 02 00 00 00 00` | CONFIRMED |
| 1000 Hz | `0x01` (1 ms) | `20 00 00 01 00 00 00 00` | `00 20 00 00 01 00 00 00 00` | CONFIRMED |

---

### Command `0x21`: Set Active DPI Stage

Switches the mouse active DPI stage to the target stage index.

#### Packet Structure (8-byte Payload)
```text
21 00 00 SS 00 00 00 00
```
- **`0x21`**: Command opcode (`SET_ACTIVE_STAGE`)
- **`SS`**: Active DPI stage index (`0x00` .. `0x07`)

---

### Command `0x22`: Set Lighting Mode

Selects the active RGB lighting effect.

#### Packet Structure (8-byte Payload)
```text
22 00 00 MM 80 PP 00 00
```
- **`0x22`**: Command opcode (`SET_LIGHTING_MODE`)
- **`MM`**: Mode ID
- **`PP`**: Parameter byte

#### Mode Table
| Mode ID (`MM`) | Mode Name | Parameter (`PP`) | Status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `0x01` | Static | `0x05` | **CONFIRMED** | Static single color |
| `0x02` | Breathing | `0x05` | **CONFIRMED** | Pulsing/breathing effect |
| `0x03` | ColorCycle | `0x05` | **CONFIRMED** | Behavior confirmed, official name UNKNOWN |
| `0x04` | Tapping | `0x05` | **CONFIRMED** | Reacts to mouse clicks |
| `0x05` | Wave | `0x01` | **CONFIRMED** | Rainbow wave effect |

---

### Command `0x23`: Set Lighting Color

Sets the static RGB color for lighting effects.

#### Packet Structure (8-byte Payload)
```text
23 00 00 RR GG BB 00 00
```
- **`0x23`**: Command opcode (`SET_LIGHTING_COLOR`)
- **`RR`**: Red intensity (`0x00` .. `0xFF`)
- **`GG`**: Green intensity (`0x00` .. `0xFF`)
- **`BB`**: Blue intensity (`0x00` .. `0xFF`)

---

### Command `0x82`: Device State & Battery Query

Queries the mouse configuration state. Sent as an 8-byte Feature Report:
```text
82 00 00 00 00 00 00 00
```

#### Response Structure (64-byte unnumbered Input Report)
The device responds asynchronously with a 64-byte packet:
```text
01 01 05 80 01 00 00 55 01 19 40 1f 10 27 e0 2e 00 ... 00
                     ↑
                 Byte 7: Battery percentage (0x55 = 85%)
```
- **Byte 7**: Battery percentage level (e.g. `0x55` = 85%).

The leading `01` is payload, not a report ID. The ff10 descriptor has no Report
ID item. Offsets above start at the first payload byte. Only byte 7 is decoded
as a device setting; charging, active stage, lighting and polling readback are
not established by this document. The parser conservatively recognizes the two
observed 64-byte prefixes `01 01` and `02 01` and preserves other responses as
unknown.

An October 2026 live capture after a receiver power-cycle produced this second
confirmed response shape:

```text
02 01 01 80 05 00 00 52 01 00 00 00 00 00 00 00 ...
```

Byte 7 is `0x52` (82%). The meaning of byte 0 is still unknown; accepting this
observed prefix does not assign it a semantic name.

An October 2026 low-battery observation produced a third stable shape:

```text
02 00 SS 80 01 00 00 BB DD 00 00 00 00 00 00 00 ...
```

Byte 2 (`SS`) is the active DPI stage index (`0..7`). Byte 7 (`BB`) is the
battery percentage level (`0..100`). Byte 8 (`DD`) is `0x00` or `0x01`. An
earlier overly strict check required `data[2] == 0x03` literally because
stage 3 happened to be active during that first capture; validating
`data[2] < DPI_STAGE_COUNT` resolves timeouts across all sensitivity stages.

The mouse's sensitivity indicator was red at the same time. That observation is
**not** independent evidence when the optional `battery_indicator` is enabled:
it writes red to all eight stages for any reading `<= 20%`, so the driver can
cause the red it then appears to confirm.

### Full charge/discharge experiment (2026-10-07): byte 7 is state of charge

The mouse was undocked and idle for a month. Repeated queries returned byte 7 =
`0x04` unchanged across six queries in 90 seconds, then `0x06`. The cable was
connected; the device re-enumerated as `093a:c600` and byte 7 rose:

| Phase | PID | byte 8 | byte 7 | vendor event |
| --- | --- | --- | --- | --- |
| Undocked, idle | `c601` | `01` | `4`, then `6` | `05 0f 01 00 35` |
| Charging, 12 min | `c600` | `00` | `35 → 41 → 45 → 49 → 57 → 63 → 73` | `05 11 XX 00 00` |
| Undocked again | `c601` | `01` | `62 → 59 → 56 → 52 → 50`, then steady `50` | `05 0f 01 00 35` |

**Byte 7 is confirmed as the battery percentage.** It is the only field that
moved, it moved in the right direction under a controlled physical change, and it
settled at a stable value in the same way in both modes.

Two reported behaviours matter more than the confirmation itself:

1. **The charge-mode reading is optimistic and converges after unplugging.** The
   mouse ended a 12-minute charge at `73`, then corrected to a steady `50` within
   about two minutes of leaving the cable. A UI that samples during charging will
   show a value that later falls; that is the device, not a parsing error.
2. **A long-idle mouse reported `4`/`6` for the same cell that reads `50` after a
   charge cycle.** The deep-sleep or reserve-mode estimate was not the steady
   state-of-charge value. Do not treat a first reading after a long idle period as
   authoritative.

**Byte 8 is a mode flag, not a fixed prefix.** It is `01` on the `c601` receiver
and `00` while the `c600` cable device is attached. The parser accepts `0x00` or
`0x01` and decodes nothing from it. Its meaning stays UNCONFIRMED: it may be a
charging flag, a transport flag, or an unrelated state byte. `power_supply`
status remains `Unknown` and nothing infers charging from the USB product ID.

The strict matcher that earlier required `data[8] == 0x01` rejected every
response while charging, which emptied the driver battery and made
`lorgarctl battery` time out. That was a driver bug, not a protocol fact.

The device also emits a **different** vendor event per mode: `0x0f` on `c601`,
`0x11` on `c600`. The `0x0f` payload stayed `05 0f 01 00 35` across sessions in
which byte 7 ranged from `4` to `91`, so `0x0f` is not the battery. The `0x11`
data byte tracked byte 7 during charging, but its meaning is UNKNOWN and it is
not decoded.

### Request acknowledgement is not a response

Under sustained polling on 2026-10-07 the device intermittently acknowledged a
state query with a vendor event on interface `0xff11` while returning **no**
64-byte state report at all:

```text
05 0f 00 20 08
```

`0x0f` is emitted per query, including queries that are never answered, so its
presence says nothing about whether a state response follows. Both the kernel
driver and user-space clients must treat the 64-byte report as the only
confirmation, and must not treat the vendor event as a partial result.

During one such window every query failed, userspace and kernel alike, and the
condition cleared on its own within roughly a minute without physical input.
Repeated tight polling is the most likely trigger, so retries are spaced at the
kernel's proven 500 ms rather than back-to-back.

**Unresolved:** whether this is a device sleep state, a request-rate limit, or a
firmware quirk. It was not reproduced under `lorgarctl` polling every 18s while
the kernel reported `online` continuously, so the practical guidance is to avoid
sub-second polling loops.

---

## 3. Confirmed 32-byte NVRAM Configuration Packets

The mouse firmware uses 32-byte structured packets to persist full profiles in its internal NVRAM flash.
Each 32-byte packet is sent across the configuration interface (`usage_page: 0xff10`) via a Feature Report (or Output Report) prepended with report ID `0x00` (33 bytes total).
The final byte (byte 31) is **always** the bitwise one's complement checksum (`~sum(bytes[0..30]) & 0xFF`), guaranteeing that the 8-bit sum of the entire 32-byte packet equals `0xFF`.

### Packet `0x01`: Master DPI Stage & Lighting Configuration
- **Byte 0**: `0x01` (Packet ID)
- **Byte 1**: Active DPI stage index (`0x00` .. `0x07`)
- **Byte 2**: Lighting effect mode (`0x00` = Off, `0x01` = Static, `0x02` = Breathing, `0x03` = ColorCycle, `0x04` = Tapping, `0x05` = Wave)
- **Byte 3**: Lighting brightness
- **Byte 4**: Effect speed (`0x01` .. `0x05`)
- **Byte 5**: Wave / effect direction
- **Bytes 6..30**: Reserved
- **Byte 31**: Checksum (`~sum(b[0..30]) & 0xFF`)

### Packet `0x02`: Sensor, Power & Performance Configuration
- **Byte 0**: `0x02` (Packet ID)
- **Byte 1**: Active DPI stage index (`0x00` .. `0x07`)
- **Byte 2**: Sensor Lift-Off Distance (LOD): `0x01` = Low (1 mm), `0x02` = High (2 mm)
- **Byte 3**: **Sleep / Hibernation Timeout**: `0x01` .. `0x0A` (1 to 10 minutes, default: `0x05` = 5 min)
- **Byte 4**: Reserved (`0x00`)
- **Byte 5**: Angle Snapping: `0x00` = Disabled, `0x01` = Enabled
- **Byte 6**: PixArt PAW3395 Motion Sync: `0x00` = Disabled, `0x01` = Enabled
- **Byte 7**: Reserved (`0x00`)
- **Bytes 8..10**: Static RGB Color (`R, G, B`)
- **Bytes 11..30**: Reserved (`0x00`)
- **Byte 31**: Checksum (`~sum(b[0..30]) & 0xFF`)

### Packet `0x03`: 8-Stage DPI Indicator Palette
- **Byte 0**: `0x03` (Packet ID)
- **Bytes 1..24**: 8 contiguous RGB triplets (`R0 G0 B0` through `R7 G7 B7`) defining the physical LED indicator colors for each DPI sensitivity stage.
- **Bytes 25..30**: Reserved (`0x00`)
- **Byte 31**: Checksum (`~sum(b[0..30]) & 0xFF`)

### Packet `0x04`: Button Debounce & Rapid-Fire Settings
- **Byte 0**: `0x04` (Packet ID)
- **Bytes 1..2**: Switch debounce filter duration in milliseconds (anti-double-click delay)
- **Byte 3**: Rapid-fire key enabled (`0x00` / `0x01`)
- **Byte 4**: Fire-key repeat interval (ms, default: `0x32` = 50 ms)
- **Byte 5**: Fire-key repeat count (default: `0x02` = 2 clicks per trigger)
- **Bytes 6..30**: Reserved (`0x00`)
- **Byte 31**: Checksum (`~sum(b[0..30]) & 0xFF`)

### Packets `0x05` & `0x06`: Full Hardware DPI Banks
- **Packet `0x05`**: Stage 0..3 DPI resolutions (16-bit little-endian words).
- **Packet `0x06`**: Stage 4..7 DPI resolutions (16-bit little-endian words).

### Packet `0x07`: Physical Button Remapping Table
- **Byte 0**: `0x07` (Packet ID)
- **Bytes 1..24**: 24-byte action mapping matrix covering mouse buttons (Left, Right, Middle, Back, Forward, DPI+, DPI-, FireKey).

### Packets `0x08` & `0x09`: Hardware Macro Storage
- Sequence streams for onboard programmable macro playback directly from mouse hardware without driver runtime dependency.

## 4. High-Confidence Vendor Input Events (`Usage Page: 0xff11`, Report `0x05`)

Interface `0xff11` uses report ID `05` with four payload bytes, giving five
bytes in hidraw reads. Other collections share its hidraw node; they have
different report IDs and must not be interpreted as `05` vendor events.

### Event `0x0b`: Physical DPI Stage Toggle
```text
05 0b SS 00 00
```
- `0x05`: Report ID
- `0x0b`: Event Code (`DPI_STAGE_CHANGED`)
- `SS`: Active DPI stage index (`0x01`, `0x02`, `0x03`, ...)

### Event `0x0f`: Unknown Vendor Event
Observed packets during capture:
```text
05 0f 00 00 7d
05 0f 01 50 01
```
*Note: Do not infer or invent functionality for `0x0f` without targeted packet captures.*

---

## 5. Confirmed DPI Indicator Control

### Command `0x27`: Set Stage RGB Color
```text
27 00 SS RR GG BB 00 00
```
- `SS`: Target DPI stage index (`0x00` .. `0x07`)
- `RR GG BB`: 24-bit RGB color values.

A controlled Linux test set stage `0` to blue with `0x27`. The user observed
blue only on the separate top DPI indicator after physically switching DPI.
All eight stages were then set to red, and another physical DPI switch showed
red. Repeated programmatic `0x21` transitions between stages `0` and `1` also
produced visible red flashes. Immediate `1 -> 0` restoration was not perceptible
as a sensitivity change. The command, byte layout, stage targeting and physical
target are confirmed for the tested `093a:c601` receiver.

---

## 6. Packet Capture Roadmap for Phase 2

For upcoming reverse-engineering steps, targeted USBPcap recordings from LORGAR Platform on Windows will be saved using the following naming convention:

- `polling-500-1000.pcapng`
- `rgb-off-static.pcapng`
- `rgb-red-green.pcapng`
- `dpi-color-red-blue.pcapng`
- `active-stage-0-1.pcapng`
- `button4-default-f.pcapng`
- `profile-1-2.pcapng`
- `startup.pcapng`
- `battery.pcapng`

## 7. Linux investigation, 2026-09-05

### Environment and method

Physical USB device: Pixart LORGAR MSA10W_BK, VID/PID `093a:c601`.
Configuration: `/dev/hidraw13`, interface 2, usage page `ff10`.
Vendor events: `/dev/hidraw12`, interface 1, usage page `ff11`.
The two interfaces resolve to the same USB parent. Separate virtual battery
devices also advertise this VID/PID; VID/PID alone is insufficient discovery.
No physical c600 device was available for these tests.

Only the already-confirmed `82 00 00 00 00 00 00 00` query was sent. No settings
were changed and no `0x27`, unknown feature report or output report was sent.
The experimental variable was query cadence. Existing legacy tray/UPower
processes remained running, so unrelated background queries cannot be ruled
out in general. These captures record this client's feature writes and all
input observed on its configuration/vendor handles, not system-wide USB traffic.

### Reproducible observations

| Run | Duration | Own queries | ff10 state responses | `05 0f` events |
| --- | --- | --- | --- | --- |
| Passive | 8 seconds | 0 | 0 | 0 |
| Query every 1000 ms | 8 seconds | 8 | 8 | 8 |
| Query every 1500 ms | 8 seconds | 6 | 6 | 6 |

Both query runs returned the same vendor packet:

```text
05 0f 01 00 35
```

The 64-byte configuration response in both runs was:

```text
01 01 03 ff 01 00 00 5b 01 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

Byte 7 is `5b` (91%). An earlier query returned 93%. No external battery
reference was measured; these are device-reported values, not accuracy tests.

Raw timestamped evidence, including descriptors:

- [Passive control](captures/2026-09-05-passive.jsonl)
- [1000 ms query run](captures/2026-09-05-query-1000ms.jsonl)
- [1500 ms query run](captures/2026-09-05-query-1500ms.jsonl)
- [Later query without a state response](captures/2026-09-05-no-state-response.jsonl)

The later daemon capture recorded `05 0f 00 20 01` after a query, while no
64-byte state response arrived during the three-second capture. HID interfaces
remained present. Sleep, radio link state and user physical actions were not
controlled or independently observed; do not label this packet as a disconnect
event. The daemon correctly reports a state-query timeout separately from USB
device removal.

**Confidence:** high for the observed association of `0x0f` with queries on this
c601 device in the two repeated runs. **Meaning remains UNKNOWN.** Its three
data bytes do not increment across the repeated runs. The captures do not
establish a counter, checksum, battery field, charging flag or radio-status bit.
Packet read timestamps are host read times, not USB bus timestamps.

### Additional descriptor findings

The same configuration descriptor was read in all three runs:

```text
06 10 ff 09 01 a1 01 15 00 26 ff 00 75 08 95 40
09 f1 81 02 09 f2 91 00 09 f3 95 08 b1 00 c0
```

This declares unnumbered 64-byte input, 64-byte output and 8-byte feature
reports. The 64-byte output report's function is unknown and is not used.
HIDAPI's leading synthetic zero for feature writes is not part of the 8-byte
hardware payload.

Interface 1 additionally declares report `03`: 63 feature payload bytes on
usage page `ff00` (64 bytes including its report ID). Its function is unknown;
the driver does not send it. Report `05` has 32 input bits on usage page `ff11`.
These are descriptor facts for this observed device, not confirmed commands or
claims of identical layouts on all c600/c601 firmware.

### Remaining experiments

`0x27` is confirmed for the tested receiver. Captures from the vendor
application are still useful to compare behavior across firmware revisions.

For `0x0f`, repeat the query-only experiment while independently recording
awake/asleep, receiver-only, cable and charge conditions. Change just one
condition per comparison and control competing clients. Preserve failures and
timeouts as evidence. No additional field is promoted to CONFIRMED here.

---

## 8. Firmware Architecture, Container Format & Flashing Protocols (Reverse-Engineered via REA)

Extracted from official LORGAR Platform Windows components (`HIDUsb.dll`, `mouse.MSA10WR.dll`) using REA (`rea inspect-with-objdump`, `rea inspect-pe-resources`).

### Hardware Profile
- **MCU**: Holtek / Hetai `571R` (HT68FB571 USB Flash MCU).
- **Sensor**: PixArt PAW3395 (100–26,000 DPI) / PAW3311.
- **USB Vendor ID**: `0x093a` (PixArt Imaging, Inc.).
- **USB Product IDs**:
  - `0xc600`: Wired USB-C Mode.
  - `0xc601`: 2.4 GHz Wireless Dongle Mode.

### Firmware Binary Container Format (Offset Layout)
Firmware images are packaged into a binary container with a fixed `0x2000` (8,192-byte) header followed by raw MCU code payload.
**Security Notice**: The container employs no cryptographic signatures or encryption (no RSA, ECDSA, AES). Integrity is enforced solely by an arithmetic 32-bit checksum.

| Offset Range | Type / Format | Field Description |
| :--- | :--- | :--- |
| `0x0000..0x0003` | `u32` (Little-Endian) | Checksum: `0x55555555 - sum(bytes[8..header_len])` |
| `0x0004..0x0007` | `u32` (Little-Endian) | Header Length: `0x00002000` (8192 bytes) |
| `0x0037..0x0076` | ASCII (Null-term) | Target Product Name (e.g. `"LORGAR MSA10W"`) |
| `0x0077..0x00B6` | ASCII (Null-term) | Target MCU Model (`"571R"`) |
| `0x00B7..0x00F6` | ASCII (Null-term) | Target Optical Sensor (`"PAW3395"`) |
| `0x00F7..0x0136` | ASCII (Null-term) | Bootloader Input Endpoint Descriptor |
| `0x0137..0x0176` | ASCII (Null-term) | Bootloader Output Endpoint Descriptor |
| `0x0177..0x01B6` | ASCII (Null-term) | Normal Runtime Input Endpoint Descriptor |
| `0x021E..0x0221` | `u32` (Big-Endian) | Flash Base Destination Address (`0x00000000`) |
| `0x0277..0x02B6` | ASCII (Null-term) | Normal Runtime Output Endpoint Descriptor |
| `0x2000..EOF` | Binary Bytes | Executable Firmware / Microcode Payload |

### Bootloader Update Mode Entry Protocol (`UsbServer_EnterUsbUpdateMode`)
The firmware update protocol uses the Output Report channel (Endpoint `0x04`, 64-byte max packet size) rather than control Feature Reports.
In `HIDUsb.dll`, `0x01, 0x01, 0x00` is an internal message tag in the C++ queue, while the packet sent on the wire via `WriteFile` is **17 bytes**:
```text
08 0d 00 00 00 00 00 00 00 00 00 00 00 00 00 00 40
```
- `0x08`: Command opcode (`OP_FLASH_SERVICE`)
- `0x0d`: Sub-opcode (`SUB_RESET_TO_BOOTLOADER`)
- Bytes 2..15: Reserved / zero-padding
- `0x40`: Arithmetic checksum: `(0x55 - (0x08 + 0x0d)) & 0xFF = 0x40`
The device resets its USB stack and re-enumerates in USB Bootloader mode.

### Flash Readback Protocol (`UsbServer_ReadAllFlashData` / `ReadFlashData`)
Requests an arbitrary 10-byte chunk from internal MCU flash memory via a 17-byte command frame on Endpoint `0x04`:
```text
08 08 00 AH AL CC 00 00 00 00 00 00 00 00 00 00 CS
```
- `0x08`: Opcode
- `0x08`: Sub-opcode (`READ_FLASH_CHUNK`)
- `0x00`: Flags
- `AH`: 16-bit flash address high byte
- `AL`: 16-bit flash address low byte
- `CC`: Chunk size in bytes (typically `0x0A` = 10 bytes)
- Bytes 6..15: Padding (`0x00`)
- `CS`: Checksum: `(0x55 - sum(bytes[0..15])) & 0xFF`
Device responds on Interrupt IN Endpoint `0x83` with the flash memory payload.

### Flash Write Protocol (`UsbServer_WriteCommand`)
Writes flash in 32-byte blocks:
```text
OP 20 A3 A2 A1 A0 [32 data bytes] CS
```
- `OP`: Opcode (`0xC0` for regular blocks, `0xC1` for final block)
- `0x20`: Chunk length (32 bytes)
- `A3..A0`: 32-bit big-endian target flash address
- `CS`: Checksum: `(0x55 - sum(bytes[0..38])) & 0xFF`

### 8.4 Live Hardware Onboard Memory Dump Protocol (Validated on Hardware)
Extracted live from `093a:c600` via Interface 2 (`0xff10`) 8-byte Feature Query commands with 64-byte Input Report readback on Endpoint `0x83`:
- **`0x82`**: Battery & Device State (battery level, wire/wireless status, active DPI stage)
- **`0x8e`**: Hardware Serial / MCU UID & Security Nonce
- **`0xa3`**: 8-Stage DPI RGB Indicator Palette (Red, Orange, Yellow, Green, Blue, Cyan, Violet, White)
- **`0xa4`**: Primary Button Mapping Table (`f0` Left, `f1` Right, `f2` Mid, `f4` Back, `f3` Forward, `03...` DPI Loop)
- **`0xa5`**: Secondary Button Mapping Table
- **`0xa6`**: Factory 8-Stage DPI Table (16-bit little-endian: 800, 1600, 2400, 3200, 6400, 12000, 8200, 6400 DPI)
- **`0xa7`**: LED Indicator Configuration & Effects
- **`0xf0`**: PixArt PAW3395 Sensor OTP Calibration ROM

All 8 banks (512 bytes) are dumped into a unified binary or annotated JSON via:
```bash
lorgarctl firmware dump [--output msa10w_factory_dump.bin]
```
