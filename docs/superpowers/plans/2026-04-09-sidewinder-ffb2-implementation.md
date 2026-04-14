# Sidewinder FFB2 Virtual Joystick — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust application + UMDF2 driver that makes a Sidewinder Force Feedback 2 appear as a modern virtual HID joystick on Windows 11 with full force feedback support.

**Architecture:** Four-crate Cargo workspace. `sidewinder-hid` reads the physical joystick via Windows HID API. `sidewinder-driver` is a UMDF2 virtual HID device (vhidmini2 pattern). `sidewinder-app` bridges them with Tokio. `sidewinder-diag` is a ratatui diagnostic TUI.

**Tech Stack:** Rust, `windows` crate (HID/SetupAPI), `windows-drivers-rs` (UMDF2), Tokio, ratatui, serde/toml, tracing.

**Spec:** `docs/superpowers/specs/2026-04-09-sidewinder-ffb2-driver-design.md`

---

## File Structure

```
sidewinder/
├── Cargo.toml                              # Workspace root
├── config/
│   └── default.toml                        # Default device config
├── crates/
│   ├── sidewinder-hid/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                      # Crate root, re-exports
│   │       ├── device.rs                   # SidewinderDevice: open, close, lifecycle
│   │       ├── enumerate.rs                # HID device enumeration via SetupAPI
│   │       ├── input.rs                    # Input report parsing, InputState type
│   │       ├── ffb.rs                      # FFB effect types and output report building
│   │       └── hid_transport.rs            # Low-level HID read/write (trait + impl)
│   ├── sidewinder-driver/
│   │   ├── Cargo.toml
│   │   ├── Makefile.toml                   # WDK build/package tasks
│   │   ├── sidewinder.inx                  # Driver INF template
│   │   └── src/
│   │       ├── lib.rs                      # DriverEntry, EvtDriverDeviceAdd
│   │       ├── hid_descriptor.rs           # HID report descriptor bytes (input + PID)
│   │       ├── ioctl.rs                    # Custom IOCTL definitions and handlers
│   │       ├── input_report.rs             # Input report construction from IOCTL data
│   │       └── ffb_handler.rs              # FFB SET_REPORT capture and queuing
│   ├── sidewinder-app/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs                     # Entry point, Tokio runtime, task spawning
│   │       ├── bridge.rs                   # Input bridge loop + FFB bridge loop
│   │       ├── config.rs                   # TOML config parsing, hot-reload
│   │       ├── mapping.rs                  # Axis mapping, deadzones, curves
│   │       ├── tray.rs                     # System tray icon and menu
│   │       └── driver_ipc.rs              # DeviceIoControl wrapper for driver IOCTLs
│   └── sidewinder-diag/
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs                     # Entry point, CLI arg parsing, mode selection
│           ├── ui.rs                       # Ratatui layout and rendering
│           ├── widgets.rs                  # Axis bars, button grid, FFB slot display
│           └── event_log.rs               # Timestamped scrollable event log
```

---

## Task 1: Cargo Workspace Setup

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/sidewinder-hid/Cargo.toml`
- Create: `crates/sidewinder-hid/src/lib.rs`
- Create: `crates/sidewinder-app/Cargo.toml`
- Create: `crates/sidewinder-app/src/main.rs`
- Create: `crates/sidewinder-diag/Cargo.toml`
- Create: `crates/sidewinder-diag/src/main.rs`
- Create: `.gitignore`
- Create: `rust-toolchain.toml`

Note: `sidewinder-driver` is excluded from the default workspace members because it requires the WDK toolchain. It will be set up in its own task.

- [ ] **Step 1: Create workspace root Cargo.toml**

```toml
[workspace]
resolver = "2"
members = [
    "crates/sidewinder-hid",
    "crates/sidewinder-app",
    "crates/sidewinder-diag",
]
# sidewinder-driver excluded: requires WDK toolchain, built separately

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"
repository = "https://github.com/ranger/sidewinder"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
thiserror = "2"
windows = { version = "0.61", features = [
    "Win32_Devices_HumanInterfaceDevice",
    "Win32_Devices_DeviceAndDriverInstallation",
    "Win32_Storage_FileSystem",
    "Win32_Foundation",
    "Win32_System_IO",
    "Win32_Security",
] }
```

- [ ] **Step 2: Create sidewinder-hid crate**

`crates/sidewinder-hid/Cargo.toml`:
```toml
[package]
name = "sidewinder-hid"
version.workspace = true
edition.workspace = true

[dependencies]
windows.workspace = true
tracing.workspace = true
thiserror.workspace = true
```

`crates/sidewinder-hid/src/lib.rs`:
```rust
pub mod device;
pub mod enumerate;
pub mod ffb;
pub mod hid_transport;
pub mod input;
```

- [ ] **Step 3: Create sidewinder-app crate**

`crates/sidewinder-app/Cargo.toml`:
```toml
[package]
name = "sidewinder-app"
version.workspace = true
edition.workspace = true

[dependencies]
sidewinder-hid = { path = "../sidewinder-hid" }
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
serde.workspace = true
toml.workspace = true
thiserror.workspace = true
windows.workspace = true
notify = "7"
```

`crates/sidewinder-app/src/main.rs`:
```rust
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("sidewinder=debug")
        .init();

    tracing::info!("Sidewinder FFB2 starting...");
}
```

- [ ] **Step 4: Create sidewinder-diag crate**

`crates/sidewinder-diag/Cargo.toml`:
```toml
[package]
name = "sidewinder-diag"
version.workspace = true
edition.workspace = true

[dependencies]
sidewinder-hid = { path = "../sidewinder-hid" }
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
thiserror.workspace = true
windows.workspace = true
ratatui = "0.29"
crossterm = "0.28"
clap = { version = "4", features = ["derive"] }
```

`crates/sidewinder-diag/src/main.rs`:
```rust
use clap::Parser;

#[derive(Parser)]
#[command(name = "sidewinder-diag", about = "Sidewinder FFB2 Diagnostics")]
struct Cli {
    #[command(subcommand)]
    mode: Mode,
}

#[derive(clap::Subcommand)]
enum Mode {
    /// Show raw physical device input
    Physical,
    /// Show virtual device state
    Virtual,
    /// Show full pipeline with FFB flow
    Full,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter("sidewinder=debug")
        .init();

    tracing::info!("Sidewinder diagnostics starting in {:?} mode", cli.mode);
}
```

- [ ] **Step 5: Create .gitignore and rust-toolchain.toml**

`.gitignore`:
```
/target
*.pdb
*.sys
*.dll
*.cat
*.cer
```

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
```

- [ ] **Step 6: Create stub module files for sidewinder-hid**

Create empty module files so the crate compiles:

`crates/sidewinder-hid/src/enumerate.rs`:
```rust
//! HID device enumeration via Windows SetupAPI.
```

`crates/sidewinder-hid/src/device.rs`:
```rust
//! SidewinderDevice lifecycle: open, close, poll, send FFB.
```

`crates/sidewinder-hid/src/input.rs`:
```rust
//! Input report parsing and InputState type.
```

`crates/sidewinder-hid/src/ffb.rs`:
```rust
//! Force feedback effect types and HID output report construction.
```

`crates/sidewinder-hid/src/hid_transport.rs`:
```rust
//! Low-level HID read/write operations.
```

- [ ] **Step 7: Verify workspace builds**

Run: `cargo build`
Expected: All three crates compile with no errors.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: initialize Cargo workspace with hid, app, and diag crates"
```

---

## Task 2: Shared Types — InputState and FfbEffect

**Files:**
- Modify: `crates/sidewinder-hid/src/input.rs`
- Modify: `crates/sidewinder-hid/src/ffb.rs`
- Create: `crates/sidewinder-hid/tests/types_test.rs`

- [ ] **Step 1: Write tests for InputState**

`crates/sidewinder-hid/tests/types_test.rs`:
```rust
use sidewinder_hid::input::{InputState, PovDirection};

#[test]
fn input_state_default_is_centered() {
    let state = InputState::default();
    assert_eq!(state.axes, [0i16; 8]);
    assert_eq!(state.buttons, 0u16);
    assert_eq!(state.pov, PovDirection::Center);
}

#[test]
fn pov_direction_from_degrees() {
    assert_eq!(PovDirection::from_degrees(0), PovDirection::North);
    assert_eq!(PovDirection::from_degrees(90), PovDirection::East);
    assert_eq!(PovDirection::from_degrees(180), PovDirection::South);
    assert_eq!(PovDirection::from_degrees(270), PovDirection::West);
    assert_eq!(PovDirection::from_degrees(45), PovDirection::NorthEast);
    assert_eq!(PovDirection::from_degrees(u16::MAX), PovDirection::Center);
}

#[test]
fn input_state_button_helpers() {
    let mut state = InputState::default();
    state.buttons = 0b0000_0000_0000_0101; // buttons 0 and 2 pressed
    assert!(state.is_button_pressed(0));
    assert!(!state.is_button_pressed(1));
    assert!(state.is_button_pressed(2));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sidewinder-hid`
Expected: FAIL — `InputState` and `PovDirection` not defined.

- [ ] **Step 3: Implement InputState and PovDirection**

`crates/sidewinder-hid/src/input.rs`:
```rust
/// Direction of the POV hat switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PovDirection {
    #[default]
    Center,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl PovDirection {
    /// Convert from HID POV degrees (0-360 in units, or 0xFFFF for center).
    pub fn from_degrees(deg: u16) -> Self {
        match deg {
            0 => Self::North,
            45 => Self::NorthEast,
            90 => Self::East,
            135 => Self::SouthEast,
            180 => Self::South,
            225 => Self::SouthWest,
            270 => Self::West,
            315 => Self::NorthWest,
            _ => Self::Center,
        }
    }
}

/// Snapshot of all joystick inputs at a point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputState {
    /// 8 axes: X, Y, throttle, rudder (Rz), slider, Rx, Ry, dial.
    /// Range: -32768 to 32767 (i16).
    pub axes: [i16; 8],

    /// 9 buttons as a bitfield (bit 0 = button 0, etc.).
    pub buttons: u16,

    /// POV hat direction.
    pub pov: PovDirection,
}

impl InputState {
    /// Check if a specific button (0-indexed) is pressed.
    pub fn is_button_pressed(&self, index: u8) -> bool {
        self.buttons & (1 << index) != 0
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p sidewinder-hid`
Expected: All 3 tests pass.

- [ ] **Step 5: Write tests for FfbEffect types**

Add to `crates/sidewinder-hid/tests/types_test.rs`:
```rust
use sidewinder_hid::ffb::{
    ConditionParams, ConditionType, FfbEffect, FfbEffectParams,
    FfbEnvelope, Waveform,
};

#[test]
fn constant_force_effect() {
    let effect = FfbEffect {
        effect_block_index: 1,
        duration_ms: 1000,
        gain: 200,
        direction: 180,
        start_delay_ms: 0,
        trigger_button: None,
        trigger_repeat_ms: 0,
        envelope: None,
        params: FfbEffectParams::ConstantForce { magnitude: 5000 },
    };
    assert_eq!(effect.effect_block_index, 1);
    assert_eq!(effect.duration_ms, 1000);
}

#[test]
fn periodic_sine_effect() {
    let effect = FfbEffect {
        effect_block_index: 2,
        duration_ms: 2000,
        gain: 255,
        direction: 0,
        start_delay_ms: 100,
        trigger_button: None,
        trigger_repeat_ms: 0,
        envelope: Some(FfbEnvelope {
            attack_level: 0,
            attack_time_ms: 500,
            fade_level: 0,
            fade_time_ms: 500,
        }),
        params: FfbEffectParams::Periodic {
            waveform: Waveform::Sine,
            magnitude: 8000,
            offset: 0,
            period_ms: 200,
            phase: 0,
        },
    };
    assert_eq!(effect.gain, 255);
}

#[test]
fn condition_spring_effect() {
    let params = ConditionParams {
        center_point_offset: 0,
        positive_coefficient: 3000,
        negative_coefficient: 3000,
        positive_saturation: 10000,
        negative_saturation: 10000,
        dead_band: 0,
    };
    let effect = FfbEffect {
        effect_block_index: 3,
        duration_ms: 0xFFFF, // infinite
        gain: 200,
        direction: 0,
        start_delay_ms: 0,
        trigger_button: None,
        trigger_repeat_ms: 0,
        envelope: None,
        params: FfbEffectParams::Condition {
            condition_type: ConditionType::Spring,
            conditions: [params, params],
        },
    };
    assert!(matches!(effect.params, FfbEffectParams::Condition { .. }));
}
```

- [ ] **Step 6: Implement FfbEffect types**

`crates/sidewinder-hid/src/ffb.rs`:
```rust
/// Waveform type for periodic effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Square,
    Sine,
    Triangle,
    SawtoothUp,
    SawtoothDown,
}

/// Condition effect type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionType {
    Spring,
    Damper,
    Inertia,
    Friction,
}

/// Envelope applied to an effect (attack/fade).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FfbEnvelope {
    pub attack_level: u16,
    pub attack_time_ms: u16,
    pub fade_level: u16,
    pub fade_time_ms: u16,
}

/// Parameters for a condition effect axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionParams {
    pub center_point_offset: i16,
    pub positive_coefficient: i16,
    pub negative_coefficient: i16,
    pub positive_saturation: u16,
    pub negative_saturation: u16,
    pub dead_band: u16,
}

/// Type-specific effect parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FfbEffectParams {
    ConstantForce {
        magnitude: i16,
    },
    Ramp {
        start: i16,
        end: i16,
    },
    Periodic {
        waveform: Waveform,
        magnitude: i16,
        offset: i16,
        period_ms: u16,
        phase: u16,
    },
    Condition {
        condition_type: ConditionType,
        conditions: [ConditionParams; 2], // X and Y axes
    },
    CustomForce {
        sample_count: u16,
        sample_period_ms: u16,
    },
}

/// A complete force feedback effect definition.
///
/// Maps to HID PID "Set Effect Report" plus type-specific data.
#[derive(Debug, Clone, PartialEq)]
pub struct FfbEffect {
    /// Effect slot index (1-based, as per HID PID).
    pub effect_block_index: u8,

