# Sidewinder Force Feedback 2 — Virtual Joystick Driver

**Date:** 2026-04-09
**Status:** Approved

## Overview

A from-scratch Rust implementation of a virtual joystick driver for Windows 11 that makes a Microsoft Sidewinder Force Feedback 2 joystick appear as a modern HID game controller with full force feedback support. No external dependencies like vJoy — the virtual device is a custom UMDF2 driver.

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Language | Rust | Systems-level control, safety, modern tooling |
| Virtual device | Custom UMDF2 driver | No external dependencies, full control over HID descriptors |
| Physical device I/O | Windows HID API via `windows` crate | Idiomatic on Windows, tight dependency tree |
| FFB scope | Full HID PID spec (12 effect types) | Maximum game compatibility, the hardware supports it |
| UI | System tray app | Visual feedback without GUI complexity |
| Mapping | 1:1 passthrough + optional TOML overrides | Zero-config default, power-user flexibility |
| Async runtime | Tokio | Clean concurrent task management |

## Architecture

### Workspace Layout

```
sidewinder/
├── crates/
│   ├── sidewinder-driver/    # UMDF2 virtual HID driver
│   ├── sidewinder-hid/       # Physical device HID communication
│   ├── sidewinder-app/       # System tray app + orchestration
│   └── sidewinder-diag/      # Diagnostic TUI app
├── config/
│   └── default.toml          # Default mapping config
└── Cargo.toml                # Workspace root
```

### Data Flow

```
Physical Sidewinder FF2
    ↕ (Windows HID API — read input reports, write FFB output reports)
sidewinder-hid
    ↕ (in-process tokio channels)
sidewinder-app (orchestration, config, tray UI)
    ↕ (IOCTL via DeviceIoControl)
sidewinder-driver (UMDF2 virtual HID device)
    ↕ (HID class driver)
Games (DirectInput / Windows.Gaming.Input)
```

### FFB Flow (game → physical device)

```
Game calls IDirectInputEffect::Start()
    → Windows HID stack sends SET_REPORT to sidewinder-driver
    → Driver queues FFB packet in ring buffer
    → sidewinder-app reads via IOCTL_SIDEWINDER_GET_FFB
    → sidewinder-hid writes effect to physical Sidewinder FF2
```

## Crate: `sidewinder-hid`

Owns all communication with the physical Sidewinder Force Feedback 2 joystick.

### Responsibilities

- Enumerate HID devices, find Sidewinder FF2 by VID/PID (`045E:001B`)
- Open device via Windows HID API (`SetupDiGetClassDevs`, `CreateFile`, `HidD_GetPreparsedData`)
- Parse HID report descriptors to discover axis/button/hat layout
- Read input reports on a background thread (~1ms polling interval)
- Write FFB output reports back to the physical device (SET_REPORT)
- Expose a clean Rust API

### Key Types

```rust
pub struct InputState {
    pub axes: [i16; 8],      // X, Y, throttle, rudder, slider, etc.
    pub buttons: u16,         // 9 buttons as bitfield
    pub pov: PovDirection,    // 8-way hat + center
}

pub enum FfbEffect {
    ConstantForce { magnitude: i16, direction: u16 },
    Periodic { waveform: Waveform, magnitude: i16, period: u32, phase: u16 },
    Condition { condition_type: ConditionType, coefficients: [ConditionParams; 2] },
    Ramp { start: i16, end: i16, duration: u32 },
    // ... all 12 PID effect types
}

pub struct SidewinderDevice { /* ... */ }
impl SidewinderDevice {
    pub fn enumerate() -> Result<Vec<DeviceInfo>>;
    pub fn open(device: &DeviceInfo) -> Result<Self>;
    pub fn poll(&self) -> Result<InputState>;
    pub fn send_ffb_effect(&self, slot: u8, effect: &FfbEffect) -> Result<()>;
    pub fn stop_effect(&self, slot: u8) -> Result<()>;
    pub fn stop_all_effects(&self) -> Result<()>;
}
```

### Threading Model

A background reader thread continuously reads HID input reports and pushes `InputState` into a lock-free ring buffer. `poll()` reads the latest state without blocking.

## Crate: `sidewinder-driver`

UMDF2 virtual HID device driver using `windows-drivers-rs`.

### Responsibilities

- Register as a UMDF2 driver
- Create a virtual HID device with a report descriptor matching the Sidewinder FF2's capabilities (8 axes, 9 buttons, 1 POV hat)
- Accept input state from user-mode app via `IOCTL_SIDEWINDER_UPDATE_STATE`
- Implement HID PID report descriptors for full force feedback
- Capture FFB SET_REPORT packets from games, expose via `IOCTL_SIDEWINDER_GET_FFB`

### IOCTLs

