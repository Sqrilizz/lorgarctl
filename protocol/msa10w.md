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
| `0x27` | **UNCONFIRMED** | Probably DPI-stage RGB color configuration |
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
not established by this document. The parser conservatively recognizes the
observed 64-byte `01 01` shape and preserves other responses as unknown.

---

## 3. High-Confidence Vendor Input Events (`Usage Page: 0xff11`, Report `0x05`)

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

## 4. Unconfirmed Hypotheses (Do Not Use in Production Core)

### Command `0x27`: Set Stage RGB Color (Hypothesis)
```text
27 00 SS RR GG BB 00 00
```
- `SS`: Target DPI stage index (`0x00` .. `0x07`)
- `RR GG BB`: 24-bit RGB color values.

The former duplicate hypothesis entries for `0x21` and `0x82` conflicted with
the confirmed table and command descriptions above. Their existing confirmed
status is retained. This editorial correction does not confirm any additional
state fields.

---

## 5. Packet Capture Roadmap for Phase 2

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

## 6. Linux investigation, 2026-09-05

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

`0x27` remains UNCONFIRMED. Its function cannot be established from the existing
Linux state query. Use the vendor application with USBPcap/usbmon and change one
DPI-stage color at a time, repeat A/B/A, and record stage/color plus packet
direction. Confirm the command, stage numbering and reserved fields together.
Do not issue guessed `0x27` reports from this driver.

For `0x0f`, repeat the query-only experiment while independently recording
awake/asleep, receiver-only, cable and charge conditions. Change just one
condition per comparison and control competing clients. Preserve failures and
timeouts as evidence. No additional field is promoted to CONFIRMED here.