    /// Duration in milliseconds. 0xFFFF = infinite.
    pub duration_ms: u16,

    /// Effect gain (0-255).
    pub gain: u8,

    /// Direction in degrees (0-359).
    pub direction: u16,

    /// Start delay in milliseconds.
    pub start_delay_ms: u16,

    /// Trigger button (0-indexed), or None for no trigger.
    pub trigger_button: Option<u8>,

    /// Trigger repeat interval in milliseconds.
    pub trigger_repeat_ms: u16,

    /// Optional attack/fade envelope.
    pub envelope: Option<FfbEnvelope>,

    /// Type-specific parameters.
    pub params: FfbEffectParams,
}

/// FFB operation commands sent to the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfbOperation {
    Start { effect_block_index: u8, solo: bool },
    Stop { effect_block_index: u8 },
    StopAll,
    Free { effect_block_index: u8 },
    FreeAll,
    SetGain { gain: u8 },
}
```

- [ ] **Step 7: Run all tests**

Run: `cargo test -p sidewinder-hid`
Expected: All 6 tests pass.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(hid): add InputState, PovDirection, and FfbEffect types"
```

---

## Task 3: HID Device Enumeration

**Files:**
- Modify: `crates/sidewinder-hid/src/enumerate.rs`
- Modify: `crates/sidewinder-hid/src/hid_transport.rs`
- Modify: `crates/sidewinder-hid/src/lib.rs`
- Create: `crates/sidewinder-hid/tests/enumerate_test.rs`

This task implements finding the Sidewinder FF2 among connected HID devices using Windows SetupAPI + HID API.

- [ ] **Step 1: Write enumeration test**

`crates/sidewinder-hid/tests/enumerate_test.rs`:
```rust
use sidewinder_hid::enumerate::{DeviceInfo, enumerate_hid_devices};

#[test]
fn enumerate_returns_vec() {
    // This test verifies the API shape. On a machine without a Sidewinder,
    // it returns an empty vec. With one plugged in, it finds it.
    let devices = enumerate_hid_devices().unwrap();
    // We can't assert specific devices in CI, but the call must not panic
    for device in &devices {
        assert!(!device.path.is_empty());
        println!(
            "Found: VID={:04X} PID={:04X} name={:?} path={}",
            device.vendor_id, device.product_id, device.product_name, device.path
        );
    }
}

#[test]
fn device_info_is_sidewinder() {
    let info = DeviceInfo {
        path: String::from(r"\\?\hid#vid_045e&pid_001b"),
        vendor_id: 0x045E,
        product_id: 0x001B,
        product_name: String::from("Sidewinder Force Feedback 2"),
    };
    assert!(info.is_sidewinder_ffb2());
}

#[test]
fn device_info_not_sidewinder() {
    let info = DeviceInfo {
        path: String::from(r"\\?\hid#vid_046d&pid_c215"),
        vendor_id: 0x046D,
        product_id: 0xC215,
        product_name: String::from("Logitech Extreme 3D"),
    };
    assert!(!info.is_sidewinder_ffb2());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sidewinder-hid`
Expected: FAIL — `DeviceInfo` and `enumerate_hid_devices` not defined.

- [ ] **Step 3: Implement DeviceInfo and enumeration**

`crates/sidewinder-hid/src/enumerate.rs`:
```rust
use thiserror::Error;
use tracing::{debug, warn};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetPreparsedData,
    HidD_GetProductString, HidP_GetCaps, HIDD_ATTRIBUTES, HIDP_CAPS,
    PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, WIN32_ERROR};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::core::GUID;

/// VID/PID for the Microsoft Sidewinder Force Feedback 2.
pub const SIDEWINDER_FFB2_VID: u16 = 0x045E;
pub const SIDEWINDER_FFB2_PID: u16 = 0x001B;

/// HID device class GUID.
const HID_GUID: GUID = GUID::from_u128(0x4d1e55b2_f16f_11cf_88cb_001111000030);

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub product_name: String,
}

impl DeviceInfo {
    pub fn is_sidewinder_ffb2(&self) -> bool {
        self.vendor_id == SIDEWINDER_FFB2_VID && self.product_id == SIDEWINDER_FFB2_PID
    }
}

#[derive(Error, Debug)]
pub enum EnumerateError {
    #[error("SetupDi call failed: {0}")]
    SetupDi(#[from] windows::core::Error),
}

/// Enumerate all HID devices on the system.
pub fn enumerate_hid_devices() -> Result<Vec<DeviceInfo>, EnumerateError> {
    let mut devices = Vec::new();

    unsafe {
        let dev_info_set = SetupDiGetClassDevsW(
            Some(&HID_GUID),
            None,
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )?;

        let mut index = 0u32;
        loop {
            let mut iface_data = SP_DEVICE_INTERFACE_DATA {
                cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };

            if SetupDiEnumDeviceInterfaces(
                dev_info_set,
                None,
                &HID_GUID,
                index,
                &mut iface_data,
            )
            .is_err()
            {
                break; // No more devices
            }

            if let Some(info) = get_device_info(dev_info_set, &mut iface_data) {
                debug!(
                    vid = format!("{:04X}", info.vendor_id),
                    pid = format!("{:04X}", info.product_id),
                    name = %info.product_name,
                    "Found HID device"
                );
                devices.push(info);
            }

            index += 1;
        }

        let _ = SetupDiDestroyDeviceInfoList(dev_info_set);
    }

    Ok(devices)
}

/// Find the first connected Sidewinder FFB2 device.
pub fn find_sidewinder() -> Result<Option<DeviceInfo>, EnumerateError> {
    let devices = enumerate_hid_devices()?;
    Ok(devices.into_iter().find(|d| d.is_sidewinder_ffb2()))
}

unsafe fn get_device_info(
    dev_info_set: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    iface_data: &mut SP_DEVICE_INTERFACE_DATA,
) -> Option<DeviceInfo> {
    // Get required buffer size
    let mut required_size = 0u32;
    let _ = SetupDiGetDeviceInterfaceDetailW(
        dev_info_set,
        iface_data,
        None,
        0,
        Some(&mut required_size),
        None,
    );

    if required_size == 0 {
        return None;
    }

    // Allocate buffer and get detail
    let mut buf = vec![0u8; required_size as usize];
    let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
    (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;

    let mut dev_info_data = SP_DEVINFO_DATA {
        cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    };

    if SetupDiGetDeviceInterfaceDetailW(
        dev_info_set,
        iface_data,
        Some(detail),
        required_size,
        None,
        Some(&mut dev_info_data),
    )
    .is_err()
    {
        return None;
    }

    let path_ptr = &(*detail).DevicePath as *const u16;
    let path_len = (0..).take_while(|&i| *path_ptr.add(i) != 0).count();
    let path = String::from_utf16_lossy(std::slice::from_raw_parts(path_ptr, path_len));

    // Open device to get attributes
    let handle = CreateFileW(
        &windows::core::HSTRING::from(&path),
        windows::Win32::Storage::FileSystem::FILE_GENERIC_READ.0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        None,
        OPEN_EXISTING,
        FILE_FLAG_OVERLAPPED,
        None,
    )
    .ok()?;

    let mut attrs = HIDD_ATTRIBUTES {
        Size: size_of::<HIDD_ATTRIBUTES>() as u32,
        ..Default::default()
    };

    if HidD_GetAttributes(handle, &mut attrs).is_err() {
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        return None;
    }

    // Get product string
    let mut name_buf = [0u16; 128];
    let product_name = if HidD_GetProductString(handle, name_buf.as_mut_ptr() as _, name_buf.len() as u32).is_ok() {
        let len = name_buf.iter().position(|&c| c == 0).unwrap_or(name_buf.len());
        String::from_utf16_lossy(&name_buf[..len])
    } else {
        String::from("Unknown")
    };

    let _ = windows::Win32::Foundation::CloseHandle(handle);

    Some(DeviceInfo {
        path,
        vendor_id: attrs.VendorID,
        product_id: attrs.ProductID,
        product_name,
    })
}
```

- [ ] **Step 4: Update lib.rs exports**

`crates/sidewinder-hid/src/lib.rs`:
```rust
pub mod device;
pub mod enumerate;
pub mod ffb;
pub mod hid_transport;
pub mod input;

// Re-export key types
pub use device::SidewinderDevice;
pub use enumerate::{DeviceInfo, enumerate_hid_devices, find_sidewinder};
pub use ffb::{FfbEffect, FfbEffectParams, FfbOperation};
pub use input::InputState;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p sidewinder-hid`
Expected: All tests pass. The `enumerate_returns_vec` test will print found HID devices (if any).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(hid): implement HID device enumeration via SetupAPI"
```

---

## Task 4: HID Transport — Read/Write Reports

**Files:**
- Modify: `crates/sidewinder-hid/src/hid_transport.rs`
- Create: `crates/sidewinder-hid/tests/transport_test.rs`

- [ ] **Step 1: Write transport trait and test**

`crates/sidewinder-hid/tests/transport_test.rs`:
```rust
use sidewinder_hid::hid_transport::MockTransport;
use sidewinder_hid::input::InputState;

#[test]
fn mock_transport_returns_default_state() {
    let transport = MockTransport::new();
    let report = transport.read_input_report().unwrap();
    // Mock returns zeroed report bytes
    assert_eq!(report.len(), 14); // Sidewinder report size
}

#[test]
fn mock_transport_accepts_output_report() {
    let transport = MockTransport::new();
    let output = vec![0x01, 0x00, 0x10]; // dummy FFB report
    assert!(transport.write_output_report(&output).is_ok());
}
```

- [ ] **Step 2: Implement HidTransport trait with real and mock implementations**

`crates/sidewinder-hid/src/hid_transport.rs`:
```rust
use std::sync::Mutex;
use thiserror::Error;
use tracing::debug;

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("device not open")]
    NotOpen,
    #[error("read failed: {0}")]
    ReadFailed(String),
    #[error("write failed: {0}")]
    WriteFailed(String),
    #[error("Windows API error: {0}")]
    Windows(#[from] windows::core::Error),
}

/// Trait for reading/writing HID reports. Enables mock testing.
pub trait HidTransport: Send + Sync {
    fn read_input_report(&self) -> Result<Vec<u8>, TransportError>;
    fn write_output_report(&self, report: &[u8]) -> Result<(), TransportError>;
    fn write_feature_report(&self, report: &[u8]) -> Result<(), TransportError>;
}

/// Real Windows HID transport using ReadFile/WriteFile.
pub struct WindowsHidTransport {
    handle: windows::Win32::Foundation::HANDLE,
}

impl WindowsHidTransport {
    /// Open a HID device by its device path.
    pub fn open(device_path: &str) -> Result<Self, TransportError> {
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        };

        let handle = unsafe {
            CreateFileW(
                &windows::core::HSTRING::from(device_path),
                (windows::Win32::Storage::FileSystem::FILE_GENERIC_READ.0
                    | windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE.0),
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )?
        };

        debug!("Opened HID device: {}", device_path);
        Ok(Self { handle })
    }
}

impl HidTransport for WindowsHidTransport {
    fn read_input_report(&self) -> Result<Vec<u8>, TransportError> {
        use windows::Win32::Storage::FileSystem::ReadFile;

        let mut buf = vec![0u8; 64]; // Max HID report size
        let mut bytes_read = 0u32;

        unsafe {
            ReadFile(self.handle, Some(&mut buf), Some(&mut bytes_read), None)
                .map_err(|e| TransportError::ReadFailed(e.to_string()))?;
        }

        buf.truncate(bytes_read as usize);
        Ok(buf)
    }

    fn write_output_report(&self, report: &[u8]) -> Result<(), TransportError> {
        use windows::Win32::Devices::HumanInterfaceDevice::HidD_SetOutputReport;

        unsafe {
            HidD_SetOutputReport(self.handle, report.as_ptr() as _, report.len() as u32)
                .map_err(|e| TransportError::WriteFailed(e.to_string()))?;
        }

        Ok(())
    }

    fn write_feature_report(&self, report: &[u8]) -> Result<(), TransportError> {
        use windows::Win32::Devices::HumanInterfaceDevice::HidD_SetFeature;

        unsafe {
            HidD_SetFeature(self.handle, report.as_ptr() as _, report.len() as u32)
                .map_err(|e| TransportError::WriteFailed(e.to_string()))?;
        }

        Ok(())
    }
}

impl Drop for WindowsHidTransport {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

/// Mock transport for unit testing without hardware.
pub struct MockTransport {
    input_data: Mutex<Vec<u8>>,
    last_output: Mutex<Vec<u8>>,
}

impl MockTransport {
    pub fn new() -> Self {
        Self {
            input_data: Mutex::new(vec![0u8; 14]), // Sidewinder report size
            last_output: Mutex::new(Vec::new()),
        }
    }

    /// Set the data that will be returned by the next read_input_report call.
    pub fn set_input_data(&self, data: Vec<u8>) {
        *self.input_data.lock().unwrap() = data;
    }

    /// Get the last output report that was written.
    pub fn last_output(&self) -> Vec<u8> {
        self.last_output.lock().unwrap().clone()
    }
}

impl HidTransport for MockTransport {
    fn read_input_report(&self) -> Result<Vec<u8>, TransportError> {
        Ok(self.input_data.lock().unwrap().clone())
    }

    fn write_output_report(&self, report: &[u8]) -> Result<(), TransportError> {
        *self.last_output.lock().unwrap() = report.to_vec();
        Ok(())
    }

