//! Input state types and HID report parsing for the Sidewinder Force Feedback 2.

use std::collections::VecDeque;
use thiserror::Error;

// ── Smoothing ────────────────────────────────────────────────────────────────

/// Rolling-average filter for a single axis.
///
/// Accumulates up to `window` samples and returns the integer average of the
/// buffer contents on each [`push`](SmoothingBuffer::push).  A window of 1
/// is a no-op — it always returns the value just pushed.
///
/// # Example
///
/// ```
/// use sideblinder_hid::input::SmoothingBuffer;
///
/// let mut buf = SmoothingBuffer::new(3);
/// assert_eq!(buf.push(10), 10);
/// assert_eq!(buf.push(20), 15);   // average of [10, 20]
/// assert_eq!(buf.push(30), 20);   // average of [10, 20, 30]
/// assert_eq!(buf.push(40), 30);   // oldest evicted: average of [20, 30, 40]
/// ```
#[derive(Debug, Clone)]
pub struct SmoothingBuffer {
    buf: VecDeque<i16>,
    window: usize,
}

impl SmoothingBuffer {
    /// Create a new buffer with the given window size.
    ///
    /// A `window` of 1 acts as a pass-through with no allocation.
    /// `window` is clamped to 1 if zero is passed. When `window == 1` no
    /// allocation is performed (the buffer is bypassed entirely in `push`).
    #[must_use]
    pub fn new(window: usize) -> Self {
        let window = window.max(1);
        let buf = if window == 1 {
            VecDeque::new()
        } else {
            VecDeque::with_capacity(window)
        };
        Self { buf, window }
    }

    /// Return the configured window size.
    #[must_use]
    pub fn window(&self) -> usize {
        self.window
    }

    /// Push a new sample and return the rolling average.
    ///
    /// If the buffer is full the oldest sample is evicted before inserting.
    pub fn push(&mut self, value: i16) -> i16 {
        if self.window == 1 {
            return value;
        }
        if self.buf.len() == self.window {
            self.buf.pop_front();
        }
        self.buf.push_back(value);
        // Use i64 arithmetic: sum of at most SMOOTHING_MAX (30) i16 samples is
        // at most 30 * 32767 = 983 010, well within i64 range. The average of
        // i16 values divided by the count is itself bounded to i16 range.
        let sum: i64 = self.buf.iter().map(|&v| i64::from(v)).sum();
        // buf.len() is bounded to SMOOTHING_MAX (30) — well within i64 range.
        let len = i64::try_from(self.buf.len()).unwrap_or(1);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "average of i16 samples divided by count is bounded to i16 range"
        )]
        let avg = (sum / len) as i16;
        avg
    }
}

// ── Types ────────────────────────────────────────────────────────────────────

/// Direction reported by the POV hat switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PovDirection {
    /// Hat centred / not pressed.
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
    /// Convert from a raw HID hat-switch value.
    ///
    /// The Sidewinder FF2 encodes the hat as a nibble: 0 = North, 1 = NE, …,
    /// 7 = NW.  Any other value (including 0xFF / 0x0F null state) maps to
    /// [`PovDirection::Center`].
    #[must_use]
    pub fn from_hid_value(raw: u8) -> Self {
        match raw {
            0 => Self::North,
            1 => Self::NorthEast,
            2 => Self::East,
            3 => Self::SouthEast,
            4 => Self::South,
            5 => Self::SouthWest,
            6 => Self::West,
            7 => Self::NorthWest,
            _ => Self::Center,
        }
    }
}

/// Snapshot of all joystick inputs at a single point in time.
///
/// Axes are in signed 16-bit range (−32 768 … 32 767).
/// Buttons are a bitfield: bit *n* corresponds to button *n + 1*.
///
/// The Sidewinder FF2 populates the first four slots in this order:
/// `[0] X`, `[1] Y`, `[2] Rz (twist)`, `[3] Slider (throttle)`.
/// Slots 4–7 are always zero for this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputState {
    /// Axes `[X, Y, Rz/twist, Slider/throttle, …]` (device populates indices 0–3).
    pub axes: [i16; 8],
    /// Buttons 1-9 packed into the low nine bits.
    pub buttons: u16,
    /// POV hat direction.
    pub pov: PovDirection,
}

