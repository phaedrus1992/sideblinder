//! Library interface for `sideblinder-app`.
//!
//! Re-exports the configuration and status types used by `sideblinder-gui`
//! without exposing the binary entry point or runtime plumbing.

pub mod config;
pub mod status;
#[cfg(any(target_os = "windows", test))]
pub mod bridge;
#[cfg(any(target_os = "windows", test))]
pub mod ipc;
#[cfg(any(target_os = "windows", test))]
pub mod gui_pipe;