    fn write_feature_report(&self, report: &[u8]) -> Result<(), TransportError> {
        *self.last_output.lock().unwrap() = report.to_vec();
        Ok(())
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p sidewinder-hid`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(hid): add HidTransport trait with Windows and mock implementations"
```

---

## Task 5: Input Report Parsing

**Files:**
- Modify: `crates/sidewinder-hid/src/input.rs`
- Modify: `crates/sidewinder-hid/tests/types_test.rs`

The Sidewinder FF2 sends HID input reports containing axis positions, buttons, and POV hat. This task parses raw report bytes into `InputState`.

- [ ] **Step 1: Write input parsing tests**

Add to `crates/sidewinder-hid/tests/types_test.rs`:
```rust
use sidewinder_hid::input::parse_input_report;

#[test]
fn parse_centered_report() {
    // Report: all axes centered (0x8000 = center for unsigned 16-bit),
    // no buttons, POV center (0xFF)
    let report = vec![
        0x00, 0x80, // X axis: 0x8000 (center)
        0x00, 0x80, // Y axis: 0x8000 (center)
        0x00, 0x80, // Throttle: 0x8000 (center)
        0x00, 0x80, // Rudder: 0x8000 (center)
        0x00, 0x00, // Buttons: none pressed
        0xFF,       // POV: center (no direction)
    ];
    let state = parse_input_report(&report).unwrap();
    assert_eq!(state.axes[0], 0); // X centered
    assert_eq!(state.axes[1], 0); // Y centered
    assert_eq!(state.buttons, 0);
    assert_eq!(state.pov, PovDirection::Center);
}

#[test]
fn parse_full_deflection_report() {
    let report = vec![
        0xFF, 0xFF, // X axis: max
        0x00, 0x00, // Y axis: min
        0xFF, 0xFF, // Throttle: max
        0x00, 0x80, // Rudder: center
        0x05, 0x00, // Buttons: 0 and 2 pressed
        0x00,       // POV: North (0 degrees)
    ];
    let state = parse_input_report(&report).unwrap();
    assert_eq!(state.axes[0], 32767);  // X max
    assert_eq!(state.axes[1], -32768); // Y min
    assert!(state.is_button_pressed(0));
    assert!(!state.is_button_pressed(1));
    assert!(state.is_button_pressed(2));
    assert_eq!(state.pov, PovDirection::North);
}

#[test]
fn parse_short_report_returns_error() {
    let report = vec![0x00, 0x80]; // Too short
    assert!(parse_input_report(&report).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sidewinder-hid`
Expected: FAIL — `parse_input_report` not defined.

- [ ] **Step 3: Implement input report parser**

Add to `crates/sidewinder-hid/src/input.rs`:
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InputParseError {
    #[error("report too short: expected at least {expected} bytes, got {got}")]
    TooShort { expected: usize, got: usize },
}

/// Minimum expected report size for the Sidewinder FF2.
const MIN_REPORT_SIZE: usize = 11; // 4 axes * 2 bytes + 2 button bytes + 1 POV

/// Parse a raw HID input report into an InputState.
///
/// The Sidewinder FF2 report layout (little-endian):
/// - Bytes 0-1:  X axis (unsigned 16-bit, 0x0000-0xFFFF)
/// - Bytes 2-3:  Y axis
/// - Bytes 4-5:  Throttle
/// - Bytes 6-7:  Rudder (Rz)
/// - Bytes 8-9:  Buttons (16-bit bitfield)
/// - Byte 10:    POV hat (0-7 for directions, 0xFF for center)
///
/// Unsigned axis values are converted to signed i16:
///   0x0000 → -32768, 0x8000 → 0, 0xFFFF → 32767
pub fn parse_input_report(report: &[u8]) -> Result<InputState, InputParseError> {
    if report.len() < MIN_REPORT_SIZE {
        return Err(InputParseError::TooShort {
            expected: MIN_REPORT_SIZE,
            got: report.len(),
        });
    }

    let mut state = InputState::default();

    // Parse 4 axes (unsigned u16 → signed i16)
    for i in 0..4 {
        let offset = i * 2;
        let raw = u16::from_le_bytes([report[offset], report[offset + 1]]);
        state.axes[i] = unsigned_to_signed(raw);
    }

    // Parse buttons
    state.buttons = u16::from_le_bytes([report[8], report[9]]);

    // Parse POV hat
    let pov_byte = report[10];
    state.pov = pov_from_byte(pov_byte);

    Ok(state)
}

/// Convert unsigned 16-bit axis value to signed.
/// 0x0000 → -32768, 0x8000 → 0, 0xFFFF → 32767
fn unsigned_to_signed(val: u16) -> i16 {
    (val as i32 - 0x8000) as i16
}

/// Convert POV byte to PovDirection.
/// The Sidewinder uses 0-7 for 8 directions (N, NE, E, ...) and 0xFF for center.
fn pov_from_byte(byte: u8) -> PovDirection {
    match byte {
        0 => PovDirection::North,
        1 => PovDirection::NorthEast,
        2 => PovDirection::East,
        3 => PovDirection::SouthEast,
        4 => PovDirection::South,
        5 => PovDirection::SouthWest,
        6 => PovDirection::West,
        7 => PovDirection::NorthWest,
        _ => PovDirection::Center,
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sidewinder-hid`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(hid): implement input report parser for Sidewinder FF2"
```

---

## Task 6: SidewinderDevice — Full Device Lifecycle

**Files:**
- Modify: `crates/sidewinder-hid/src/device.rs`
- Create: `crates/sidewinder-hid/tests/device_test.rs`

- [ ] **Step 1: Write device lifecycle tests using MockTransport**

`crates/sidewinder-hid/tests/device_test.rs`:
```rust
use sidewinder_hid::device::SidewinderDevice;
use sidewinder_hid::hid_transport::MockTransport;
use sidewinder_hid::input::PovDirection;
use std::sync::Arc;

#[test]
fn device_polls_input_state() {
    let transport = Arc::new(MockTransport::new());
    // Set up a report: X=max, Y=center, throttle=center, rudder=center,
    // button 0 pressed, POV=East
    transport.set_input_data(vec![
        0xFF, 0xFF, // X: max
        0x00, 0x80, // Y: center
        0x00, 0x80, // Throttle: center
        0x00, 0x80, // Rudder: center
        0x01, 0x00, // Button 0
        0x02,       // POV: East
    ]);

    let device = SidewinderDevice::from_transport(transport);
    let state = device.poll().unwrap();

    assert_eq!(state.axes[0], 32767); // X at max
    assert_eq!(state.axes[1], 0);     // Y centered
    assert!(state.is_button_pressed(0));
    assert_eq!(state.pov, PovDirection::East);
}

#[test]
fn device_sends_ffb_operation() {
    let transport = Arc::new(MockTransport::new());
    transport.set_input_data(vec![0u8; 11]);

    let device = SidewinderDevice::from_transport(transport.clone());
    use sidewinder_hid::ffb::FfbOperation;

    device.send_ffb_operation(FfbOperation::SetGain { gain: 200 }).unwrap();

    let output = transport.last_output();
    assert!(!output.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sidewinder-hid`
Expected: FAIL — `SidewinderDevice::from_transport` not defined.

- [ ] **Step 3: Implement SidewinderDevice**

`crates/sidewinder-hid/src/device.rs`:
```rust
use crate::enumerate::{self, DeviceInfo, EnumerateError};
use crate::ffb::FfbOperation;
use crate::hid_transport::{HidTransport, TransportError, WindowsHidTransport};
use crate::input::{parse_input_report, InputState};
use std::sync::Arc;
use thiserror::Error;
use tracing::info;

#[derive(Error, Debug)]
pub enum DeviceError {
    #[error("device not found")]
    NotFound,
    #[error("enumeration failed: {0}")]
    Enumerate(#[from] EnumerateError),
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("input parse error: {0}")]
    InputParse(#[from] crate::input::InputParseError),
}

pub struct SidewinderDevice {
    transport: Arc<dyn HidTransport>,
}

impl SidewinderDevice {
    /// Open the first connected Sidewinder FFB2 device.
    pub fn open() -> Result<Self, DeviceError> {
        let device_info = enumerate::find_sidewinder()?.ok_or(DeviceError::NotFound)?;
        info!(
            name = %device_info.product_name,
            path = %device_info.path,
            "Opening Sidewinder FFB2"
        );
        let transport = Arc::new(WindowsHidTransport::open(&device_info.path)?);
        Ok(Self { transport })
    }

    /// Create a device with a custom transport (for testing).
    pub fn from_transport(transport: Arc<dyn HidTransport>) -> Self {
        Self { transport }
    }

    /// Read the current joystick state.
    pub fn poll(&self) -> Result<InputState, DeviceError> {
        let report = self.transport.read_input_report()?;
        Ok(parse_input_report(&report)?)
    }

    /// Send an FFB operation command to the physical device.
    pub fn send_ffb_operation(&self, op: FfbOperation) -> Result<(), DeviceError> {
        let report = build_ffb_operation_report(op);
        self.transport.write_output_report(&report)?;
        Ok(())
    }

    /// Send a complete FFB effect definition to the physical device.
    pub fn send_ffb_effect(&self, effect: &crate::ffb::FfbEffect) -> Result<(), DeviceError> {
        let reports = crate::ffb::build_ffb_effect_reports(effect);
        for report in reports {
            self.transport.write_output_report(&report)?;
        }
        Ok(())
    }
}

/// Build HID output report bytes for an FFB operation.
fn build_ffb_operation_report(op: FfbOperation) -> Vec<u8> {
    match op {
        FfbOperation::Start { effect_block_index, solo } => {
            vec![
                0x0A, // Effect Operation Report ID
                effect_block_index,
                if solo { 0x02 } else { 0x01 }, // 1=Start, 2=StartSolo
                0x01, // Loop count = 1
            ]
        }
        FfbOperation::Stop { effect_block_index } => {
            vec![0x0A, effect_block_index, 0x03, 0x00] // 3=Stop
        }
        FfbOperation::StopAll => {
            vec![0x0A, 0xFF, 0x03, 0x00] // 0xFF = all effects
        }
        FfbOperation::Free { effect_block_index } => {
            vec![0x0A, effect_block_index, 0x00, 0x00] // Free slot
        }
        FfbOperation::FreeAll => {
            vec![0x0A, 0xFF, 0x00, 0x00]
        }
        FfbOperation::SetGain { gain } => {
            vec![0x0D, gain] // Device Gain Report
        }
    }
}
```

- [ ] **Step 4: Add stub for build_ffb_effect_reports in ffb.rs**

Add to `crates/sidewinder-hid/src/ffb.rs`:
```rust
/// Build HID output report byte sequences for a complete FFB effect.
///
/// Returns multiple reports: the main Set Effect Report, plus type-specific
/// parameter reports (constant force, periodic, condition, etc.) and
/// optionally an envelope report.
pub fn build_ffb_effect_reports(effect: &FfbEffect) -> Vec<Vec<u8>> {
    let mut reports = Vec::new();

    // Set Effect Report (Report ID 0x01)
    let mut effect_report = vec![
        0x01, // Report ID: Set Effect
        effect.effect_block_index,
        effect_type_byte(&effect.params),
        (effect.duration_ms & 0xFF) as u8,
        ((effect.duration_ms >> 8) & 0xFF) as u8,
        (effect.trigger_repeat_ms & 0xFF) as u8,
        ((effect.trigger_repeat_ms >> 8) & 0xFF) as u8,
        0x00, 0x00, // Sample period (not used for most effects)
        (effect.start_delay_ms & 0xFF) as u8,
        ((effect.start_delay_ms >> 8) & 0xFF) as u8,
        effect.gain,
        effect.trigger_button.map(|b| b + 1).unwrap_or(0xFF), // 1-based or 0xFF
        0x03, // Axes enabled: X and Y
        0x01, // Direction enabled
        (effect.direction & 0xFF) as u8,
        ((effect.direction >> 8) & 0xFF) as u8,
    ];
    reports.push(effect_report);

    // Type-specific report
    match &effect.params {
        FfbEffectParams::ConstantForce { magnitude } => {
            reports.push(vec![
                0x05, // Report ID: Set Constant Force
                effect.effect_block_index,
                (*magnitude & 0xFF) as u8,
                ((*magnitude >> 8) & 0xFF) as u8,
            ]);
        }
        FfbEffectParams::Ramp { start, end } => {
            reports.push(vec![
                0x06, // Report ID: Set Ramp Force
                effect.effect_block_index,
                (*start & 0xFF) as u8,
                ((*start >> 8) & 0xFF) as u8,
                (*end & 0xFF) as u8,
                ((*end >> 8) & 0xFF) as u8,
            ]);
        }
        FfbEffectParams::Periodic { waveform: _, magnitude, offset, period_ms, phase } => {
            reports.push(vec![
                0x04, // Report ID: Set Periodic
                effect.effect_block_index,
                (*magnitude & 0xFF) as u8,
                ((*magnitude >> 8) & 0xFF) as u8,
                (*offset & 0xFF) as u8,
                ((*offset >> 8) & 0xFF) as u8,
                (*period_ms & 0xFF) as u8,
                ((*period_ms >> 8) & 0xFF) as u8,
                (*phase & 0xFF) as u8,
                ((*phase >> 8) & 0xFF) as u8,
            ]);
        }
        FfbEffectParams::Condition { condition_type: _, conditions } => {
            for (i, cond) in conditions.iter().enumerate() {
                reports.push(vec![
                    0x03, // Report ID: Set Condition
                    effect.effect_block_index,
                    i as u8, // Parameter block offset (axis index)
                    (cond.center_point_offset & 0xFF) as u8,
                    ((cond.center_point_offset >> 8) & 0xFF) as u8,
                    (cond.positive_coefficient & 0xFF) as u8,
                    ((cond.positive_coefficient >> 8) & 0xFF) as u8,
                    (cond.negative_coefficient & 0xFF) as u8,
                    ((cond.negative_coefficient >> 8) & 0xFF) as u8,
                    (cond.positive_saturation & 0xFF) as u8,
                    ((cond.positive_saturation >> 8) & 0xFF) as u8,
                    (cond.negative_saturation & 0xFF) as u8,
                    ((cond.negative_saturation >> 8) & 0xFF) as u8,
                    (cond.dead_band & 0xFF) as u8,
                    ((cond.dead_band >> 8) & 0xFF) as u8,
                ]);
            }
        }
        FfbEffectParams::CustomForce { .. } => {
            // Custom force data is sent separately via data reports
        }
    }

    // Envelope report (if present)
    if let Some(env) = &effect.envelope {
        reports.push(vec![
            0x02, // Report ID: Set Envelope
            effect.effect_block_index,
            (env.attack_level & 0xFF) as u8,
            ((env.attack_level >> 8) & 0xFF) as u8,
            (env.fade_level & 0xFF) as u8,
            ((env.fade_level >> 8) & 0xFF) as u8,
            (env.attack_time_ms & 0xFF) as u8,
            ((env.attack_time_ms >> 8) & 0xFF) as u8,
            (env.fade_time_ms & 0xFF) as u8,
            ((env.fade_time_ms >> 8) & 0xFF) as u8,
        ]);
    }

    reports
}

fn effect_type_byte(params: &FfbEffectParams) -> u8 {
    match params {
        FfbEffectParams::ConstantForce { .. } => 0x01,
        FfbEffectParams::Ramp { .. } => 0x02,
        FfbEffectParams::Periodic { waveform, .. } => match waveform {
            Waveform::Square => 0x03,
            Waveform::Sine => 0x04,
            Waveform::Triangle => 0x05,
            Waveform::SawtoothUp => 0x06,
            Waveform::SawtoothDown => 0x07,
        },
        FfbEffectParams::Condition { condition_type, .. } => match condition_type {
            ConditionType::Spring => 0x08,
            ConditionType::Damper => 0x09,
            ConditionType::Inertia => 0x0A,
            ConditionType::Friction => 0x0B,
        },
        FfbEffectParams::CustomForce { .. } => 0x0C,
    }
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test -p sidewinder-hid`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(hid): implement SidewinderDevice with polling and FFB output"
```

---

## Task 7: UMDF2 Driver Skeleton

**Files:**
- Create: `crates/sidewinder-driver/Cargo.toml`
- Create: `crates/sidewinder-driver/Makefile.toml`
- Create: `crates/sidewinder-driver/build.rs`
- Create: `crates/sidewinder-driver/src/lib.rs`
- Create: `crates/sidewinder-driver/sidewinder.inx`

This is the most novel part of the project — a UMDF2 HID minidriver in Rust following the vhidmini2 pattern. No one has done this before with `windows-drivers-rs`, so expect iteration.

- [ ] **Step 1: Create Cargo.toml for the driver crate**

`crates/sidewinder-driver/Cargo.toml`:
```toml
[package]
name = "sidewinder-driver"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[package.metadata.wdk.driver-model]
driver-type = "UMDF"
umdf-version-major = 2
target-umdf-version-minor = 33

[dependencies]
wdk = "0.4"
wdk-sys = { version = "0.5", features = ["hid"] }

[build-dependencies]
wdk-build = "0.5"

[features]
default = []
hid = ["wdk-sys/hid"]

[profile.dev]
lto = true
panic = "unwind"

[profile.release]
lto = true
panic = "unwind"
```

- [ ] **Step 2: Create build.rs**

`crates/sidewinder-driver/build.rs`:
```rust
fn main() -> Result<(), wdk_build::ConfigError> {
    wdk_build::configure_wdk_binary_build()
}
```

- [ ] **Step 3: Create the driver entry point**

`crates/sidewinder-driver/src/lib.rs`:
```rust
//! Sidewinder FFB2 Virtual HID Device — UMDF2 Driver
//!
//! This driver creates a virtual HID joystick that presents itself to
//! Windows as a game controller with force feedback support. The
//! companion user-mode app (sidewinder-app) feeds it input state and
//! reads FFB commands back.

mod ffb_handler;
mod hid_descriptor;
mod input_report;
mod ioctl;

use wdk::println;
use wdk_sys::*;
use wdk_sys::macros::call_unsafe_wdf_function_binding;

/// Driver entry point — called by Windows when the driver loads.
///
/// Sets up the WDF driver object and registers the DeviceAdd callback.
#[unsafe(export_name = "DriverEntry")]
pub unsafe extern "system" fn driver_entry(
    driver: PDRIVER_OBJECT,
    registry_path: PCUNICODE_STRING,
) -> NTSTATUS {
    println!("Sidewinder driver: DriverEntry");

    let mut driver_config = WDF_DRIVER_CONFIG {
        Size: core::mem::size_of::<WDF_DRIVER_CONFIG>() as u32,
        EvtDriverDeviceAdd: Some(evt_driver_device_add),
        ..Default::default()
    };

    let status = call_unsafe_wdf_function_binding!(
        WdfDriverCreate,
        driver,
        registry_path,
        core::ptr::null_mut(), // No object attributes
        &mut driver_config,
        core::ptr::null_mut(), // Don't need driver handle
    );

    if !NT_SUCCESS(status) {
        println!("Sidewinder driver: WdfDriverCreate failed: {:#X}", status);
    }

    status
}

/// Called when PnP manager finds a device matching our INF.
///
/// Creates the WDF device and sets up I/O queues for HID minidriver
/// operation (vhidmini2 pattern).
unsafe extern "C" fn evt_driver_device_add(
    _driver: WDFDRIVER,
    device_init: *mut WDFDEVICE_INIT,
) -> NTSTATUS {
    println!("Sidewinder driver: DeviceAdd");

    // Mark as filter driver (required for UMDF HID minidriver)
    call_unsafe_wdf_function_binding!(WdfFdoInitSetFilter, device_init);

    // Create the device
    let mut device = core::ptr::null_mut();
    let mut device_attrs = WDF_OBJECT_ATTRIBUTES {
        Size: core::mem::size_of::<WDF_OBJECT_ATTRIBUTES>() as u32,
        ..Default::default()
    };

    let status = call_unsafe_wdf_function_binding!(
        WdfDeviceCreate,
        &mut device_init as *mut *mut WDFDEVICE_INIT,
        &mut device_attrs,
        &mut device,
    );

    if !NT_SUCCESS(status) {
        println!("Sidewinder driver: WdfDeviceCreate failed: {:#X}", status);
        return status;
    }

    // Set up I/O queue for HID IOCTL requests
    let mut queue_config = WDF_IO_QUEUE_CONFIG {
        Size: core::mem::size_of::<WDF_IO_QUEUE_CONFIG>() as u32,
        DispatchType: WDF_IO_QUEUE_DISPATCH_TYPE::WdfIoQueueDispatchParallel,
        EvtIoInternalDeviceControl: Some(ioctl::evt_io_internal_device_control),
        PowerManaged: WDF_TRI_STATE::WdfFalse,
        ..Default::default()
    };

    let status = call_unsafe_wdf_function_binding!(
        WdfIoQueueCreate,
        device,
        &mut queue_config,
        core::ptr::null_mut(),
        core::ptr::null_mut(),
    );

    if !NT_SUCCESS(status) {
        println!("Sidewinder driver: WdfIoQueueCreate failed: {:#X}", status);
    }

    status
}
```

- [ ] **Step 4: Create stub modules**

`crates/sidewinder-driver/src/hid_descriptor.rs`:
```rust
//! HID report descriptor for the virtual joystick device.
//! Includes both input reports (axes/buttons/hat) and PID reports (FFB).
```

`crates/sidewinder-driver/src/ioctl.rs`:
```rust
//! IOCTL handler for HID minidriver requests.

use wdk::println;
use wdk_sys::*;

/// Handle HID internal device control IOCTLs.
///
/// This is the main dispatch function where the HID class driver sends
/// us requests for report descriptors, input reports, and FFB data.
pub unsafe extern "C" fn evt_io_internal_device_control(
    _queue: WDFQUEUE,
    request: WDFREQUEST,
    _output_buffer_length: usize,
    _input_buffer_length: usize,
    io_control_code: u32,
) {
    println!("Sidewinder driver: IOCTL {:#X}", io_control_code);

    // TODO: Handle IOCTL_HID_GET_DEVICE_DESCRIPTOR,
    //       IOCTL_HID_GET_REPORT_DESCRIPTOR,
    //       IOCTL_HID_READ_REPORT, etc.

    let status = STATUS_NOT_SUPPORTED;
    wdk_sys::macros::call_unsafe_wdf_function_binding!(
        WdfRequestComplete,
        request,
        status,
    );
}
```

`crates/sidewinder-driver/src/input_report.rs`:
```rust
//! Construct HID input reports from joystick state received via IOCTL.
```

`crates/sidewinder-driver/src/ffb_handler.rs`:
```rust
//! Capture and queue FFB SET_REPORT packets from games.
```

- [ ] **Step 5: Create the INF template**

`crates/sidewinder-driver/sidewinder.inx`:
```inf
;
; Sidewinder FFB2 Virtual HID Device
;

[Version]
Signature   = "$Windows NT$"
Class       = HIDClass
ClassGuid   = {745a17a0-74d3-11d0-b6fe-00a0c90f57da}
Provider    = %ProviderString%
CatalogFile = sidewinder.cat
DriverVer   =
PnpLockdown = 1

[DestinationDirs]
DefaultDestDir = 13

[SourceDisksNames]
1 = %DiskName%

[SourceDisksFiles]
sidewinder_driver.dll = 1

[Manufacturer]
%ManufacturerString% = Standard, NT$ARCH$

[Standard.NT$ARCH$]
%DeviceDescription% = Sidewinder_Install, Root\SidewinderFFB2

[Sidewinder_Install.NT]
CopyFiles = UMDriverCopy

[Sidewinder_Install.NT.hw]
AddReg = Sidewinder_AddReg

[Sidewinder_Install.NT.Services]
AddService = WUDFRd, 0x000001fa, WUDFRD_ServiceInstall

[Sidewinder_Install.NT.Wdf]
UmdfService         = SidewinderFFB2, Sidewinder_UmdfInstall
UmdfServiceOrder     = SidewinderFFB2
UmdfKernelModeClientPolicy = AllowKernelModeClients
UmdfFileObjectPolicy = AllowNullAndUnknownFileObjects
UmdfMethodNeitherAction = Copy
UmdfFsContextUsePolicy = CanUseFsContext2

[Sidewinder_UmdfInstall]
UmdfLibraryVersion  = $UMDFVERSION$
ServiceBinary       = %13%\sidewinder_driver.dll

[WUDFRD_ServiceInstall]
DisplayName   = %WudfRdDisplayName%
ServiceType   = 1
StartType     = 3
ErrorControl  = 1
ServiceBinary = %12%\WUDFRd.sys

[Sidewinder_AddReg]
HKR,,"LowerFilters",0x00010008,"WUDFRd"

[UMDriverCopy]
sidewinder_driver.dll

[Strings]
ProviderString       = "Sidewinder"
ManufacturerString   = "Sidewinder Project"
DeviceDescription    = "Sidewinder Force Feedback 2 (Virtual)"
DiskName             = "Sidewinder FFB2 Driver Installation Disk"
WudfRdDisplayName    = "Windows Driver Foundation - User-mode Driver Framework Reflector"
```

- [ ] **Step 6: Create Makefile.toml**

`crates/sidewinder-driver/Makefile.toml`:
```toml
extend = "target/wdk-build-config/Makefile.toml"
```

Note: The actual build is done from an eWDK developer command prompt:
```bash
cd crates/sidewinder-driver
cargo make
```

- [ ] **Step 7: Verify the driver crate compiles (requires WDK)**

Run (from eWDK command prompt): `cd crates/sidewinder-driver && cargo build`
Expected: Compiles (if WDK is installed) or clear error about missing WDK.

Note: This task may need iteration as the `windows-drivers-rs` ecosystem is young. Document any issues encountered.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(driver): add UMDF2 driver skeleton with vhidmini2 pattern"
```

---

## Task 8: Driver — HID Report Descriptor

**Files:**
- Modify: `crates/sidewinder-driver/src/hid_descriptor.rs`
- Modify: `crates/sidewinder-driver/src/ioctl.rs`

This task builds the HID report descriptor that tells Windows what our virtual joystick looks like: 8 axes, 9 buttons, 1 POV hat, plus PID (force feedback) reports.

- [ ] **Step 1: Implement the HID report descriptor**

`crates/sidewinder-driver/src/hid_descriptor.rs`:
```rust
//! HID Report Descriptor for the Sidewinder FFB2 virtual device.
//!
//! This descriptor defines:
//! - Input Report: 8 axes (16-bit), 9 buttons, 1 POV hat
//! - PID Output Reports: Effect definition, operation, gain
//! - PID Feature Reports: Block load, pool, effect state

/// The complete HID report descriptor bytes.
///
/// Built to match the Sidewinder FF2's capabilities and comply with
/// the HID PID (Physical Interface Device) specification for force feedback.
pub static REPORT_DESCRIPTOR: &[u8] = &[
    // ---- Usage Page: Generic Desktop, Usage: Joystick ----
    0x05, 0x01,       // Usage Page (Generic Desktop)
    0x09, 0x04,       // Usage (Joystick)
    0xA1, 0x01,       // Collection (Application)

    // ---- Input Report (Report ID 1) ----
    0x85, 0x01,       //   Report ID (1)

    // 8 axes, 16-bit signed each
    0x05, 0x01,       //   Usage Page (Generic Desktop)
    0x09, 0x30,       //   Usage (X)
    0x09, 0x31,       //   Usage (Y)
    0x09, 0x32,       //   Usage (Z) — Throttle
    0x09, 0x35,       //   Usage (Rz) — Rudder
    0x09, 0x36,       //   Usage (Slider)
    0x09, 0x33,       //   Usage (Rx)
    0x09, 0x34,       //   Usage (Ry)
    0x09, 0x37,       //   Usage (Dial)
    0x16, 0x00, 0x80, //   Logical Minimum (-32768)
    0x26, 0xFF, 0x7F, //   Logical Maximum (32767)
    0x75, 0x10,       //   Report Size (16 bits)
    0x95, 0x08,       //   Report Count (8 axes)
    0x81, 0x02,       //   Input (Data, Var, Abs)

    // 9 buttons
    0x05, 0x09,       //   Usage Page (Buttons)
    0x19, 0x01,       //   Usage Minimum (Button 1)
    0x29, 0x09,       //   Usage Maximum (Button 9)
    0x15, 0x00,       //   Logical Minimum (0)
    0x25, 0x01,       //   Logical Maximum (1)
    0x75, 0x01,       //   Report Size (1 bit)
    0x95, 0x09,       //   Report Count (9 buttons)
    0x81, 0x02,       //   Input (Data, Var, Abs)
    // 7-bit padding to byte-align
    0x75, 0x07,       //   Report Size (7 bits)
    0x95, 0x01,       //   Report Count (1)
    0x81, 0x01,       //   Input (Const)

    // 1 POV hat (4-bit, values 0-7 for directions, null for center)
    0x05, 0x01,       //   Usage Page (Generic Desktop)
    0x09, 0x39,       //   Usage (Hat Switch)
    0x15, 0x00,       //   Logical Minimum (0)
    0x25, 0x07,       //   Logical Maximum (7)
    0x35, 0x00,       //   Physical Minimum (0)
    0x46, 0x3B, 0x01, //   Physical Maximum (315)
    0x65, 0x14,       //   Unit (Degrees)
    0x75, 0x04,       //   Report Size (4 bits)
    0x95, 0x01,       //   Report Count (1)
    0x81, 0x42,       //   Input (Data, Var, Abs, Null)
    // 4-bit padding
    0x75, 0x04,       //   Report Size (4 bits)
    0x95, 0x01,       //   Report Count (1)
    0x81, 0x01,       //   Input (Const)

    // ---- PID Usage Page: Force Feedback ----
    0x05, 0x0F,       //   Usage Page (PID)

    // Set Effect Report (Output, Report ID 0x11)
    0x09, 0x21,       //   Usage (Set Effect Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x11,       //     Report ID (0x11)
    0x09, 0x22,       //     Usage (Effect Block Index)
    0x15, 0x01,       //     Logical Minimum (1)
    0x25, 0x28,       //     Logical Maximum (40)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)

    0x09, 0x25,       //     Usage (Effect Type)
    0xA1, 0x02,       //     Collection (Logical)
    0x09, 0x26,       //       Usage (ET Constant Force)
    0x09, 0x27,       //       Usage (ET Ramp)
    0x09, 0x30,       //       Usage (ET Square)
    0x09, 0x31,       //       Usage (ET Sine)
    0x09, 0x32,       //       Usage (ET Triangle)
    0x09, 0x33,       //       Usage (ET Sawtooth Up)
    0x09, 0x34,       //       Usage (ET Sawtooth Down)
    0x09, 0x40,       //       Usage (ET Spring)
    0x09, 0x41,       //       Usage (ET Damper)
    0x09, 0x42,       //       Usage (ET Inertia)
    0x09, 0x43,       //       Usage (ET Friction)
    0x09, 0x28,       //       Usage (ET Custom Force)
    0x25, 0x0C,       //       Logical Maximum (12)
    0x15, 0x01,       //       Logical Minimum (1)
    0x75, 0x08,       //       Report Size (8)
    0x95, 0x01,       //       Report Count (1)
    0x91, 0x00,       //       Output (Data, Arr, Abs)
    0xC0,             //     End Collection

    0x09, 0x50,       //     Usage (Duration)
    0x09, 0x54,       //     Usage (Trigger Repeat Interval)
    0x09, 0xA7,       //     Usage (Start Delay)
    0x15, 0x00,       //     Logical Minimum (0)
    0x26, 0xFF, 0x7F, //     Logical Maximum (32767)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x03,       //     Report Count (3)
    0x91, 0x02,       //     Output (Data, Var, Abs)

    0x09, 0x52,       //     Usage (Gain)
    0x15, 0x00,       //     Logical Minimum (0)
    0x26, 0xFF, 0x00, //     Logical Maximum (255)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)

    0x09, 0x51,       //     Usage (Trigger Button)
    0x15, 0x01,       //     Logical Minimum (1)
    0x25, 0x09,       //     Logical Maximum (9)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)

    // Direction (2 axes)
    0x09, 0x57,       //     Usage (Direction)
    0xA1, 0x02,       //     Collection (Logical)
    0x0B, 0x01, 0x00, 0x0A, 0x00, // Usage (Ordinals: Instance 1)
    0x0B, 0x02, 0x00, 0x0A, 0x00, // Usage (Ordinals: Instance 2)
    0x66, 0x14, 0x00, //       Unit (Degrees)
    0x15, 0x00,       //       Logical Minimum (0)
    0x27, 0xA0, 0x8C, 0x00, 0x00, // Logical Maximum (36000)
    0x75, 0x10,       //       Report Size (16)
    0x95, 0x02,       //       Report Count (2)
    0x91, 0x02,       //       Output (Data, Var, Abs)
    0x65, 0x00,       //       Unit (None)
    0x55, 0x00,       //       Unit Exponent (0)
    0xC0,             //     End Collection
    0xC0,             //   End Collection (Set Effect Report)

    // Effect Operation Report (Output, Report ID 0x12)
    0x09, 0x77,       //   Usage (Effect Operation Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x12,       //     Report ID (0x12)
    0x09, 0x22,       //     Usage (Effect Block Index)
    0x15, 0x01,       //     Logical Minimum (1)
    0x25, 0x28,       //     Logical Maximum (40)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0x09, 0x78,       //     Usage (Effect Operation)
    0xA1, 0x02,       //     Collection (Logical)
    0x09, 0x79,       //       Usage (Op Start)
    0x09, 0x7A,       //       Usage (Op Start Solo)
    0x09, 0x7B,       //       Usage (Op Stop)
    0x15, 0x01,       //       Logical Minimum (1)
    0x25, 0x03,       //       Logical Maximum (3)
    0x75, 0x08,       //       Report Size (8)
    0x95, 0x01,       //       Report Count (1)
    0x91, 0x00,       //       Output (Data, Arr, Abs)
    0xC0,             //     End Collection
    0x09, 0x7C,       //     Usage (Loop Count)
    0x15, 0x00,       //     Logical Minimum (0)
    0x26, 0xFF, 0x00, //     Logical Maximum (255)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0xC0,             //   End Collection

    // Device Gain Report (Output, Report ID 0x13)
    0x09, 0x7D,       //   Usage (Device Gain Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x13,       //     Report ID (0x13)
    0x09, 0x7E,       //     Usage (Device Gain)
    0x15, 0x00,       //     Logical Minimum (0)
    0x26, 0xFF, 0x00, //     Logical Maximum (255)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0xC0,             //   End Collection

    // Constant Force Report (Output, Report ID 0x14)
    0x09, 0x73,       //   Usage (Set Constant Force Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x14,       //     Report ID (0x14)
    0x09, 0x22,       //     Usage (Effect Block Index)
    0x15, 0x01,       //     Logical Minimum (1)
    0x25, 0x28,       //     Logical Maximum (40)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0x09, 0x70,       //     Usage (Magnitude)
    0x16, 0x00, 0x80, //     Logical Minimum (-32768)
    0x26, 0xFF, 0x7F, //     Logical Maximum (32767)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0xC0,             //   End Collection

    // Envelope Report (Output, Report ID 0x15)
    0x09, 0x5A,       //   Usage (Set Envelope Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x15,       //     Report ID (0x15)
    0x09, 0x22,       //     Usage (Effect Block Index)
    0x15, 0x01,       //     Logical Minimum (1)
    0x25, 0x28,       //     Logical Maximum (40)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0x09, 0x5B,       //     Usage (Attack Level)
    0x09, 0x5D,       //     Usage (Fade Level)
    0x15, 0x00,       //     Logical Minimum (0)
    0x26, 0xFF, 0x7F, //     Logical Maximum (32767)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x02,       //     Report Count (2)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0x09, 0x5C,       //     Usage (Attack Time)
    0x09, 0x5E,       //     Usage (Fade Time)
    0x26, 0xFF, 0x7F, //     Logical Maximum (32767)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x02,       //     Report Count (2)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0xC0,             //   End Collection

    // Condition Report (Output, Report ID 0x16)
    0x09, 0x65,       //   Usage (Set Condition Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x16,       //     Report ID (0x16)
    0x09, 0x22,       //     Usage (Effect Block Index)
    0x15, 0x01,       //     Logical Minimum (1)
    0x25, 0x28,       //     Logical Maximum (40)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0x09, 0x23,       //     Usage (Parameter Block Offset)
    0x15, 0x00,       //     Logical Minimum (0)
    0x25, 0x01,       //     Logical Maximum (1)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0x09, 0x60,       //     Usage (CP Offset)
    0x09, 0x61,       //     Usage (Positive Coefficient)
    0x09, 0x62,       //     Usage (Negative Coefficient)
    0x16, 0x00, 0x80, //     Logical Minimum (-32768)
    0x26, 0xFF, 0x7F, //     Logical Maximum (32767)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x03,       //     Report Count (3)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0x09, 0x63,       //     Usage (Positive Saturation)
    0x09, 0x64,       //     Usage (Negative Saturation)
    0x09, 0x66,       //     Usage (Dead Band)
    0x15, 0x00,       //     Logical Minimum (0)
    0x26, 0xFF, 0x7F, //     Logical Maximum (32767)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x03,       //     Report Count (3)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0xC0,             //   End Collection

    // Periodic Report (Output, Report ID 0x17)
    0x09, 0x6E,       //   Usage (Set Periodic Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x17,       //     Report ID (0x17)
    0x09, 0x22,       //     Usage (Effect Block Index)
    0x15, 0x01,       //     Logical Minimum (1)
    0x25, 0x28,       //     Logical Maximum (40)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0x09, 0x70,       //     Usage (Magnitude)
    0x09, 0x6F,       //     Usage (Offset)
    0x16, 0x00, 0x80, //     Logical Minimum (-32768)
    0x26, 0xFF, 0x7F, //     Logical Maximum (32767)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x02,       //     Report Count (2)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0x09, 0x71,       //     Usage (Phase)
    0x09, 0x72,       //     Usage (Period)
    0x15, 0x00,       //     Logical Minimum (0)
    0x26, 0xFF, 0x7F, //     Logical Maximum (32767)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x02,       //     Report Count (2)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0xC0,             //   End Collection

    // Ramp Force Report (Output, Report ID 0x18)
    0x09, 0x74,       //   Usage (Set Ramp Force Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x18,       //     Report ID (0x18)
    0x09, 0x22,       //     Usage (Effect Block Index)
    0x15, 0x01,       //     Logical Minimum (1)
    0x25, 0x28,       //     Logical Maximum (40)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0x09, 0x75,       //     Usage (Ramp Start)
    0x09, 0x76,       //     Usage (Ramp End)
    0x16, 0x00, 0x80, //     Logical Minimum (-32768)
    0x26, 0xFF, 0x7F, //     Logical Maximum (32767)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x02,       //     Report Count (2)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0xC0,             //   End Collection

    // ---- PID Feature Reports ----

    // Block Load Report (Feature, Report ID 0x21)
    0x09, 0x89,       //   Usage (PID Block Load Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x21,       //     Report ID (0x21)
    0x09, 0x22,       //     Usage (Effect Block Index)
    0x15, 0x01,       //     Logical Minimum (1)
    0x25, 0x28,       //     Logical Maximum (40)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0xB1, 0x02,       //     Feature (Data, Var, Abs)
    0x09, 0x8B,       //     Usage (Block Load Status)
    0xA1, 0x02,       //     Collection (Logical)
    0x09, 0x8C,       //       Usage (Block Load Success)
    0x09, 0x8D,       //       Usage (Block Load Full)
    0x09, 0x8E,       //       Usage (Block Load Error)
    0x25, 0x03,       //       Logical Maximum (3)
    0x15, 0x01,       //       Logical Minimum (1)
    0x75, 0x08,       //       Report Size (8)
    0x95, 0x01,       //       Report Count (1)
    0xB1, 0x00,       //       Feature (Data, Arr, Abs)
    0xC0,             //     End Collection
    0x09, 0xAC,       //     Usage (RAM Pool Available)
    0x15, 0x00,       //     Logical Minimum (0)
    0x26, 0xFF, 0xFF, //     Logical Maximum (65535)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x01,       //     Report Count (1)
    0xB1, 0x02,       //     Feature (Data, Var, Abs)
    0xC0,             //   End Collection

    // Pool Report (Feature, Report ID 0x22)
    0x09, 0x7F,       //   Usage (PID Pool Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x22,       //     Report ID (0x22)
    0x09, 0x80,       //     Usage (RAM Pool Size)
    0x15, 0x00,       //     Logical Minimum (0)
    0x26, 0xFF, 0xFF, //     Logical Maximum (65535)
    0x75, 0x10,       //     Report Size (16)
    0x95, 0x01,       //     Report Count (1)
    0xB1, 0x02,       //     Feature (Data, Var, Abs)
    0x09, 0x83,       //     Usage (Simultaneous Effects Max)
    0x25, 0x28,       //     Logical Maximum (40)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0xB1, 0x02,       //     Feature (Data, Var, Abs)
    0xC0,             //   End Collection

    // Create New Effect Report (Feature, Report ID 0x23)
    0x09, 0xAB,       //   Usage (Create New Effect Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x23,       //     Report ID (0x23)
    0x09, 0x25,       //     Usage (Effect Type)
    0xA1, 0x02,       //     Collection (Logical)
    0x09, 0x26,       //       Usage (ET Constant Force)
    0x09, 0x27,       //       Usage (ET Ramp)
    0x09, 0x30,       //       Usage (ET Square)
    0x09, 0x31,       //       Usage (ET Sine)
    0x09, 0x32,       //       Usage (ET Triangle)
    0x09, 0x33,       //       Usage (ET Sawtooth Up)
    0x09, 0x34,       //       Usage (ET Sawtooth Down)
    0x09, 0x40,       //       Usage (ET Spring)
    0x09, 0x41,       //       Usage (ET Damper)
    0x09, 0x42,       //       Usage (ET Inertia)
    0x09, 0x43,       //       Usage (ET Friction)
    0x09, 0x28,       //       Usage (ET Custom Force)
    0x25, 0x0C,       //       Logical Maximum (12)
    0x15, 0x01,       //       Logical Minimum (1)
    0x75, 0x08,       //       Report Size (8)
    0x95, 0x01,       //       Report Count (1)
    0xB1, 0x00,       //       Feature (Data, Arr, Abs)
    0xC0,             //     End Collection
    0xC0,             //   End Collection

    // Block Free Report (Output, Report ID 0x19)
    0x09, 0x90,       //   Usage (PID Block Free Report)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x19,       //     Report ID (0x19)
    0x09, 0x22,       //     Usage (Effect Block Index)
    0x15, 0x01,       //     Logical Minimum (1)
    0x25, 0x28,       //     Logical Maximum (40)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x02,       //     Output (Data, Var, Abs)
    0xC0,             //   End Collection

    // Device Control Report (Output, Report ID 0x1A)
    0x09, 0x96,       //   Usage (PID Device Control)
    0xA1, 0x02,       //   Collection (Logical)
    0x85, 0x1A,       //     Report ID (0x1A)
    0x09, 0x97,       //     Usage (DC Enable Actuators)
    0x09, 0x98,       //     Usage (DC Disable Actuators)
    0x09, 0x99,       //     Usage (DC Stop All Effects)
    0x09, 0x9A,       //     Usage (DC Device Reset)
    0x09, 0x9B,       //     Usage (DC Device Pause)
    0x09, 0x9C,       //     Usage (DC Device Continue)
    0x15, 0x01,       //     Logical Minimum (1)
    0x25, 0x06,       //     Logical Maximum (6)
    0x75, 0x08,       //     Report Size (8)
    0x95, 0x01,       //     Report Count (1)
    0x91, 0x00,       //     Output (Data, Arr, Abs)
    0xC0,             //   End Collection

    0xC0,             // End Collection (Application)
];

/// HID descriptor (top-level, returned for IOCTL_HID_GET_DEVICE_DESCRIPTOR).
pub static HID_DESCRIPTOR: &[u8] = &[
    0x09,       // bLength
    0x21,       // bDescriptorType (HID)
    0x11, 0x01, // bcdHID (1.11)
    0x00,       // bCountryCode
    0x01,       // bNumDescriptors
    0x22,       // bDescriptorType (Report)
    (REPORT_DESCRIPTOR.len() & 0xFF) as u8,
    ((REPORT_DESCRIPTOR.len() >> 8) & 0xFF) as u8,
];

/// Size of the input report (Report ID 1):
/// 8 axes * 2 bytes + 2 button bytes + 1 POV byte = 19 bytes + 1 report ID = 20
pub const INPUT_REPORT_SIZE: usize = 20;
```

- [ ] **Step 2: Wire up IOCTL handler to serve descriptors**

This step updates the IOCTL handler to respond to the HID class driver's requests for descriptors and reports. The actual implementation will need iteration against the real WDK, so this is the target structure:

`crates/sidewinder-driver/src/ioctl.rs`:
```rust
//! IOCTL handler for HID minidriver requests.
//!
//! The HID class driver (hidclass.sys) sends us IOCTLs for:
//! - Device/report descriptors (at init time)
//! - Read reports (ongoing input polling)
//! - Write reports / set feature (FFB from games)
//! - Device attributes

use crate::hid_descriptor;
use wdk::println;
use wdk_sys::*;
use wdk_sys::macros::call_unsafe_wdf_function_binding;

// HID IOCTL codes
const IOCTL_HID_GET_DEVICE_DESCRIPTOR: u32 = 0xB0003;
const IOCTL_HID_GET_REPORT_DESCRIPTOR: u32 = 0xB0007;
const IOCTL_HID_READ_REPORT: u32 = 0xB000B;
const IOCTL_HID_GET_DEVICE_ATTRIBUTES: u32 = 0xB0027;
const IOCTL_HID_WRITE_REPORT: u32 = 0xB000F;
const IOCTL_HID_SET_FEATURE: u32 = 0xB0191;
const IOCTL_HID_GET_FEATURE: u32 = 0xB0193;

/// Vendor/Product IDs for our virtual device.
const VENDOR_ID: u16 = 0x045E;  // Microsoft
const PRODUCT_ID: u16 = 0xFF1B; // Virtual Sidewinder FFB2 (custom PID)

pub unsafe extern "C" fn evt_io_internal_device_control(
    _queue: WDFQUEUE,
    request: WDFREQUEST,
    _output_buffer_length: usize,
    _input_buffer_length: usize,
    io_control_code: u32,
) {
    let status = match io_control_code {
        IOCTL_HID_GET_DEVICE_DESCRIPTOR => {
            handle_get_device_descriptor(request)
        }
        IOCTL_HID_GET_REPORT_DESCRIPTOR => {
            handle_get_report_descriptor(request)
        }
        IOCTL_HID_GET_DEVICE_ATTRIBUTES => {
            handle_get_device_attributes(request)
        }
        IOCTL_HID_READ_REPORT => {
            handle_read_report(request)
        }
        IOCTL_HID_WRITE_REPORT => {
            handle_write_report(request)
        }
        IOCTL_HID_SET_FEATURE => {
            handle_set_feature(request)
        }
        IOCTL_HID_GET_FEATURE => {
            handle_get_feature(request)
        }
        other => {
            println!("Sidewinder: unhandled IOCTL {:#X}", other);
            STATUS_NOT_SUPPORTED
        }
    };

    if status != STATUS_PENDING {
        call_unsafe_wdf_function_binding!(WdfRequestComplete, request, status);
    }
}

unsafe fn handle_get_device_descriptor(request: WDFREQUEST) -> NTSTATUS {
    println!("Sidewinder: GET_DEVICE_DESCRIPTOR");

    let mut buffer: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut buffer_len: usize = 0;

    let status = call_unsafe_wdf_function_binding!(
        WdfRequestRetrieveOutputBuffer,
        request,
        hid_descriptor::HID_DESCRIPTOR.len(),
        &mut buffer,
        &mut buffer_len,
    );

    if !NT_SUCCESS(status) {
        return status;
    }

    core::ptr::copy_nonoverlapping(
        hid_descriptor::HID_DESCRIPTOR.as_ptr(),
        buffer as *mut u8,
        hid_descriptor::HID_DESCRIPTOR.len(),
    );

    call_unsafe_wdf_function_binding!(
        WdfRequestSetInformation,
        request,
        hid_descriptor::HID_DESCRIPTOR.len() as u64,
    );

    STATUS_SUCCESS
}

unsafe fn handle_get_report_descriptor(request: WDFREQUEST) -> NTSTATUS {
    println!("Sidewinder: GET_REPORT_DESCRIPTOR");

    let mut buffer: *mut core::ffi::c_void = core::ptr::null_mut();
    let mut buffer_len: usize = 0;

    let status = call_unsafe_wdf_function_binding!(
        WdfRequestRetrieveOutputBuffer,
        request,
        hid_descriptor::REPORT_DESCRIPTOR.len(),
        &mut buffer,
        &mut buffer_len,
    );

    if !NT_SUCCESS(status) {
        return status;
    }

    core::ptr::copy_nonoverlapping(
        hid_descriptor::REPORT_DESCRIPTOR.as_ptr(),
        buffer as *mut u8,
        hid_descriptor::REPORT_DESCRIPTOR.len(),
    );

    call_unsafe_wdf_function_binding!(
        WdfRequestSetInformation,
        request,
        hid_descriptor::REPORT_DESCRIPTOR.len() as u64,
    );

    STATUS_SUCCESS
}

unsafe fn handle_get_device_attributes(request: WDFREQUEST) -> NTSTATUS {
    println!("Sidewinder: GET_DEVICE_ATTRIBUTES");

    let mut buffer: *mut core::ffi::c_void = core::ptr::null_mut();

    let status = call_unsafe_wdf_function_binding!(
        WdfRequestRetrieveOutputBuffer,
        request,
        core::mem::size_of::<HID_DEVICE_ATTRIBUTES>(),
        &mut buffer,
        core::ptr::null_mut::<usize>(),
    );

    if !NT_SUCCESS(status) {
        return status;
    }

    let attrs = buffer as *mut HID_DEVICE_ATTRIBUTES;
    (*attrs).Size = core::mem::size_of::<HID_DEVICE_ATTRIBUTES>() as u32;
    (*attrs).VendorID = VENDOR_ID;
    (*attrs).ProductID = PRODUCT_ID;
    (*attrs).VersionNumber = 0x0100;

    call_unsafe_wdf_function_binding!(
        WdfRequestSetInformation,
        request,
        core::mem::size_of::<HID_DEVICE_ATTRIBUTES>() as u64,
    );

    STATUS_SUCCESS
}

unsafe fn handle_read_report(_request: WDFREQUEST) -> NTSTATUS {
    // Pend the request — we'll complete it when sidewinder-app
    // pushes new input state via our custom IOCTL.
    // For now, return pending. Task 9 will implement the queue.
    STATUS_PENDING
}

unsafe fn handle_write_report(_request: WDFREQUEST) -> NTSTATUS {
    // FFB output report from a game. Task 10 will implement capture.
    println!("Sidewinder: WRITE_REPORT (FFB)");
    STATUS_SUCCESS
}

unsafe fn handle_set_feature(_request: WDFREQUEST) -> NTSTATUS {
    // FFB feature report (Create New Effect, etc.). Task 10.
    println!("Sidewinder: SET_FEATURE (FFB)");
    STATUS_SUCCESS
}

unsafe fn handle_get_feature(_request: WDFREQUEST) -> NTSTATUS {
    // Block Load Report, Pool Report. Task 10.
    println!("Sidewinder: GET_FEATURE");
    STATUS_NOT_SUPPORTED
}
```

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(driver): implement HID report descriptor with PID force feedback"
```

---

## Task 9: Driver — Input State IOCTL

**Files:**
- Modify: `crates/sidewinder-driver/src/lib.rs`
- Modify: `crates/sidewinder-driver/src/ioctl.rs`
- Modify: `crates/sidewinder-driver/src/input_report.rs`

The companion app needs to push joystick state into the driver, which then completes pending HID READ_REPORT requests.

- [ ] **Step 1: Define shared IOCTL codes and the input state structure**

`crates/sidewinder-driver/src/input_report.rs`:
```rust
//! Input report construction and IOCTL data structures.

use crate::hid_descriptor::INPUT_REPORT_SIZE;

/// Custom IOCTL for pushing input state from the user-mode app.
/// CTL_CODE(FILE_DEVICE_UNKNOWN=0x22, 0x800, METHOD_BUFFERED=0, FILE_ANY_ACCESS=0)
pub const IOCTL_SIDEWINDER_UPDATE_STATE: u32 = 0x00222000;

/// Input state structure passed from user-mode app via IOCTL.
/// Must match the layout expected by sidewinder-app's driver_ipc module.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SidewinderInputState {
    pub axes: [i16; 8],
    pub buttons: u16,
    pub pov: u8,    // 0-7 for directions, 0xFF for center
    pub _pad: u8,
}

/// Build a HID input report from the input state.
///
/// Layout (Report ID 0x01):
/// - Byte 0: Report ID (0x01)
/// - Bytes 1-16: 8 axes, 16-bit LE each
/// - Bytes 17-18: 9 buttons + 7 padding bits
/// - Byte 19: POV hat (4 bits) + 4 padding bits
pub fn build_input_report(state: &SidewinderInputState) -> [u8; INPUT_REPORT_SIZE] {
    let mut report = [0u8; INPUT_REPORT_SIZE];
    report[0] = 0x01; // Report ID

    // 8 axes, 16-bit LE
    for i in 0..8 {
        let bytes = state.axes[i].to_le_bytes();
        report[1 + i * 2] = bytes[0];
        report[2 + i * 2] = bytes[1];
    }

    // Buttons (9 bits) + 7 padding
    let btn_bytes = state.buttons.to_le_bytes();
    report[17] = btn_bytes[0];
    report[18] = btn_bytes[1] & 0x01; // Only bit 0 of second byte (button 9)

    // POV hat (4 bits) + 4 padding
    report[19] = if state.pov <= 7 { state.pov } else { 0x0F }; // 0x0F = null state

    report
}
```

- [ ] **Step 2: Add device context and pending read queue to driver**

Update `crates/sidewinder-driver/src/lib.rs` to store a manual queue for pending READ_REPORT requests and handle the custom UPDATE_STATE IOCTL. This requires a device context structure stored via WDF object context.

This is the most complex WDK integration point and will likely need iteration. The key pattern is:
1. Store a manual I/O queue in the device context
2. When READ_REPORT arrives, forward it to the manual queue
3. When UPDATE_STATE arrives, build a report and complete the oldest pending READ_REPORT

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(driver): add input state IOCTL and report construction"
```

---

## Task 10: Driver — FFB Capture IOCTL

**Files:**
- Modify: `crates/sidewinder-driver/src/ffb_handler.rs`
- Modify: `crates/sidewinder-driver/src/ioctl.rs`

When games send FFB effects to our virtual device (via WRITE_REPORT / SET_FEATURE), we capture them and make them available to the user-mode app.

- [ ] **Step 1: Define FFB IOCTL and packet structure**

`crates/sidewinder-driver/src/ffb_handler.rs`:
```rust
//! FFB packet capture and queuing.
//!
//! Games send FFB effects via HID SET_REPORT/WRITE_REPORT.
//! We capture these raw packets and queue them for the user-mode app
//! to retrieve via IOCTL_SIDEWINDER_GET_FFB.

/// Custom IOCTL for retrieving FFB packets.
pub const IOCTL_SIDEWINDER_GET_FFB: u32 = 0x00222004;

/// Maximum FFB packet size (report ID + data).
pub const MAX_FFB_PACKET_SIZE: usize = 32;

/// FFB packet structure returned to user-mode.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FfbPacket {
    /// Report ID identifying the FFB report type.
    pub report_id: u8,
    /// Length of valid data in `data`.
    pub data_len: u8,
    /// Raw report data (after report ID).
    pub data: [u8; MAX_FFB_PACKET_SIZE],
}

impl Default for FfbPacket {
    fn default() -> Self {
        Self {
            report_id: 0,
            data_len: 0,
            data: [0u8; MAX_FFB_PACKET_SIZE],
        }
    }
}
```

- [ ] **Step 2: Implement FFB capture in IOCTL handlers**

Wire up `handle_write_report` and `handle_set_feature` in `ioctl.rs` to extract the FFB report bytes and queue them. Wire up `handle_get_feature` to return Block Load and Pool reports.

The pattern mirrors the input state IOCTL: a manual queue for pending GET_FFB requests, completed when FFB packets arrive from the game side.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(driver): add FFB capture and GET_FFB IOCTL"
```

---

## Task 11: App — Driver IPC Layer

**Files:**
- Create: `crates/sidewinder-app/src/driver_ipc.rs`
- Create: `crates/sidewinder-app/tests/driver_ipc_test.rs`

- [ ] **Step 1: Write tests for the IPC layer**

`crates/sidewinder-app/tests/driver_ipc_test.rs`:
```rust
use sidewinder_app::driver_ipc::{DriverHandle, InputStateIoctl};
use sidewinder_hid::input::{InputState, PovDirection};

#[test]
fn input_state_to_ioctl_struct() {
    let state = InputState {
        axes: [100, -200, 300, -400, 0, 0, 0, 0],
        buttons: 0b0000_0001_0000_0101,
        pov: PovDirection::North,
    };

    let ioctl = InputStateIoctl::from_input_state(&state);
    assert_eq!(ioctl.axes[0], 100);
    assert_eq!(ioctl.axes[1], -200);
    assert_eq!(ioctl.buttons, 0b0000_0001_0000_0101);
    assert_eq!(ioctl.pov, 0);
}

#[test]
fn pov_center_maps_to_0xff() {
    let state = InputState {
        axes: [0; 8],
        buttons: 0,
        pov: PovDirection::Center,
    };
    let ioctl = InputStateIoctl::from_input_state(&state);
    assert_eq!(ioctl.pov, 0xFF);
}
```

- [ ] **Step 2: Implement DriverHandle**

`crates/sidewinder-app/src/driver_ipc.rs`:
```rust
use sidewinder_hid::input::{InputState, PovDirection};
use thiserror::Error;
use tracing::debug;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

const IOCTL_SIDEWINDER_UPDATE_STATE: u32 = 0x00222000;
const IOCTL_SIDEWINDER_GET_FFB: u32 = 0x00222004;

#[derive(Error, Debug)]
pub enum DriverError {
    #[error("driver not found — is the Sidewinder driver installed?")]
    NotFound,
    #[error("IOCTL failed: {0}")]
    Ioctl(#[from] windows::core::Error),
}

/// Data structure matching the driver's SidewinderInputState.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct InputStateIoctl {
    pub axes: [i16; 8],
    pub buttons: u16,
    pub pov: u8,
    pub _pad: u8,
}

impl InputStateIoctl {
    pub fn from_input_state(state: &InputState) -> Self {
        Self {
            axes: state.axes,
            buttons: state.buttons,
            pov: pov_to_byte(state.pov),
            _pad: 0,
        }
    }
}

fn pov_to_byte(pov: PovDirection) -> u8 {
    match pov {
        PovDirection::North => 0,
        PovDirection::NorthEast => 1,
        PovDirection::East => 2,
        PovDirection::SouthEast => 3,
        PovDirection::South => 4,
        PovDirection::SouthWest => 5,
        PovDirection::West => 6,
        PovDirection::NorthWest => 7,
        PovDirection::Center => 0xFF,
    }
}

/// FFB packet received from the driver.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FfbPacketIoctl {
    pub report_id: u8,
    pub data_len: u8,
    pub data: [u8; 32],
}

/// Handle to the Sidewinder virtual device driver.
pub struct DriverHandle {
    handle: HANDLE,
}

impl DriverHandle {
    /// Open a handle to the driver device.
    pub fn open() -> Result<Self, DriverError> {
        let path = r"\\.\SidewinderFFB2";
        let handle = unsafe {
            CreateFileW(
                &windows::core::HSTRING::from(path),
                (windows::Win32::Storage::FileSystem::FILE_GENERIC_READ.0
                    | windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE.0),
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
            .map_err(|_| DriverError::NotFound)?
        };

        debug!("Opened driver handle");
        Ok(Self { handle })
    }

    /// Push new input state to the virtual device.
    pub fn update_state(&self, state: &InputState) -> Result<(), DriverError> {
        let ioctl_data = InputStateIoctl::from_input_state(state);
        let mut bytes_returned = 0u32;

        unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_SIDEWINDER_UPDATE_STATE,
                Some(&ioctl_data as *const _ as *const _),
                std::mem::size_of::<InputStateIoctl>() as u32,
                None,
                0,
                Some(&mut bytes_returned),
                None,
            )?;
        }

        Ok(())
    }

    /// Read the next FFB packet from the driver (blocking).
    pub fn read_ffb_packet(&self) -> Result<FfbPacketIoctl, DriverError> {
        let mut packet = FfbPacketIoctl::default();
        let mut bytes_returned = 0u32;

        unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_SIDEWINDER_GET_FFB,
                None,
                0,
                Some(&mut packet as *mut _ as *mut _),
                std::mem::size_of::<FfbPacketIoctl>() as u32,
                Some(&mut bytes_returned),
                None,
            )?;
        }

