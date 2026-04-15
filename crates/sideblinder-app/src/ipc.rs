//! Driver IPC layer — pushes input state to and pulls FFB reports from the
//! Sideblinder UMDF2 driver via `DeviceIoControl`.
//!
//! On non-Windows hosts this module compiles to no-ops so the rest of the app
//! (config, logging, etc.) remains buildable and testable.

use thiserror::Error;

// ── Custom IOCTL codes (must match sideblinder-driver/src/ioctl.rs) ────────────

/// App → Driver: push a new input snapshot.
pub const IOCTL_SIDEBLINDER_UPDATE_INPUT: u32 =
    (0x0022u32 << 16) | (0x0002u32 << 14) | (0x0800u32 << 2);

/// App ← Driver: pop the next FFB output report.
pub const IOCTL_SIDEBLINDER_GET_FFB: u32 = (0x0022u32 << 16) | (0x0001u32 << 14) | (0x0801u32 << 2);

// ── Error ─────────────────────────────────────────────────────────────────────

/// Errors from driver IPC operations.
#[derive(Debug, Error)]
pub enum IpcError {
    /// Could not open a handle to the driver device.
    ///
    /// Constructed only in `windows_impl`; gated so non-Windows builds don't
    /// see dead code.
    #[cfg(target_os = "windows")]
    #[error("{0}")]
    Open(String),
    /// A `DeviceIoControl` call failed.
    ///
    /// Constructed only in `windows_impl`; gated so non-Windows builds don't
    /// see dead code.
    #[cfg(target_os = "windows")]
    #[error("{0}")]
    Ioctl(String),
}

// ── Snapshot type (mirrors driver's InputSnapshot — must stay in sync) ────────

/// Joystick state transmitted to the driver.
///
/// `buttons` is a bitmask of virtual button indices (0-based).  The field is
/// `u32` so that up to 32 virtual buttons can be reported — physical buttons
/// (0–8) plus hat-as-buttons directions all fit comfortably within that range.
///
/// **ABI note:** this struct is sent verbatim via `DeviceIoControl` and must
/// match the layout expected by the driver.  `#[repr(C)]` ensures no padding
/// surprises; field types and order must stay in sync with the driver's
/// `InputSnapshot` definition.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct InputSnapshot {
    pub axes: [i16; 4],
    pub buttons: u32,
    pub pov: u8,
}

impl InputSnapshot {
    /// Build from a [`sideblinder_hid::input::InputState`].
    /// Called by the Windows bridge and in tests.
    #[cfg(test)]
    #[must_use]
    pub fn from_input_state(state: &sideblinder_hid::input::InputState) -> Self {
        Self {
            axes: [state.axes[0], state.axes[1], state.axes[2], state.axes[3]],
            buttons: u32::from(state.buttons),
            pov: match state.pov {
                sideblinder_hid::input::PovDirection::North => 0,
                sideblinder_hid::input::PovDirection::NorthEast => 1,
                sideblinder_hid::input::PovDirection::East => 2,
                sideblinder_hid::input::PovDirection::SouthEast => 3,
                sideblinder_hid::input::PovDirection::South => 4,
                sideblinder_hid::input::PovDirection::SouthWest => 5,
                sideblinder_hid::input::PovDirection::West => 6,
                sideblinder_hid::input::PovDirection::NorthWest => 7,
                sideblinder_hid::input::PovDirection::Center => 0xFF,
            },
        }
    }
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Abstracts driver IPC so the bridge can be tested without a real driver.
pub trait DriverIpc: Send + Sync {
    /// Push a new joystick snapshot to the virtual device.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] on failure.
    fn push_input(&self, snap: InputSnapshot) -> Result<(), IpcError>;

    /// Pull the next pending FFB report from the driver.
    ///
    /// Returns `Ok(None)` when no report is queued.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] on I/O failure.
    fn get_ffb(&self) -> Result<Option<Vec<u8>>, IpcError>;
}

// ── Mock ──────────────────────────────────────────────────────────────────────

/// Test double that records the last pushed snapshot.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct MockDriverIpc {
    pub last_input: std::sync::Mutex<Option<InputSnapshot>>,
}

