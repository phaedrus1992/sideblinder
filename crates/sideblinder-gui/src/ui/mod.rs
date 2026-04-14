//! Screen renderers for the Sideblinder settings GUI.
//!
//! Each sub-module owns one screen.  All screens receive a `&mut ScreenContext`
//! which bundles the shared state they need: the config path, the mutable
//! in-memory config, and the latest live frame.

pub mod axes;
pub mod buttons;
pub mod dashboard;

use sideblinder_ipc::GuiFrame;
use sideblinder_app::config::Config;
use std::path::Path;

/// Shared context passed to every screen renderer.
pub struct ScreenContext<'a> {
    /// Path to the TOML config file used for write-back.
    pub config_path: &'a Path,
    /// In-memory config — updated in-place when the user changes a value.
    pub config: &'a mut Config,
    /// Latest live-state frame received from the backend, if any.
    pub frame: Option<GuiFrame>,
}
