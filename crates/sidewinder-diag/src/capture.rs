//! Capture and replay support for HID input reports (Issue #21).
//!
//! # File format (.swcf)
//!
//! ```text
//! Header (16 bytes):
//!   magic:    [u8; 4] = b"SWCF"
//!   version:  u8      = 1
//!   reserved: [u8; 11]
//!
//! Records (repeated until EOF):
//!   timestamp_ms: u32  — milliseconds since capture start (little-endian)
//!   len:          u8   — byte count of following data
//!   data:         [u8; len]
//! ```

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

use tracing::warn;

#[cfg(any(target_os = "windows", test))]
use std::{
    io::{BufWriter, Write},
    time::{Duration, Instant},
};

use thiserror::Error;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Four-byte magic that identifies a Sidewinder capture file.
const MAGIC: &[u8; 4] = b"SWCF";

/// Current file format version.
const FILE_VERSION: u8 = 1;

/// Total header size in bytes.
const HEADER_LEN: usize = 16;

/// Maximum single-report byte count.
///
/// The on-disk `len` field is a `u8`, so a record can never exceed 255 bytes.
/// Only the write path enforces this limit; the read path cannot exceed it by
/// construction.
#[cfg(any(target_os = "windows", test))]
const MAX_REPORT_LEN: usize = u8::MAX as usize; // 255

// ── Error ─────────────────────────────────────────────────────────────────────

/// Errors from capture / replay operations.
#[derive(Debug, Error)]
pub enum CaptureError {
    /// An I/O operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// The file does not start with the expected magic bytes.
    #[error("not a capture file: bad magic")]
    BadMagic,

    /// The file version is not supported by this build.
    #[error("unsupported capture file version {0}")]
    UnsupportedVersion(u8),

    /// A report was too large to write (write path only; the read path
    /// cannot produce this because the on-disk `len` field is a `u8`).
    #[cfg(any(target_os = "windows", test))]
    #[error("report too large to record: {0} bytes (max {MAX_REPORT_LEN})")]
    RecordTooLarge(usize),
}

// ── Record ────────────────────────────────────────────────────────────────────

/// A single captured HID input report with its capture-relative timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRecord {
    /// Milliseconds since the start of the capture session.
    pub timestamp_ms: u32,
    /// Raw HID report bytes (no report-ID prefix).
    pub data: Vec<u8>,
}

// ── Writer ────────────────────────────────────────────────────────────────────

/// Writes a capture session to a file.
///
/// Available on Windows (the `capture` CLI subcommand) and in tests.
/// On non-Windows non-test builds the type exists but is not callable from the
/// CLI, so the compiler would otherwise emit a dead-code warning.
#[cfg(any(target_os = "windows", test))]
pub struct CaptureWriter {
    writer: BufWriter<File>,
    start: Instant,
}

#[cfg(any(target_os = "windows", test))]
impl CaptureWriter {
    /// Open `path` for writing and write the file header.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Io`] if the file cannot be created or the
    /// header write fails.
    pub fn create(path: &Path) -> Result<Self, CaptureError> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // Header: magic + version + 11 reserved bytes.
        let mut header = [0u8; HEADER_LEN];
        header[..4].copy_from_slice(MAGIC);
        header[4] = FILE_VERSION;
        writer.write_all(&header)?;

