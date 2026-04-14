//! Input and FFB bridge loops.
//!
//! Two Tokio tasks run concurrently (Windows only):
//!
//! 1. **Input loop** — polls the physical Sidewinder via `SidewinderDevice`,
//!    applies axis/button mapping from the current config, and pushes the
//!    result to the virtual device via the driver IPC.
//!
//! 2. **FFB loop** — polls the driver for pending force-feedback output reports
//!    and forwards them to the physical device.

use sidewinder_hid::input::InputState;
#[cfg(target_os = "windows")]
use sidewinder_hid::input::SmoothingBuffer;

#[cfg(target_os = "windows")]
use crate::status::ConnectionStatus;
use crate::{config::Config, ipc::InputSnapshot};

// ── GuiFrame helpers ──────────────────────────────────────────────────────────

/// Build a [`sidewinder_ipc::GuiFrame`] from a raw [`InputState`] and config scalars.
///
/// Called by the GUI pipe server task each tick to construct the frame that is
/// written to the named pipe.  Kept here (close to `apply_config`) so the two
/// functions evolve together.
#[cfg(any(target_os = "windows", test))]
#[must_use]
pub fn gui_frame_from_input(
    state: &InputState,
    connected: bool,
    ffb_enabled: bool,
    ffb_gain: u8,
) -> sidewinder_ipc::GuiFrame {
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
    sidewinder_ipc::GuiFrame {
        axes: state.axes,
        buttons: state.buttons,
        pov,
        connected: u8::from(connected),
        ffb_enabled: u8::from(ffb_enabled),
        ffb_gain,
    }
}

// ── Bridge ────────────────────────────────────────────────────────────────────

/// Holds the shared state needed by both bridge loops.
///
/// Only constructed on Windows where both the physical device and driver IPC
/// are available.  On other platforms the struct exists for compilation only.
#[cfg(target_os = "windows")]
pub struct Bridge {
    transport: std::sync::Arc<dyn sidewinder_hid::hid_transport::HidTransport>,
    ipc: std::sync::Arc<dyn crate::ipc::DriverIpc>,
    config_rx: tokio::sync::watch::Receiver<Config>,
    status_tx: tokio::sync::watch::Sender<ConnectionStatus>,
    /// Sends raw (post-smoothing, pre-config) input snapshots to the GUI pipe
    /// server so the settings window can show live axis values.
    input_state_tx: tokio::sync::watch::Sender<InputState>,
}

#[cfg(target_os = "windows")]
impl Bridge {
    /// Construct a bridge with the given transport, IPC client, and config
    /// receiver.
    ///
    /// Returns the bridge, a watch receiver for physical-device connection state,
    /// and a watch receiver for raw input state consumed by the GUI pipe server.
    pub fn new(
        transport: std::sync::Arc<dyn sidewinder_hid::hid_transport::HidTransport>,
        ipc: std::sync::Arc<dyn crate::ipc::DriverIpc>,
        config_rx: tokio::sync::watch::Receiver<Config>,
    ) -> (
        Self,
        tokio::sync::watch::Receiver<ConnectionStatus>,
        tokio::sync::watch::Receiver<InputState>,
    ) {
        let (status_tx, status_rx) = tokio::sync::watch::channel(ConnectionStatus::Connected);
        let (input_state_tx, input_state_rx) =
            tokio::sync::watch::channel(InputState::default());
        (
            Self {
                transport,
                ipc,
                config_rx,
                status_tx,
                input_state_tx,
            },
            status_rx,
            input_state_rx,
        )
    }

    /// Spawn both bridge loops and return immediately.
    ///
    /// Both tasks run until a fatal error occurs or the process exits.
    pub fn spawn(self) {
        // Shared transport watch: input_loop sends an updated Arc on reconnect;
        // ffb_loop reads the latest Arc at the top of each iteration so it
        // never writes to a stale handle after a device replug.
        let (transport_tx, transport_rx) = tokio::sync::watch::channel(self.transport.clone());

        let ipc_input = self.ipc.clone();
        let config_rx_input = self.config_rx.clone();
        let status_tx_input = self.status_tx;
        let input_state_tx_input = self.input_state_tx;

        let ipc_ffb = self.ipc.clone();
        let config_rx_ffb = self.config_rx.clone();

        tokio::spawn(input_loop(
            self.transport,
            transport_tx,
            ipc_input,
            config_rx_input,
            status_tx_input,
            input_state_tx_input,
        ));
        tokio::spawn(ffb_loop(transport_rx, ipc_ffb, config_rx_ffb));
    }
}

