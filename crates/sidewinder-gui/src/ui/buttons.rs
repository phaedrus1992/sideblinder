//! Buttons screen — remap, hat config, and layer config.
//!
//! ## Remap grid
//! Two columns: physical buttons on the left, virtual slots on the right.
//! Click a physical button to select it (highlighted), then click a virtual
//! slot to apply the remap.  Physical buttons that are currently pressed
//! (per the live frame) are shown in green.
//!
//! ## Hat switch
//! Toggle "Report as buttons" to send hat directions as discrete virtual
//! button presses instead of a POV value.  When enabled, a sub-panel lets
//! the user assign a virtual button to each of the 8 directions.
//!
//! ## Layer / shift
//! Assign a physical button as the shift key.  While held, the layer
//! button assignments replace the normal mappings.

use super::ScreenContext;
use crate::config_writer;
use egui::{Color32, RichText, Ui};
use sidewinder_app::config::{BUTTON_COUNT, BUTTON_COUNT_U8};

// ── Internal state ────────────────────────────────────────────────────────────

/// Ephemeral UI state stored in egui memory between frames.
#[derive(Clone, Copy, Default)]
struct ButtonsUiState {
    /// Physical button currently selected for remapping (`None` = none selected).
    selected_phys: Option<usize>,
}

const STATE_ID: &str = "sidewinder_buttons_ui_state";

// ── Public entry point ────────────────────────────────────────────────────────

/// Render the Buttons configuration screen.
pub fn show(ui: &mut Ui, ctx: &mut ScreenContext<'_>) {
    ui.heading("Buttons");
    ui.separator();

    let mut state: ButtonsUiState = ui
        .ctx()
        .data(|d| d.get_temp(egui::Id::new(STATE_ID)))
        .unwrap_or_default();

    show_remap_grid(ui, ctx, &mut state);
    ui.add_space(8.0);
    show_hat_config(ui, ctx);
    ui.add_space(8.0);
    show_layer_config(ui, ctx);

    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new(STATE_ID), state));
}

// ── Remap grid ────────────────────────────────────────────────────────────────

fn show_remap_grid(ui: &mut Ui, ctx: &mut ScreenContext<'_>, state: &mut ButtonsUiState) {
    ui.label(
        "Click a physical button to select it, then click a virtual slot to remap.",
    );
    let buttons_pressed = ctx.frame.map_or(0, |f| f.buttons);

    ui.columns(2, |cols| {
        // ── Left: physical buttons ─────────────────────────────────────────
        cols[0].label(RichText::new("Physical Buttons").strong());
        for phys in 0..BUTTON_COUNT {
            #[expect(clippy::cast_possible_truncation, reason = "phys < BUTTON_COUNT = 9, fits in u16")]
            let is_pressed = (buttons_pressed >> phys as u16) & 1 == 1;
            let is_selected = state.selected_phys == Some(phys);
            #[expect(clippy::cast_possible_truncation, reason = "phys < BUTTON_COUNT = 9, fits in u8")]
            let current_virt = ctx.config.buttons.virtual_for(phys as u8);

            let fill = if is_selected {
                Color32::from_rgb(60, 100, 200)
            } else if is_pressed {
                Color32::from_rgb(60, 180, 60)
            } else {
                Color32::from_gray(50)
            };
            let text_color = if is_selected || is_pressed {
                Color32::WHITE
            } else {
                Color32::from_gray(200)
            };

            let label = RichText::new(format!("Phys {} → Virt {}", phys + 1, current_virt + 1))
                .color(text_color);
            if cols[0]
                .add(
                    egui::Button::new(label)
                        .fill(fill)
                        .min_size([180.0, 26.0].into()),
                )
                .clicked()
            {
                state.selected_phys = if is_selected { None } else { Some(phys) };
            }
        }

        // ── Right: virtual slots ───────────────────────────────────────────
        cols[1].label(RichText::new("Virtual Slots").strong());
        if state.selected_phys.is_some() {
            cols[1].label(
                RichText::new("← Click a slot to apply remap")
                    .color(Color32::from_rgb(200, 200, 60)),
            );
        } else {
            cols[1].label(" ");
        }

        for virt in 0..BUTTON_COUNT {
            // Show which physical maps to this virtual (if any).
            let mapped_from: Vec<usize> = (0..BUTTON_COUNT)
                .filter(|&p| {
                    #[expect(clippy::cast_possible_truncation, reason = "p < BUTTON_COUNT = 9, fits in u8")]
                    let p_u8 = p as u8;
                    ctx.config.buttons.virtual_for(p_u8) as usize == virt
                })
                .collect();
            let label_str = if mapped_from.is_empty() {
                format!("Slot {} (free)", virt + 1)
            } else {
                let phys_list: Vec<String> =
                    mapped_from.iter().map(|p| format!("{}", p + 1)).collect();
                format!("Slot {} ← Phys {}", virt + 1, phys_list.join(", "))
            };

            let highlight = state.selected_phys.is_some();
            let fill = if highlight {
                Color32::from_rgb(60, 50, 80)
            } else {
                Color32::from_gray(45)
            };

            if cols[1]
                .add(
                    egui::Button::new(label_str)
                        .fill(fill)
                        .min_size([180.0, 26.0].into()),
                )
                .clicked()
                && let Some(phys) = state.selected_phys.take()
            {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "virt < BUTTON_COUNT = 9, fits in u8"
                )]
                let virt_u8 = virt as u8;
                let key = format!("buttons.{phys}");
                match config_writer::patch_u8(ctx.config_path, &key, virt_u8) {
                    Err(e) => {
                        tracing::warn!(internal_error = %e, key, "button remap write failed — remap not applied");
                    }
                    Ok(()) => {
                        // Reload config so the table is rebuilt with the new mapping.
                        match sidewinder_app::config::Config::load(ctx.config_path) {
                            Ok(new_cfg) => *ctx.config = new_cfg,
                            Err(e) => tracing::warn!(
                                internal_error = %e,
                                "button remap written but config reload failed — restart to pick up change"
                            ),
                        }
                    }
                }
            }
        }
    });
}