        Ok(Self {
            writer,
            start: Instant::now(),
        })
    }

    /// Append one HID report to the capture file.
    ///
    /// The timestamp is computed automatically from the moment `write_record`
    /// is called.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Io`] on write failure.
    /// Returns [`CaptureError::RecordTooLarge`] if `data` exceeds 255 bytes.
    pub fn write_record(&mut self, data: &[u8]) -> Result<(), CaptureError> {
        if data.len() > MAX_REPORT_LEN {
            return Err(CaptureError::RecordTooLarge(data.len()));
        }

        // Clamp to u32::MAX ms (~49 days) before converting; the .min() ensures
        // as_millis() fits in u32, so cast_possible_truncation is expected here.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "clamped to u32::MAX before cast"
        )]
        let elapsed_ms = self
            .start
            .elapsed()
            .min(Duration::from_millis(u64::from(u32::MAX)))
            .as_millis() as u32;

        // data.len() is already validated ≤ MAX_REPORT_LEN (255 = u8::MAX) above.
        #[expect(clippy::cast_possible_truncation, reason = "len validated ≤ 255 above")]
        let len = data.len() as u8;

        self.writer.write_all(&elapsed_ms.to_le_bytes())?;
        self.writer.write_all(&[len])?;
        self.writer.write_all(data)?;
        Ok(())
    }

    /// Flush and close the writer.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::Io`] on flush failure.
    pub fn finish(mut self) -> Result<(), CaptureError> {
        self.writer.flush()?;
        Ok(())
    }
}

// ── Reader ────────────────────────────────────────────────────────────────────

/// Reads all records from a capture file produced by [`CaptureWriter`].
///
/// # Errors
///
/// Returns:
/// - [`CaptureError::Io`] on read failure
/// - [`CaptureError::BadMagic`] if the file is not a `.swcf` file
/// - [`CaptureError::UnsupportedVersion`] for unknown format versions
/// - [`CaptureError::RecordTooLarge`] is only returned by [`CaptureWriter::write_record`],
///   not by this function (the on-disk `len` field is a `u8`, so it is always ≤ 255)
pub fn read_capture(path: &Path) -> Result<Vec<CaptureRecord>, CaptureError> {
    let mut file = File::open(path)?;

    // Validate header.
    let mut header = [0u8; HEADER_LEN];
    file.read_exact(&mut header)?;

    if header[..4] != *MAGIC {
        return Err(CaptureError::BadMagic);
    }
    if header[4] != FILE_VERSION {
        return Err(CaptureError::UnsupportedVersion(header[4]));
    }

    // Read records until EOF.
    let mut records = Vec::new();
    loop {
        // Use `read` rather than `read_exact` for the first byte so we can
        // distinguish true EOF (0 bytes) from a truncated timestamp field
        // (1–3 bytes), which indicates file corruption.
        let mut ts_buf = [0u8; 4];
        let n = file.read(&mut ts_buf[..1]).map_err(CaptureError::Io)?;
        if n == 0 {
            break; // clean EOF between records
        }
        file.read_exact(&mut ts_buf[1..])
            .map_err(CaptureError::Io)?;
        let timestamp_ms = u32::from_le_bytes(ts_buf);

        let mut len_buf = [0u8; 1];
        file.read_exact(&mut len_buf)?;
        // len_buf[0] is a u8, so its range is 0..=255 = 0..=MAX_REPORT_LEN.
        // The RecordTooLarge guard is only meaningful on the write path where
        // the caller supplies an arbitrary &[u8]; here the format enforces it.
        let len = len_buf[0] as usize;

        let mut data = vec![0u8; len];
        file.read_exact(&mut data)?;

        records.push(CaptureRecord { timestamp_ms, data });
    }

    Ok(records)
}

// ── Replay runner ─────────────────────────────────────────────────────────────

/// Parse every record in a capture file through `parse_input_report` and
/// return the decoded states alongside the original records.
///
/// `replay_capture` does not depend on any hardware — it is intentionally
/// usable in tests and offline environments.
///
/// # Errors
///
/// Returns any [`CaptureError`] that [`read_capture`] would return.
pub fn replay_capture(
    path: &Path,
) -> Result<Vec<(CaptureRecord, sidewinder_hid::input::InputState)>, CaptureError> {
    use sidewinder_hid::input::parse_input_report;

    let records = read_capture(path)?;
    let decoded = records
        .into_iter()
        .filter_map(|rec| match parse_input_report(&rec.data) {
            Ok(state) => Some((rec, state)),
            Err(e) => {
                warn!(
                    timestamp_ms = rec.timestamp_ms,
                    len = rec.data.len(),
                    "skipping unparse-able record: {e}"
                );
                None
            }
        })
        .collect();
    Ok(decoded)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code — panicking on failure is the correct behaviour"
)]
mod tests {
    use super::*;
    use sidewinder_hid::input::PovDirection;

