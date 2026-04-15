//! Sidewinder Force Feedback 2 settings GUI.
//!
//! On startup the app tries to connect to a running `sideblinder-app` instance
//! via the named pipe [`sideblinder_ipc::PIPE_NAME`].  If the pipe is not
//! available it falls back to an embedded bridge that opens the physical device
//! directly.
//!
//! All three screens share the same [`GuiState`] value, which is updated by a
//! background thread and read by the egui render loop each frame.

mod app;
mod backend;
mod config_writer;
mod embedded_backend;
mod pipe_backend;
mod ui;

use std::process::ExitCode;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!("sideblinder-gui starting");

    let config_path = sideblinder_app::config::default_config_path();

    // Attempt to attach to a running sideblinder-app via the named pipe.
    // Fall back to an embedded bridge if the pipe is not available.
    let backend: Box<dyn backend::GuiBackend> =
        match pipe_backend::PipeBackend::connect() {
            Ok(b) => {
                tracing::info!("attached to running sideblinder-app via IPC pipe");
                Box::new(b)
            }
            Err(e) => {
                tracing::info!(reason = %e, "pipe unavailable — starting embedded bridge");
                match embedded_backend::EmbeddedBackend::start(config_path.clone()) {
                    Ok(b) => Box::new(b),
                    Err(start_err) => {
                        tracing::error!(
                            internal_error = %start_err,
                            "Could not open joystick — running in disconnected state. Check that it is plugged in."
                        );
                        Box::new(embedded_backend::EmbeddedBackend::disconnected())
                    }
                }
            }
        };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sideblinder Settings")
            .with_min_inner_size([600.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Sideblinder Settings",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::SideblinderApp::new(cc, backend, config_path)))
        }),
    )
    .map_or_else(
        |e| {
            tracing::error!(internal_error = %e, "GUI failed to start");
            ExitCode::FAILURE
        },
        |()| ExitCode::SUCCESS,
    )
}