// ── Input loop ────────────────────────────────────────────────────────────────

/// Per-axis rolling-average smoothing buffers for the input loop.
///
/// Rebuilt whenever the config's smoothing windows change.
#[cfg(target_os = "windows")]
struct AxisSmoothing {
    x: SmoothingBuffer,
    y: SmoothingBuffer,
    rz: SmoothingBuffer,
    slider: SmoothingBuffer,
}

#[cfg(target_os = "windows")]
impl AxisSmoothing {
    fn from_config(config: &Config) -> Self {
        // Clamp smoothing to SMOOTHING_MAX so out-of-range config values
        // (which validate() warns about but does not reject) don't allocate
        // oversized buffers.
        let window = |s: u8| (s.min(crate::config::SMOOTHING_MAX) as usize).max(1);
        Self {
            x: SmoothingBuffer::new(window(config.axis_x.smoothing)),
            y: SmoothingBuffer::new(window(config.axis_y.smoothing)),
            rz: SmoothingBuffer::new(window(config.axis_rz.smoothing)),
            slider: SmoothingBuffer::new(window(config.axis_slider.smoothing)),
        }
    }

    fn smoothing_matches(&self, config: &Config) -> bool {
        self.x.window() == config.axis_x.smoothing as usize
            && self.y.window() == config.axis_y.smoothing as usize
            && self.rz.window() == config.axis_rz.smoothing as usize
            && self.slider.window() == config.axis_slider.smoothing as usize
    }
}

/// Reconnect interval when the device is absent.
///
/// 2 s is short enough that the user barely notices the gap, but long enough
/// to avoid hammering the device enumeration API while the OS re-enumerates
/// after a replug.
#[cfg(target_os = "windows")]
const RECONNECT_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(2);