impl InputState {
    /// Returns `true` if button `index` (0-based) is currently pressed.
    #[must_use]
    pub fn is_button_pressed(self, index: u8) -> bool {
        self.buttons & (1 << index) != 0
    }
}

// ── Report parsing ───────────────────────────────────────────────────────────

/// Error returned when a HID input report cannot be parsed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InputParseError {
    /// The report buffer is shorter than the minimum expected size.
    #[error("report too short: expected at least {expected} bytes, got {got}")]
    TooShort { expected: usize, got: usize },
}

/// Minimum valid report size for the Sidewinder FF2 (after report-ID strip).
///
/// 4 axes × 2 bytes + 2 button bytes + 1 POV byte = 11 bytes.
/// The real device sends 12 bytes (1 trailing pad); 11 is the minimum we need.
const MIN_REPORT_SIZE: usize = 11;

/// Parse a raw HID input report into an [`InputState`].
///
/// The Sidewinder FF2 report layout (no report-ID prefix, little-endian).
/// All axis values are signed i16 directly from the device:
///
/// | Bytes  | Field                        |
/// |--------|------------------------------|
/// | 0-1    | X axis (i16)                 |
/// | 2-3    | Y axis (i16)                 |
/// | 4-5    | Rz / twist axis (i16)        |
/// | 6-7    | Slider / throttle (i16)      |
/// | 8-9    | Buttons (u16, 9 bits used)   |
/// | 10     | POV hat (u8, low nibble)     |
///
/// # Errors
///
/// Returns [`InputParseError::TooShort`] if `report` has fewer than 11 bytes.
pub fn parse_input_report(report: &[u8]) -> Result<InputState, InputParseError> {
    if report.len() < MIN_REPORT_SIZE {
        return Err(InputParseError::TooShort {
            expected: MIN_REPORT_SIZE,
            got: report.len(),
        });
    }

    let mut state = InputState::default();

    for i in 0..4 {
        state.axes[i] = i16::from_le_bytes([report[i * 2], report[i * 2 + 1]]);
    }

    state.buttons = u16::from_le_bytes([report[8], report[9]]);
    state.pov = PovDirection::from_hid_value(report[10]);

    Ok(state)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_centred() {
        let s = InputState::default();
        assert_eq!(s.axes, [0i16; 8]);
        assert_eq!(s.buttons, 0);
        assert_eq!(s.pov, PovDirection::Center);
    }

    #[test]
    fn button_helper() {
        let s = InputState {
            buttons: 0b0000_0101, // buttons 0 and 2
            ..Default::default()
        };
        assert!(s.is_button_pressed(0));
        assert!(!s.is_button_pressed(1));
        assert!(s.is_button_pressed(2));
    }

    #[test]
    fn pov_from_hid_value() {
        assert_eq!(PovDirection::from_hid_value(0), PovDirection::North);
        assert_eq!(PovDirection::from_hid_value(2), PovDirection::East);
        assert_eq!(PovDirection::from_hid_value(7), PovDirection::NorthWest);
        assert_eq!(PovDirection::from_hid_value(0xFF), PovDirection::Center);
        assert_eq!(PovDirection::from_hid_value(0x0F), PovDirection::Center);
    }

    #[test]
    fn parse_centred_report() {
        // Axes are signed i16; 0x0000 = centre, 0x7FFF = max, 0x8000 = min.
        let report = [
            0x00u8, 0x00, // X centred
            0x00, 0x00, // Y centred
            0x00, 0x00, // Rz/twist centred
            0x00, 0x00, // Slider/throttle centred
            0x00, 0x00, // No buttons
            0xFF, // POV centre
        ];
        let s = parse_input_report(&report).expect("centred report should parse");
        assert_eq!(s.axes[0], 0);
        assert_eq!(s.axes[1], 0);
        assert_eq!(s.buttons, 0);
        assert_eq!(s.pov, PovDirection::Center);
    }

    #[test]
    fn parse_full_deflection() {
        let report = [
            0xFF, 0x7F, // X max → +32767
            0x00, 0x80, // Y min → -32768
            0x00, 0x00, // Rz/twist centred
            0x00, 0x00, // Slider/throttle centred
            0x05, 0x00, // Buttons 0 and 2
            0x00, // POV North
        ];
        let s = parse_input_report(&report).expect("full-deflection report should parse");
        assert_eq!(s.axes[0], 32767);
        assert_eq!(s.axes[1], -32768);
        assert!(s.is_button_pressed(0));
        assert!(!s.is_button_pressed(1));
        assert!(s.is_button_pressed(2));
        assert_eq!(s.pov, PovDirection::North);
    }

    #[test]
    fn parse_short_report_errors() {
        let err = parse_input_report(&[0x00, 0x80]).expect_err("2-byte report must be too short");
        assert_eq!(
            err,
            InputParseError::TooShort {
                expected: 11,
                got: 2
            }
        );
    }

    // Regression tests for Issue #19: verify byte-offset layout so that
    // axis movement never fires spurious button events and vice versa.

    /// Each axis lives at its expected byte offset; moving one axis must not
    /// affect any other axis or the button / POV fields.
    #[test]
    fn parse_x_axis_only() {
        let mut report = [0u8; 11];
        // X = +32767 at bytes 0-1 (LE), all else zero / neutral.
        report[0] = 0xFF;
        report[1] = 0x7F;
        report[10] = 0xFF; // POV centre
        let s = parse_input_report(&report).expect("X-only report should parse");
        assert_eq!(s.axes[0], 32767, "X axis value");
        assert_eq!(s.axes[1], 0, "Y must be unaffected");
        assert_eq!(s.axes[2], 0, "Rz must be unaffected");
        assert_eq!(s.axes[3], 0, "Slider must be unaffected");
        assert_eq!(s.buttons, 0, "no spurious buttons from X axis");
        assert_eq!(s.pov, PovDirection::Center);
    }

    #[test]
    fn parse_y_axis_only() {
        let mut report = [0u8; 11];
        report[2] = 0x00;
        report[3] = 0x80; // Y = -32768
        report[10] = 0xFF;
        let s = parse_input_report(&report).expect("Y-only report should parse");
        assert_eq!(s.axes[1], -32768, "Y axis value");
        assert_eq!(s.axes[0], 0, "X must be unaffected");
        assert_eq!(s.buttons, 0, "no spurious buttons from Y axis");
    }

    /// Bytes 4-5 are Rz/twist — not Z/throttle.  This is the critical slot
    /// that was mis-labelled in an earlier revision (Issue #19).
    #[test]
    fn parse_rz_twist_axis_only() {
        let mut report = [0u8; 11];
        report[4] = 0x00;
        report[5] = 0x40; // Rz = +16384
        report[10] = 0xFF;
        let s = parse_input_report(&report).expect("Rz-only report should parse");
        assert_eq!(s.axes[2], 0x4000i16, "Rz/twist at index 2");
        assert_eq!(s.axes[0], 0);
        assert_eq!(s.axes[1], 0);
        assert_eq!(s.axes[3], 0);
        assert_eq!(s.buttons, 0, "no spurious buttons from Rz axis");
    }

    /// Bytes 6-7 are Slider/throttle — not Rz.
    #[test]
    fn parse_slider_throttle_axis_only() {
        let mut report = [0u8; 11];
        report[6] = 0x00;
        report[7] = 0x40; // Slider = +16384
        report[10] = 0xFF;
        let s = parse_input_report(&report).expect("Slider-only report should parse");
        assert_eq!(s.axes[3], 0x4000i16, "Slider/throttle at index 3");
        assert_eq!(s.axes[2], 0, "Rz must be unaffected");
        assert_eq!(s.buttons, 0, "no spurious buttons from Slider axis");
    }

    /// Button bits live at bytes 8-9; the preceding axis bytes must not bleed
    /// into the button field even at maximum axis values.
    #[test]
    fn parse_max_axis_values_no_button_bleed() {
        let report = [
            0xFF, 0x7F, // X = +32767
            0x00, 0x80, // Y = -32768
            0xFF, 0x7F, // Rz = +32767
            0x00, 0x80, // Slider = -32768
            0x00, 0x00, // No buttons — must stay zero
            0xFF, // POV centre
        ];
        let s = parse_input_report(&report).expect("max-axis report should parse");
        assert_eq!(s.buttons, 0, "all-axis max must not produce button events");
    }

    /// Button bits must not affect axis readings.
    #[test]
    fn parse_all_buttons_no_axis_corruption() {
        let report = [
            0x00, 0x00, // X = 0
            0x00, 0x00, // Y = 0
            0x00, 0x00, // Rz = 0
            0x00, 0x00, // Slider = 0
            0xFF, 0x01, // All 9 buttons pressed (0x01FF)
            0xFF, // POV centre
        ];
        let s = parse_input_report(&report).expect("all-buttons report should parse");
        assert_eq!(s.axes[0], 0, "X must not be corrupted by buttons");
        assert_eq!(s.axes[1], 0, "Y must not be corrupted by buttons");
        assert_eq!(s.axes[2], 0, "Rz must not be corrupted by buttons");
        assert_eq!(s.axes[3], 0, "Slider must not be corrupted by buttons");
        // Low 9 bits all set.
        assert_eq!(s.buttons, 0x01FF, "all 9 buttons");
    }

    // ── Property-based tests ─────────────────────────────────────────────────

    /// Build a well-formed 11-byte report from explicit field values and assert
    /// that parsing recovers those exact values.
    ///
    /// This tests the full encoding → parsing roundtrip for all four axes,
    /// the button word, and every valid POV nibble (0-7 and the null-centre
    /// value 0xFF).
    #[cfg(test)]
    mod props {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn roundtrip_axes_buttons_pov(
                x in i16::MIN..=i16::MAX,
                y in i16::MIN..=i16::MAX,
                rz in i16::MIN..=i16::MAX,
                slider in i16::MIN..=i16::MAX,
                buttons in 0u16..=u16::MAX,
                // 0-7 → named directions; anything else → Center.  Test both
                // valid nibbles and the out-of-range sentinel (0xFF).
                raw_pov in prop_oneof![0u8..=7u8, Just(0xFFu8)],
            ) {
                let mut report = [0u8; 11];
                report[0..2].copy_from_slice(&x.to_le_bytes());
                report[2..4].copy_from_slice(&y.to_le_bytes());
                report[4..6].copy_from_slice(&rz.to_le_bytes());
                report[6..8].copy_from_slice(&slider.to_le_bytes());
                report[8..10].copy_from_slice(&buttons.to_le_bytes());
                report[10] = raw_pov;

                let state = parse_input_report(&report)?;

                prop_assert_eq!(state.axes[0], x);
                prop_assert_eq!(state.axes[1], y);
                prop_assert_eq!(state.axes[2], rz);
                prop_assert_eq!(state.axes[3], slider);
                // Axes 4-7 are device-unused; parser leaves them at the
                // default zero value.
                for i in 4..8 {
                    prop_assert_eq!(state.axes[i], 0i16, "axis {} must be zero", i);
                }
                prop_assert_eq!(state.buttons, buttons);
                prop_assert_eq!(
                    state.pov,
                    PovDirection::from_hid_value(raw_pov),
                );
            }

            /// Any slice shorter than MIN_REPORT_SIZE (11) must return TooShort,
            /// regardless of its contents.
            #[test]
            fn short_input_always_errors(
                len in 0usize..11usize,
                fill in 0u8..=u8::MAX,
            ) {
                let report = vec![fill; len];
                let err = parse_input_report(&report).expect_err("short report must fail");
                prop_assert_eq!(
                    err,
                    InputParseError::TooShort { expected: 11, got: len }
                );
            }

            /// Extra trailing bytes beyond the minimum 11 must be silently
            /// ignored; the parse result is identical to the first 11 bytes.
            #[test]
            fn trailing_bytes_ignored(
                base in prop::array::uniform11(0u8..=u8::MAX),
                extra in prop::collection::vec(0u8..=u8::MAX, 0..64),
            ) {
                let short: &[u8] = &base;
                let mut long = base.to_vec();
                long.extend_from_slice(&extra);

                let state_short = parse_input_report(short)?;
                let state_long = parse_input_report(&long)?;

                prop_assert_eq!(state_short, state_long);
            }
        }
    }

    /// Reports longer than the minimum (e.g. with a trailing padding byte)
    /// are accepted without error.
    #[test]
    fn parse_12_byte_report_accepted() {
        let report = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // buttons
            0xFF, // POV centre
            0x00, // trailing pad — must be ignored
        ];
        assert!(parse_input_report(&report).is_ok());
    }

    /// Exactly 10 bytes is one byte too short — must return `TooShort`.
    #[test]
    fn parse_10_byte_report_too_short() {
        let err = parse_input_report(&[0u8; 10]).expect_err("10-byte report must be too short");
        assert_eq!(
            err,
            InputParseError::TooShort {
                expected: 11,
                got: 10,
            }
        );
    }

    // ── SmoothingBuffer tests ─────────────────────────────────────────────────

    /// Window of 1 is a pass-through with no buffering.
    #[test]
    fn smoothing_window_1_passthrough() {
        let mut buf = SmoothingBuffer::new(1);
        for v in [-32768i16, 0, 100, 32767] {
            assert_eq!(buf.push(v), v, "window=1 must pass through every value");
        }
    }

    /// Window of 0 is clamped to 1 (pass-through).
    #[test]
    fn smoothing_window_0_clamped_to_1() {
        let mut buf = SmoothingBuffer::new(0);
        assert_eq!(buf.push(42), 42);
    }

    /// Rolling average converges as the buffer fills.
    #[test]
    fn smoothing_rolling_average() {
        let mut buf = SmoothingBuffer::new(3);
        assert_eq!(buf.push(10), 10); // [10]        → 10
        assert_eq!(buf.push(20), 15); // [10, 20]    → 15
        assert_eq!(buf.push(30), 20); // [10, 20, 30]→ 20
        assert_eq!(buf.push(40), 30); // [20, 30, 40]→ 30 (10 evicted)
    }

    /// Constant input produces constant output regardless of window size.
    #[test]
    fn smoothing_constant_input() {
        let mut buf = SmoothingBuffer::new(5);
        for _ in 0..10 {
            assert_eq!(buf.push(1000), 1000);
        }
    }

    // ── SmoothingBuffer property-based tests ──────────────────────────────────

    mod smoothing_props {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// The output of `push()` is always within the i16 range for any
            /// sequence of inputs and any window size in [1, 30].
            ///
            /// This verifies the `#[expect(cast_possible_truncation)]` invariant:
            /// the average of i16 values is always itself within i16 bounds.
            #[test]
            fn smoothing_output_always_in_i16_range(
                values in proptest::collection::vec(i16::MIN..=i16::MAX, 1..=30usize),
                window in 1usize..=30usize,
            ) {
                let mut buf = SmoothingBuffer::new(window);
                for v in values {
                    let out = i32::from(buf.push(v));
                    prop_assert!(
                        i16::try_from(out).is_ok(),
                        "SmoothingBuffer output {out} outside i16 range"
                    );
                }
            }
        }
    }
}