#[cfg(test)]
impl DriverIpc for MockDriverIpc {
    fn push_input(&self, snap: InputSnapshot) -> Result<(), IpcError> {
        #[expect(
            clippy::expect_used,
            reason = "MockDriverIpc is a test double; mutex poisoning is a test bug"
        )]
        let mut guard = self.last_input.lock().expect("mutex poisoned");
        *guard = Some(snap);
        Ok(())
    }

    fn get_ffb(&self) -> Result<Option<Vec<u8>>, IpcError> {
        Ok(None)
    }
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub use windows_impl::WindowsDriverIpc;

#[cfg(target_os = "windows")]
#[expect(
    unsafe_code,
    reason = "Windows named-pipe IPC requires raw Win32 CreateFile/WriteFile/ReadFile FFI calls"
)]
mod windows_impl {
    use super::{
        DriverIpc, IOCTL_SIDEBLINDER_GET_FFB, IOCTL_SIDEBLINDER_UPDATE_INPUT, InputSnapshot, IpcError,
    };
    use std::{mem, sync::Mutex};
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_NO_MORE_ITEMS, GENERIC_READ, GENERIC_WRITE, HANDLE,
            INVALID_HANDLE_VALUE,
        },
        Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
        System::IO::DeviceIoControl,
    };

    // Symbolic link created by the driver's INF / INX for user-mode access.
    const DEVICE_SYMLINK: &str = "\\\\.\\SideblinderFFB2";

    /// Production IPC client backed by `DeviceIoControl`.
    pub struct WindowsDriverIpc {
        handle: Mutex<HANDLE>,
    }

    // SAFETY: Send — HANDLE (*mut c_void) is safe to move to another thread because
    // Win32 kernel objects are not thread-affine; the handle value is just an opaque index.
    // Sync — all access to the handle is serialised through the Mutex, so concurrent
    // calls are safe.
    unsafe impl Send for WindowsDriverIpc {}
    unsafe impl Sync for WindowsDriverIpc {}

    impl WindowsDriverIpc {
        /// Open a handle to the driver device.
        ///
        /// # Errors
        ///
        /// Returns [`IpcError::Open`] if `CreateFileW` fails.
        pub fn open() -> Result<Self, IpcError> {
            let wide: Vec<u16> = DEVICE_SYMLINK
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            // SAFETY: wide is null-terminated; constants are valid FFI values.
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                )
            };

            if handle == INVALID_HANDLE_VALUE {
                // SAFETY: GetLastError is always safe to call after a Win32 failure.
                let err_code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
                return Err(IpcError::Open(format!(
                    "Couldn't talk to the driver (CreateFileW error {err_code:#010x}). \
                     Try reinstalling it using the install script."
                )));
            }

            Ok(Self {
                handle: Mutex::new(handle),
            })
        }
    }

    impl Drop for WindowsDriverIpc {
        #[expect(
            clippy::expect_used,
            reason = "poisoned mutex in Drop is unrecoverable"
        )]
        fn drop(&mut self) {
            let h = *self.handle.lock().expect("mutex poisoned");
            if h != INVALID_HANDLE_VALUE {
                // SAFETY: handle is valid and owned here.
                unsafe { CloseHandle(h) };
            }
        }
    }

    impl DriverIpc for WindowsDriverIpc {
        #[expect(clippy::expect_used, reason = "poisoned mutex is unrecoverable in IPC")]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "InputSnapshot size always fits in u32"
        )]
        fn push_input(&self, snap: InputSnapshot) -> Result<(), IpcError> {
            let handle = *self.handle.lock().expect("mutex poisoned");
            let mut bytes_returned: u32 = 0;

            // SAFETY: snap is repr(C); handle is valid.
            let ok = unsafe {
                DeviceIoControl(
                    handle,
                    IOCTL_SIDEBLINDER_UPDATE_INPUT,
                    (&raw const snap).cast(),
                    mem::size_of::<InputSnapshot>() as u32,
                    std::ptr::null_mut(),
                    0,
                    &raw mut bytes_returned,
                    std::ptr::null_mut(),
                )
            };

            if ok == 0 {
                // SAFETY: GetLastError is always safe to call after a Win32 failure.
                let err_code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
                Err(IpcError::Ioctl(format!(
                    "Couldn't send input to the driver (DeviceIoControl error {err_code:#010x}). \
                     Try reinstalling it using the install script."
                )))
            } else {
                Ok(())
            }
        }

        #[expect(clippy::expect_used, reason = "poisoned mutex is unrecoverable in IPC")]
        #[expect(clippy::cast_possible_truncation, reason = "BUF_LEN fits in u32")]
        fn get_ffb(&self) -> Result<Option<Vec<u8>>, IpcError> {
            const BUF_LEN: usize = 64;
            let handle = *self.handle.lock().expect("mutex poisoned");
            let mut buf = [0u8; BUF_LEN];
            let mut bytes_returned: u32 = 0;

            // SAFETY: buf is valid; handle is valid.
            let ok = unsafe {
                DeviceIoControl(
                    handle,
                    IOCTL_SIDEBLINDER_GET_FFB,
                    std::ptr::null(),
                    0,
                    buf.as_mut_ptr().cast(),
                    BUF_LEN as u32,
                    &raw mut bytes_returned,
                    std::ptr::null_mut(),
                )
            };

            if ok == 0 {
                // SAFETY: GetLastError is always safe to call after a Win32 failure.
                let err_code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
                // ERROR_NO_MORE_ITEMS (0x103) means the FFB queue is empty — not an error.
                // Any other code indicates a real I/O failure.
                if err_code == ERROR_NO_MORE_ITEMS {
                    return Ok(None);
                }
                return Err(IpcError::Ioctl(format!(
                    "Couldn't read force feedback from the driver \
                     (DeviceIoControl error {err_code:#010x}). \
                     Try reinstalling it using the install script."
                )));
            }

            if bytes_returned == 0 {
                Ok(None)
            } else {
                Ok(Some(buf[..bytes_returned as usize].to_vec()))
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code — panics are acceptable")]
mod tests {
    use super::*;

    #[test]
    fn mock_records_last_input() {
        let ipc = MockDriverIpc::default();
        let snap = InputSnapshot {
            axes: [100, -200, 0, 0],
            buttons: 0b101,
            pov: 0xFF,
        };
        ipc.push_input(snap).expect("push_input must succeed");
        let guard = ipc.last_input.lock().expect("mutex must not be poisoned");
        let recorded = guard.expect("snapshot must have been recorded");
        assert_eq!(recorded.axes[0], 100);
        assert_eq!(recorded.buttons, 0b101);
    }

    #[test]
    fn mock_get_ffb_returns_none() {
        let ipc = MockDriverIpc::default();
        assert!(ipc.get_ffb().expect("get_ffb must succeed").is_none());
    }

    #[test]
    fn ioctl_codes_are_nonzero_and_distinct() {
        assert_ne!(IOCTL_SIDEBLINDER_UPDATE_INPUT, 0);
        assert_ne!(IOCTL_SIDEBLINDER_GET_FFB, 0);
        assert_ne!(IOCTL_SIDEBLINDER_UPDATE_INPUT, IOCTL_SIDEBLINDER_GET_FFB);
    }

    #[test]
    fn snapshot_from_input_state_center() {
        use sideblinder_hid::input::{InputState, PovDirection};
        let state = InputState {
            axes: [0, 0, 0, 0, 0, 0, 0, 0],
            buttons: 0,
            pov: PovDirection::Center,
        };
        let snap = InputSnapshot::from_input_state(&state);
        assert_eq!(snap.pov, 0xFF);
        assert_eq!(snap.axes, [0, 0, 0, 0]);
    }
}
