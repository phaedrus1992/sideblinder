# Microsoft SideWinder Force Feedback 2 — Hardware Specification

**Source:** Microsoft SideWinder FF2 driver CD (circa 2002), files:
- `sw13w2k.in_` / `sw13w98.in_` — Windows INF (expanded)
- `GcKernel_win2k.sys` — kernel filter driver (decompiled with Ghidra 12.0.4)
- `SWPIDFLT2.DLL` — DirectInput PID filter object (strings)

---

## 1. USB Identity

| Field              | Value                            |
|--------------------|----------------------------------|
| Vendor ID (VID)    | `0x045E` (Microsoft)             |
| Product ID (PID)   | `0x001B`                         |
| Device class       | HID (class `0x03`)               |
| INF class GUID     | `{745a17a0-74d3-11d0-b6fe-00a0c90f57da}` (HIDClass) |

---

## 2. HID Input Report Layout

The device sends a single input report (no report-ID prefix byte in the data
stream; the kernel driver strips it before passing to user space).

### 2.1 Report size

| Source                      | Size        |
|-----------------------------|-------------|
| `GCK_IP_Init` buffer alloc  | `*(ushort *)(ext + 0x60)` bytes |
| Observed on real hardware   | **12 bytes** (11 data + 1 trailing pad) |
| Minimum parseable           | **11 bytes** |

### 2.2 Byte map

All multi-byte values are **little-endian**.  Axis values are **signed i16**
in the range −32 768 … +32 767.

| Bytes | Field            | Type   | Notes                                      |
|-------|------------------|--------|--------------------------------------------|
| 0–1   | X axis           | i16 LE | Left/right. Full deflection = ±32 767.     |
| 2–3   | Y axis           | i16 LE | Forward/back. Full deflection = ±32 767.   |
| 4–5   | Rz / twist axis  | i16 LE | Handle twist. Full deflection = ±32 767.   |
| 6–7   | Slider/throttle  | i16 LE | Throttle lever. Full deflection = ±32 767. |
| 8–9   | Buttons          | u16 LE | Bitfield; bit *n* = button *n+1*. 9 buttons used (bits 0–8). |
| 10    | POV hat          | u8     | Low nibble: 0=N, 1=NE, 2=E, 3=SE, 4=S, 5=SW, 6=W, 7=NW; 0xFF/0x0F = center. |
| 11    | Padding          | u8     | Always 0x00; ignored.                      |

**Driver type tags (from `InputItemFactory` switch):**

| Type ID | Class           | Axis slot |
|---------|-----------------|-----------|
| 1       | `CAxesInput`    | X (0) and Y (1) |
| 5       | `CPOVInput`     | POV hat   |
| 6       | `CThrottleInput`| Slider/throttle (3) |
| 7       | `CRudderInput`  | Rz/twist (2) |
| 9       | `CButtonsInput` | Buttons bitfield |

`CRudderInput` maps to **Rz (twist, bytes 4–5)**, not to a rudder pedal axis.
`CThrottleInput` maps to **Slider (bytes 6–7)**. Both are confirmed by the
`InputItemFactory` constructor chain.

### 2.3 POV hat null state

The HID spec allows 0xFF (or any nibble > 7) as the "not pressed" sentinel.
The device uses **0xFF** for center; any value 8–0xFF maps to `Center`.

---

## 3. Force Feedback

### 3.1 Architecture

The driver uses two kernel components:

- **`GcKernel.sys`** — lower filter on the USB HID device.  Intercepts input
  reports, forwards them to `CDeviceFilter::ProcessInput`, and passes FFB
  output reports through `GCKF_InternalWriteFile` → `IrpMajorWrite` down to
  the physical device.
- **`SWPIDFLT2.DLL`** — user-space DirectInput PID (Physical Interface Device)
  filter.  Translates DI effect parameters into HID PID output reports and
  delivers them to the kernel via IOCTL.

### 3.2 FFB output report format

> **Important:** The Sidewinder FF2 physical device does **not** use standard
> HID PID output reports.  The HID PID report set (Set Effect, Set Envelope,
> etc.) is the *virtual device* format exposed to games via DirectInput.
> `GcKernel.sys` translates those into a proprietary wire format before writing
> to the physical USB device.  The report IDs below are physical-device IDs,
> not the virtual-device IDs used by `sidewinder-hid/src/ffb.rs`.

`_GCKF_OnForceFeedbackChangeNotification` and `_GCKF_GetForceFeedbackData`
both use **0x11 (17) bytes** for the FFB output report buffer.
`GCKF_InternalWriteFile` writes **0x10 (16) bytes** per call (the data
payload; the report-ID byte may or may not be included depending on context).

