//! Device handle and lifecycle management for connected joysticks.
//!
//! [`SideblinderDevice`] owns a [`HidTransport`] and exposes a clean interface
//! for polling input and sending force-feedback commands.

use crate::{
    ffb::{FfbEffect, FfbOperation, build_effect_reports, build_operation_report},
    hid_transport::{HidTransport, TransportError},
    input::{InputParseError, InputState, parse_input_report},
};
use thiserror::Error;

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error returned by [`SideblinderDevice`] operations.
#[derive(Debug, Error)]
pub enum DeviceError {
    /// The underlying transport reported an error.
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),

    /// An input report could not be parsed.
    #[error("parse error: {0}")]
    Parse(#[from] InputParseError),
}

// ── Device ────────────────────────────────────────────────────────────────────

/// High-level handle for a single connected Sidewinder Force Feedback 2.
pub struct SideblinderDevice {
    transport: Box<dyn HidTransport>,
}

impl SideblinderDevice {
    /// Wrap an existing transport.  Prefer [`SideblinderDevice::open`] in
    /// production code; use this directly in tests with a [`MockTransport`].
    ///
    /// [`MockTransport`]: crate::hid_transport::MockTransport
    #[must_use]
    pub fn from_transport(transport: Box<dyn HidTransport>) -> Self {
        Self { transport }
    }

    /// Consume this device and return the underlying transport.
    ///
    /// Useful when the caller wants to share the transport across tasks via
    /// `Arc<dyn HidTransport>` after opening.
    #[must_use]
    pub fn into_transport(self) -> Box<dyn HidTransport> {
        self.transport
    }

    /// Open the first Sidewinder FF2 found on the system.
    ///
    /// Only available on Windows; on other platforms this always returns an
    /// error so that callers remain compilable in a cross-build environment.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError::Transport`] if no device is found or it cannot
    /// be opened.
    #[cfg(target_os = "windows")]
    pub fn open() -> Result<Self, DeviceError> {
        use crate::enumerate::find_sideblinder;
        use crate::hid_transport::WindowsHidTransport;

        let info = find_sideblinder()
            .map_err(|_| DeviceError::Transport(TransportError::NotOpen))?
            .ok_or(DeviceError::Transport(TransportError::NotOpen))?;

        let wide: Vec<u16> = info.path.encode_utf16().chain(std::iter::once(0)).collect();

        let transport = WindowsHidTransport::open(&wide).map_err(DeviceError::Transport)?;

        Ok(Self::from_transport(Box::new(transport)))
    }

    /// Block until one input report arrives and return the decoded state.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError::Transport`] on I/O failure or
    /// [`DeviceError::Parse`] if the report is malformed.
    pub fn poll(&self) -> Result<InputState, DeviceError> {
        let report = self.transport.read_input_report()?;
        let state = parse_input_report(&report)?;
        Ok(state)
    }

    /// Block until one input report arrives and return both the raw bytes and
    /// the decoded state.
    ///
    /// Use this when the caller needs the raw HID bytes (e.g. for a hex dump
    /// view) in addition to the parsed values.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError::Transport`] on I/O failure or
    /// [`DeviceError::Parse`] if the report is malformed.
    pub fn poll_raw(&self) -> Result<(Vec<u8>, InputState), DeviceError> {
        let report = self.transport.read_input_report()?;
        let state = parse_input_report(&report)?;
        Ok((report, state))
    }

    /// Send a force-feedback effect to the device.
    ///
    /// Writes all HID reports needed to define the effect on the device.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError::Transport`] if any write fails.
    pub fn send_ffb_effect(&self, effect: &FfbEffect) -> Result<(), DeviceError> {
        for report in build_effect_reports(effect) {
            self.transport.write_output_report(&report)?;
        }
        Ok(())
    }

    /// Send a force-feedback operation command (start, stop, device gain, etc).
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError::Transport`] if the write fails.
    pub fn send_ffb_operation(&self, op: FfbOperation) -> Result<(), DeviceError> {
        let report = build_operation_report(op);
        self.transport.write_output_report(&report)?;
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ffb::{FfbEffect, FfbEffectParams, FfbOperation, Waveform},
        hid_transport::MockTransport,
        input::PovDirection,
    };
    use std::sync::Arc;

    fn centred_report() -> Vec<u8> {
        vec![
            0x00, 0x00, // X centred (signed i16)
            0x00, 0x00, // Y centred
            0x00, 0x00, // Rz/twist centred
            0x00, 0x00, // Slider/throttle centred
            0x00, 0x00, // No buttons
            0xFF, // POV centre
        ]
    }

    fn make_device() -> (SideblinderDevice, Arc<MockTransport>) {
        let mock = Arc::new(MockTransport::new());
        let transport = mock.clone();
        let device = SideblinderDevice::from_transport(Box::new(MockWrapper(transport)));
        (device, mock)
    }

    /// Newtype so we can clone the Arc while still satisfying `Box<dyn HidTransport>`.
    struct MockWrapper(Arc<MockTransport>);

    impl HidTransport for MockWrapper {
        fn read_input_report(&self) -> Result<Vec<u8>, TransportError> {
            self.0.read_input_report()
        }
        fn write_output_report(&self, report: &[u8]) -> Result<(), TransportError> {
            self.0.write_output_report(report)
        }
        fn write_feature_report(&self, report: &[u8]) -> Result<(), TransportError> {
            self.0.write_feature_report(report)
        }
    }

    #[test]
    fn poll_returns_centred_state() {
        let (device, mock) = make_device();
        mock.set_input_data(centred_report());

        let state = device
            .poll()
            .expect("poll should succeed with valid report");
        assert_eq!(state.axes[0], 0);
        assert_eq!(state.buttons, 0);
        assert_eq!(state.pov, PovDirection::Center);
    }

    #[test]
    fn poll_short_report_returns_parse_error() {
        let (device, mock) = make_device();
        mock.set_input_data(vec![0x00, 0x80]); // too short

        let err = device
            .poll()
            .expect_err("poll should fail on too-short report");
        assert!(matches!(err, DeviceError::Parse(_)));
    }

    #[test]
    fn send_ffb_effect_writes_reports() {
        let (device, mock) = make_device();

        let effect = FfbEffect {
            effect_block_index: 1,
            params: FfbEffectParams::Periodic {
                waveform: Waveform::Sine,
                magnitude: 10000,
                offset: 0,
                phase: 0,
                period_ms: 100,
            },
            duration_ms: 1000,
            trigger_button: None,
            direction: 9000,
            gain: 255,
            start_delay_ms: 0,
            trigger_repeat_ms: 0,
            envelope: None,
        };

        device
            .send_ffb_effect(&effect)
            .expect("send_ffb_effect should succeed");
        assert!(mock.last_output().is_some());
    }

    #[test]
    fn send_ffb_operation_writes_report() {
        let (device, mock) = make_device();
        device
            .send_ffb_operation(FfbOperation::SetGain { gain: 200 })
            .expect("send_ffb_operation should succeed");
        let out = mock
            .last_output()
            .expect("output should be captured after write");
        assert_eq!(out[0], 0x0D); // DEVICE_GAIN report ID
    }
}
