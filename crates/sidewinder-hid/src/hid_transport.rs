//! Low-level HID read/write transport layer.
//!
//! Abstracts platform I/O behind a trait so that higher-level code and tests
//! are not coupled to the Windows HID API.

use thiserror::Error;

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error type for transport-level I/O operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransportError {
    /// The device handle is not open.
    #[error("device is not open")]
    NotOpen,

    /// A read operation failed.
    #[error("read failed: {0}")]
    ReadFailed(String),

    /// A write operation failed.
    #[error("write failed: {0}")]
    WriteFailed(String),
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Abstracts HID I/O so tests can inject a [`MockTransport`] and production
/// code uses [`WindowsHidTransport`].
pub trait HidTransport: Send + Sync {
    /// Block until an input report is available and return it.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::NotOpen`] or [`TransportError::ReadFailed`].
    fn read_input_report(&self) -> Result<Vec<u8>, TransportError>;

    /// Send an output report to the device.
    ///
    /// `report` must include the report-ID byte as its first element.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::NotOpen`] or [`TransportError::WriteFailed`].
    fn write_output_report(&self, report: &[u8]) -> Result<(), TransportError>;

    /// Send a feature report to the device.
    ///
    /// `report` must include the report-ID byte as its first element.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::NotOpen`] or [`TransportError::WriteFailed`].
    fn write_feature_report(&self, report: &[u8]) -> Result<(), TransportError>;
}

// ── Mock ──────────────────────────────────────────────────────────────────────

/// In-memory transport used in tests.
///
/// Inject input data with [`MockTransport::set_input_data`] and inspect what
/// was sent with [`MockTransport::last_output`].
#[derive(Debug, Default)]
pub struct MockTransport {
    input_data: std::sync::Mutex<Vec<u8>>,
    last_output: std::sync::Mutex<Option<Vec<u8>>>,
}

impl MockTransport {
    /// Create a new mock with no queued input.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-load the bytes that will be returned by the next `read_input_report`.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned (impossible in normal use).
    #[expect(
        clippy::expect_used,
        reason = "mutex can only be poisoned by a concurrent panic, impossible in practice"
    )]
    pub fn set_input_data(&self, data: Vec<u8>) {
        *self.input_data.lock().expect("mutex poisoned") = data;
    }

    /// Return the last report written via `write_output_report` or
    /// `write_feature_report`, or `None` if nothing has been written yet.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned (impossible in normal use).
    #[expect(
        clippy::expect_used,
        reason = "mutex can only be poisoned by a concurrent panic, impossible in practice"
    )]
    pub fn last_output(&self) -> Option<Vec<u8>> {
        self.last_output.lock().expect("mutex poisoned").clone()
    }
}

impl HidTransport for MockTransport {
    #[expect(
        clippy::expect_used,
        reason = "mutex can only be poisoned by a concurrent panic, impossible in practice"
    )]
    fn read_input_report(&self) -> Result<Vec<u8>, TransportError> {
        Ok(self.input_data.lock().expect("mutex poisoned").clone())
    }

    #[expect(
        clippy::expect_used,
        reason = "mutex can only be poisoned by a concurrent panic, impossible in practice"
    )]
    fn write_output_report(&self, report: &[u8]) -> Result<(), TransportError> {
        *self.last_output.lock().expect("mutex poisoned") = Some(report.to_vec());
        Ok(())
    }

    #[expect(
        clippy::expect_used,
        reason = "mutex can only be poisoned by a concurrent panic, impossible in practice"
    )]
    fn write_feature_report(&self, report: &[u8]) -> Result<(), TransportError> {
        *self.last_output.lock().expect("mutex poisoned") = Some(report.to_vec());
        Ok(())
    }
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub use windows_impl::WindowsHidTransport;

