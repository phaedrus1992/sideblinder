//! HID report descriptor for the Sidewinder Force Feedback 2 virtual device.
//!
//! The descriptor declares:
//!
//! - **Input reports** (Report ID 0): axes (X, Y, Z, Rz), buttons 1-9, hat
//! - **PID force-feedback output/feature reports** (Report IDs 0x01–0x0F):
//!   Set Effect, Set Envelope, Set Condition, Set Periodic, Set Constant Force,
//!   Set Ramp Force, Custom Force, Effect Operation, Block Free, Device Control,
//!   Device Gain, Create New Effect, Block Load, PID State
//!
//! Returned to HIDCLASS in response to `IOCTL_HID_GET_REPORT_DESCRIPTOR`.

// ── HID constants ─────────────────────────────────────────────────────────────

// Usage Pages
const UP_GENERIC_DESKTOP: u8 = 0x01;
const UP_SIMULATION: u8 = 0x02;
const UP_PID: u8 = 0x0F;

// Generic Desktop usages
const GD_JOYSTICK: u8 = 0x04;
const GD_X: u8 = 0x30;
const GD_Y: u8 = 0x31;
const GD_Z: u8 = 0x32;
const GD_RZ: u8 = 0x35;
const GD_SLIDER: u8 = 0x36;
const GD_HAT_SWITCH: u8 = 0x39;

// Simulation usages
const SIM_THROTTLE: u8 = 0xBB;