        Ok(packet)
    }
}

impl Drop for DriverHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p sidewinder-app`
Expected: Unit tests pass (the IOCTL conversion tests don't need the driver installed).

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(app): add driver IPC layer with input state and FFB IOCTLs"
```

---

## Task 12: App — Bridge Loops

**Files:**
- Create: `crates/sidewinder-app/src/bridge.rs`
- Modify: `crates/sidewinder-app/src/main.rs`

- [ ] **Step 1: Implement the input and FFB bridge loops**

`crates/sidewinder-app/src/bridge.rs`:
```rust
use crate::driver_ipc::{DriverHandle, FfbPacketIoctl};
use crate::mapping::MappingConfig;
use sidewinder_hid::device::{DeviceError, SidewinderDevice};
use sidewinder_hid::ffb;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

/// Run the input bridge: physical device → virtual device.
///
/// Polls the physical Sidewinder at ~1ms intervals and pushes
/// the mapped state to the driver.
pub async fn input_bridge_loop(
    device: Arc<SidewinderDevice>,
    driver: Arc<DriverHandle>,
    mapping: watch::Receiver<MappingConfig>,
) {
    info!("Input bridge started");
    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(1));

    loop {
        interval.tick().await;

        let state = match device.poll() {
            Ok(state) => state,
            Err(DeviceError::Transport(_)) => {
                warn!("Device read failed, will retry...");
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                continue;
            }
            Err(e) => {
                error!("Fatal device error: {}", e);
                return;
            }
        };

        let mapped = mapping.borrow().apply(state);

        if let Err(e) = driver.update_state(&mapped) {
            error!("Failed to push state to driver: {}", e);
            return;
        }
    }
}

/// Run the FFB bridge: virtual device → physical device.
///
/// Reads FFB packets from the driver (blocking) and translates
/// them into commands for the physical Sidewinder.
pub async fn ffb_bridge_loop(
    device: Arc<SidewinderDevice>,
    driver: Arc<DriverHandle>,
) {
    info!("FFB bridge started");

    loop {
        // Read FFB packet from driver (blocking call, run in thread)
        let driver_clone = driver.clone();
        let packet = match tokio::task::spawn_blocking(move || {
            driver_clone.read_ffb_packet()
        })
        .await
        {
            Ok(Ok(packet)) => packet,
            Ok(Err(e)) => {
                error!("FFB read error: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                continue;
            }
            Err(e) => {
                error!("FFB task panicked: {}", e);
                return;
            }
        };

        if packet.data_len == 0 {
            continue;
        }

        debug!(
            report_id = packet.report_id,
            len = packet.data_len,
            "FFB packet received"
        );

        // Forward raw FFB report to the physical device
        let report_bytes = build_ffb_report(&packet);
        if let Err(e) = device.send_ffb_operation(
            ffb::FfbOperation::SetGain { gain: 0xFF } // placeholder
        ) {
            warn!("FFB send failed: {}", e);
        }
    }
}

fn build_ffb_report(packet: &FfbPacketIoctl) -> Vec<u8> {
    let mut report = Vec::with_capacity(1 + packet.data_len as usize);
    report.push(packet.report_id);
    report.extend_from_slice(&packet.data[..packet.data_len as usize]);
    report
}
```