/// Continuously read from the physical device and forward to the virtual one.
///
/// On a read error the loop transitions to a reconnect phase: it logs the
/// disconnect, signals [`ConnectionStatus::Disconnected`] to the tray, and
/// retries `SidewinderDevice::open` every [`RECONNECT_INTERVAL`].  Once the
/// device is available again it publishes the new transport to `transport_tx`
/// (so `ffb_loop` picks it up too), transitions back to
/// [`ConnectionStatus::Connected`], and resumes forwarding input.  The virtual
/// device continues to exist throughout (axes stay frozen at their last value
/// while disconnected).
#[cfg(target_os = "windows")]
async fn input_loop(
    transport: std::sync::Arc<dyn sidewinder_hid::hid_transport::HidTransport>,
    transport_tx: tokio::sync::watch::Sender<
        std::sync::Arc<dyn sidewinder_hid::hid_transport::HidTransport>,
    >,
    ipc: std::sync::Arc<dyn crate::ipc::DriverIpc>,
    mut config_rx: tokio::sync::watch::Receiver<Config>,
    status_tx: tokio::sync::watch::Sender<ConnectionStatus>,
    input_state_tx: tokio::sync::watch::Sender<InputState>,
) {
    use sidewinder_hid::device::SidewinderDevice;
    use tracing::{debug, error, info, warn};

    let initial_config = config_rx.borrow_and_update().clone();
    let mut smoothing = AxisSmoothing::from_config(&initial_config);
    drop(initial_config);

    // Start with the transport that was opened during startup.
    let mut active_transport = transport;

    loop {
        // Snapshot the current config and mark it seen so the next iteration
        // picks up any change that arrived while we were blocked on I/O.
        let config = config_rx.borrow_and_update().clone();

        // Rebuild smoothing buffers if window sizes changed (hot-reload).
        if !smoothing.smoothing_matches(&config) {
            smoothing = AxisSmoothing::from_config(&config);
        }

        // Block-read one input report.  This is a synchronous call; we run it
        // on a blocking thread so the async executor isn't stalled.
        let transport_ref = active_transport.clone();
        let raw = tokio::task::spawn_blocking(move || transport_ref.read_input_report()).await;

        let report = match raw {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                info!("Joystick disconnected — waiting for reconnect... ({e})");
                if status_tx.send(ConnectionStatus::Disconnected).is_err() {
                    debug!("status channel closed (tray exited); continuing without UI updates");
                }

                // Reconnect loop: retry opening the device until it comes back.
                // `SidewinderDevice::open` runs in `spawn_blocking` to avoid
                // blocking the async executor during USB re-enumeration, which
                // can take tens of milliseconds right after a replug event.
                loop {
                    tokio::time::sleep(RECONNECT_INTERVAL).await;
                    let open_result = tokio::task::spawn_blocking(SidewinderDevice::open).await;
                    match open_result {
                        Ok(Ok(device)) => {
                            let new_transport: std::sync::Arc<
                                dyn sidewinder_hid::hid_transport::HidTransport,
                            > = std::sync::Arc::from(device.into_transport());
                            // Publish to ffb_loop so it stops writing to the stale handle.
                            if transport_tx.send(new_transport.clone()).is_err() {
                                error!("FFB loop has exited; FFB will not work after reconnect");
                            }
                            active_transport = new_transport;
                            info!("Joystick reconnected");
                            if status_tx.send(ConnectionStatus::Connected).is_err() {
                                debug!("status channel closed; tray will not reflect reconnect");
                            }
                            break;
                        }
                        Ok(Err(open_err)) => {
                            debug!("reconnect attempt failed: {open_err}");
                        }
                        Err(e) => {
                            error!("spawn_blocking panicked during reconnect open: {e}");
                        }
                    }
                }
                continue;
            }
            Err(e) => {
                error!("spawn_blocking panicked: {e}");
                // Restart the loop rather than exiting — otherwise the FFB loop
                // keeps running while input stops permanently.
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                continue;
            }
        };

        let mut state = match sidewinder_hid::input::parse_input_report(&report) {
            Ok(s) => s,
            Err(e) => {
                warn!(
                internal_error = %e,
                "Received an unexpected response from the joystick. If this keeps happening, run Diagnose from the tray menu."
            );
                continue;
            }
        };

        // Apply per-axis smoothing before config processing.
        state.axes[0] = smoothing.x.push(state.axes[0]);
        state.axes[1] = smoothing.y.push(state.axes[1]);
        state.axes[2] = smoothing.rz.push(state.axes[2]);
        state.axes[3] = smoothing.slider.push(state.axes[3]);

        // Publish post-smoothing raw input to the GUI pipe server.
        // Watch channels are last-write-wins; a slow GUI reader never stalls
        // this loop.  Ignore send errors — the receiver being gone is fine.
        let _ = input_state_tx.send(state);

        let snap = apply_config(&config, &state);
        debug!(
            "input: axes={:?} btns={:#05x} pov={:?}",
            snap.axes, snap.buttons, state.pov
        );

        if let Err(e) = ipc.push_input(snap) {
            error!(
                internal_error = %e,
                "Couldn't send joystick state to the driver. Try reinstalling it using the install script."
            );
        }

        tokio::task::yield_now().await;
    }
}

// ── FFB loop ──────────────────────────────────────────────────────────────────

/// Report ID for HID PID Device Gain — used to identify and scale gain reports.
///
/// Must match `sidewinder_hid::ffb::report_id::DEVICE_GAIN` (0x0D).
/// Defined here to avoid a cross-crate private-module dependency.
#[cfg(target_os = "windows")]
const REPORT_ID_DEVICE_GAIN: u8 = 0x0D;

/// Scale a raw HID Device Gain byte by the user-configured gain factor.
///
/// Both operands are in `[0, 255]`; the maximum product is 255 × 255 = 65 025
/// which fits in `u32` before the division, so there is no overflow risk.
/// The result is always in `[0, 255]`.
#[cfg(any(target_os = "windows", test))]
#[expect(
    clippy::cast_possible_truncation,
    reason = "both operands are u8-range values; max product / 255 = 255"
)]
fn scale_gain(packet_gain: u8, ffb_gain: u8) -> u8 {
    ((u32::from(packet_gain) * u32::from(ffb_gain)) / 255) as u8
}