// PID usages
const PID_SET_EFFECT_REPORT: u8 = 0x21;
const PID_EFFECT_BLOCK_INDEX: u8 = 0x22;
const PID_PARAM_BLOCK_OFFSET: u8 = 0x23;
const PID_ROM_FLAG: u8 = 0x24;
const PID_EFFECT_TYPE: u8 = 0x25;
const PID_ET_CONSTANT_FORCE: u8 = 0x26;
const PID_ET_RAMP: u8 = 0x27;
const PID_ET_SQUARE: u8 = 0x30;
const PID_ET_SINE: u8 = 0x31;
const PID_ET_TRIANGLE: u8 = 0x32;
const PID_ET_SAWTOOTH_UP: u8 = 0x33;
const PID_ET_SAWTOOTH_DOWN: u8 = 0x34;
const PID_ET_SPRING: u8 = 0x40;
const PID_ET_DAMPER: u8 = 0x41;
const PID_ET_INERTIA: u8 = 0x42;
const PID_ET_FRICTION: u8 = 0x43;
const PID_ET_CUSTOM_FORCE_DATA: u8 = 0x50;
const PID_AXES_ENABLE: u8 = 0x55;
const PID_DIRECTION_ENABLE: u8 = 0x56;
const PID_DIRECTION: u8 = 0x57;
const PID_TYPE_SPECIFIC_BLOCK_OFFSET: u8 = 0x58;
const PID_BLOCK_TYPE: u8 = 0x59;
const PID_SET_ENVELOPE_REPORT: u8 = 0x5A;
const PID_ATTACK_LEVEL: u8 = 0x5B;
const PID_ATTACK_TIME: u8 = 0x5C;
const PID_FADE_LEVEL: u8 = 0x5D;
const PID_FADE_TIME: u8 = 0x5E;
const PID_SET_CONDITION_REPORT: u8 = 0x5F;
const PID_CP_OFFSET: u8 = 0x60;
const PID_POSITIVE_COEFFICIENT: u8 = 0x61;
const PID_NEGATIVE_COEFFICIENT: u8 = 0x62;
const PID_POSITIVE_SATURATION: u8 = 0x63;
const PID_NEGATIVE_SATURATION: u8 = 0x64;
const PID_DEAD_BAND: u8 = 0x65;
const PID_DOWNLOAD_FORCE_SAMPLE: u8 = 0x66;
const PID_ISOCH_CUSTOM_FORCE_ENABLE: u8 = 0x67;
const PID_CUSTOM_FORCE_DATA_REPORT: u8 = 0x68;
const PID_CUSTOM_FORCE_DATA: u8 = 0x69;
const PID_CUSTOM_FORCE_VENDOR_DEFINED_DATA: u8 = 0x6A;
const PID_SET_CUSTOM_FORCE_REPORT: u8 = 0x6B;
const PID_CUSTOM_FORCE_DATA_CHUNK_OFFSET: u8 = 0x6C;
const PID_CUSTOM_FORCE_DATA_CHUNK_COUNT: u8 = 0x6D;
const PID_EFFECT_OPERATION_REPORT: u8 = 0x77;
const PID_EFFECT_OPERATION: u8 = 0x78;
const PID_OP_EFFECT_START: u8 = 0x01;
const PID_OP_EFFECT_START_SOLO: u8 = 0x02;
const PID_OP_EFFECT_STOP: u8 = 0x03;
const PID_LOOP_COUNT: u8 = 0x7C;
const PID_BLOCK_FREE_REPORT: u8 = 0x90;
const PID_TYPE_SPECIFIC_BLOCK_HANDLE: u8 = 0x91;
const PID_DEVICE_CONTROL_REPORT: u8 = 0x96;
const PID_DEVICE_CONTROL: u8 = 0x97;
const PID_DC_ENABLE_ACTUATORS: u8 = 0x98;
const PID_DC_DISABLE_ACTUATORS: u8 = 0x99;
const PID_DC_STOP_ALL_EFFECTS: u8 = 0x9A;
const PID_DC_DEVICE_RESET: u8 = 0x9B;
const PID_DC_DEVICE_PAUSE: u8 = 0x9C;
const PID_DC_DEVICE_CONTINUE: u8 = 0x9D;
const PID_DEVICE_GAIN_REPORT: u8 = 0x9E;
const PID_DEVICE_GAIN: u8 = 0x9F;
const PID_PID_STATE_REPORT: u8 = 0xA3;
const PID_EFFECT_PLAYING: u8 = 0xA8;
const PID_ACTUATORS_ENABLED: u8 = 0xA9;
const PID_PAUSED: u8 = 0xAA;
const PID_SAFETY_SWITCH: u8 = 0xAB;
const PID_ACTUATOR_POWER: u8 = 0xAC;
const PID_CREATE_NEW_EFFECT: u8 = 0xAB;
const PID_RAM_POOL_AVAILABLE: u8 = 0xAC;
const PID_MAGNITUDE: u8 = 0x70;
const PID_OFFSET: u8 = 0x71;
const PID_PHASE: u8 = 0x72;
const PID_PERIOD: u8 = 0x73;
const PID_START_DELAY: u8 = 0xA7;
const PID_TRIGGER_BUTTON: u8 = 0xA4;
const PID_TRIGGER_REPEAT_INTERVAL: u8 = 0xA5;
const PID_DURATION: u8 = 0xA6;
const PID_SAMPLE_PERIOD: u8 = 0x28;
const PID_GAIN: u8 = 0x6F;
const PID_SET_PERIODIC_REPORT: u8 = 0x6E;
const PID_SET_CONSTANT_FORCE_REPORT: u8 = 0x73;
const PID_MAGNITUDE_CF: u8 = 0x70;
const PID_SET_RAMP_FORCE_REPORT: u8 = 0x74;
const PID_RAMP_START: u8 = 0x75;
const PID_RAMP_END: u8 = 0x76;

// Button page
const UP_BUTTON: u8 = 0x09;

// Item tags (short-form)
const INPUT: u8 = 0x81;
const OUTPUT: u8 = 0x91;
const FEATURE: u8 = 0xB1;
const COLLECTION: u8 = 0xA1;
const END_COLLECTION: u8 = 0xC0;
const USAGE: u8 = 0x09;
const USAGE_PAGE: u8 = 0x05;
const USAGE_MIN: u8 = 0x19;
const USAGE_MAX: u8 = 0x29;
const LOGICAL_MIN: u8 = 0x15;
const LOGICAL_MAX: u8 = 0x25;
const LOGICAL_MAX_32: u8 = 0x27; // 4-byte logical max
const LOGICAL_MIN_32: u8 = 0x17; // 4-byte logical min (signed)
const PHYSICAL_MIN: u8 = 0x35;
const PHYSICAL_MAX: u8 = 0x45;
const UNIT: u8 = 0x55;
const UNIT_EXPONENT: u8 = 0x55;
const REPORT_ID: u8 = 0x85;
const REPORT_SIZE: u8 = 0x75;
const REPORT_COUNT: u8 = 0x95;