- [ ] **Step 2: Create mapping module stub**

`crates/sidewinder-app/src/mapping.rs`:
```rust
use sidewinder_hid::input::InputState;

/// Configuration for axis mapping, deadzones, and response curves.
#[derive(Debug, Clone)]
pub struct MappingConfig {
    pub axes: [AxisConfig; 8],
}

#[derive(Debug, Clone)]
pub struct AxisConfig {
    pub deadzone: f32,
    pub curve: ResponseCurve,
    pub inverted: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum ResponseCurve {
    Linear,
    Quadratic,
    Cubic,
    SCurve,
}

impl Default for MappingConfig {
    fn default() -> Self {
        Self {
            axes: std::array::from_fn(|_| AxisConfig {
                deadzone: 0.0,
                curve: ResponseCurve::Linear,
                inverted: false,
            }),
        }
    }
}

impl MappingConfig {
    /// Apply mapping to an input state. With default config, this is 1:1 passthrough.
    pub fn apply(&self, mut state: InputState) -> InputState {
        for (i, axis_cfg) in self.axes.iter().enumerate() {
            let val = state.axes[i] as f32 / 32767.0;

            // Apply deadzone
            let val = if val.abs() < axis_cfg.deadzone {
                0.0
            } else {
                // Rescale so the range starts at the edge of the deadzone
                let sign = val.signum();
                let magnitude = (val.abs() - axis_cfg.deadzone) / (1.0 - axis_cfg.deadzone);
                sign * magnitude
            };

            // Apply response curve
            let val = match axis_cfg.curve {
                ResponseCurve::Linear => val,
                ResponseCurve::Quadratic => val.signum() * val * val,
                ResponseCurve::Cubic => val * val * val,
                ResponseCurve::SCurve => {
                    // Attempt a smooth S-curve using tanh scaling
                    let scaled = val * 2.5;
                    scaled.tanh() / (2.5f32).tanh()
                }
            };

            // Apply inversion
            let val = if axis_cfg.inverted { -val } else { val };

            state.axes[i] = (val * 32767.0).clamp(-32768.0, 32767.0) as i16;
        }

        state
    }
}
```

