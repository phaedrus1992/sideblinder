//! Inter-process communication protocol between `sidewinder-app` and `sidewinder-gui`.
//!
//! The protocol is a simple length-prefixed binary framing over a Windows named pipe.
//! `sidewinder-app` acts as the server; `sidewinder-gui` connects as the client.
//!
//! Data flow is **server → client only** for this wire protocol. Config changes
//! made in the GUI are written directly to the config file via `toml_edit`; the
//! app's existing `notify` hot-reload watcher picks them up automatically.

use thiserror::Error;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Windows named pipe that the app creates and the GUI connects to.
pub const PIPE_NAME: &str = r"\\.\pipe\SidewinderGui";

/// Number of bytes used by the length prefix in a framed message.
pub const FRAME_PREFIX_LEN: usize = 4;

/// Number of bytes in the `GuiFrame` payload (wire format).
pub const FRAME_PAYLOAD_LEN: usize = 22;

/// Total wire size of one framed `GuiFrame`: prefix + payload.
pub const FRAME_TOTAL_LEN: usize = FRAME_PREFIX_LEN + FRAME_PAYLOAD_LEN;

// ── Error ─────────────────────────────────────────────────────────────────────

/// Errors from encoding or decoding protocol frames.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    /// The buffer is shorter than a complete frame.
    #[error("buffer too short: need {need} bytes, have {have}")]
    TooShort { need: usize, have: usize },
    /// The length prefix does not match the expected payload size.
    #[error("length mismatch: expected {expected}, got {got}")]
    LengthMismatch { expected: usize, got: usize },
}

// ── GuiFrame ──────────────────────────────────────────────────────────────────

/// A live-state snapshot sent from `sidewinder-app` to `sidewinder-gui` at ~30 Hz.
///
/// Contains all data the GUI needs to render the Dashboard, Axes, and Buttons
/// screens without querying any other source.
///
/// # Wire format
///
/// The struct is serialised field-by-field in little-endian order:
///
/// | Offset | Size | Field        |
/// |--------|------|--------------|
/// | 0      | 16   | `axes`       |
/// | 16     | 2    | `buttons`    |
/// | 18     | 1    | `pov`        |
/// | 19     | 1    | `connected`  |
/// | 20     | 1    | `ffb_enabled`|
/// | 21     | 1    | `ffb_gain`   |
///
/// Total: 22 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GuiFrame {
    /// Raw axis values `[X, Y, Rz, Slider, …]` from the HID input state.
    pub axes: [i16; 8],
    /// Raw button bitmask from the HID input state.
    pub buttons: u16,
    /// POV hat direction encoded as 0–7 (North = 0, clockwise), 0xFF = centre.
    pub pov: u8,
    /// 1 = physical device connected, 0 = disconnected / reconnecting.
    pub connected: u8,
    /// 1 = FFB actuators enabled, 0 = disabled.
    pub ffb_enabled: u8,
    /// FFB gain 0–255 (255 = full gain).
    pub ffb_gain: u8,
}

impl GuiFrame {
    /// Serialise into a 22-byte payload (little-endian fields).
    ///
    /// This is the raw payload; call [`encode`](GuiFrame::encode) to get a
    /// length-prefixed frame ready for the pipe.
    #[must_use]
    pub fn to_payload(&self) -> [u8; FRAME_PAYLOAD_LEN] {
        let mut out = [0u8; FRAME_PAYLOAD_LEN];
        for (i, &ax) in self.axes.iter().enumerate() {
            let b = ax.to_le_bytes();
            out[i * 2] = b[0];
            out[i * 2 + 1] = b[1];
        }
        let btn = self.buttons.to_le_bytes();
        out[16] = btn[0];
        out[17] = btn[1];
        out[18] = self.pov;
        out[19] = self.connected;
        out[20] = self.ffb_enabled;
        out[21] = self.ffb_gain;
        out
    }

    /// Deserialise from a raw payload slice of at least [`FRAME_PAYLOAD_LEN`] bytes.
    ///
    /// Any trailing bytes beyond [`FRAME_PAYLOAD_LEN`] are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::TooShort`] if `payload` is shorter than
    /// [`FRAME_PAYLOAD_LEN`].
    pub fn from_payload(payload: &[u8]) -> Result<Self, ProtocolError> {
        if payload.len() < FRAME_PAYLOAD_LEN {
            return Err(ProtocolError::TooShort {
                need: FRAME_PAYLOAD_LEN,
                have: payload.len(),
            });
        }
        let axes = std::array::from_fn(|i| {
            i16::from_le_bytes([payload[i * 2], payload[i * 2 + 1]])
        });
        let buttons = u16::from_le_bytes([payload[16], payload[17]]);
        Ok(Self {
            axes,
            buttons,
            pov: payload[18],
            connected: payload[19],
            ffb_enabled: payload[20],
            ffb_gain: payload[21],
        })
    }

