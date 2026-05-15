//! Named-pipe server that broadcasts `GuiFrame` snapshots to `sideblinder-gui`.
//!
//! Spawns a background tokio task that creates `\\.\pipe\SideblinderGui`, accepts
//! one client at a time, and streams a 27-byte framed [`sideblinder_ipc::GuiFrame`]
//! at ~30 Hz.  When the client disconnects the task loops back and waits for the
//! next connection.
//!
//! Data flow is **server → client only**.  Config changes made in the GUI are
//! written directly to the TOML config file via `toml_edit`; the app's existing
//! `notify` watcher picks them up without any IPC round-trip.

use crate::{config::Config, status::ConnectionStatus};
use sideblinder_hid::input::InputState;

// ── Frame builder ─────────────────────────────────────────────────────────────

/// Build a [`sideblinder_ipc::GuiFrame`] from the current watch-channel state.
///
/// Available in test builds on all platforms (bridge module is compiled for
/// tests) so that the tests below can run in CI without a physical device.
#[cfg(any(target_os = "windows", test))]
fn build_gui_frame(
    input_rx: &tokio::sync::watch::Receiver<InputState>,
    config_rx: &tokio::sync::watch::Receiver<Config>,
    status_rx: &tokio::sync::watch::Receiver<ConnectionStatus>,
) -> sideblinder_ipc::GuiFrame {
    let state = *input_rx.borrow();
    let (ffb_enabled, ffb_gain) = {
        let cfg = config_rx.borrow();
        (cfg.ffb_enabled, cfg.ffb_gain)
    };
    let connected = matches!(*status_rx.borrow(), ConnectionStatus::Connected);
    crate::bridge::gui_frame_from_input(&state, connected, ffb_enabled, ffb_gain)
}

// ── Pipe server ───────────────────────────────────────────────────────────────

/// ~30 Hz frame interval for the GUI pipe server.
///
/// 33 ms gives a comfortable headroom below the 60 Hz monitor refresh that
/// most GUI toolkits target, so the GUI is always reading fresh data.
#[cfg(target_os = "windows")]
const FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_millis(33);

/// Retry delay when `CreateNamedPipeW` fails (e.g. during process startup).
#[cfg(target_os = "windows")]
const PIPE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

/// Spawn a background tokio task that streams [`sideblinder_ipc::GuiFrame`]s
/// to a connected GUI client at ~30 Hz.
///
/// The task creates `\\.\pipe\SideblinderGui` and waits for one client at a
/// time.  When the client disconnects the task creates a new pipe instance and
/// waits again.  Errors from pipe creation are logged and retried after a short
/// delay so a transient failure does not permanently disable the GUI feed.
#[cfg(target_os = "windows")]
pub fn spawn_gui_pipe_server(
    input_rx: tokio::sync::watch::Receiver<InputState>,
    config_rx: tokio::sync::watch::Receiver<Config>,
    status_rx: tokio::sync::watch::Receiver<ConnectionStatus>,
) {
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt as _;
        use tokio::net::windows::named_pipe::ServerOptions;

        loop {
            let mut server = match ServerOptions::new()
                .access_outbound(true)
                .access_inbound(false)
                .create(sideblinder_ipc::PIPE_NAME)
            {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(
                        internal_error = %e,
                        "GUI pipe: failed to create named pipe instance — will retry"
                    );
                    tokio::time::sleep(PIPE_RETRY_DELAY).await;
                    continue;
                }
            };

            tracing::debug!("GUI pipe: waiting for GUI client");
            if let Err(e) = server.connect().await {
                tracing::warn!(internal_error = %e, "GUI pipe: connect wait failed");
                continue;
            }
            tracing::debug!("GUI pipe: client connected");

            // Stream frames until the client disconnects or a write fails.
            let mut interval = tokio::time::interval(FRAME_INTERVAL);
            loop {
                interval.tick().await;
                let frame = build_gui_frame(&input_rx, &config_rx, &status_rx);
                if let Err(e) = server.write_all(&frame.encode()).await {
                    tracing::debug!(internal_error = %e, "GUI pipe: client disconnected");
                    break;
                }
            }
            // `server` is dropped here; the pipe instance is cleaned up by the OS.
        }
    });
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::build_gui_frame;
    use crate::config::Config;
    use crate::status::ConnectionStatus;
    use sideblinder_hid::input::InputState;

    fn make_receivers(
        state: InputState,
        config: Config,
        status: ConnectionStatus,
    ) -> (
        tokio::sync::watch::Receiver<InputState>,
        tokio::sync::watch::Receiver<Config>,
        tokio::sync::watch::Receiver<ConnectionStatus>,
    ) {
        (
            tokio::sync::watch::channel(state).1,
            tokio::sync::watch::channel(config).1,
            tokio::sync::watch::channel(status).1,
        )
    }

    #[test]
    fn connected_status_sets_frame_connected() {
        let (input_rx, config_rx, status_rx) =
            make_receivers(InputState::default(), Config::default(), ConnectionStatus::Connected);
        let frame = build_gui_frame(&input_rx, &config_rx, &status_rx);
        assert_eq!(frame.connected, 1, "Connected status should produce connected=1");
    }

    #[test]
    fn disconnected_status_sets_frame_connected_zero() {
        let (input_rx, config_rx, status_rx) = make_receivers(
            InputState::default(),
            Config::default(),
            ConnectionStatus::Disconnected,
        );
        let frame = build_gui_frame(&input_rx, &config_rx, &status_rx);
        assert_eq!(frame.connected, 0, "Disconnected status should produce connected=0");
    }

    #[test]
    fn ffb_gain_from_config_is_reflected_in_frame() {
        let config = Config { ffb_gain: 128, ..Config::default() };
        let (input_rx, config_rx, status_rx) =
            make_receivers(InputState::default(), config, ConnectionStatus::Connected);
        let frame = build_gui_frame(&input_rx, &config_rx, &status_rx);
        assert_eq!(frame.ffb_gain, 128);
    }

    #[test]
    fn ffb_disabled_config_sets_frame_ffb_enabled_zero() {
        let config = Config { ffb_enabled: false, ..Config::default() };
        let (input_rx, config_rx, status_rx) =
            make_receivers(InputState::default(), config, ConnectionStatus::Connected);
        let frame = build_gui_frame(&input_rx, &config_rx, &status_rx);
        assert_eq!(frame.ffb_enabled, 0);
    }

    #[test]
    fn axis_values_from_input_state_are_passed_through() {
        let mut state = InputState::default();
        state.axes[0] = 1000;
        state.axes[1] = -2000;
        let (input_rx, config_rx, status_rx) =
            make_receivers(state, Config::default(), ConnectionStatus::Connected);
        let frame = build_gui_frame(&input_rx, &config_rx, &status_rx);
        assert_eq!(frame.axes[0], 1000);
        assert_eq!(frame.axes[1], -2000);
    }
}