- [ ] **Step 3: Update main.rs with bridge task spawning**

`crates/sidewinder-app/src/main.rs`:
```rust
pub mod bridge;
pub mod config;
pub mod driver_ipc;
pub mod mapping;
pub mod tray;

use driver_ipc::DriverHandle;
use mapping::MappingConfig;
use sidewinder_hid::device::SidewinderDevice;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("sidewinder=debug")
        .init();

    info!("Sidewinder FFB2 starting...");

    // Open physical device
    let device = match SidewinderDevice::open() {
        Ok(d) => Arc::new(d),
        Err(e) => {
            error!("Failed to open Sidewinder FFB2: {}", e);
            error!("Make sure the joystick is plugged in.");
            return;
        }
    };

    // Open driver handle
    let driver = match DriverHandle::open() {
        Ok(d) => Arc::new(d),
        Err(e) => {
            error!("Failed to open virtual device driver: {}", e);
            error!("Make sure the Sidewinder driver is installed.");
            return;
        }
    };

    // Config channel (default mapping for now)
    let (mapping_tx, mapping_rx) = watch::channel(MappingConfig::default());

    // Spawn bridge tasks
    let input_handle = tokio::spawn(bridge::input_bridge_loop(
        device.clone(),
        driver.clone(),
        mapping_rx,
    ));

    let ffb_handle = tokio::spawn(bridge::ffb_bridge_loop(
        device.clone(),
        driver.clone(),
    ));

    info!("Sidewinder FFB2 running. Press Ctrl+C to quit.");

    // Wait for shutdown
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Shutting down...");
        }
        result = input_handle => {
            error!("Input bridge exited: {:?}", result);
        }
        result = ffb_handle => {
            error!("FFB bridge exited: {:?}", result);
        }
    }
}
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(app): implement input and FFB bridge loops with Tokio"
```