// ── Hat config ────────────────────────────────────────────────────────────────

fn show_hat_config(ui: &mut Ui, ctx: &mut ScreenContext<'_>) {
    ui.collapsing("Hat Switch", |ui| {
        let mut as_buttons = ctx.config.hat.as_buttons;
        if ui.checkbox(&mut as_buttons, "Report hat as virtual buttons").changed() {
            match config_writer::patch_bool(ctx.config_path, "hat.as_buttons", as_buttons) {
                Ok(()) => ctx.config.hat.as_buttons = as_buttons,
                Err(e) => tracing::warn!(internal_error = %e, "hat.as_buttons write failed"),
            }
        }

        if ctx.config.hat.as_buttons {
            ui.add_space(4.0);
            ui.label("Virtual button assigned to each direction:");
            egui::Grid::new("hat_dir_grid").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
                hat_dir_row(ui, ctx, "North",     "hat.north",      ctx.config.hat.north);
                hat_dir_row(ui, ctx, "North-East","hat.north_east", ctx.config.hat.north_east);
                hat_dir_row(ui, ctx, "East",      "hat.east",       ctx.config.hat.east);
                hat_dir_row(ui, ctx, "South-East","hat.south_east", ctx.config.hat.south_east);
                hat_dir_row(ui, ctx, "South",     "hat.south",      ctx.config.hat.south);
                hat_dir_row(ui, ctx, "South-West","hat.south_west", ctx.config.hat.south_west);
                hat_dir_row(ui, ctx, "West",      "hat.west",       ctx.config.hat.west);
                hat_dir_row(ui, ctx, "North-West","hat.north_west", ctx.config.hat.north_west);
            });
        }
    });
}

/// Max virtual button index accepted by the config validator (0-based, inclusive).
/// Mirrors `Config::validate_hat` in sidewinder-app which warns on idx >= 32.
const HAT_VIRT_MAX: u8 = 31;

fn hat_dir_row(ui: &mut Ui, ctx: &mut ScreenContext<'_>, label: &str, key: &str, current: u8) {
    ui.label(label);
    let mut val = current;
    if ui
        .add(egui::DragValue::new(&mut val).range(0..=HAT_VIRT_MAX))
        .changed()
    {
        match config_writer::patch_u8(ctx.config_path, key, val) {
            Ok(()) => {
                // Update the in-memory config for the changed field.
                match key {
                    "hat.north"      => ctx.config.hat.north = val,
                    "hat.north_east" => ctx.config.hat.north_east = val,
                    "hat.east"       => ctx.config.hat.east = val,
                    "hat.south_east" => ctx.config.hat.south_east = val,
                    "hat.south"      => ctx.config.hat.south = val,
                    "hat.south_west" => ctx.config.hat.south_west = val,
                    "hat.west"       => ctx.config.hat.west = val,
                    "hat.north_west" => ctx.config.hat.north_west = val,
                    _ => {}
                }
            }
            Err(e) => tracing::warn!(internal_error = %e, key, "hat direction write failed"),
        }
    }
    ui.end_row();
}

// ── Layer config ──────────────────────────────────────────────────────────────

fn show_layer_config(ui: &mut Ui, ctx: &mut ScreenContext<'_>) {
    ui.collapsing("Shift Layer", |ui| {
        ui.label(
            "Hold a shift button to activate the second layer of button mappings.",
        );

        ui.horizontal(|ui| {
            ui.label("Shift button (1-based, 0 = disabled):");
            let mut shift = ctx.config.layer.shift_button;
            if ui.add(egui::DragValue::new(&mut shift).range(0..=BUTTON_COUNT_U8)).changed() {
                if let Err(e) =
                    config_writer::patch_u8(ctx.config_path, "layer.shift_button", shift)
                {
                    tracing::warn!(internal_error = %e, "layer.shift_button write failed");
                }
                ctx.config.layer.shift_button = shift;
            }
        });
    });
}
