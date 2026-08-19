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

#### Response Structure (64-byte Input Report)
The device responds asynchronously with a 64-byte packet:
```text
01 01 05 80 01 00 00 55 01 19 40 1f 10 27 e0 2e 00 ... 00
                     ↑
                 Byte 7: Battery percentage (0x55 = 85%)
```
- **Byte 7**: Battery percentage level (e.g. `0x55` = 85%).

---

## 3. High-Confidence Vendor Input Events (`Usage Page: 0xff11`, Report `0x05`)

Interface `0xff11` emits 4-byte Vendor Input Reports on physical interactions.

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

### Command `0x21`: Set Active DPI Stage (Hypothesis)
```text
21 00 00 SS 00 00 00 00
```
- `SS`: Target active DPI stage index.

### Command `0x27`: Set Stage RGB Color (Hypothesis)
```text
27 00 SS RR GG BB 00 00
```
- `SS`: Target DPI stage index (`0x00` .. `0x07`)
- `RR GG BB`: 24-bit RGB color values.

### Command `0x82`: Query Configuration / State (Hypothesis)
```text
82 00 00 00 00 00 00 00
```
- Triggers a 64-byte Input Report response. Structure remains unparsed.

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