---

## Task 13: App — TOML Configuration

**Files:**
- Create: `crates/sidewinder-app/src/config.rs`
- Create: `config/default.toml`
- Create: `crates/sidewinder-app/tests/config_test.rs`

- [ ] **Step 1: Write config parsing tests**

`crates/sidewinder-app/tests/config_test.rs`:
```rust
use sidewinder_app::config::AppConfig;

#[test]
fn parse_default_config() {
    let toml_str = r#"
[device]
vid = "045E"
pid = "001B"

[ffb]
global_gain = 0.75
enabled = true
"#;
    let config: AppConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.device.vid, "045E");
    assert_eq!(config.ffb.global_gain, 0.75);
    assert!(config.ffb.enabled);
}

#[test]
fn parse_config_with_axis_overrides() {
    let toml_str = r#"
[device]
vid = "045E"
pid = "001B"

[ffb]
global_gain = 1.0
enabled = true

[axes.x]
deadzone = 0.05
curve = "quadratic"

[axes.throttle]
inverted = true
"#;
    let config: AppConfig = toml::from_str(toml_str).unwrap();
    let x = config.axes.get("x").unwrap();
    assert_eq!(x.deadzone, Some(0.05));
    assert_eq!(x.curve.as_deref(), Some("quadratic"));

    let throttle = config.axes.get("throttle").unwrap();
    assert_eq!(throttle.inverted, Some(true));
}

#[test]
fn missing_axes_section_is_ok() {
    let toml_str = r#"
[device]
vid = "045E"
pid = "001B"

[ffb]
global_gain = 0.5
enabled = true
"#;
    let config: AppConfig = toml::from_str(toml_str).unwrap();
    assert!(config.axes.is_empty());
}
```

- [ ] **Step 2: Implement config types**

`crates/sidewinder-app/src/config.rs`:
```rust
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("failed to read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub device: DeviceConfig,
    pub ffb: FfbConfig,
    #[serde(default)]
    pub axes: HashMap<String, AxisConfigToml>,
    #[serde(default)]
    pub buttons: HashMap<String, u8>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DeviceConfig {
    pub vid: String,
    pub pid: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FfbConfig {
    pub global_gain: f32,
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AxisConfigToml {
    pub deadzone: Option<f32>,
    pub curve: Option<String>,
    pub inverted: Option<bool>,
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)?;
        info!("Loaded config from {}", path.display());
        Ok(config)
    }

    pub fn load_or_default(path: &Path) -> Self {
        match Self::load(path) {
            Ok(config) => config,
            Err(e) => {
                warn!("Failed to load config ({}), using defaults", e);
                Self::default()
            }
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            device: DeviceConfig {
                vid: String::from("045E"),
                pid: String::from("001B"),
            },
            ffb: FfbConfig {
                global_gain: 0.75,
                enabled: true,
            },
            axes: HashMap::new(),
            buttons: HashMap::new(),
        }
    }
}
```

