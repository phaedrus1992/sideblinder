//! Library interface for `sidewinder-app`.
//!
//! Re-exports the configuration and status types used by `sidewinder-gui`
//! without exposing the binary entry point or runtime plumbing.

pub mod config;
pub mod status;
#[cfg(any(target_os = "windows", test))]
pub mod bridge;
#[cfg(any(target_os = "windows", test))]
pub mod ipc;
#[cfg(any(target_os = "windows", test))]
pub mod gui_pipe;