    /// Encode into a [`FRAME_TOTAL_LEN`]-byte wire frame (4-byte LE length + payload).
    #[must_use]
    pub fn encode(&self) -> [u8; FRAME_TOTAL_LEN] {
        let mut out = [0u8; FRAME_TOTAL_LEN];
        #[expect(
            clippy::cast_possible_truncation,
            reason = "FRAME_PAYLOAD_LEN = 22, always fits in u32"
        )]
        let len_bytes = (FRAME_PAYLOAD_LEN as u32).to_le_bytes();
        out[..FRAME_PREFIX_LEN].copy_from_slice(&len_bytes);
        out[FRAME_PREFIX_LEN..].copy_from_slice(&self.to_payload());
        out
    }

    /// Decode a [`GuiFrame`] from a length-prefixed wire frame.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::TooShort`] if `buf` is shorter than
    /// [`FRAME_TOTAL_LEN`], or [`ProtocolError::LengthMismatch`] if the
    /// encoded length differs from [`FRAME_PAYLOAD_LEN`].
    pub fn decode(buf: &[u8]) -> Result<Self, ProtocolError> {
        if buf.len() < FRAME_TOTAL_LEN {
            return Err(ProtocolError::TooShort {
                need: FRAME_TOTAL_LEN,
                have: buf.len(),
            });
        }
        let encoded_len =
            u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if encoded_len != FRAME_PAYLOAD_LEN {
            return Err(ProtocolError::LengthMismatch {
                expected: FRAME_PAYLOAD_LEN,
                got: encoded_len,
            });
        }
        Self::from_payload(&buf[FRAME_PREFIX_LEN..])
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code — panics are the failure mode")]
mod tests {
    use super::*;

    fn sample_frame() -> GuiFrame {
        GuiFrame {
            axes: [1000, -2000, 500, -500, 0, 0, 0, 0],
            buttons: 0b1010_1010,
            pov: 3,
            connected: 1,
            ffb_enabled: 1,
            ffb_gain: 200,
        }
    }

    #[test]
    fn payload_roundtrip() {
        let frame = sample_frame();
        let payload = frame.to_payload();
        let decoded = GuiFrame::from_payload(&payload).expect("decode must succeed");
        assert_eq!(decoded, frame);
    }

    #[test]
    fn wire_roundtrip() {
        let frame = sample_frame();
        let wire = frame.encode();
        assert_eq!(wire.len(), FRAME_TOTAL_LEN);
        let decoded = GuiFrame::decode(&wire).expect("decode must succeed");
        assert_eq!(decoded, frame);
    }

    #[test]
    fn decode_rejects_too_short_buffer() {
        let short = [0u8; FRAME_TOTAL_LEN - 1];
        let err = GuiFrame::decode(&short).expect_err("must fail on short buffer");
        assert!(matches!(err, ProtocolError::TooShort { .. }));
    }

    #[test]
    fn decode_rejects_wrong_length_prefix() {
        let frame = sample_frame();
        let mut wire = frame.encode();
        // Corrupt the length prefix to claim 99 bytes.
        wire[0] = 99;
        wire[1] = 0;
        wire[2] = 0;
        wire[3] = 0;
        let err = GuiFrame::decode(&wire).expect_err("must fail on length mismatch");
        assert!(matches!(err, ProtocolError::LengthMismatch { .. }));
    }

    #[test]
    fn from_payload_rejects_empty_slice() {
        let err = GuiFrame::from_payload(&[]).expect_err("must fail on empty slice");
        assert!(matches!(err, ProtocolError::TooShort { .. }));
    }

    #[test]
    fn axes_are_little_endian() {
        let frame = GuiFrame {
            axes: [0x0102, 0, 0, 0, 0, 0, 0, 0],
            ..Default::default()
        };
        let payload = frame.to_payload();
        // Little-endian: low byte first.
        assert_eq!(payload[0], 0x02, "low byte of axis 0");
        assert_eq!(payload[1], 0x01, "high byte of axis 0");
    }

    #[test]
    fn pov_centre_preserved() {
        let frame = GuiFrame {
            pov: 0xFF,
            connected: 1,
            ..Default::default()
        };
        let decoded = GuiFrame::decode(&frame.encode()).expect("decode must succeed");
        assert_eq!(decoded.pov, 0xFF);
    }

    #[test]
    fn frame_total_len_is_26() {
        // Regression guard: protocol is versioned by this constant.
        assert_eq!(FRAME_TOTAL_LEN, 26);
    }
}