From `_GCKF_OnForceFeedbackChangeNotification`, two output reports are
constructed and written:

**Report 1 — device gain / master enable (report ID 0x01):**

| Byte | Value                 | Meaning                              |
|------|-----------------------|--------------------------------------|
| 0    | `0x01`                | Report ID                            |
| 1    | `0x01`                | Enable device output                 |
| 2    | `0x01`                | (flag)                               |
| 3    | *(zero)*              | —                                    |
| 4    | gain (0–0xFF)         | `(param[0x0D] / 100) * 0xFF / 100`  |
| 5    | *(zero)*              | —                                    |
| 6    | `0x84`                | (flag / actuator enable mask)        |
| 7    | `0xFF`                | (flag)                               |
| 8–15 | `0x00`               | Padding                              |

**Report 2 — spring/condition (report ID 0x0D):**

| Byte | Value                 | Meaning                              |
|------|-----------------------|--------------------------------------|
| 0    | `0x0D`                | Report ID                            |
| 1    | gain (0–0xFF)         | `(param[0x0F] / 100) * 0xFF / 100`  |
| 2–15 | `0x00`               | Padding                              |

### 3.3 FFB capability attributes (from INF registry data)

`HKLM\…\OEMForceFeedback\Attributes` (binary, 12 bytes):

| Bytes | Value      | Decoded                    |
|-------|------------|----------------------------|
| 0–3   | 0x00000000 | Capability flags: none     |
| 4–7   | 0x000F4240 | Min update period: 1 000 000 µs (1 s) |
| 8–11  | 0x000F4240 | Max update period: 1 000 000 µs (1 s) |

### 3.4 Supported FFB effects

All effects use **HID Usage Page 0x0F** (Physical Interface Device).

| Effect           | HID Usage | DI type           | Envelope | Trigger |
|------------------|-----------|-------------------|----------|---------|
| Constant Force   | `0x26`    | CONSTANTFORCE     | Yes      | Yes     |
| Ramp Force       | `0x27`    | RAMPFORCE         | Yes      | Yes     |
| Square Wave      | `0x30`    | PERIODIC          | Yes      | Yes     |
| Sine Wave        | `0x31`    | PERIODIC          | Yes      | Yes     |
| Triangle Wave    | `0x32`    | PERIODIC          | Yes      | Yes     |
| Sawtooth Up      | `0x33`    | PERIODIC          | Yes      | Yes     |
| Sawtooth Down    | `0x34`    | PERIODIC          | Yes      | Yes     |
| Spring           | `0x40`    | CONDITIONBLOCK    | No       | No      |
| Damper           | `0x41`    | CONDITIONBLOCK    | No       | No      |
| Inertia          | `0x42`    | CONDITIONBLOCK    | No       | No      |
| Friction         | `0x43`    | CONDITIONBLOCK    | No       | No      |
| Custom Force     | `0x28`    | CUSTOMFORCE       | Yes      | Yes     |

**Static parameters supported by all effects** (from `DIEFFECTATTRIBUTES`):

- DURATION, GAIN, AXES, DIRECTION, TYPESPECIFICPARAMS, STARTDELAY
- Envelope effects additionally: TRIGGERBUTTON, TRIGGERREPEATINTERVAL, ENVELOPE
- Condition effects: no TRIGGERBUTTON / ENVELOPE

**FFB axes:** bit field `0x00000020` in effect `dwCoords` — XY plane only
(bits 0x20 = flag for the two-axis constraint).

### 3.5 PID filter CLSID

```
{db11d351-3bf6-4f2c-a82b-b26cb9676d2b}   SWPIDFilterCLSID
```

---

## 4. Driver Architecture Notes

### 4.1 Kernel filter driver (`GcKernel.sys`)

```
USB Host → HidUsb.sys → [GcKernel.sys filter] → HID class driver
```

`GcKernel` is loaded as an **FDO** (Function Device Object) rather than a
lower filter due to an NT bug noted in the INF comments:

```ini
; Current NT bug breaks this (even though technically that's what we should be.
; Loading as an FDO works on NT because the HID PDO's are RAW.
```

The filter intercepts:
- **Read IRPs**: polls via `_GCK_IP_FullTimePoll` → `IoBuildAsynchronousFsdRequest`
  with `IRP_MJ_READ` → completion callback `_GCK_IP_ReadComplete` →
  `_GCKF_IncomingInputReports` → `CDeviceFilter::ProcessInput`.