    fn centred_report() -> Vec<u8> {
        vec![
            0x00, 0x00, // X centred
            0x00, 0x00, // Y centred
            0x00, 0x00, // Rz/twist centred
            0x00, 0x00, // Slider/throttle centred
            0x00, 0x00, // No buttons
            0xFF, // POV centre
        ]
    }

    fn max_x_report() -> Vec<u8> {
        vec![
            0xFF, 0x7F, // X = +32767
            0x00, 0x00, // Y centred
            0x00, 0x00, // Rz centred
            0x00, 0x00, // Slider centred
            0x05, 0x00, // Buttons 0 and 2
            0x00, // POV North
        ]
    }

    /// Write a capture file with two records, read it back, and verify that
    /// the round-trip is lossless.
    #[test]
    fn test_round_trip_two_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.swcf");

        {
            let mut writer = CaptureWriter::create(&path).unwrap();
            writer.write_record(&centred_report()).unwrap();
            writer.write_record(&max_x_report()).unwrap();
            writer.finish().unwrap();
        }

        let records = read_capture(&path).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].data, centred_report());
        assert_eq!(records[1].data, max_x_report());
        // Second record's timestamp must be ≥ first's (monotonic).
        assert!(records[1].timestamp_ms >= records[0].timestamp_ms);
    }

    /// An empty capture (zero records) is valid and readable.
    #[test]
    fn test_empty_capture_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.swcf");

        CaptureWriter::create(&path).unwrap().finish().unwrap();

        let records = read_capture(&path).unwrap();
        assert!(records.is_empty());
    }

    /// A file with wrong magic must return `BadMagic`.
    #[test]
    fn test_bad_magic_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.swcf");

        // Write a header with garbage magic.
        let mut f = File::create(&path).unwrap();
        f.write_all(&[0xFF; 16]).unwrap();

        let err = read_capture(&path).unwrap_err();
        assert!(matches!(err, CaptureError::BadMagic));
    }

    /// A file with an unsupported version number must return
    /// `UnsupportedVersion`.
    #[test]
    fn test_unsupported_version_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.swcf");

        let mut header = [0u8; 16];
        header[..4].copy_from_slice(MAGIC);
        header[4] = 99; // version 99 is not supported
        std::fs::write(&path, header).unwrap();

        let err = read_capture(&path).unwrap_err();
        assert!(matches!(err, CaptureError::UnsupportedVersion(99)));
    }

    /// `replay_capture` must decode the `InputState` from each record and
    /// produce the same result as calling `parse_input_report` directly.
    #[test]
    fn test_replay_round_trip_state_matches_parser() {
        use sidewinder_hid::input::parse_input_report;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("replay.swcf");

        let reports: Vec<Vec<u8>> = vec![centred_report(), max_x_report()];

        {
            let mut writer = CaptureWriter::create(&path).unwrap();
            for r in &reports {
                writer.write_record(r).unwrap();
            }
            writer.finish().unwrap();
        }

        let replayed = replay_capture(&path).unwrap();
        assert_eq!(replayed.len(), 2);

        for (i, (rec, state)) in replayed.iter().enumerate() {
            let expected = parse_input_report(&rec.data).unwrap();
            assert_eq!(*state, expected, "record {i}: decoded state mismatch");
        }

        // Spot-check: first record is centred, second has buttons and X deflection.
        assert_eq!(replayed[0].1.buttons, 0);
        assert_eq!(replayed[0].1.pov, PovDirection::Center);
        assert_eq!(replayed[1].1.axes[0], 32767);
        assert_eq!(replayed[1].1.buttons, 0x05);
        assert_eq!(replayed[1].1.pov, PovDirection::North);
    }

    /// `write_record` must reject a report that exceeds 255 bytes.
    #[test]
    fn test_write_record_too_large_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.swcf");

        let mut writer = CaptureWriter::create(&path).unwrap();
        let big = vec![0u8; 256];
        let err = writer.write_record(&big).unwrap_err();
        assert!(matches!(err, CaptureError::RecordTooLarge(256)));
    }

    // ── Property-based tests ─────────────────────────────────────────────────

    #[cfg(test)]
    mod props {
        use super::*;
        use proptest::prelude::*;

        // Generate a sequence of valid reports (each 0-255 bytes) and verify
        // that every byte is recovered exactly after a write → read roundtrip.
        // Timestamps are wall-clock and therefore cannot be compared for exact
        // equality, but they must be monotonically non-decreasing.
        proptest! {
            #[test]
            fn roundtrip_arbitrary_records(
                records in prop::collection::vec(
                    prop::collection::vec(0u8..=u8::MAX, 0..=255usize),
                    0..=16usize,
                ),
            ) {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("prop.swcf");

                {
                    let mut writer = CaptureWriter::create(&path).unwrap();
                    for data in &records {
                        writer.write_record(data).unwrap();
                    }
                    writer.finish().unwrap();
                }

                let read_back = read_capture(&path).unwrap();

                prop_assert_eq!(read_back.len(), records.len());
                for (i, (rec, expected)) in read_back.iter().zip(records.iter()).enumerate() {
                    prop_assert_eq!(&rec.data, expected, "record {}: data mismatch", i);
                }
                // Timestamps must be monotonically non-decreasing.
                for window in read_back.windows(2) {
                    prop_assert!(
                        window[1].timestamp_ms >= window[0].timestamp_ms,
                        "timestamps out of order at records {:?} and {:?}",
                        window[0].timestamp_ms,
                        window[1].timestamp_ms,
                    );
                }
            }

            /// Exactly 255 bytes is the maximum valid report length.
            /// `write_record` must accept it; `read_capture` must recover it.
            #[test]
            fn max_length_report_accepted(fill in 0u8..=u8::MAX) {
                let data = vec![fill; 255];
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("max.swcf");

                let mut writer = CaptureWriter::create(&path).unwrap();
                writer.write_record(&data).unwrap();
                writer.finish().unwrap();

                let read_back = read_capture(&path).unwrap();
                prop_assert_eq!(read_back.len(), 1);
                prop_assert_eq!(&read_back[0].data, &data, "max-length roundtrip");
            }

            /// Any report longer than 255 bytes must be rejected before any
            /// bytes are written, leaving the writer usable for subsequent calls.
            #[test]
            fn oversized_report_rejected(
                len in 256usize..=512usize,
                fill in 0u8..=u8::MAX,
            ) {
                let big = vec![fill; len];
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("big.swcf");

                let mut writer = CaptureWriter::create(&path).unwrap();
                let err = writer.write_record(&big).unwrap_err();
                prop_assert!(
                    matches!(err, CaptureError::RecordTooLarge(n) if n == len),
                );

                // The writer must still be usable: a subsequent valid record
                // must round-trip cleanly.
                let valid = vec![fill; 11];
                writer.write_record(&valid).unwrap();
                writer.finish().unwrap();

                let read_back = read_capture(&path).unwrap();
                prop_assert_eq!(read_back.len(), 1);
                prop_assert_eq!(&read_back[0].data, &valid, "post-rejection roundtrip");
            }
        }
    }

    /// Records that fail to parse (e.g. too short) are silently skipped by
    /// `replay_capture` rather than returning an error.
    #[test]
    fn test_replay_skips_unparse_able_records() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mixed.swcf");

        {
            let mut writer = CaptureWriter::create(&path).unwrap();
            writer.write_record(&[0x00, 0x01]).unwrap(); // too short — will be skipped
            writer.write_record(&centred_report()).unwrap(); // valid
            writer.finish().unwrap();
        }

        let replayed = replay_capture(&path).unwrap();
        assert_eq!(
            replayed.len(),
            1,
            "only the parseable record must be present"
        );
    }
}