// Data flags
const DATA_VAR_ABS: u8 = 0x02;
const DATA_VAR_REL: u8 = 0x06;
const CONST_VAR_ABS: u8 = 0x03;
const DATA_ARR_ABS: u8 = 0x00;
const NARY: u8 = 0x20;

// Collection types
const COL_APPLICATION: u8 = 0x01;
const COL_LOGICAL: u8 = 0x02;
const COL_PHYSICAL: u8 = 0x00;

// ── Descriptor ────────────────────────────────────────────────────────────────

/// Full HID report descriptor for the Sidewinder FF2 virtual device.
///
/// This is a static byte array; the driver hands it verbatim to HIDCLASS
/// in response to `IOCTL_HID_GET_REPORT_DESCRIPTOR`.
#[rustfmt::skip]
pub static REPORT_DESCRIPTOR: &[u8] = &[
    // ── Joystick application collection ─────────────────────────────────────
    USAGE_PAGE, UP_GENERIC_DESKTOP,
    USAGE,      GD_JOYSTICK,
    COLLECTION, COL_APPLICATION,

        // Input report (no report ID — report ID 0)
        // Axes: X, Y, Z (throttle), Rz (rudder) — 16-bit signed, ±32767
        USAGE,        GD_X,
        USAGE,        GD_Y,
        USAGE,        GD_Z,
        USAGE,        GD_RZ,
        LOGICAL_MIN_32, 0x00, 0x80, 0xFF, 0xFF,    // -32768 as i32
        LOGICAL_MAX_32, 0xFF, 0x7F, 0x00, 0x00,    // +32767 as i32
        PHYSICAL_MIN, 0x00,
        PHYSICAL_MAX, 0x00,
        REPORT_SIZE,  0x10,   // 16 bits
        REPORT_COUNT, 0x04,   // 4 axes
        INPUT,        DATA_VAR_ABS,

        // Buttons 1–9 (1-bit each, 7 padding bits)
        USAGE_PAGE,   UP_BUTTON,
        USAGE_MIN,    0x01,
        USAGE_MAX,    0x09,
        LOGICAL_MIN,  0x00,
        LOGICAL_MAX,  0x01,
        REPORT_SIZE,  0x01,
        REPORT_COUNT, 0x09,
        INPUT,        DATA_VAR_ABS,
        // 7 pad bits to complete the byte
        REPORT_SIZE,  0x01,
        REPORT_COUNT, 0x07,
        INPUT,        CONST_VAR_ABS,

        // Hat switch — 4-bit null-capable (0=N..7=NW, 0xF=null)
        USAGE_PAGE,   UP_GENERIC_DESKTOP,
        USAGE,        GD_HAT_SWITCH,
        LOGICAL_MIN,  0x00,
        LOGICAL_MAX,  0x07,
        PHYSICAL_MIN, 0x00,
        PHYSICAL_MAX, 0x08, // 0–315 degrees in 45° steps; OS maps 0xF→null
        REPORT_SIZE,  0x04,
        REPORT_COUNT, 0x01,
        INPUT,        DATA_VAR_ABS,
        // 4 pad bits
        REPORT_SIZE,  0x04,
        REPORT_COUNT, 0x01,
        INPUT,        CONST_VAR_ABS,

    // ── PID force-feedback reports ────────────────────────────────────────────
    USAGE_PAGE, UP_PID,

        // ── Set Effect (Output, Report ID 0x01) ──────────────────────────────
        USAGE,      PID_SET_EFFECT_REPORT,
        COLLECTION, COL_LOGICAL,
            REPORT_ID,    0x01,

            USAGE,        PID_EFFECT_BLOCK_INDEX,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x28,   // max 40 effects
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            USAGE,        PID_PARAM_BLOCK_OFFSET,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX,  0x7F,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            // Effect type nominal array (11 named values + custom)
            USAGE,        PID_EFFECT_TYPE,
            USAGE,        PID_ET_CONSTANT_FORCE,
            USAGE,        PID_ET_RAMP,
            USAGE,        PID_ET_SQUARE,
            USAGE,        PID_ET_SINE,
            USAGE,        PID_ET_TRIANGLE,
            USAGE,        PID_ET_SAWTOOTH_UP,
            USAGE,        PID_ET_SAWTOOTH_DOWN,
            USAGE,        PID_ET_SPRING,
            USAGE,        PID_ET_DAMPER,
            USAGE,        PID_ET_INERTIA,
            USAGE,        PID_ET_FRICTION,
            USAGE,        PID_ET_CUSTOM_FORCE_DATA,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x0C,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_ARR_ABS | NARY,

            // Duration (ms, u16)
            USAGE,        PID_DURATION,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX_32, 0xFF, 0xFF, 0x00, 0x00,
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            // Sample period (ms, u16)
            USAGE,        PID_SAMPLE_PERIOD,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX_32, 0xFF, 0xFF, 0x00, 0x00,
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            // Gain (u8)
            USAGE,        PID_GAIN,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX,  0xFF,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            // Direction (hundredths of a degree, u16)
            USAGE,        PID_DIRECTION,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX_32, 0x2F, 0x8C, 0x00, 0x00,  // 35887 (max 359.99°)
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            // Trigger button (0-based, 0xFF = none)
            USAGE,        PID_TRIGGER_BUTTON,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX,  0xFF,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            // Trigger repeat interval (ms, u16)
            USAGE,        PID_TRIGGER_REPEAT_INTERVAL,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX_32, 0xFF, 0xFF, 0x00, 0x00,
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            // Start delay (ms, u16)
            USAGE,        PID_START_DELAY,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX_32, 0xFF, 0xFF, 0x00, 0x00,
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,
        END_COLLECTION,

        // ── Set Envelope (Output, Report ID 0x02) ────────────────────────────
        USAGE,      PID_SET_ENVELOPE_REPORT,
        COLLECTION, COL_LOGICAL,
            REPORT_ID,    0x02,

            USAGE,        PID_EFFECT_BLOCK_INDEX,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x28,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            USAGE,        PID_ATTACK_LEVEL,
            USAGE,        PID_FADE_LEVEL,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX_32, 0xFF, 0x7F, 0x00, 0x00,  // 32767
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x02,
            OUTPUT,       DATA_VAR_ABS,

            USAGE,        PID_ATTACK_TIME,
            USAGE,        PID_FADE_TIME,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX_32, 0xFF, 0xFF, 0x00, 0x00,
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x02,
            OUTPUT,       DATA_VAR_ABS,
        END_COLLECTION,

        // ── Set Condition (Output, Report ID 0x03) ───────────────────────────
        USAGE,      PID_SET_CONDITION_REPORT,
        COLLECTION, COL_LOGICAL,
            REPORT_ID,    0x03,

            USAGE,        PID_EFFECT_BLOCK_INDEX,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x28,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            USAGE,        PID_CP_OFFSET,
            USAGE,        PID_POSITIVE_COEFFICIENT,
            USAGE,        PID_NEGATIVE_COEFFICIENT,
            LOGICAL_MIN_32, 0x00, 0x80, 0xFF, 0xFF,    // -32768
            LOGICAL_MAX_32, 0xFF, 0x7F, 0x00, 0x00,    // +32767
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x03,
            OUTPUT,       DATA_VAR_ABS,

            USAGE,        PID_POSITIVE_SATURATION,
            USAGE,        PID_NEGATIVE_SATURATION,
            USAGE,        PID_DEAD_BAND,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX_32, 0xFF, 0x7F, 0x00, 0x00,
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x03,
            OUTPUT,       DATA_VAR_ABS,
        END_COLLECTION,

        // ── Set Periodic (Output, Report ID 0x04) ────────────────────────────
        USAGE,      PID_SET_PERIODIC_REPORT,
        COLLECTION, COL_LOGICAL,
            REPORT_ID,    0x04,

            USAGE,        PID_EFFECT_BLOCK_INDEX,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x28,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            USAGE,        PID_MAGNITUDE,
            USAGE,        PID_OFFSET,
            LOGICAL_MIN_32, 0x00, 0x80, 0xFF, 0xFF,
            LOGICAL_MAX_32, 0xFF, 0x7F, 0x00, 0x00,
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x02,
            OUTPUT,       DATA_VAR_ABS,

            USAGE,        PID_PHASE,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX_32, 0x2F, 0x8C, 0x00, 0x00,  // 35887
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            USAGE,        PID_PERIOD,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX_32, 0xFF, 0xFF, 0x00, 0x00,
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,
        END_COLLECTION,

        // ── Set Constant Force (Output, Report ID 0x05) ──────────────────────
        USAGE,      PID_SET_CONSTANT_FORCE_REPORT,
        COLLECTION, COL_LOGICAL,
            REPORT_ID,    0x05,

            USAGE,        PID_EFFECT_BLOCK_INDEX,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x28,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            USAGE,        PID_MAGNITUDE_CF,
            LOGICAL_MIN_32, 0x00, 0x80, 0xFF, 0xFF,
            LOGICAL_MAX_32, 0xFF, 0x7F, 0x00, 0x00,
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,
        END_COLLECTION,

        // ── Set Ramp Force (Output, Report ID 0x06) ──────────────────────────
        USAGE,      PID_SET_RAMP_FORCE_REPORT,
        COLLECTION, COL_LOGICAL,
            REPORT_ID,    0x06,

            USAGE,        PID_EFFECT_BLOCK_INDEX,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x28,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            USAGE,        PID_RAMP_START,
            USAGE,        PID_RAMP_END,
            LOGICAL_MIN_32, 0x00, 0x80, 0xFF, 0xFF,
            LOGICAL_MAX_32, 0xFF, 0x7F, 0x00, 0x00,
            REPORT_SIZE,  0x10,
            REPORT_COUNT, 0x02,
            OUTPUT,       DATA_VAR_ABS,
        END_COLLECTION,

        // ── Effect Operation (Output, Report ID 0x0A) ─────────────────────────
        USAGE,      PID_EFFECT_OPERATION_REPORT,
        COLLECTION, COL_LOGICAL,
            REPORT_ID,    0x0A,

            USAGE,        PID_EFFECT_BLOCK_INDEX,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x28,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,

            USAGE,        PID_EFFECT_OPERATION,
            USAGE,        PID_OP_EFFECT_START,
            USAGE,        PID_OP_EFFECT_START_SOLO,
            USAGE,        PID_OP_EFFECT_STOP,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x03,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_ARR_ABS | NARY,

            USAGE,        PID_LOOP_COUNT,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX,  0xFF,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,
        END_COLLECTION,

        // ── Block Free (Output, Report ID 0x0B) ──────────────────────────────
        USAGE,      PID_BLOCK_FREE_REPORT,
        COLLECTION, COL_LOGICAL,
            REPORT_ID,    0x0B,

            USAGE,        PID_EFFECT_BLOCK_INDEX,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x28,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,
        END_COLLECTION,

        // ── Device Control (Output, Report ID 0x0C) ──────────────────────────
        USAGE,      PID_DEVICE_CONTROL_REPORT,
        COLLECTION, COL_LOGICAL,
            REPORT_ID,    0x0C,

            USAGE,        PID_DEVICE_CONTROL,
            USAGE,        PID_DC_ENABLE_ACTUATORS,
            USAGE,        PID_DC_DISABLE_ACTUATORS,
            USAGE,        PID_DC_STOP_ALL_EFFECTS,
            USAGE,        PID_DC_DEVICE_RESET,
            USAGE,        PID_DC_DEVICE_PAUSE,
            USAGE,        PID_DC_DEVICE_CONTINUE,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x06,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_ARR_ABS | NARY,
        END_COLLECTION,

        // ── Device Gain (Output, Report ID 0x0D) ─────────────────────────────
        USAGE,      PID_DEVICE_GAIN_REPORT,
        COLLECTION, COL_LOGICAL,
            REPORT_ID,    0x0D,

            USAGE,        PID_DEVICE_GAIN,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX,  0xFF,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            OUTPUT,       DATA_VAR_ABS,
        END_COLLECTION,

        // ── PID State (Input, Report ID 0x02) ────────────────────────────────
        USAGE,      PID_PID_STATE_REPORT,
        COLLECTION, COL_LOGICAL,
            REPORT_ID,    0x02,

            USAGE,        PID_EFFECT_BLOCK_INDEX,
            LOGICAL_MIN,  0x01,
            LOGICAL_MAX,  0x28,
            REPORT_SIZE,  0x08,
            REPORT_COUNT, 0x01,
            INPUT,        DATA_VAR_ABS,

            USAGE,        PID_EFFECT_PLAYING,
            USAGE,        PID_ACTUATORS_ENABLED,
            USAGE,        PID_PAUSED,
            USAGE,        PID_SAFETY_SWITCH,
            USAGE,        PID_ACTUATOR_POWER,
            LOGICAL_MIN,  0x00,
            LOGICAL_MAX,  0x01,
            REPORT_SIZE,  0x01,
            REPORT_COUNT, 0x05,
            INPUT,        DATA_VAR_ABS,
            // 3 pad bits
            REPORT_SIZE,  0x01,
            REPORT_COUNT, 0x03,
            INPUT,        CONST_VAR_ABS,
        END_COLLECTION,

    END_COLLECTION,   // Application/Joystick
];