/// Continuously poll the driver for FFB output reports and forward them to
/// the physical device.
///
/// `transport_rx` carries the current live transport.  `input_loop` publishes
/// a new value whenever the device reconnects so this loop always writes to
/// the current handle.
///
/// `config_rx` is read on every iteration so that `ffb_gain` and `ffb_enabled`
/// changes hot-reload without a restart.
#[cfg(target_os = "windows")]
async fn ffb_loop(
    mut transport_rx: tokio::sync::watch::Receiver<
        std::sync::Arc<dyn sidewinder_hid::hid_transport::HidTransport>,
    >,
    ipc: std::sync::Arc<dyn crate::ipc::DriverIpc>,
    mut config_rx: tokio::sync::watch::Receiver<Config>,
) {
    use sidewinder_hid::ffb::{FfbOperation, build_operation_report};
    use tracing::{debug, error, info};

    let mut ffb_enabled_prev = true; // match Config default

    loop {
        // Always read the latest transport Arc so we use the current handle
        // after a reconnect.  When the transport changes (device reconnected),
        // force-resend the current enable/disable state so the new device
        // handle reflects the user's config without requiring a config change.
        let transport_changed = transport_rx.has_changed().unwrap_or(false);
        let transport = transport_rx.borrow_and_update().clone();
        if transport_changed {
            // Invalidate prev so the enable/disable command is resent this iteration.
            ffb_enabled_prev = !ffb_enabled_prev;
        }

        // Read the two scalar config fields we need; drop the borrow before any
        // await point so the watch channel is not held across a suspension.
        let (ffb_enabled, ffb_gain) = {
            let config = config_rx.borrow_and_update();
            (config.ffb_enabled, config.ffb_gain)
        };

        // Send Device Control report when enable/disable state changes.
        if ffb_enabled != ffb_enabled_prev {
            let report = build_operation_report(FfbOperation::EnableActuators {
                enable: ffb_enabled,
            });
            if let Err(e) = transport.write_output_report(&report) {
                error!(
                    internal_error = %e,
                    "Lost connection to the joystick. Waiting for reconnect..."
                );
                // Do not update ffb_enabled_prev — retry the enable/disable on
                // the next iteration so the command is not silently lost.
            } else {
                info!(
                    ffb_enabled,
                    "FFB actuators {}",
                    if ffb_enabled { "enabled" } else { "disabled" }
                );
                ffb_enabled_prev = ffb_enabled;
            }
        }

        match ipc.get_ffb() {
            Ok(Some(mut report)) => {
                // Drop packets entirely when FFB is disabled.
                if !ffb_enabled {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                    continue;
                }

                // Scale Device Gain reports by the configured gain.
                // Device Gain report: [0x0D, gain_byte]
                if report.first().copied() == Some(REPORT_ID_DEVICE_GAIN) && report.len() < 2 {
                    tracing::warn!(
                        len = report.len(),
                        "Device Gain report too short to scale — forwarding raw"
                    );
                } else if report.first().copied() == Some(REPORT_ID_DEVICE_GAIN) {
                    let packet_gain = report[1];
                    let scaled = scale_gain(packet_gain, ffb_gain);
                    report[1] = scaled;
                    debug!(
                        original = packet_gain,
                        scaled,
                        "Device Gain scaled by config"
                    );
                }

                debug!(
                    "FFB report id={:#04x} len={}",
                    report.first().copied().unwrap_or(0),
                    report.len()
                );
                if let Err(e) = transport.write_output_report(&report) {
                    error!(
                        internal_error = %e,
                        "Lost connection to the joystick. Waiting for reconnect..."
                    );
                }
            }
            Ok(None) => {
                // No report queued — back off briefly.
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
            }
            Err(e) => {
                error!(
                    internal_error = %e,
                    "Couldn't read force-feedback data from the driver. Try reinstalling it using the install script."
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
    }
}

// ── Config application ────────────────────────────────────────────────────────

/// Map a raw [`InputState`] through the current config to produce an
/// [`InputSnapshot`] ready to push to the driver.
///
/// Processing order:
/// 1. Axes — calibration, dead-zone, curve, scale, invert
/// 2. Buttons — layer selection, then button remapping
/// 3. Hat — either POV value or virtual buttons (if `hat.as_buttons`)
#[cfg(any(target_os = "windows", test))]
#[must_use]
pub fn apply_config(config: &Config, state: &InputState) -> InputSnapshot {
    use crate::config::BUTTON_COUNT;
    use sidewinder_hid::input::PovDirection;

    // ── Axes ──────────────────────────────────────────────────────────────────
    let cal = &config.calibration;
    let axes = [
        Config::apply_axis(&config.axis_x, cal.x_min, cal.x_max, state.axes[0]),
        Config::apply_axis(&config.axis_y, cal.y_min, cal.y_max, state.axes[1]),
        Config::apply_axis(&config.axis_rz, cal.rz_min, cal.rz_max, state.axes[2]),
        Config::apply_axis(
            &config.axis_slider,
            cal.slider_min,
            cal.slider_max,
            state.axes[3],
        ),
    ];

    // ── Buttons + layer ───────────────────────────────────────────────────────
    // Physical buttons arrive as a u16 bitmask from the HID report (bit 0 = button 1).
    // Virtual buttons are accumulated into a u32 bitmask for the IPC snapshot.
    let shift_idx = config.layer.shift_button.saturating_sub(1) as usize;
    let shift_held =
        config.layer.shift_button > 0 && shift_idx < 16 && (state.buttons >> shift_idx) & 1 == 1;

    // Select the active button map (layer 2 when shift held, else layer 1).
    let btn_map = if shift_held {
        &config.layer.buttons
    } else {
        &config.buttons
    };

    let mut buttons: u32 = 0;
    for phys in 0..BUTTON_COUNT {
        if (state.buttons >> phys) & 1 == 0 {
            continue; // physical button not pressed
        }
        // Suppress the shift button itself — it's consumed, not forwarded.
        if shift_held && phys == shift_idx {
            continue;
        }
        // BUTTON_COUNT = 9, which fits in u8 (max 255), so this cast is safe.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "BUTTON_COUNT = 9, always fits in u8"
        )]
        let virt = btn_map.virtual_for(phys as u8) as usize;
        if virt < 32 {
            buttons |= 1 << virt;
        }
    }

    // ── Hat switch ────────────────────────────────────────────────────────────
    let pov = if config.hat.as_buttons {
        // Fire a virtual button for the hat direction; report POV as centred.
        if let Some(btn_idx) = config.hat.button_for(state.pov) {
            let virt = btn_idx as usize;
            if virt < 32 {
                buttons |= 1 << virt;
            }
        }
        0xFF // hat centred in POV report
    } else {
        match state.pov {
            PovDirection::North => 0,
            PovDirection::NorthEast => 1,
            PovDirection::East => 2,
            PovDirection::SouthEast => 3,
            PovDirection::South => 4,
            PovDirection::SouthWest => 5,
            PovDirection::West => 6,
            PovDirection::NorthWest => 7,
            PovDirection::Center => 0xFF,
        }
    };

    InputSnapshot { axes, buttons, pov }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{AxisConfig, ButtonMapConfig, HatConfig, LayerConfig},
        status::ConnectionStatus,
    };
    use sidewinder_hid::input::PovDirection;

    // ── InputState channel tests ───────────────────────────────────────────────

    /// `InputState` watch channels carry state correctly.
    ///
    /// Guards the contract relied upon by the GUI pipe server: a sender pushed
    /// from `input_loop` produces a value readable by the pipe server's receiver.
    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test assertions — panics are the failure mode"
    )]
    fn input_state_channel_carries_latest_value() {
        use sidewinder_hid::input::PovDirection;
        let (tx, mut rx) = tokio::sync::watch::channel(InputState::default());
        let state = InputState {
            axes: [100, -200, 500, -500, 0, 0, 0, 0],
            buttons: 0b111,
            pov: PovDirection::North,
        };
        tx.send(state).expect("send must succeed");
        assert_eq!(*rx.borrow_and_update(), state);
    }

    /// `gui_frame_from_input` encodes connection flags and all axis/button/pov
    /// fields correctly into a [`sidewinder_ipc::GuiFrame`].
    #[test]
    fn gui_frame_from_input_encodes_fields() {
        use sidewinder_hid::input::PovDirection;
        let state = InputState {
            axes: [1000, -2000, 500, -500, 0, 0, 0, 0],
            buttons: 0b10101,
            pov: PovDirection::SouthWest,
        };
        let frame = gui_frame_from_input(&state, true, true, 200);
        assert_eq!(frame.axes, state.axes);
        assert_eq!(frame.buttons, state.buttons);
        assert_eq!(frame.pov, 5, "SouthWest = pov index 5");
        assert_eq!(frame.connected, 1);
        assert_eq!(frame.ffb_enabled, 1);
        assert_eq!(frame.ffb_gain, 200);
    }

    #[test]
    fn gui_frame_from_input_disconnected() {
        let frame = gui_frame_from_input(&InputState::default(), false, false, 0);
        assert_eq!(frame.connected, 0);
        assert_eq!(frame.ffb_enabled, 0);
        assert_eq!(frame.pov, 0xFF, "Center = 0xFF");
    }

    // ── Connection status tests ────────────────────────────────────────────────

    /// `ConnectionStatus` watch channels carry state correctly.
    ///
    /// The tray and the input loop both rely on this contract: the receiver
    /// sees the initial `Connected` value and observes each subsequent change.
    /// This test guards that contract independently of the Windows-only
    /// `Bridge::new` and `input_loop` paths.
    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test assertions — panics are the failure mode"
    )]
    fn status_channel_initial_value_and_update() {
        let (tx, mut rx) = tokio::sync::watch::channel(ConnectionStatus::Connected);
        assert_eq!(*rx.borrow(), ConnectionStatus::Connected);

        tx.send(ConnectionStatus::Disconnected)
            .expect("channel send should succeed while receiver is alive");
        assert!(
            rx.has_changed().expect("channel should not be closed"),
            "receiver must see the change"
        );
        assert_eq!(*rx.borrow_and_update(), ConnectionStatus::Disconnected);

        tx.send(ConnectionStatus::Connected)
            .expect("channel send should succeed while receiver is alive");
        assert!(
            rx.has_changed().expect("channel should not be closed"),
            "receiver must see the change"
        );
        assert_eq!(*rx.borrow_and_update(), ConnectionStatus::Connected);
    }

    /// When the sender is dropped (e.g. `input_loop` panics), the channel is
    /// closed: `has_changed()` returns `Err` and `borrow()` still returns the
    /// last sent value.  The tray's watcher thread calls
    /// `status_rx.changed().blocking_recv()` and must break out cleanly rather
    /// than blocking forever.
    #[test]
    fn status_channel_sender_drop_closes_receiver() {
        let (tx, rx) = tokio::sync::watch::channel(ConnectionStatus::Connected);
        drop(tx);
        // Channel is closed; last value still readable.
        assert_eq!(*rx.borrow(), ConnectionStatus::Connected);
        // has_changed() returns Err when the sender is gone.
        assert!(rx.has_changed().is_err(), "closed channel must return Err");
    }

    /// Multiple sends before any read collapse to the last value (tokio watch
    /// last-write-wins semantics).  The reconnect loop can fire `Disconnected`
    /// then immediately `Connected` before the tray wakes; the tray must only
    /// see `Connected` when it finally reads.
    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test assertions — panics are the failure mode"
    )]
    fn status_channel_last_write_wins() {
        let (tx, mut rx) = tokio::sync::watch::channel(ConnectionStatus::Connected);
        // borrow_and_update to clear the initial "changed" flag
        let _ = rx.borrow_and_update();

        tx.send(ConnectionStatus::Disconnected)
            .expect("send should succeed");
        tx.send(ConnectionStatus::Connected)
            .expect("send should succeed");

        // Only one change is visible; the intermediate Disconnected is gone.
        assert!(
            rx.has_changed().expect("channel should be open"),
            "at least one unread change must exist"
        );
        assert_eq!(
            *rx.borrow_and_update(),
            ConnectionStatus::Connected,
            "last write wins"
        );
        assert!(
            !rx.has_changed().expect("channel should be open"),
            "no further changes after borrow_and_update"
        );
    }

    /// Sending the same value still marks `has_changed()` as true.  Tokio watch
    /// does NOT deduplicate.  The tray watcher thread must handle this correctly
    /// (it re-reads the value and applies the tooltip update idempotently).
    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test assertions — panics are the failure mode"
    )]
    fn status_channel_same_value_still_marks_changed() {
        let (tx, mut rx) = tokio::sync::watch::channel(ConnectionStatus::Connected);
        let _ = rx.borrow_and_update(); // clear initial flag

        tx.send(ConnectionStatus::Connected)
            .expect("send should succeed");
        assert!(
            rx.has_changed().expect("channel should be open"),
            "same-value send must still mark changed"
        );
    }

    /// Cloned receivers are independent: each sees the same value after a send,
    /// but `borrow_and_update` on one does not affect the other's changed flag.
    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test assertions — panics are the failure mode"
    )]
    fn status_channel_cloned_receivers_see_same_value() {
        let (tx, mut rx1) = tokio::sync::watch::channel(ConnectionStatus::Connected);
        let mut rx2 = rx1.clone();
        let _ = rx1.borrow_and_update();
        let _ = rx2.borrow_and_update();

        tx.send(ConnectionStatus::Disconnected)
            .expect("send should succeed");

        assert_eq!(*rx1.borrow_and_update(), ConnectionStatus::Disconnected);
        assert_eq!(*rx2.borrow_and_update(), ConnectionStatus::Disconnected);
    }

    // ── apply_config tests ─────────────────────────────────────────────────────

    fn make_state(x: i16, y: i16, buttons: u16, pov: PovDirection) -> InputState {
        let mut axes = [0i16; 8];
        axes[0] = x;
        axes[1] = y;
        InputState { axes, buttons, pov }
    }

    #[test]
    fn apply_config_passthrough() {
        let cfg = Config::default();
        let state = make_state(1000, -2000, 0b11, PovDirection::East);
        let snap = apply_config(&cfg, &state);
        assert_eq!(snap.axes[0], 1000);
        assert_eq!(snap.axes[1], -2000);
        assert_eq!(snap.buttons, 0b11);
        assert_eq!(snap.pov, 2); // East
    }

    #[test]
    fn apply_config_invert_x() {
        let cfg = Config {
            axis_x: AxisConfig {
                invert: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let state = make_state(5000, 0, 0, PovDirection::Center);
        let snap = apply_config(&cfg, &state);
        assert_eq!(snap.axes[0], -5000);
    }

    /// Axis slot 2 feeds `axis_rz` (Rz/twist), not `axis_z`.
    /// Axis slot 3 feeds `axis_slider` (throttle), not `axis_rz`.
    /// This is a regression guard for Issue #19 (wrong axis slot assignments).
    #[test]
    fn apply_config_axis_slot_assignments() {
        let mut axes = [0i16; 8];
        axes[2] = 10000; // Rz/twist
        axes[3] = -5000; // Slider/throttle
        let state = InputState {
            axes,
            buttons: 0,
            pov: PovDirection::Center,
        };
        let cfg = Config::default();
        let snap = apply_config(&cfg, &state);
        assert_eq!(snap.axes[2], 10000, "Rz/twist at slot 2");
        assert_eq!(snap.axes[3], -5000, "Slider/throttle at slot 3");
    }

    /// Invert only the Rz axis — slider must be unaffected.
    #[test]
    fn apply_config_invert_rz_not_slider() {
        let cfg = Config {
            axis_rz: AxisConfig {
                invert: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut axes = [0i16; 8];
        axes[2] = 8000; // Rz
        axes[3] = 4000; // Slider
        let state = InputState {
            axes,
            buttons: 0,
            pov: PovDirection::Center,
        };
        let snap = apply_config(&cfg, &state);
        assert_eq!(snap.axes[2], -8000, "Rz inverted");
        assert_eq!(snap.axes[3], 4000, "Slider unchanged");
    }

    #[test]
    fn apply_config_pov_center() {
        let cfg = Config::default();
        let state = make_state(0, 0, 0, PovDirection::Center);
        let snap = apply_config(&cfg, &state);
        assert_eq!(snap.pov, 0xFF);
    }

    // ── Button remap tests ─────────────────────────────────────────────────────

    #[test]
    fn apply_config_button_remap_swap() {
        // Swap buttons 0 and 1 (0-based).
        let cfg = Config {
            buttons: ButtonMapConfig::from_pairs(&[(0, 1), (1, 0)]),
            ..Default::default()
        };
        // Physical button 0 pressed.
        let state = make_state(0, 0, 0b01, PovDirection::Center);
        let snap = apply_config(&cfg, &state);
        // Should appear as virtual button 1 (bit 1).
        assert_eq!(snap.buttons, 0b10, "button 0 maps to virtual 1");
    }

    #[test]
    fn apply_config_button_remap_identity() {
        // No remapping — default config preserves button bits.
        let state = make_state(0, 0, 0b1010_0101, PovDirection::Center);
        let snap = apply_config(&Config::default(), &state);
        assert_eq!(snap.buttons, 0b1010_0101);
    }

    // ── Hat-as-buttons tests ───────────────────────────────────────────────────

    #[test]
    fn apply_config_hat_as_buttons_north() {
        let cfg = Config {
            hat: HatConfig {
                as_buttons: true,
                north: 9, // virtual button index 9 (0-based)
                ..Default::default()
            },
            ..Default::default()
        };
        let state = make_state(0, 0, 0, PovDirection::North);
        let snap = apply_config(&cfg, &state);
        // POV should be centred when as_buttons is active.
        assert_eq!(snap.pov, 0xFF, "pov must be centred when as_buttons=true");
        // Virtual button 9 (bit 9) must be set.
        assert_eq!(snap.buttons & (1 << 9), 1 << 9, "north fires button 9");
    }

    #[test]
    fn apply_config_hat_as_buttons_center_fires_nothing() {
        let cfg = Config {
            hat: HatConfig {
                as_buttons: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let state = make_state(0, 0, 0, PovDirection::Center);
        let snap = apply_config(&cfg, &state);
        assert_eq!(snap.buttons, 0, "center hat fires no buttons");
        assert_eq!(snap.pov, 0xFF);
    }

    #[test]
    fn apply_config_hat_normal_mode_reports_pov() {
        let cfg = Config::default(); // as_buttons = false
        let state = make_state(0, 0, 0, PovDirection::SouthWest);
        let snap = apply_config(&cfg, &state);
        assert_eq!(snap.pov, 5, "SouthWest = pov index 5");
        assert_eq!(snap.buttons, 0, "no buttons from hat in pov mode");
    }

    // ── Layer tests ────────────────────────────────────────────────────────────

    #[test]
    fn apply_config_layer_shift_not_held_uses_layer1() {
        // Shift = button 6 (0-based: 5). Physical button 0 pressed, no shift.
        let cfg = Config {
            layer: LayerConfig {
                shift_button: 6,
                buttons: ButtonMapConfig::from_pairs(&[(0, 9)]), // button 0 → virtual 9
            },
            ..Default::default()
        };
        let state = make_state(0, 0, 0b0000_0001, PovDirection::Center); // button 0 pressed
        let snap = apply_config(&cfg, &state);
        // Without shift, layer-1 (identity) applies: button 0 → virtual 0.
        assert_eq!(snap.buttons, 0b0000_0001);
    }

    #[test]
    fn apply_config_layer_shift_held_uses_layer2() {
        // Shift = button 6 (0-based: 5). Button 0 pressed with shift.
        let cfg = Config {
            layer: LayerConfig {
                shift_button: 6,
                buttons: ButtonMapConfig::from_pairs(&[(0, 9)]), // button 0 → virtual 9
            },
            ..Default::default()
        };
        // Bit 0 (button 0) + bit 5 (shift button 6) pressed.
        let state = make_state(0, 0, 0b0010_0001, PovDirection::Center);
        let snap = apply_config(&cfg, &state);
        // Shift button (bit 5) must not appear in output.
        assert_eq!(snap.buttons & (1 << 5), 0, "shift button not forwarded");
        // Button 0 maps to virtual 9 in layer 2.
        assert_eq!(snap.buttons & (1 << 9), 1 << 9, "button 0 → virtual 9");
    }

    #[test]
    fn apply_config_layer_disabled_is_zero_overhead() {
        // shift_button = 0 → no layer logic, identity passthrough.
        let state = make_state(0, 0, 0b1111, PovDirection::Center);
        let snap = apply_config(&Config::default(), &state);
        assert_eq!(snap.buttons, 0b1111);
    }

    // ── Gain scaling tests ─────────────────────────────────────────────────────

    /// Hard silencing: zero config gain always produces zero output.
    #[test]
    fn scale_gain_config_zero_silences_all() {
        assert_eq!(scale_gain(0, 0), 0);
        assert_eq!(scale_gain(128, 0), 0);
        assert_eq!(scale_gain(255, 0), 0);
    }

    /// Pass-through: full config gain leaves the driver value unchanged.
    #[test]
    fn scale_gain_config_full_is_identity() {
        assert_eq!(scale_gain(0, 255), 0);
        assert_eq!(scale_gain(128, 255), 128);
        assert_eq!(scale_gain(255, 255), 255);
    }

    /// Boundary: near-zero driver gain must not be rounded up to a non-zero value.
    #[test]
    fn scale_gain_packet_zero_silences_all() {
        assert_eq!(scale_gain(0, 255), 0);
        assert_eq!(scale_gain(0, 128), 0);
    }

    /// Symmetry: scaling 128 by 255 and 255 by 128 both give ~128.
    #[test]
    fn scale_gain_half_gain_halves_output() {
        assert_eq!(scale_gain(255, 128), 128);
        assert_eq!(scale_gain(128, 255), 128);
    }

    /// Result must always be in [0, 255] — no overflow.
    #[test]
    fn scale_gain_never_exceeds_255() {
        assert_eq!(scale_gain(255, 255), 255);
    }
}
