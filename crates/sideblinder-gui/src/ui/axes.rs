//! Axes screen — per-axis configuration with live curve preview.
//!
//! Four collapsible panels (X, Y, Rz/Twist, Throttle/Slider) each expose:
//! - Response curve selector
//! - Dead zone, scale, and smoothing sliders
//! - Invert checkbox
//! - A small `egui_plot` showing the response curve with a live dot at the
//!   current raw axis position.

use super::ScreenContext;
use crate::config_writer;
use egui::Ui;
use egui_plot::{Line, Plot, PlotPoints, Points};
use sideblinder_app::config::{AxisConfig, ResponseCurve};

// ── Per-axis metadata ─────────────────────────────────────────────────────────

struct AxisMeta {
    label: &'static str,
    /// Dot-separated TOML key prefix for this axis (e.g. `"axis_x"`).
    key_prefix: &'static str,
}

const AXES: [AxisMeta; 4] = [
    AxisMeta { label: "X Axis",            key_prefix: "axis_x"      },
    AxisMeta { label: "Y Axis",            key_prefix: "axis_y"      },
    AxisMeta { label: "Twist (Rz)",        key_prefix: "axis_rz"     },
    AxisMeta { label: "Throttle (Slider)", key_prefix: "axis_slider" },
];

const AXIS_MAX: f32 = 32767.0;
const SMOOTHING_MAX: f32 = sideblinder_app::config::SMOOTHING_MAX as f32;

// ── Public entry point ────────────────────────────────────────────────────────

/// Render the Axes configuration screen.
pub fn show(ui: &mut Ui, ctx: &mut ScreenContext<'_>) {
    ui.heading("Axes");
    ui.label("Configure each axis — changes are written immediately to the config file.");
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        // Collect all four (label, config, frame_value) tuples before borrowing mutably.
        let configs: [AxisConfig; 4] = [
            ctx.config.axis_x.clone(),
            ctx.config.axis_y.clone(),
            ctx.config.axis_rz.clone(),
            ctx.config.axis_slider.clone(),
        ];
        let frame_values: [i16; 4] = ctx
            .frame
            .map_or([0i16; 4], |f| [f.axes[0], f.axes[1], f.axes[2], f.axes[3]]);

        for (i, meta) in AXES.iter().enumerate() {
            let config = configs[i].clone();
            let raw_value = frame_values[i];
            let changed_config = show_axis_panel(ui, meta, config, raw_value);
            if let Some(new_cfg) = changed_config {
                write_axis_config(ctx, meta.key_prefix, &new_cfg);
                match i {
                    0 => ctx.config.axis_x = new_cfg,
                    1 => ctx.config.axis_y = new_cfg,
                    2 => ctx.config.axis_rz = new_cfg,
                    3 => ctx.config.axis_slider = new_cfg,
                    _ => {}
                }
            }
            ui.separator();
        }
    });
}

// ── Axis panel ────────────────────────────────────────────────────────────────

/// Render one axis panel.  Returns the updated [`AxisConfig`] if any field changed.
fn show_axis_panel(
    ui: &mut Ui,
    meta: &AxisMeta,
    mut cfg: AxisConfig,
    raw_value: i16,
) -> Option<AxisConfig> {
    let original = cfg.clone();
    let id = egui::Id::new(meta.key_prefix);

    egui::collapsing_header::CollapsingHeader::new(meta.label)
        .id_salt(id)
        .default_open(true)
        .show(ui, |ui| {
            ui.columns(2, |cols| {
                show_axis_controls(&mut cols[0], &mut cfg, meta);
                show_curve_preview(&mut cols[1], &cfg, raw_value, meta.key_prefix);
            });
        });

    if cfg == original { None } else { Some(cfg) }
}

// ── Controls (left column) ────────────────────────────────────────────────────

fn show_axis_controls(ui: &mut Ui, cfg: &mut AxisConfig, meta: &AxisMeta) {

    // Invert
    ui.checkbox(&mut cfg.invert, "Invert");

    // Curve
    let mut selected_curve = cfg.curve;
    egui::ComboBox::from_id_salt(egui::Id::new((meta.key_prefix, "curve")))
        .selected_text(curve_label(selected_curve))
        .show_ui(ui, |ui| {
            for curve in [
                ResponseCurve::Linear,
                ResponseCurve::Quadratic,
                ResponseCurve::Cubic,
                ResponseCurve::SCurve,
            ] {
                ui.selectable_value(&mut selected_curve, curve, curve_label(curve));
            }
        });
    cfg.curve = selected_curve;

    // Dead zone
    ui.add(
        egui::Slider::new(&mut cfg.dead_zone, 0.0..=0.5)
            .text("Dead Zone")
            .fixed_decimals(2),
    );

    // Scale
    ui.add(
        egui::Slider::new(&mut cfg.scale, 0.1..=2.0)
            .text("Scale")
            .fixed_decimals(2),
    );

    // Smoothing
    let mut smoothing_f = f32::from(cfg.smoothing);
    ui.add(
        egui::Slider::new(&mut smoothing_f, 1.0..=SMOOTHING_MAX)
            .text("Smoothing")
            .fixed_decimals(0),
    );
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "slider clamped to 1..=30; fits in u8"
    )]
    {
        cfg.smoothing = smoothing_f.clamp(1.0, SMOOTHING_MAX) as u8;
    }
}