/// Length of the report descriptor in bytes.
pub const REPORT_DESCRIPTOR_LEN: usize = REPORT_DESCRIPTOR.len();

// ── HID Class descriptor header ───────────────────────────────────────────────

/// The 9-byte HID class descriptor that precedes the report descriptor in the
/// HID descriptor returned by `IOCTL_HID_GET_DEVICE_DESCRIPTOR`.
#[repr(C, packed)]
pub struct HidClassDescriptor {
    /// Total length of this structure.
    pub length: u8,
    /// Descriptor type (0x21 = HID).
    pub descriptor_type: u8,
    /// HID specification release (1.11 → 0x0111, little-endian).
    pub hid_cd: u16,
    /// Country code (0 = not localised).
    pub country_code: u8,
    /// Number of subordinate descriptors.
    pub num_descriptors: u8,
    /// Type of the first subordinate descriptor (0x22 = Report).
    pub report_descriptor_type: u8,
    /// Length of the report descriptor.
    pub report_descriptor_length: u16,
}

impl HidClassDescriptor {
    /// Build the HID class descriptor for this device.
    pub const fn new() -> Self {
        Self {
            length: core::mem::size_of::<Self>() as u8,
            descriptor_type: 0x21,
            hid_cd: 0x0111_u16.to_le(),
            country_code: 0,
            num_descriptors: 1,
            report_descriptor_type: 0x22,
            report_descriptor_length: (REPORT_DESCRIPTOR_LEN as u16).to_le(),
        }
    }
}

impl Default for HidClassDescriptor {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_descriptor_is_nonempty() {
        assert!(!REPORT_DESCRIPTOR.is_empty());
    }

    #[test]
    fn report_descriptor_starts_with_usage_page_generic_desktop() {
        assert_eq!(REPORT_DESCRIPTOR[0], USAGE_PAGE);
        assert_eq!(REPORT_DESCRIPTOR[1], UP_GENERIC_DESKTOP);
    }

    #[test]
    fn report_descriptor_ends_with_end_collection() {
        assert_eq!(*REPORT_DESCRIPTOR.last().unwrap(), END_COLLECTION);
    }

    #[test]
    fn hid_class_descriptor_size() {
        assert_eq!(core::mem::size_of::<HidClassDescriptor>(), 9);
    }

    #[test]
    fn hid_class_descriptor_fields() {
        let d = HidClassDescriptor::new();
        assert_eq!(d.length, 9);
        assert_eq!(d.descriptor_type, 0x21);
        assert_eq!(u16::from_le(d.hid_cd), 0x0111);
        assert_eq!(d.num_descriptors, 1);
        assert_eq!(d.report_descriptor_type, 0x22);
    }
}
