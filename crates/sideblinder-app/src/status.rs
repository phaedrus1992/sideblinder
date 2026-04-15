//! Shared device-connection status type.
//!
//! Kept in its own module so it can be imported by both `bridge` (Windows-only)
//! and `tray` (all platforms) without conditional compilation complexity.

/// Whether the Windows kernel driver IPC device was successfully opened at startup.
///
/// Determined once before the bridge tasks are spawned; drives the tray tooltip
/// text and the "Install Driver…" menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStatus {
    /// `\\.\SideblinderFFB2` opened successfully — full FFB bridge is running.
    Present,
    /// The driver device was not found; app is running in input-only mode.
    ///
    /// Constructed in Windows startup code and used in cross-platform test helpers
    /// (`status_label`). Gated so non-test non-Windows builds don't see dead code.
    #[cfg(any(target_os = "windows", test))]
    Missing,
}

/// Aggregated result of the startup self-test performed before spawning bridge tasks.
///
/// Passed to the tray so the tooltip and menu can reflect the system state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupStatus {
    /// Whether the driver IPC device was opened successfully.
    pub driver: DriverStatus,
    /// Whether axis calibration data has been recorded (i.e. the `[calibration]`
    /// section in config differs from the uncalibrated defaults).
    pub calibration_set: bool,
}

/// Physical device connection state broadcast from the input loop to the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Device is present and the input loop is running.
    Connected,
    /// Device was disconnected; the bridge is retrying.
    ///
    /// Constructed in Windows bridge code and used in cross-platform tests.
    /// Gated so non-test non-Windows builds don't see dead code.
    #[cfg(any(target_os = "windows", test))]
    Disconnected,
}