| IOCTL | Direction | Purpose |
|---|---|---|
| `UPDATE_STATE` | App → Driver | Push latest axes/buttons/hat state |
| `GET_FFB` | Driver → App | Retrieve queued FFB effect packets |
| `GET_FFB_STATUS` | Driver → App | Query FFB pool state |
| `SET_DEVICE_CONFIG` | App → Driver | Set axis count, button count, HID descriptor |

### HID Report Descriptor

The driver presents a fully compliant HID descriptor:
- Usage Page: Generic Desktop / Joystick
- Input reports: 8 axes (16-bit each), 9 buttons, 1 4-bit POV hat
- PID output reports: Effect definitions, gain control, effect operations (start/stop/free)
- PID feature reports: Block Load, Pool Report, Effect State

### FFB Support

Full HID PID spec — 12 effect types:
- Basic: Constant Force, Ramp
- Periodic: Square, Sine, Triangle, Sawtooth Up, Sawtooth Down
- Conditional: Spring, Damper, Inertia, Friction
- Custom: Custom Force

Up to 10 simultaneous effects with pool management.

### Build Requirements

- Windows Driver Kit (WDK)
- `windows-drivers-rs` crate
- Test-signed for development (requires `bcdedit /set testsigning on`)
- EV code-signed + Microsoft attestation signing for distribution

## Crate: `sidewinder-app`

User-facing system tray application that bridges physical and virtual devices.

### Tokio Tasks

| Task | Rate | Purpose |
|---|---|---|
| `input_bridge_loop` | ~1ms | Poll physical device → apply mapping/curves → push to driver IOCTL |
| `ffb_bridge_loop` | Event-driven | Read FFB packets from driver → translate → send to physical device |
| `config_watcher` | On file change | Watch TOML config, hot-reload mapping/deadzone/curve changes |
| `tray_ui` | Event-driven | System tray icon, status display, gain slider, enable/disable FFB |

### Configuration (`sidewinder.toml`)

```toml
[device]
vid = "045E"
pid = "001B"

[ffb]
global_gain = 0.75
enabled = true

[axes.x]
deadzone = 0.02
curve = "linear"    # "linear" | "quadratic" | "cubic" | "s-curve"

[axes.throttle]
inverted = true
deadzone = 0.0
curve = "linear"

[buttons]
# Optional remapping: physical = virtual
# 7 = 1
```

Default behavior is 1:1 passthrough — config file is optional.

### System Tray

- Green/yellow/red icon for connection status
- Right-click menu: FFB gain slider, enable/disable FFB, open config file, quit
- Tooltip shows device name and current state

### Error Handling

- Physical device disconnect: yellow icon, retry every 2 seconds
- Driver not installed: red icon, notification with install instructions
- Config parse error: log warning, continue with previous config

## Crate: `sidewinder-diag`

Diagnostic TUI for real-time event visualization using `ratatui` + `crossterm`.

### Display

- Live axis bars with numeric values
- Button state indicators
- POV hat direction
- FFB effect slots with type, parameters, and status
- Scrollable timestamped event log

### Modes

- `sidewinder-diag physical` — raw physical device input only (no driver needed)
- `sidewinder-diag virtual` — virtual device state via driver IOCTLs
- `sidewinder-diag full` — both sides + FFB flow between them

### Implementation

- Uses same `sidewinder-hid` crate and driver IOCTLs as the main app
- Events streamed via `tokio::sync::broadcast` channels
- Standalone binary, no dependency on `sidewinder-app`

## Key Dependencies

| Crate | Purpose |
|---|---|
| `windows` | Windows API bindings (HID, SetupAPI, COM) |
| `windows-drivers` | UMDF2 driver framework |
| `tokio` | Async runtime |
| `serde` + `toml` | Config parsing |
| `tracing` | Structured logging |
| `notify` | File watcher for config hot-reload |
| `ratatui` + `crossterm` | Diagnostic TUI |

## Testing Strategy

| Layer | Strategy |
|---|---|
| `sidewinder-hid` | Integration tests with real hardware. Mock `HidTransport` trait for unit tests. |
| `sidewinder-driver` | WDK test framework + manual testing with `joy.cpl` and DirectInput test apps. |
| `sidewinder-app` | Bridge logic is pure data transformation — unit testable without hardware. |
| `sidewinder-diag` | Manual testing with physical hardware. |
| End-to-end | Physical stick → virtual device in `joy.cpl` → FFB in a real game (IL-2, DCS World). |

## Development Workflow

1. Enable test signing: `bcdedit /set testsigning on`
2. Build driver with WDK, deploy with `pnputil`
3. Run app, confirm virtual device in `joy.cpl`
4. Use `sidewinder-diag` and USB analyzers for debugging
