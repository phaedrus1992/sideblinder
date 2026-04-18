//! `PipeBackend`: reads live `GuiFrame`s from a running `sideblinder-app` instance
//! via the named pipe `\\.\pipe\SideblinderGui`.
//!
//! A background thread connects to the pipe and reads 27-byte length-prefixed
//! frames in a blocking loop, forwarding each frame via an `mpsc` channel.  The
//! egui render thread calls `poll()` each frame to drain the latest value.

use crate::backend::{BackendError, GuiBackend};
use sideblinder_ipc::GuiFrame;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

/// Live-data backend that reads [`GuiFrame`]s from the `sideblinder-app` IPC pipe.
pub struct PipeBackend {
    rx: mpsc::Receiver<GuiFrame>,
    alive: Arc<AtomicBool>,
}

impl PipeBackend {
    /// Try to connect to a running `sideblinder-app` instance.
    ///
    /// Opens `\\.\pipe\SideblinderGui` and spawns a background thread that reads
    /// frames continuously.  Returns [`BackendError::PipeConnect`] if the pipe
    /// is not available (app not running, or non-Windows platform).
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::PipeConnect`] if the named pipe cannot be opened.
    pub fn connect() -> Result<Self, BackendError> {
        connect_impl()
    }
}

impl GuiBackend for PipeBackend {
    fn poll(&mut self) -> Option<GuiFrame> {
        // Drain all queued frames and return the latest; non-blocking.
        let mut latest = None;
        while let Ok(frame) = self.rx.try_recv() {
            latest = Some(frame);
        }
        latest
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
}

// ── Platform implementations ──────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
fn connect_impl() -> Result<PipeBackend, BackendError> {
    Err(BackendError::PipeConnect(
        "Windows named pipes are only available on Windows".to_owned(),
    ))
}

#[cfg(target_os = "windows")]
fn connect_impl() -> Result<PipeBackend, BackendError> {
    windows_impl::connect()
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
#[expect(
    unsafe_code,
    reason = "Windows named pipe client requires raw Win32 CreateFileW / ReadFile / CloseHandle FFI"
)]
mod windows_impl {
    use super::{AtomicBool, Arc, BackendError, GuiFrame, Ordering, PipeBackend};
    use sideblinder_ipc::FRAME_TOTAL_LEN;
    use std::sync::mpsc;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, ReadFile, OPEN_EXISTING},
    };

    /// Wraps a Win32 `HANDLE` so that `Drop` closes it and the value can be
    /// moved to the reader thread.
    struct OwnedHandle(HANDLE);

    // SAFETY: Win32 kernel objects are not thread-affine.  A HANDLE is an opaque
    // index into the kernel handle table; moving it between threads is safe.
    unsafe impl Send for OwnedHandle {}

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: self.0 is a valid handle obtained from CreateFileW.
            // CloseHandle on a valid handle is always safe to call.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// Try to open the named pipe as a client and return a connected backend.
    pub(super) fn connect() -> Result<PipeBackend, BackendError> {
        let pipe_name_wide: Vec<u16> = sideblinder_ipc::PIPE_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // SAFETY: pipe_name_wide is null-terminated; all other arguments are
        // well-known constants.  INVALID_HANDLE_VALUE check follows immediately.
        let handle = unsafe {
            CreateFileW(
                pipe_name_wide.as_ptr(),
                GENERIC_READ,
                0, // no sharing
                std::ptr::null(),
                OPEN_EXISTING,
                0, // synchronous I/O
                std::ptr::null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return Err(BackendError::PipeConnect(
                "sideblinder-app pipe not available — is the app running?".to_owned(),
            ));
        }

        let (tx, rx) = mpsc::sync_channel::<GuiFrame>(4);
        let alive = Arc::new(AtomicBool::new(true));
        let alive_clone = alive.clone();
        let owned = OwnedHandle(handle);

        std::thread::spawn(move || {
            read_loop(owned, tx, alive_clone);
        });

        Ok(PipeBackend { rx, alive })
    }

    /// Blocking reader loop: reads 27-byte frames from the pipe until the
    /// server disconnects or a read error occurs.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "thread function — takes ownership of handle, tx, and alive for its entire lifetime"
    )]
    fn read_loop(handle: OwnedHandle, tx: mpsc::SyncSender<GuiFrame>, alive: Arc<AtomicBool>) {
        loop {
            let mut buf = [0u8; FRAME_TOTAL_LEN];
            let mut offset = 0usize;

            // Read exactly FRAME_TOTAL_LEN bytes, handling short reads.
            while offset < FRAME_TOTAL_LEN {
                let mut bytes_read: u32 = 0;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "FRAME_TOTAL_LEN = 27, always fits in u32"
                )]
                // SAFETY: handle is valid; buf[offset..] slice pointer and length are correct.
                let ok = unsafe {
                    ReadFile(
                        handle.0,
                        buf[offset..].as_mut_ptr().cast(),
                        (FRAME_TOTAL_LEN - offset) as u32,
                        std::ptr::addr_of_mut!(bytes_read),
                        std::ptr::null_mut(),
                    )
                };
                if ok == 0 || bytes_read == 0 {
                    alive.store(false, Ordering::Relaxed);
                    return;
                }
                offset += bytes_read as usize;
            }

            if let Ok(frame) = GuiFrame::decode(&buf) {
                // If the channel is full, drop the oldest frame rather than blocking.
                let _ = tx.try_send(frame);
            }
        }
    }
}