- [ ] **Step 3: Create default config file**

`config/default.toml`:
```toml
[device]
vid = "045E"
pid = "001B"

[ffb]
global_gain = 0.75
enabled = true

# Axis overrides (optional — defaults to 1:1 passthrough)
# [axes.x]
# deadzone = 0.02
# curve = "linear"    # "linear" | "quadratic" | "cubic" | "s-curve"
# inverted = false

# [axes.throttle]
# inverted = true
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sidewinder-app`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(app): add TOML config parsing with axis override support"
```

---

## Task 14: Diagnostic TUI — Physical Device Mode

**Files:**
- Modify: `crates/sidewinder-diag/src/main.rs`
- Create: `crates/sidewinder-diag/src/ui.rs`
- Create: `crates/sidewinder-diag/src/widgets.rs`
- Create: `crates/sidewinder-diag/src/event_log.rs`

This task builds the `sidewinder-diag physical` mode — a live TUI showing raw joystick input.

- [ ] **Step 1: Implement the event log**

`crates/sidewinder-diag/src/event_log.rs`:
```rust
use std::collections::VecDeque;

const MAX_LOG_ENTRIES: usize = 200;

pub struct EventLog {
    entries: VecDeque<LogEntry>,
}

pub struct LogEntry {
    pub timestamp: String,
    pub message: String,
}

impl EventLog {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_LOG_ENTRIES),
        }
    }

    pub fn push(&mut self, message: String) {
        let now = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
        if self.entries.len() >= MAX_LOG_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(LogEntry {
            timestamp: now,
            message,
        });
    }

    pub fn entries(&self) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
```

Note: Add `chrono = "0.4"` to `sidewinder-diag/Cargo.toml` dependencies.

- [ ] **Step 2: Implement axis bar and button widgets**

`crates/sidewinder-diag/src/widgets.rs`:
```rust
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use sidewinder_hid::input::InputState;

const AXIS_NAMES: [&str; 8] = ["X", "Y", "Throttle", "Rudder", "Slider", "Rx", "Ry", "Dial"];

pub fn render_axes(state: &InputState, area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .title(" Axes ")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    block.render(area, buf);

    let axis_height = 1;
    for (i, name) in AXIS_NAMES.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }
        let y = inner.y + i as u16;
        let val = state.axes[i];
        let normalized = (val as f64 + 32768.0) / 65535.0; // 0.0 to 1.0

        // Name label
        let label = format!("{:>8}: ", name);
        let label_area = Rect::new(inner.x, y, 10, 1);
        Paragraph::new(label).render(label_area, buf);

        // Bar
        let bar_width = inner.width.saturating_sub(18);
        let bar_area = Rect::new(inner.x + 10, y, bar_width, 1);
        Gauge::default()
            .ratio(normalized.clamp(0.0, 1.0))
            .render(bar_area, buf);

        // Value
        let val_str = format!("{:>6}", val);
        let val_area = Rect::new(inner.x + 10 + bar_width + 1, y, 7, 1);
        Paragraph::new(val_str).render(val_area, buf);
    }
}

pub fn render_buttons(state: &InputState, area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .title(" Buttons ")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    block.render(area, buf);

    let mut labels = String::new();
    for i in 0..9u8 {
        if state.is_button_pressed(i) {
            labels.push_str(&format!("[{}] ", i + 1));
        } else {
            labels.push_str(&format!(" .  "));
        }
    }

    Paragraph::new(labels).render(inner, buf);
}

pub fn render_pov(state: &InputState, area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .title(" POV ")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    block.render(area, buf);

    let pov_str = format!("{:?}", state.pov);
    Paragraph::new(pov_str).render(inner, buf);
}
```

- [ ] **Step 3: Implement the main UI layout**

`crates/sidewinder-diag/src/ui.rs`:
```rust
use crate::event_log::EventLog;
use crate::widgets;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use sidewinder_hid::input::InputState;

pub fn draw(frame: &mut Frame, state: &InputState, log: &EventLog, device_status: &str) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Length(10), // Axes
            Constraint::Length(3),  // Buttons
            Constraint::Length(3),  // POV
            Constraint::Min(5),    // Event log
        ])
        .split(frame.area());

    // Header
    let header = Paragraph::new(format!(
        " Sidewinder FFB2 Diagnostics | {}",
        device_status
    ))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(header, chunks[0]);

    // Axes
    widgets::render_axes(state, chunks[1], frame.buffer_mut());

    // Buttons
    widgets::render_buttons(state, chunks[2], frame.buffer_mut());

    // POV
    widgets::render_pov(state, chunks[3], frame.buffer_mut());

    // Event log
    let log_block = Block::default()
        .title(" Event Log ")
        .borders(Borders::ALL);
    let log_inner = log_block.inner(chunks[4]);
    frame.render_widget(log_block, chunks[4]);

    let log_text: Vec<Line> = log
        .entries()
        .rev()
        .take(log_inner.height as usize)
        .map(|entry| Line::from(format!("{}  {}", entry.timestamp, entry.message)))
        .collect();

    let log_paragraph = Paragraph::new(log_text).wrap(Wrap { trim: true });
    frame.render_widget(log_paragraph, log_inner);
}
```

- [ ] **Step 4: Wire up the main loop for physical mode**

Update `crates/sidewinder-diag/src/main.rs`:
```rust
mod event_log;
mod ui;
mod widgets;

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use event_log::EventLog;
use ratatui::prelude::*;
use sidewinder_hid::device::SidewinderDevice;
use sidewinder_hid::input::InputState;
use std::io::stdout;
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "sidewinder-diag", about = "Sidewinder FFB2 Diagnostics")]
struct Cli {
    #[command(subcommand)]
    mode: Mode,
}

#[derive(clap::Subcommand, Debug)]
enum Mode {
    /// Show raw physical device input
    Physical,
    /// Show virtual device state
    Virtual,
    /// Show full pipeline with FFB flow
    Full,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.mode {
        Mode::Physical => run_physical_mode().await?,
        Mode::Virtual => {
            eprintln!("Virtual mode not yet implemented");
        }
        Mode::Full => {
            eprintln!("Full mode not yet implemented");
        }
    }

    Ok(())
}

async fn run_physical_mode() -> Result<(), Box<dyn std::error::Error>> {
    // Open device
    let device = SidewinderDevice::open()?;
    let mut log = EventLog::new();
    let mut prev_state = InputState::default();

    log.push("Connected to Sidewinder FFB2".to_string());

    // Set up terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    loop {
        // Poll for input
        let state = match device.poll() {
            Ok(s) => s,
            Err(e) => {
                log.push(format!("Read error: {}", e));
                prev_state
            }
        };

        // Log changes
        for i in 0..9u8 {
            let was = prev_state.is_button_pressed(i);
            let now = state.is_button_pressed(i);
            if now && !was {
                log.push(format!("Button {} pressed", i + 1));
            } else if !now && was {
                log.push(format!("Button {} released", i + 1));
            }
        }
        if state.pov != prev_state.pov {
            log.push(format!("POV: {:?}", state.pov));
        }
        prev_state = state;

        // Draw
        terminal.draw(|f| ui::draw(f, &state, &log, "Physical Device"))?;

        // Check for quit key
        if event::poll(Duration::from_millis(1))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
                    break;
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}
```

- [ ] **Step 5: Add chrono dependency**

Update `crates/sidewinder-diag/Cargo.toml` to add `chrono = "0.4"`.

- [ ] **Step 6: Verify it compiles**

Run: `cargo build -p sidewinder-diag`
Expected: Compiles successfully.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(diag): implement physical device diagnostic TUI"
```

---

## Task 15: App — System Tray

**Files:**
- Create: `crates/sidewinder-app/src/tray.rs`
- Modify: `crates/sidewinder-app/Cargo.toml`

- [ ] **Step 1: Add tray-icon dependency**

Add to `crates/sidewinder-app/Cargo.toml`:
```toml
tray-icon = "0.19"
```

- [ ] **Step 2: Implement system tray**

`crates/sidewinder-app/src/tray.rs`:
```rust
use tokio::sync::watch;
use tracing::{error, info};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayStatus {
    Connected,
    Disconnected,
    DriverMissing,
}

#[derive(Debug, Clone)]
pub enum TrayCommand {
    Quit,
    ToggleFfb,
    OpenConfig,
}

/// Run the system tray UI.
///
/// This must be called from the main thread on Windows (Win32 message pump).
/// It blocks until the user quits.
pub fn run_tray(
    status_rx: watch::Receiver<TrayStatus>,
    command_tx: tokio::sync::mpsc::UnboundedSender<TrayCommand>,
) {
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem},
        TrayIconBuilder,
    };

    let menu = Menu::new();
    let toggle_ffb = MenuItem::new("Toggle Force Feedback", true, None);
    let open_config = MenuItem::new("Open Config", true, None);
    let quit = MenuItem::new("Quit", true, None);

    let toggle_id = toggle_ffb.id().clone();
    let config_id = open_config.id().clone();
    let quit_id = quit.id().clone();

    let _ = menu.append(&toggle_ffb);
    let _ = menu.append(&open_config);
    let _ = menu.append(&quit);

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Sidewinder FFB2")
        .build();

    info!("System tray running");

    // Win32 message loop
    let menu_channel = MenuEvent::receiver();
    loop {
        if let Ok(event) = menu_channel.try_recv() {
            if event.id == quit_id {
                let _ = command_tx.send(TrayCommand::Quit);
                break;
            } else if event.id == toggle_id {
                let _ = command_tx.send(TrayCommand::ToggleFfb);
            } else if event.id == config_id {
                let _ = command_tx.send(TrayCommand::OpenConfig);
            }
        }

        // Brief sleep to avoid busy-waiting
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
```

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(app): add system tray with FFB toggle and config access"
```

---

## Task 16: End-to-End Integration

**Files:**
- Modify: `crates/sidewinder-app/src/main.rs`

This task wires everything together: config loading, device connection with retry, tray UI, and graceful shutdown.

- [ ] **Step 1: Update main.rs with full integration**

Integrate config loading, tray commands, FFB toggle, and config hot-reload into the main function. Use `notify` crate for file watching.

- [ ] **Step 2: Manual end-to-end test**

With the driver installed and joystick plugged in:
1. `cargo run -p sidewinder-app`
2. Verify virtual device appears in `joy.cpl`
3. Move joystick axes, verify virtual device mirrors them
4. Run a game with FFB, verify effects reach the physical joystick
5. Modify `sidewinder.toml`, verify changes apply without restart

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(app): wire up full integration with config, tray, and retry logic"
```

---

## Task 17: Diagnostic TUI — Full Pipeline Mode

**Files:**
- Modify: `crates/sidewinder-diag/src/main.rs`
- Modify: `crates/sidewinder-diag/src/ui.rs`
- Modify: `crates/sidewinder-diag/src/widgets.rs`

- [ ] **Step 1: Add FFB slot display widget**

Add to `crates/sidewinder-diag/src/widgets.rs`:
```rust
pub fn render_ffb_slots(slots: &[FfbSlotInfo], area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .title(" FFB Effects (virtual -> physical) ")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    block.render(area, buf);

    for (i, slot) in slots.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }
        let y = inner.y + i as u16;
        let text = match &slot.effect_type {
            Some(name) => format!(
                "  Slot {:>2}: {:<16} mag={:<6} dir={:>3}deg  {}",
                i, name, slot.magnitude, slot.direction,
                if slot.playing { "PLAYING" } else { "STOPPED" }
            ),
            None => format!("  Slot {:>2}: (empty)", i),
        };
        Paragraph::new(text).render(Rect::new(inner.x, y, inner.width, 1), buf);
    }
}

pub struct FfbSlotInfo {
    pub effect_type: Option<String>,
    pub magnitude: i16,
    pub direction: u16,
    pub playing: bool,
}
```

- [ ] **Step 2: Implement full pipeline mode**

Wire up the `full` mode in `main.rs` to show both physical input, virtual state, and FFB flow using the driver IPC layer.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(diag): implement full pipeline mode with FFB visualization"
```

---

## Task 18: Driver Installer Script

**Files:**
- Create: `scripts/install-driver.ps1`
- Create: `scripts/uninstall-driver.ps1`

- [ ] **Step 1: Create PowerShell install script**

`scripts/install-driver.ps1`:
```powershell
#Requires -RunAsAdministrator

$ErrorActionPreference = "Stop"

$driverDir = "$PSScriptRoot\..\crates\sidewinder-driver\target\x86_64-pc-windows-msvc\release\package"
$infPath = "$driverDir\sidewinder.inf"

if (-not (Test-Path $infPath)) {
    Write-Error "Driver not found at $infPath. Build the driver first."
    exit 1
}

Write-Host "Installing Sidewinder FFB2 virtual device driver..."

# Install the driver package
pnputil /add-driver $infPath /install

# Create the device node
$devconPath = "C:\Program Files (x86)\Windows Kits\10\Tools\x64\devcon.exe"
if (Test-Path $devconPath) {
    & $devconPath install $infPath "Root\SidewinderFFB2"
} else {
    Write-Warning "devcon.exe not found. Install the device manually via Device Manager."
    Write-Host "Hardware ID: Root\SidewinderFFB2"
}

Write-Host "Driver installed successfully."
```

- [ ] **Step 2: Create uninstall script**

`scripts/uninstall-driver.ps1`:
```powershell
#Requires -RunAsAdministrator

Write-Host "Removing Sidewinder FFB2 virtual device..."

$devconPath = "C:\Program Files (x86)\Windows Kits\10\Tools\x64\devcon.exe"
if (Test-Path $devconPath) {
    & $devconPath remove "Root\SidewinderFFB2"
}

# Remove the driver package
$packages = pnputil /enum-drivers | Select-String -Pattern "sidewinder" -Context 0, 5
foreach ($match in $packages) {
    $oem = ($match.Context.PreContext + $match.Line + $match.Context.PostContext) -match "(oem\d+\.inf)"
    if ($Matches[1]) {
        pnputil /delete-driver $Matches[1] /force
    }
}

Write-Host "Driver removed."
```

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat: add PowerShell scripts for driver install/uninstall"
```
