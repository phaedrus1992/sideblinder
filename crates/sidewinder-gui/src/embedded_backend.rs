//! `EmbeddedBackend`: opens the physical Sidewinder device directly in-process.
//!
//! Used as a fallback when `sidewinder-app` is not running and the named pipe
//! is unavailable.  A background thread opens the device, reads input at the
//! device's native polling rate, builds `GuiFrame` snapshots, and forwards
//! them via an `mpsc` channel.  The config file is re-read every second so
//! that changes written by the GUI are picked up without restarting.
//!
//! On non-Windows platforms [`EmbeddedBackend::start`] always returns an
//! error; the GUI falls through to [`disconnected`] mode.

use crate::backend::{BackendError, GuiBackend};
use sidewinder_ipc::GuiFrame;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

/// Live-data backend that drives the physical joystick in-process.
pub struct EmbeddedBackend {
    rx: mpsc::Receiver<GuiFrame>,
    alive: Arc<AtomicBool>,
}

impl EmbeddedBackend {
    /// Open the physical Sidewinder device and start streaming frames.
    ///
    /// Spawns a background thread that polls the device and rebuilds
    /// [`GuiFrame`]s at the device's native HID polling rate (~100 Hz).
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::DeviceOpen`] if the device cannot be opened
    /// (not connected, driver issue, or non-Windows platform).
    pub fn start(config_path: PathBuf) -> Result<Self, BackendError> {
        start_impl(config_path)
    }

    /// Return a backend that immediately reports disconnected state.
    ///
    /// Used when the device could not be opened so the GUI can still launch
    /// and show the "disconnected" screen.
    #[must_use]
    pub fn disconnected() -> Self {
        // Create a channel that will never receive anything.
        let (_tx, rx) = mpsc::sync_channel::<GuiFrame>(0);
        let alive = Arc::new(AtomicBool::new(false));
        Self { rx, alive }
    }
}

impl GuiBackend for EmbeddedBackend {
    fn poll(&mut self) -> Option<GuiFrame> {
        // Drain all queued frames and return the latest; non-blocking.
        let mut latest = self.rx.try_recv().ok()?;
        while let Ok(frame) = self.rx.try_recv() {
            latest = frame;
        }
        Some(latest)
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
}

// ── Platform implementations ──────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
fn start_impl(_config_path: PathBuf) -> Result<EmbeddedBackend, BackendError> {
    Err(BackendError::DeviceOpen(
        "Direct device access is only supported on Windows".to_owned(),
    ))
}

#[cfg(target_os = "windows")]
fn start_impl(config_path: PathBuf) -> Result<EmbeddedBackend, BackendError> {
    use sidewinder_hid::device::SidewinderDevice;

    let device = SidewinderDevice::open().map_err(|e| BackendError::DeviceOpen(e.to_string()))?;
    let (tx, rx) = mpsc::sync_channel::<GuiFrame>(4);
    let alive = Arc::new(AtomicBool::new(true));
    let alive_clone = alive.clone();

    std::thread::spawn(move || {
        device_read_loop(device, config_path, tx, alive_clone);
    });

    Ok(EmbeddedBackend { rx, alive })
}

// ── Device reader loop ────────────────────────────────────────────────────────

/// Build a [`GuiFrame`] from raw [`sidewinder_hid::input::InputState`] and config scalars.
///
/// Duplicates the logic in `sidewinder-app/bridge.rs` so the GUI does not need
/// to import platform-gated bridge internals.
#[cfg(target_os = "windows")]
fn frame_from_state(
    state: &sidewinder_hid::input::InputState,
    connected: bool,
    ffb_enabled: bool,
    ffb_gain: u8,
) -> GuiFrame {
    use sidewinder_hid::input::PovDirection;
    let pov = match state.pov {
        PovDirection::North => 0,
        PovDirection::NorthEast => 1,
        PovDirection::East => 2,
        PovDirection::SouthEast => 3,
        PovDirection::South => 4,
        PovDirection::SouthWest => 5,
        PovDirection::West => 6,
        PovDirection::NorthWest => 7,
        PovDirection::Center => 0xFF,
    };
    GuiFrame {
        axes: state.axes,
        buttons: state.buttons,
        pov,
        connected: u8::from(connected),
        ffb_enabled: u8::from(ffb_enabled),
        ffb_gain,
    }
}

/// Config reload interval: re-read the file at most once per second.
///
/// The GUI writes config via `toml_edit`; the embedded backend needs to pick
/// up those changes so sliders and toggles feel responsive.
#[cfg(target_os = "windows")]
const CONFIG_RELOAD_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

#[cfg(target_os = "windows")]
#[expect(
    clippy::needless_pass_by_value,
    reason = "thread function — takes ownership for its entire lifetime"
)]
fn device_read_loop(
    device: sidewinder_hid::device::SidewinderDevice,
    config_path: PathBuf,
    tx: mpsc::SyncSender<GuiFrame>,
    alive: Arc<AtomicBool>,
) {
    let mut config = sidewinder_app::config::Config::load(&config_path).unwrap_or_default();
    let mut last_reload = std::time::Instant::now();

    loop {
        // Reload config periodically so GUI write-backs are reflected.
        if last_reload.elapsed() >= CONFIG_RELOAD_INTERVAL {
            match sidewinder_app::config::Config::load(&config_path) {
                Ok(new_cfg) => config = new_cfg,
                Err(e) => tracing::warn!(
                    internal_error = %e,
                    "embedded backend: config reload failed — using stale config"
                ),
            }
            last_reload = std::time::Instant::now();
        }

        match device.poll() {
            Ok(state) => {
                let frame =
                    frame_from_state(&state, true, config.ffb_enabled, config.ffb_gain);
                // Drop frame on backpressure; GUI will pick up the next one.
                let _ = tx.try_send(frame);
            }
            Err(e) => {
                tracing::warn!(internal_error = %e, "embedded backend: device read failed");
                alive.store(false, Ordering::Relaxed);
                return;
            }
        }
    }
}
