//! Root eframe application: layout, navigation, and per-frame update loop.
//!
//! `SidewinderApp` renders a left-side navigation panel with three tabs
//! (Dashboard, Axes, Buttons) and delegates rendering to the matching screen
//! module.  Live data is polled from the `GuiBackend` once per frame and cached
//! in `latest_frame` so each screen can read it without mutability conflicts.
//!
//! Implements `eframe::App` via the eframe 0.34 `fn ui` entry point.

use crate::{
    backend::GuiBackend,
    ui::{self, ScreenContext},
};
use egui::{Color32, RichText};
use sideblinder_app::config::{Config, ConfigError};
use sideblinder_ipc::GuiFrame;
use std::path::PathBuf;

// ── Screen ────────────────────────────────────────────────────────────────────

/// The active left-nav screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Screen {
    #[default]
    Dashboard,
    Axes,
    Buttons,
}

// ── App ───────────────────────────────────────────────────────────────────────

/// Root application state owned by the eframe runtime.
pub struct SidewinderApp {
    backend: Box<dyn GuiBackend>,
    config_path: PathBuf,
    config: Config,
    latest_frame: Option<GuiFrame>,
    screen: Screen,
}

impl SidewinderApp {
    /// Construct the app with the given backend and config path.
    ///
    /// The config file is loaded eagerly; missing files fall back to defaults.
    #[must_use]
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        backend: Box<dyn GuiBackend>,
        config_path: PathBuf,
    ) -> Self {
        let config = match Config::load(&config_path) {
            Ok(c) => c,
            Err(ConfigError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                Config::default()
            }
            Err(e) => {
                tracing::warn!(
                    internal_error = %e,
                    path = %config_path.display(),
                    "config file could not be loaded — using defaults"
                );
                Config::default()
            }
        };
        Self {
            backend,
            config_path,
            config,
            latest_frame: None,
            screen: Screen::default(),
        }
    }
}

impl eframe::App for SidewinderApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Check liveness first so a valid frame received on the same tick
        // as a disconnect is not discarded before rendering.
        if self.backend.is_alive() {
            if let Some(frame) = self.backend.poll() {
                self.latest_frame = Some(frame);
            }
        } else {
            self.latest_frame = None;
        }

        // ── Left navigation panel ────────────────────────────────────────
        egui::Panel::left("nav_panel")
            .exact_size(130.0)
            .show_inside(ui, |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("Sideblinder").strong().size(14.0));
                ui.add_space(12.0);

                nav_button(ui, &mut self.screen, Screen::Dashboard, "Dashboard");
                nav_button(ui, &mut self.screen, Screen::Axes, "Axes");
                nav_button(ui, &mut self.screen, Screen::Buttons, "Buttons");

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    if !self.backend.is_alive() {
                        ui.label(
                            RichText::new("● offline")
                                .color(Color32::from_rgb(200, 80, 80)),
                        );
                    }
                });
            });

        // ── Bottom status bar ────────────────────────────────────────────
        egui::Panel::bottom("status_bar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let status = match self.latest_frame {
                    Some(f) if f.connected == 1 => {
                        RichText::new("● Connected").color(Color32::from_rgb(80, 200, 80))
                    }
                    Some(_) => RichText::new("● Reconnecting…")
                        .color(Color32::from_rgb(220, 120, 40)),
                    None => RichText::new("○ No device").color(Color32::from_gray(140)),
                };
                ui.label(status);
            });
        });

        // ── Main content area ────────────────────────────────────────────
        let mut screen_ctx = ScreenContext {
            config_path: &self.config_path,
            config: &mut self.config,
            frame: self.latest_frame,
        };

        match self.screen {
            Screen::Dashboard => ui::dashboard::show(ui, &mut screen_ctx),
            Screen::Axes => ui::axes::show(ui, &mut screen_ctx),
            Screen::Buttons => ui::buttons::show(ui, &mut screen_ctx),
        }

        // Drive repaints at ~30 Hz to keep live data current.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
}

// ── Navigation helpers ────────────────────────────────────────────────────────

fn nav_button(ui: &mut egui::Ui, current: &mut Screen, target: Screen, label: &str) {
    let selected = *current == target;
    let fill = if selected {
        Color32::from_rgb(50, 80, 160)
    } else {
        Color32::TRANSPARENT
    };
    let text = RichText::new(label).color(if selected {
        Color32::WHITE
    } else {
        Color32::from_gray(200)
    });
    if ui
        .add(
            egui::Button::new(text)
                .fill(fill)
                .min_size([120.0, 28.0].into()),
        )
        .clicked()
    {
        *current = target;
    }
}