fn curve_label(curve: ResponseCurve) -> &'static str {
    match curve {
        ResponseCurve::Linear => "Linear",
        ResponseCurve::Quadratic => "Quadratic (x²)",
        ResponseCurve::Cubic => "Cubic (x³)",
        ResponseCurve::SCurve => "S-Curve",
    }
}

// ── Curve preview (right column) ──────────────────────────────────────────────

fn show_curve_preview(ui: &mut Ui, cfg: &AxisConfig, raw_value: i16, id_prefix: &str) {
    // Build 64-sample curve line from -1.0 to 1.0.
    const SAMPLES: u8 = 64;
    let points: PlotPoints = (0u8..=SAMPLES)
        .map(|i| {
            let x = (f64::from(i) / f64::from(SAMPLES)) * 2.0 - 1.0;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "x is in [-1.0, 1.0]; no truncation"
            )]
            let y = f64::from(apply_curve(cfg.curve, x as f32));
            [x, y]
        })
        .collect();

    // Live dot at the current axis position.
    let norm_x = (f32::from(raw_value) / AXIS_MAX).clamp(-1.0, 1.0);
    let dot_y = apply_curve(cfg.curve, norm_x);
    let dot: PlotPoints = vec![[f64::from(norm_x), f64::from(dot_y)]].into();

    Plot::new(id_prefix)
        .height(120.0)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_zoom(false)
        .allow_boxed_zoom(false)
        .include_x(-1.0)
        .include_x(1.0)
        .include_y(-1.0)
        .include_y(1.0)
        .show(ui, |plot_ui| {
            plot_ui.line(Line::new(curve_label(cfg.curve), points));
            plot_ui.points(Points::new("Current", dot).radius(5.0));
        });
}

/// Apply a response curve to a value in `[-1.0, 1.0]`.
///
/// Mirrors `ResponseCurve::apply` from `sideblinder-app/config.rs` without the
/// `#[cfg(any(target_os = "windows", test))]` gate so it can run in the GUI.
fn apply_curve(curve: ResponseCurve, v: f32) -> f32 {
    let sign = v.signum();
    let x = v.abs();
    let shaped = match curve {
        ResponseCurve::Linear => x,
        ResponseCurve::Quadratic => x * x,
        ResponseCurve::Cubic => x * x * x,
        ResponseCurve::SCurve => x * x * (3.0 - 2.0 * x),
    };
    sign * shaped
}

// ── Write-back ────────────────────────────────────────────────────────────────

fn write_axis_config(ctx: &ScreenContext<'_>, prefix: &str, cfg: &AxisConfig) {
    let path = ctx.config_path;

    macro_rules! patch {
        ($suffix:literal, $fn:ident, $val:expr) => {{
            let key = format!("{}.{}", prefix, $suffix);
            if let Err(e) = config_writer::$fn(path, &key, $val) {
                tracing::warn!(internal_error = %e, key, "axis config write failed");
            }
        }};
    }

    patch!("invert",    patch_bool, cfg.invert);
    patch!("dead_zone", patch_f32,  cfg.dead_zone);
    patch!("scale",     patch_f32,  cfg.scale);
    patch!("smoothing", patch_u8,   cfg.smoothing);
    patch!("curve",     patch_str,  curve_toml_str(cfg.curve));
}

fn curve_toml_str(curve: ResponseCurve) -> &'static str {
    match curve {
        ResponseCurve::Linear => "linear",
        ResponseCurve::Quadratic => "quadratic",
        ResponseCurve::Cubic => "cubic",
        ResponseCurve::SCurve => "s-curve",
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::apply_curve;
    use sideblinder_app::config::ResponseCurve;

    #[test]
    fn linear_curve_is_identity() {
        assert!((apply_curve(ResponseCurve::Linear, 0.5) - 0.5).abs() < 1e-6);
        assert!((apply_curve(ResponseCurve::Linear, -0.5) - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn curve_maps_zero_to_zero() {
        for curve in [
            ResponseCurve::Linear,
            ResponseCurve::Quadratic,
            ResponseCurve::Cubic,
            ResponseCurve::SCurve,
        ] {
            assert!(apply_curve(curve, 0.0).abs() < 1e-6, "{curve:?} should map 0→0");
        }
    }

    #[test]
    fn curve_maps_one_to_one() {
        for curve in [
            ResponseCurve::Linear,
            ResponseCurve::Quadratic,
            ResponseCurve::Cubic,
            ResponseCurve::SCurve,
        ] {
            let out = apply_curve(curve, 1.0);
            assert!((out - 1.0).abs() < 1e-6, "{curve:?} should map 1→1, got {out}");
        }
    }

    #[test]
    fn curve_preserves_sign() {
        for curve in [ResponseCurve::Quadratic, ResponseCurve::Cubic, ResponseCurve::SCurve] {
            let pos = apply_curve(curve, 0.7);
            let neg = apply_curve(curve, -0.7);
            assert!(pos > 0.0, "{curve:?} positive input should produce positive output");
            assert!(neg < 0.0, "{curve:?} negative input should produce negative output");
            assert!((pos + neg).abs() < 1e-6, "{curve:?} should be sign-symmetric");
        }
    }
}
