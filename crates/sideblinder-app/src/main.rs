//! Sidewinder Force Feedback 2 background service.
//!
//! Reads joystick input via HID, applies axis/button mapping from TOML config,
//! pushes state to the virtual UMDF2 device, captures FFB output reports from
//! the driver, and forwards them back to the physical joystick.
//!
//! # Config subcommands
//!
//! `sideblinder-app config --validate [--config <path>]` — validate a config file.
//! `sideblinder-app config --generate [--config <path>]` — write a documented default config.

// bridge, ipc, and gui_pipe are only called from the Windows-specific startup
// block.  Compile them on all platforms so tests remain runnable everywhere.
#[cfg(any(target_os = "windows", test))]
mod bridge;
mod config;
#[cfg(target_os = "windows")]
mod gui_pipe;
#[cfg(any(target_os = "windows", test))]
mod ipc;
mod status;
mod tray;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use tracing::info;
use tracing_subscriber::EnvFilter;

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "sideblinder-app",
    about = "Sidewinder Force Feedback 2 bridge service"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Config file utilities.
    Config {
        /// Validate the config file and print a human-readable report.
        #[arg(long, conflicts_with = "generate")]
        validate: bool,
        /// Write a documented default config file (no-op if the file already exists).
        #[arg(long)]
        generate: bool,
        /// Path to the config file (defaults to the platform standard location).
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

// ── Config subcommand ─────────────────────────────────────────────────────────

/// Handle the `config` subcommand and return the appropriate exit code.
///
/// Returns `Some(ExitCode)` when a config subcommand was handled (caller should
/// return immediately), or `None` when no config subcommand was present.
#[expect(
    clippy::print_stderr,
    reason = "CLI binary — eprintln! is correct for user-facing error output"
)]
fn handle_config_command(cli: &Cli) -> Option<ExitCode> {
    let Command::Config {
        validate,
        generate,
        config,
    } = cli.command.as_ref()?;

    let path = config.clone().unwrap_or_else(config::default_config_path);

    if *validate {
        Some(match config::run_validate(&path) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(e) => {
                eprintln!("Error: {e}");
                ExitCode::from(2)
            }
        })
    } else if *generate {
        Some(match config::run_generate(&path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Error: {e}");
                ExitCode::from(2)
            }
        })
    } else {
        eprintln!("Specify --validate or --generate.");
        Some(ExitCode::from(2))
    }
}

// ── Windows helpers ───────────────────────────────────────────────────────────