- **Write IRPs**: FFB output reports from user space → `GCKF_InternalWriteFile`
  → `IoBuildSynchronousFsdRequest` with `IRP_MJ_WRITE` down to device.

### 4.2 HID descriptor retrieval sequence

From `_GCK_GetHidInformation`:

1. `IOCTL_HID_GET_COLLECTION_INFORMATION` (`0x000B01A8`) — get preparsed data
   size.
2. Allocate pool buffer of that size.
3. `IOCTL_HID_GET_REPORT_DESCRIPTOR` (`0x000B0193`) — fill preparsed data.
4. `HidP_GetCaps` — extract caps (stored at `ext + 0x5C`).

### 4.3 Input processing pipeline

```
Raw report bytes
    → _GCKF_IncomingInputReports         (IRP completion)
    → CDeviceFilter::ProcessInput        (spinlock held)
        → CControlItemCollectionImpl::ReadFromReport
            → HidP_GetUsageValue / HidP_GetUsages per control item
        → CheckTriggers                  (button/macro actions)
        → Jog                            (timing)
```

---

## 5. OEM Joystick Capabilities (`OEMData`)

Registry value `OEMData` (binary): `20 00 00 10 06 00 00 00`

| Bytes | Value        | Meaning                                   |
|-------|--------------|-------------------------------------------|
| 0–3   | `0x10000020` | dword1: `num_items=0x20`, `flags=0x1000`  |
| 4–7   | `0x00000006` | dword2: axis count = 6 (X, Y, Rz, Slider, + 2 unused) |

---

## 6. Virtual Devices

`GcKernel.sys` also exposes two virtual HID devices (used for macro/LED
features):

- **`GCK_VKBD`** — virtual keyboard (`SideWinderVirtualKeyboard`)
- **`GCK_VMOU`** — virtual mouse (`SideWinderVirtualMouse`)

These appear on the `SWVBENUM` bus. They are unrelated to joystick I/O and are
irrelevant to force-feedback bridging.

---

## 7. Key Constants for Implementation

```rust
// USB identity
pub const VID: u16 = 0x045E;
pub const PID: u16 = 0x001B;

// HID input report
pub const INPUT_REPORT_MIN_LEN: usize = 11;
pub const INPUT_REPORT_DEVICE_LEN: usize = 12; // device sends 12, we need 11

// Axis byte offsets (little-endian i16)
pub const AXIS_X_OFFSET: usize      = 0;  // bytes 0-1
pub const AXIS_Y_OFFSET: usize      = 2;  // bytes 2-3
pub const AXIS_RZ_OFFSET: usize     = 4;  // bytes 4-5 (twist)
pub const AXIS_SLIDER_OFFSET: usize = 6;  // bytes 6-7 (throttle)
pub const BUTTONS_OFFSET: usize     = 8;  // bytes 8-9 (u16, 9 bits)
pub const POV_OFFSET: usize         = 10; // byte 10

// Axis range
pub const AXIS_MIN: i16 = i16::MIN; // -32768
pub const AXIS_MAX: i16 = i16::MAX; // +32767

// POV hat encoding (nibble value → direction)
// 0=N, 1=NE, 2=E, 3=SE, 4=S, 5=SW, 6=W, 7=NW, 0xFF=center (any value >7)
pub const POV_CENTER_SENTINEL: u8 = 0xFF;

// FFB output report size (driver internal)
pub const FFB_REPORT_LEN: usize = 16; // 0x10 bytes written per GCKF_InternalWriteFile call

// HID Usage Page for FFB effects
pub const HID_USAGE_PAGE_PID: u16 = 0x0F;

// FFB effect HID usages (Usage Page 0x0F)
pub const EFFECT_CONSTANT_FORCE: u16 = 0x26;
pub const EFFECT_RAMP_FORCE:     u16 = 0x27;
pub const EFFECT_CUSTOM_FORCE:   u16 = 0x28;
pub const EFFECT_SQUARE:         u16 = 0x30;
pub const EFFECT_SINE:           u16 = 0x31;
pub const EFFECT_TRIANGLE:       u16 = 0x32;
pub const EFFECT_SAWTOOTH_UP:    u16 = 0x33;
pub const EFFECT_SAWTOOTH_DOWN:  u16 = 0x34;
pub const EFFECT_SPRING:         u16 = 0x40;
pub const EFFECT_DAMPER:         u16 = 0x41;
pub const EFFECT_INERTIA:        u16 = 0x42;
pub const EFFECT_FRICTION:       u16 = 0x43;
```
