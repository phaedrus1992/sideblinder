//! `GuiBackend` trait: abstracts the live-data source for the GUI.
//!
//! Two concrete implementations exist:
//! - [`PipeBackend`](crate::pipe_backend::PipeBackend) — connects to a running
//!   `sidewinder-app` over the IPC named pipe.
//! - [`EmbeddedBackend`](crate::embedded_backend::EmbeddedBackend) — opens the
//!   physical Sidewinder device directly in this process.

use sidewinder_ipc::GuiFrame;
use thiserror::Error;

/// Error type for backend operations.
#[derive(Debug, Error)]
pub enum BackendError {
    /// The named pipe could not be connected to.
    #[error("pipe connect failed: {0}")]
    PipeConnect(String),
    /// The physical device could not be opened.
    #[error("device open failed: {0}")]
    DeviceOpen(String),
    /// A background I/O thread failed to start.
    #[error("thread spawn failed: {0}")]
    #[expect(dead_code, reason = "reserved for future thread-based backends")]
    ThreadSpawn(String),
}

/// Abstracts the live-data source for the GUI render loop.
///
/// Implementations must be `Send` so they can be moved into the eframe
/// closure. The `poll` method is called once per frame (~60 Hz) and must
/// return without blocking.
pub trait GuiBackend: Send {
    /// Return the latest live-state frame, or `None` if no new data has
    /// arrived since the last call.  Non-blocking.
    fn poll(&mut self) -> Option<GuiFrame>;

    /// Return `true` if the underlying data source is still alive.  When this
    /// returns `false` the GUI should show a "reconnecting…" indicator.
    fn is_alive(&self) -> bool;
}
