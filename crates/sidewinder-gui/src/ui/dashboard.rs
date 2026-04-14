//! Dashboard screen — live device status at a glance.
//!
//! Shows:
//! - Connection status indicator (connected / disconnected / no pipe)
//! - Four axis progress bars with centred zero points (X, Y, Rz, Slider)
//! - 9-button bitmask grid (lit when pressed)
//! - POV hat direction
//! - FFB master enable toggle and gain slider

use super::ScreenContext;
use crate::config_writer;
use egui::{Color32, ProgressBar, RichText, Slider, Ui};

// ── Axis display helpers ──────────────────────────────────────────────────────

/// Axis labels in [`sidewinder_ipc::GuiFrame::axes`] order.
const AXIS_LABELS: [&str; 4] = ["X", "Y", "Twist (Rz)", "Throttle"];

/// Raw axis value range for normalisation.
const AXIS_MAX: f32 = 32767.0;

// ── Public entry point ────────────────────────────────────────────────────────

/// Render the Dashboard screen.
pub fn show(ui: &mut Ui, ctx: &mut ScreenContext<'_>) {
    ui.heading("Dashboard");
    ui.separator();

    show_connection_status(ui, ctx);
    ui.add_space(8.0);

    show_axes(ui, ctx);
    ui.add_space(8.0);

    show_buttons(ui, ctx);
    ui.add_space(8.0);

    show_ffb_controls(ui, ctx);
}

// ── Connection status ─────────────────────────────────────────────────────────

fn show_connection_status(ui: &mut Ui, ctx: &ScreenContext<'_>) {
    ui.horizontal(|ui| {
        match ctx.frame {
            Some(f) if f.connected == 1 => {
                ui.label(RichText::new("● Connected").color(Color32::from_rgb(80, 200, 80)));
            }
            Some(_) => {
                ui.label(
                    RichText::new("● Disconnected — reconnecting…")
                        .color(Color32::from_rgb(220, 120, 40)),
                );
            }
            None => {
                ui.label(
                    RichText::new("○ No data — start sidewinder-app or reconnect device")
                        .color(Color32::from_gray(140)),
                );
            }
        }
    });
}

// ── Axis bars ─────────────────────────────────────────────────────────────────

fn show_axes(ui: &mut Ui, ctx: &ScreenContext<'_>) {
    ui.label("Axes");
    egui::Grid::new("dashboard_axes")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for (i, label) in AXIS_LABELS.iter().enumerate() {
                let raw = ctx.frame.map_or(0, |f| f.axes[i]);
                // Map [-32767, 32767] → [0.0, 1.0] for ProgressBar.
                let norm = (f32::from(raw) / AXIS_MAX / 2.0) + 0.5;
                ui.label(*label);
                ui.add(
                    ProgressBar::new(norm.clamp(0.0, 1.0))
                        .desired_width(260.0)
                        .text(format!("{raw}")),
                );
                ui.end_row();
            }
        });
}

// ── Button grid ───────────────────────────────────────────────────────────────

fn show_buttons(ui: &mut Ui, ctx: &ScreenContext<'_>) {
    ui.label("Buttons");
    let buttons = ctx.frame.map_or(0, |f| f.buttons);
    ui.horizontal_wrapped(|ui| {
        for bit in 0..9u16 {
            let pressed = (buttons >> bit) & 1 == 1;
            let color = if pressed {
                Color32::from_rgb(80, 200, 80)
            } else {
                Color32::from_gray(55)
            };
            let label = RichText::new(format!("{}", bit + 1))
                .color(if pressed {
                    Color32::BLACK
                } else {
                    Color32::from_gray(180)
                })
                .strong();
            ui.add(egui::Button::new(label).fill(color).min_size([32.0, 28.0].into()));
        }

        // POV hat indicator
        let pov = ctx.frame.map_or(0xFF, |f| f.pov);
        let pov_text = pov_label(pov);
        ui.add_space(8.0);
        ui.label(format!("POV: {pov_text}"));
    });
}

fn pov_label(pov: u8) -> &'static str {
    match pov {
        0 => "N",
        1 => "NE",
        2 => "E",
        3 => "SE",
        4 => "S",
        5 => "SW",
        6 => "W",
        7 => "NW",
        _ => "—",
    }
}

// ── FFB controls ──────────────────────────────────────────────────────────────

fn show_ffb_controls(ui: &mut Ui, ctx: &mut ScreenContext<'_>) {
    ui.label("Force Feedback");
    ui.horizontal(|ui| {
        let mut enabled = ctx.config.ffb_enabled;
        if ui.checkbox(&mut enabled, "Enable FFB").changed() {
            match config_writer::patch_bool(ctx.config_path, "ffb_enabled", enabled) {
                Ok(()) => ctx.config.ffb_enabled = enabled,
                Err(e) => tracing::warn!(internal_error = %e, "could not write ffb_enabled"),
            }
        }
    });

    ui.horizontal(|ui| {
        ui.label("Gain");
        let mut gain_f = f32::from(ctx.config.ffb_gain);
        if ui
            .add(Slider::new(&mut gain_f, 0.0..=255.0).show_value(true))
            .changed()
        {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "slider is clamped to 0.0..=255.0 before cast"
            )]
            let gain = gain_f.clamp(0.0, 255.0) as u8;
            match config_writer::patch_u8(ctx.config_path, "ffb_gain", gain) {
                Ok(()) => ctx.config.ffb_gain = gain,
                Err(e) => tracing::warn!(internal_error = %e, "could not write ffb_gain"),
            }
        }
    });
}