#[cfg(target_os = "windows")]
#[expect(
    unsafe_code,
    reason = "Windows HID transport requires raw Win32 CreateFile/HidD/ReadFile FFI calls"
)]
mod windows_impl {
    use super::{HidTransport, TransportError};
    use std::sync::Mutex;
    use windows_sys::Win32::{
        Devices::HumanInterfaceDevice::{
            HIDP_CAPS, HidD_FreePreparsedData, HidD_GetPreparsedData, HidD_SetFeature,
            HidD_SetOutputReport, HidP_GetCaps,
        },
        Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile,
        },
    };

    /// `NTSTATUS` returned by `HidP_GetCaps` on success.
    const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;

    /// Newtype that asserts `HANDLE` (`*mut c_void`) is `Send + Sync`.
    ///
    /// # Safety
    ///
    /// Windows kernel objects (files, devices) can safely be used from any
    /// thread; all concurrent access here is serialised through the outer
    /// `Mutex`.
    struct SafeHandle(HANDLE);
    // SAFETY: SafeHandle wraps a Windows HANDLE that is always accessed under the outer Mutex,
    // so it is safe to send and share across threads.
    unsafe impl Send for SafeHandle {}
    unsafe impl Sync for SafeHandle {}

    /// Production HID transport backed by Windows `CreateFile` + `HIDClass` API.
    pub struct WindowsHidTransport {
        handle: Mutex<SafeHandle>,
        report_len: usize,
    }

    impl WindowsHidTransport {
        /// Open the HID device at `path` (e.g. `\\?\HID#...`).
        ///
        /// The input-report buffer length is queried from the device via
        /// `HidP_GetCaps`; falls back to 12 bytes if the query fails.
        ///
        /// # Errors
        ///
        /// Returns [`TransportError::NotOpen`] if `CreateFileW` fails.
        pub fn open(path: &[u16]) -> Result<Self, TransportError> {
            // SAFETY: path is a valid null-terminated wide string; all other
            // args are well-typed FFI constants. The resulting HANDLE is owned
            // by this struct and closed in Drop.
            let raw = unsafe {
                CreateFileW(
                    path.as_ptr(),
                    0x8000_0000 | 0x4000_0000, // GENERIC_READ | GENERIC_WRITE
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,                    // no overlapped — blocking synchronous reads
                    std::ptr::null_mut(), // hTemplateFile: HANDLE = *mut c_void
                )
            };

            if raw == INVALID_HANDLE_VALUE {
                // SAFETY: called immediately after the failing CreateFileW.
                let code = unsafe { GetLastError() };
                tracing::error!(
                    error_code = code,
                    "Your joystick wasn't found. Check that it's plugged in and try again."
                );
                return Err(TransportError::NotOpen);
            }

            let report_len = query_input_report_len(raw);
            tracing::debug!(report_len, "opened HID device");

            Ok(Self {
                handle: Mutex::new(SafeHandle(raw)),
                report_len,
            })
        }
    }

    /// Query the input-report byte length from the device capabilities.
    ///
    /// Falls back to 12 if either HID caps call fails.
    fn query_input_report_len(handle: HANDLE) -> usize {
        const DEFAULT: usize = 12;

        let mut preparsed: isize = 0;

        // SAFETY: handle is valid and open; we free preparsed before returning.
        let ok = unsafe { HidD_GetPreparsedData(handle, &raw mut preparsed) };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            tracing::warn!(
                error_code = code,
                "HidD_GetPreparsedData failed; using default report_len"
            );
            return DEFAULT;
        }

        // SAFETY: HIDP_CAPS is a POD struct; zeroed is valid before HidP_GetCaps writes into it.
        let mut caps: HIDP_CAPS = unsafe { std::mem::zeroed() };
        // SAFETY: preparsed is a valid opaque pointer from HidD_GetPreparsedData.
        let status = unsafe { HidP_GetCaps(preparsed, &raw mut caps) };
        // SAFETY: preparsed was obtained from HidD_GetPreparsedData above.
        unsafe { HidD_FreePreparsedData(preparsed) };

        if status == HIDP_STATUS_SUCCESS {
            let len = caps.InputReportByteLength as usize;
            tracing::debug!(input_report_byte_length = len, "HidP_GetCaps succeeded");
            len
        } else {
            tracing::warn!(status, "HidP_GetCaps failed; using default report_len");
            DEFAULT
        }
    }

    impl Drop for WindowsHidTransport {
        #[expect(
            clippy::expect_used,
            reason = "poisoned mutex in Drop is unrecoverable"
        )]
        fn drop(&mut self) {
            let h = self.handle.lock().expect("mutex poisoned").0;
            if h != INVALID_HANDLE_VALUE {
                // SAFETY: handle is valid and exclusively owned here.
                unsafe { CloseHandle(h) };
            }
        }
    }

    impl HidTransport for WindowsHidTransport {
        #[expect(
            clippy::expect_used,
            reason = "poisoned mutex is unrecoverable in HID transport"
        )]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "report buffers are always well under u32::MAX"
        )]
        fn read_input_report(&self) -> Result<Vec<u8>, TransportError> {
            let handle = self.handle.lock().expect("mutex poisoned").0;
            if handle == INVALID_HANDLE_VALUE {
                return Err(TransportError::NotOpen);
            }

            let mut buf = vec![0u8; self.report_len];
            let mut bytes_read: u32 = 0;

            // SAFETY: buf is valid for `report_len` bytes; handle is open.
            let ok = unsafe {
                ReadFile(
                    handle,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &raw mut bytes_read,
                    std::ptr::null_mut(),
                )
            };

            if ok == 0 {
                // SAFETY: called immediately after the failing ReadFile.
                let code = unsafe { GetLastError() };
                Err(TransportError::ReadFailed(format!(
                    "ReadFile failed (error {code:#010x})"
                )))
            } else {
                buf.truncate(bytes_read as usize);
                // Windows HID always prepends the report-ID byte (0x01 for
                // the Sidewinder FF2).  Strip it so callers see raw report
                // data, matching the layout parse_input_report expects.
                if buf.len() > 1 {
                    buf.remove(0);
                }
                Ok(buf)
            }
        }

        #[expect(
            clippy::expect_used,
            reason = "poisoned mutex is unrecoverable in HID transport"
        )]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "report buffers are always well under u32::MAX"
        )]
        fn write_output_report(&self, report: &[u8]) -> Result<(), TransportError> {
            let handle = self.handle.lock().expect("mutex poisoned").0;
            if handle == INVALID_HANDLE_VALUE {
                return Err(TransportError::NotOpen);
            }

            // SAFETY: handle is valid; report slice is valid for its length.
            let ok = unsafe {
                HidD_SetOutputReport(
                    handle,
                    report.as_ptr().cast_mut().cast(),
                    report.len() as u32,
                )
            };

            if ok == 0 {
                // SAFETY: called immediately after the failing HidD_SetOutputReport.
                let code = unsafe { GetLastError() };
                Err(TransportError::WriteFailed(format!(
                    "HidD_SetOutputReport failed (error {code:#010x})"
                )))
            } else {
                Ok(())
            }
        }

        #[expect(
            clippy::expect_used,
            reason = "poisoned mutex is unrecoverable in HID transport"
        )]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "report buffers are always well under u32::MAX"
        )]
        fn write_feature_report(&self, report: &[u8]) -> Result<(), TransportError> {
            let handle = self.handle.lock().expect("mutex poisoned").0;
            if handle == INVALID_HANDLE_VALUE {
                return Err(TransportError::NotOpen);
            }

            // SAFETY: handle is valid; report slice is valid for its length.
            let ok = unsafe {
                HidD_SetFeature(
                    handle,
                    report.as_ptr().cast_mut().cast(),
                    report.len() as u32,
                )
            };

            if ok == 0 {
                let code = unsafe { GetLastError() };
                Err(TransportError::WriteFailed(format!(
                    "HidD_SetFeature failed (error {code:#010x})"
                )))
            } else {
                Ok(())
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_read_returns_injected_data() {
        let t = MockTransport::new();
        t.set_input_data(vec![0x01, 0x02, 0x03]);
        assert_eq!(
            t.read_input_report().expect("mock read should succeed"),
            vec![0x01, 0x02, 0x03]
        );
    }

    #[test]
    fn mock_output_report_is_captured() {
        let t = MockTransport::new();
        t.write_output_report(&[0x0A, 0x01, 0xFF])
            .expect("mock write should succeed");
        assert_eq!(t.last_output(), Some(vec![0x0A, 0x01, 0xFF]));
    }

    #[test]
    fn mock_feature_report_is_captured() {
        let t = MockTransport::new();
        t.write_feature_report(&[0x0D, 0x64])
            .expect("mock feature write should succeed");
        assert_eq!(t.last_output(), Some(vec![0x0D, 0x64]));
    }

    #[test]
    fn mock_last_output_none_initially() {
        let t = MockTransport::new();
        assert_eq!(t.last_output(), None);
    }
}