/// Start a background task that polls for the physical Sidewinder device every
/// 2 seconds and broadcasts connection-state changes.
///
/// Called when the device is found at startup but the driver IPC is missing
/// (input-only mode).  The 2-second interval is long enough to avoid hammering
/// the USB stack while still reflecting reconnects in the tray within a few
/// seconds.
#[cfg(target_os = "windows")]
fn spawn_input_only_status_poller(
) -> tokio::sync::watch::Receiver<status::ConnectionStatus> {
    use sideblinder_hid::device::SideblinderDevice;
    use tokio::sync::watch;

    info!("physical device found; starting in input-only mode (FFB unavailable)");
    let (status_tx, status_rx) = watch::channel(status::ConnectionStatus::Connected);
    tokio::spawn(async move {
        let mut last = status::ConnectionStatus::Connected;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let next = match tokio::task::spawn_blocking(SideblinderDevice::open).await {
                Ok(Ok(_)) => status::ConnectionStatus::Connected,
                Ok(Err(e)) => {
                    tracing::warn!(
                        internal_error = %e,
                        "Joystick not found. Check that it's plugged in and recognised by Windows."
                    );
                    status::ConnectionStatus::Disconnected
                }
                Err(e) => {
                    tracing::warn!(
                        internal_error = %e,
                        "Spawn blocking panicked during device poll"
                    );
                    continue;
                }
            };
            if next != last {
                if status_tx.send(next).is_err() {
                    break;
                }
                last = next;
            }
        }
    });
    status_rx
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    // Config subcommands run and exit before the tracing subscriber or app loop.
    if let Some(code) = handle_config_command(&cli) {
        return code;
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!("sideblinder-app starting");

    // Load config (falls back to defaults if the file doesn't exist).
    let config_path = config::default_config_path();
    let config_rx = config::watch_config(&config_path);

    info!("config loaded from {:?}", config_path);

    // Open the physical Sidewinder device and driver IPC, then spawn the bridge.
    // On Windows: use the real implementations.
    // On other platforms: log and idle (there's no physical device or driver).
    //
    // The bridge returns a status receiver that reflects physical-device
    // connection state; we forward it to the tray so the icon can change.
    #[cfg(target_os = "windows")]
    let (status_rx, startup) = {
        use ipc::WindowsDriverIpc;
        use sideblinder_hid::{device::SideblinderDevice, input::InputState};
        use std::sync::Arc;
        use tokio::sync::watch;

        // Attempt to open the driver IPC. Failure is non-fatal: we start in
        // input-only mode so users can still verify physical device parsing
        // without the full driver installed.
        let driver_result = WindowsDriverIpc::open();
        let driver = match &driver_result {
            Ok(_) => status::DriverStatus::Present,
            Err(e) => {
                tracing::warn!(
                    internal_error = %e,
                    "Driver not installed. Run the install script as administrator."
                );
                status::DriverStatus::Missing
            }
        };

        let (status_rx, input_state_rx) = match (SideblinderDevice::open(), driver_result) {
            (Ok(device), Ok(driver_ipc)) => {
                info!("physical device and driver IPC opened — starting full bridge");
                let transport: Arc<dyn sideblinder_hid::hid_transport::HidTransport> =
                    Arc::from(device.into_transport());
                let ipc = Arc::new(driver_ipc);
                let (bridge, status_rx, input_state_rx) =
                    bridge::Bridge::new(transport, ipc, config_rx.clone());
                bridge.spawn();
                (status_rx, input_state_rx)
            }
            (Ok(_), Err(_)) => {
                // Driver missing; driver already set to Missing above.
                // Still poll the physical device so disconnect/reconnect events
                // are reflected in the tray — just skip the FFB loop.
                let status_rx = spawn_input_only_status_poller();
                let input_state_rx = watch::channel(InputState::default()).1;
                (status_rx, input_state_rx)
            }
            (Err(e), _) => {
                tracing::warn!(
                    internal_error = %e,
                    "Joystick not found. Check that it's plugged in and recognised by Windows."
                );
                let input_state_rx = watch::channel(InputState::default()).1;
                (watch::channel(status::ConnectionStatus::Disconnected).1, input_state_rx)
            }
        };

        // Stream GuiFrame snapshots to any connected sideblinder-gui instance.
        gui_pipe::spawn_gui_pipe_server(
            input_state_rx,
            config_rx.clone(),
            status_rx.clone(),
        );

        // Check 4: calibration set — compare against the uncalibrated defaults.
        let calibration_set =
            config_rx.borrow().calibration != config::CalibrationConfig::default();
        if !calibration_set {
            tracing::info!(
                "Calibration not set — run Calibrate for best axis accuracy."
            );
        }

        let startup = status::StartupStatus { driver, calibration_set };
        (status_rx, startup)
    };

    #[cfg(not(target_os = "windows"))]
    let (status_rx, startup) = {
        use tokio::sync::watch;
        info!("running on non-Windows platform — bridge not started");
        let startup = status::StartupStatus {
            driver: status::DriverStatus::Present,
            calibration_set: true,
        };
        (
            watch::channel(status::ConnectionStatus::Connected).1,
            startup,
        )
    };

    // System tray (Windows) or headless (other platforms).
    let mut quit_rx = tray::spawn_tray(config_rx.clone(), status_rx, startup);

    // Wait until the user requests quit via the tray menu, or the tray thread exits.
    if quit_rx.changed().await.is_ok() {
        info!("quit requested — shutting down");
    } else {
        tracing::warn!("tray thread exited unexpectedly — shutting down");
    }
    ExitCode::SUCCESS
}
