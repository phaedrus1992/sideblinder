//! TOML configuration with hot-reload via `notify`.
//!
//! The config file lives at `%APPDATA%\Sidewinder\config.toml` on Windows, or
//! `~/.config/sidewinder/config.toml` elsewhere.  The app watches the file for
//! changes and reloads atomically via a `tokio::sync::watch` channel.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::sync::watch;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Maximum allowed value for `AxisConfig::smoothing`.
pub const SMOOTHING_MAX: u8 = 30;

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error returned by config operations.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The config file could not be read.
    #[error("I/O error reading config: {0}")]
    Io(#[from] std::io::Error),
    /// The TOML could not be parsed.
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
}

// ── Types ─────────────────────────────────────────────────────────────────────

/// Response curve applied to a normalised axis value.
///
/// All curves:
/// - Operate on a normalised input in `[-1.0, 1.0]`
/// - Produce a normalised output in `[-1.0, 1.0]`
/// - Are sign-preserving: `sign(output) == sign(input)`
/// - Map `0 → 0` and `1 → 1`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResponseCurve {
    /// 1:1 passthrough — output equals input exactly.
    #[default]
    Linear,
    /// x² — gentle near centre, more aggressive at extremes.
    Quadratic,
    /// x³ — strong centre deadening, very aggressive at extremes.
    Cubic,
    /// Smoothstep 3x² − 2x³ — soft near centre AND soft near extremes.
    #[serde(rename = "s-curve")]
    SCurve,
}

impl ResponseCurve {
    /// Apply the curve to a normalised value in `[-1.0, 1.0]`.
    ///
    /// Sign is always preserved: the curve is applied to the absolute value,
    /// then the original sign is restored.
    #[cfg(any(target_os = "windows", test))]
    #[must_use]
    pub fn apply(self, v: f32) -> f32 {
        let sign = v.signum();
        let x = v.abs();
        let shaped = match self {
            Self::Linear => x,
            Self::Quadratic => x * x,
            Self::Cubic => x * x * x,
            Self::SCurve => x * x * (3.0 - 2.0 * x),
        };
        sign * shaped
    }
}

/// Per-axis configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AxisConfig {
    /// Invert the axis (multiply by -1 before forwarding).
    pub invert: bool,
    /// Dead zone as a fraction of the full range (0.0 – 0.5).
    pub dead_zone: f32,
    /// Scale factor applied after dead-zone (1.0 = full range).
    pub scale: f32,
    /// Response curve applied after dead-zone, before scale.
    pub curve: ResponseCurve,
    /// Rolling-average window size for jitter reduction (1 = no smoothing).
    ///
    /// Valid range: 1–[`SMOOTHING_MAX`].
    pub smoothing: u8,
}

impl Default for AxisConfig {
    fn default() -> Self {
        Self {
            invert: false,
            dead_zone: 0.0,
            scale: 1.0,
            curve: ResponseCurve::default(),
            smoothing: 1,
        }
    }
}

/// Hardware calibration data for all four axes.
///
/// Written by the `sidewinder-diag calibrate` wizard and read by the bridge
/// when normalising raw axis values.  The default covers the full i16 range,
/// which is a no-op equivalent to uncalibrated operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalibrationConfig {
    /// Observed minimum raw value for the X axis.
    pub x_min: i16,
    /// Observed maximum raw value for the X axis.
    pub x_max: i16,
    /// Observed minimum raw value for the Y axis.
    pub y_min: i16,
    /// Observed maximum raw value for the Y axis.
    pub y_max: i16,
    /// Observed minimum raw value for the Rz / twist axis.
    pub rz_min: i16,
    /// Observed maximum raw value for the Rz / twist axis.
    pub rz_max: i16,
    /// Observed minimum raw value for the Slider / throttle axis.
    pub slider_min: i16,
    /// Observed maximum raw value for the Slider / throttle axis.
    pub slider_max: i16,
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            x_min: i16::MIN,
            x_max: i16::MAX,
            y_min: i16::MIN,
            y_max: i16::MAX,
            rz_min: i16::MIN,
            rz_max: i16::MAX,
            slider_min: i16::MIN,
            slider_max: i16::MAX,
        }
    }
}

impl CalibrationConfig {
    /// Normalise a raw axis value against the measured hardware range.
    ///
    /// Maps `raw` from `[cal_min, cal_max]` onto the full i16 range
    /// `[-32768, 32767]`, clamping out-of-range inputs.  When the config is
    /// default (full i16 range) this is a pass-through within floating-point
    /// precision.
    ///
    /// # Arguments
    ///
    /// * `raw` — the raw value from the HID report
    /// * `cal_min` — the observed hardware minimum
    /// * `cal_max` — the observed hardware maximum
    #[cfg(any(target_os = "windows", test))]
    pub fn apply(raw: i16, cal_min: i16, cal_max: i16) -> i16 {
        // Guard against a degenerate calibration where min >= max (e.g. uninitialised
        // or corrupted calibration data).  Pass through rather than divide by zero.
        if cal_min >= cal_max {
            tracing::warn!(
                cal_min,
                cal_max,
                "degenerate calibration range (min >= max); passing through raw value"
            );
            return raw;
        }
        let raw = f32::from(raw);
        let min = f32::from(cal_min);
        let span = f32::from(cal_max) - min;
        // Map [cal_min..cal_max] -> [0.0..1.0], scale to full u16 width (65535),
        // then shift down by 32768 to centre on zero.
        // clamp guarantees the value is in [-32768.0, 32767.0], so the f32→i16 cast is safe.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "clamped to i16 bounds before cast"
        )]
        let result = ((raw - min) / span * 65535.0 - 32768.0).clamp(-32768.0, 32767.0) as i16;
        result
    }
}

// ── Button map ────────────────────────────────────────────────────────────────

/// The maximum number of physical buttons the Sidewinder FF2 can report.
pub const BUTTON_COUNT: usize = 9;

/// [`BUTTON_COUNT`] as `u8`. Safe because `BUTTON_COUNT = 9 ≤ u8::MAX`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "BUTTON_COUNT = 9, always fits in u8"
)]
pub const BUTTON_COUNT_U8: u8 = BUTTON_COUNT as u8;

/// Maps physical button numbers to virtual button numbers.
///
/// Physical and virtual button indices are **0-based** in the config file.
/// The default is the identity map (each button maps to itself).
///
/// Example TOML — swap buttons 1 and 2 (0-based: swap 0 and 1):
/// ```toml
/// [buttons]
/// 0 = 1
/// 1 = 0
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonMapConfig {
    /// Keys are 0-based physical button indices (as strings in TOML).
    /// Values are 0-based virtual button indices.
    /// Missing entries default to identity.
    #[serde(flatten)]
    map: std::collections::BTreeMap<String, u8>,
    /// Precomputed lookup table: `table[phys]` = virtual button index for
    /// physical button `phys`.  Built from `map` after deserialization.
    /// Skipped during serialization; rebuilt via [`ButtonMapConfig::default`].
    #[serde(skip)]
    table: [u8; BUTTON_COUNT],
}

impl Default for ButtonMapConfig {
    fn default() -> Self {
        // BUTTON_COUNT = 9, so i is always 0..=8, which fits in u8 (BUTTON_COUNT_U8).
        #[expect(
            clippy::cast_possible_truncation,
            reason = "BUTTON_COUNT = 9, index always fits in u8"
        )]
        let mut s = Self {
            map: std::collections::BTreeMap::new(),
            table: std::array::from_fn(|i| i as u8),
        };
        s.rebuild_table();
        s
    }
}

impl ButtonMapConfig {
    /// Construct from an explicit physical→virtual map (keys are 0-based physical
    /// button indices; values are 0-based virtual button indices).
    #[cfg(test)]
    #[must_use]
    pub fn from_pairs(pairs: &[(u8, u8)]) -> Self {
        let map = pairs
            .iter()
            .map(|&(phys, virt)| (phys.to_string(), virt))
            .collect();
        // BUTTON_COUNT = 9, so i is always 0..=8, which fits in u8.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "BUTTON_COUNT = 9, index always fits in u8"
        )]
        let mut s = Self {
            map,
            table: std::array::from_fn(|i| i as u8),
        };
        s.rebuild_table();
        s
    }

    /// Rebuild the precomputed lookup table from `self.map`.
    ///
    /// Called after deserialization (via a custom [`Deserialize`] impl on
    /// [`Config`]) and after construction to ensure the table is always
    /// consistent with `map`.
    fn rebuild_table(&mut self) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "phys < BUTTON_COUNT = 9, always fits in u8"
        )]
        for phys in 0..BUTTON_COUNT {
            self.table[phys] = self
                .map
                .get(&phys.to_string())
                .copied()
                .unwrap_or(phys as u8);
        }
    }

    /// Return the virtual button index (0-based) for a given physical button
    /// index (0-based).  Missing entries default to identity.
    ///
    /// Indices at or beyond [`BUTTON_COUNT`] are returned as-is (identity).
    #[must_use]
    pub fn virtual_for(&self, physical: u8) -> u8 {
        let idx = physical as usize;
        if idx < BUTTON_COUNT {
            self.table[idx]
        } else {
            physical
        }
    }
}

// ── Hat config ────────────────────────────────────────────────────────────────

/// Configuration for the 8-way POV hat switch.
///
/// When `as_buttons` is `true`, the hat directions are mapped to discrete
/// virtual buttons instead of being reported as a POV value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HatConfig {
    /// When `true`, the hat fires virtual buttons instead of reporting a POV.
    pub as_buttons: bool,
    /// Virtual button index (0-based) for North. Default: 9.
    pub north: u8,
    /// Virtual button index (0-based) for North-East. Default: 10.
    pub north_east: u8,
    /// Virtual button index (0-based) for East. Default: 11.
    pub east: u8,
    /// Virtual button index (0-based) for South-East. Default: 12.
    pub south_east: u8,
    /// Virtual button index (0-based) for South. Default: 13.
    pub south: u8,
    /// Virtual button index (0-based) for South-West. Default: 14.
    pub south_west: u8,
    /// Virtual button index (0-based) for West. Default: 15.
    pub west: u8,
    /// Virtual button index (0-based) for North-West. Default: 16.
    pub north_west: u8,
}

impl Default for HatConfig {
    fn default() -> Self {
        Self {
            as_buttons: false,
            // Buttons 10–17 (0-based: 9–16) follow the 9 physical buttons.
            north: 9,
            north_east: 10,
            east: 11,
            south_east: 12,
            south: 13,
            south_west: 14,
            west: 15,
            north_west: 16,
        }
    }
}

impl HatConfig {
    /// Return the virtual button index for a given [`PovDirection`], or `None`
    /// if the direction is `Center`.
    #[cfg(any(target_os = "windows", test))]
    #[must_use]
    pub fn button_for(&self, pov: sidewinder_hid::input::PovDirection) -> Option<u8> {
        use sidewinder_hid::input::PovDirection;
        match pov {
            PovDirection::North => Some(self.north),
            PovDirection::NorthEast => Some(self.north_east),
            PovDirection::East => Some(self.east),
            PovDirection::SouthEast => Some(self.south_east),
            PovDirection::South => Some(self.south),
            PovDirection::SouthWest => Some(self.south_west),
            PovDirection::West => Some(self.west),
            PovDirection::NorthWest => Some(self.north_west),
            PovDirection::Center => None,
        }
    }
}

// ── Layer config ──────────────────────────────────────────────────────────────

/// Two-layer button system: hold a shift button to access a second set of
/// button mappings.
///
/// Example TOML:
/// ```toml
/// [layer]
/// shift_button = 6   # physical button 6 (1-based) acts as the shift key
///
/// [layer.buttons]
/// 0 = 10   # physical button index 0 + shift → virtual button 10
/// 1 = 11
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerConfig {
    /// Physical button number (1-based) that acts as the shift key.
    ///
    /// Set to `0` to disable layers entirely.  When shift is held, the shift
    /// button itself is not forwarded to the virtual device.
    pub shift_button: u8,
    /// Layer-2 button mappings (same format as `[buttons]`).
    ///
    /// Only used while `shift_button` is held.
    pub buttons: ButtonMapConfig,
}

// ── Top-level config ──────────────────────────────────────────────────────────

/// Top-level configuration.
///
/// Axis names follow the physical Sidewinder FF2 HID report layout:
/// `axis_x` (bytes 0-1), `axis_y` (bytes 2-3), `axis_rz` / twist (bytes 4-5),
/// `axis_slider` / throttle (bytes 6-7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// X axis configuration.
    pub axis_x: AxisConfig,
    /// Y axis configuration.
    pub axis_y: AxisConfig,
    /// Rz / twist axis configuration (bytes 4-5 of the HID report).
    pub axis_rz: AxisConfig,
    /// Slider / throttle axis configuration (bytes 6-7 of the HID report).
    pub axis_slider: AxisConfig,
    /// Hardware calibration data (written by the calibrate wizard).
    pub calibration: CalibrationConfig,
    /// Button remapping: physical button → virtual button.
    pub buttons: ButtonMapConfig,
    /// Hat switch configuration.
    pub hat: HatConfig,
    /// Two-layer button system.
    pub layer: LayerConfig,
    /// Global force-feedback gain (0 – 255).
    pub ffb_gain: u8,
    /// Enable or disable all force-feedback output.
    ///
    /// When `false`, FFB packets from the driver are dropped and the physical
    /// motors are silenced via a Device Control: Disable Actuators report.
    #[serde(default = "default_ffb_enabled")]
    pub ffb_enabled: bool,
    /// Log level override (e.g. `"debug"`, `"info"`).
    pub log_level: String,
}

fn default_ffb_enabled() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            axis_x: AxisConfig::default(),
            axis_y: AxisConfig::default(),
            axis_rz: AxisConfig::default(),
            axis_slider: AxisConfig::default(),
            calibration: CalibrationConfig::default(),
            buttons: ButtonMapConfig::default(),
            hat: HatConfig::default(),
            layer: LayerConfig::default(),
            ffb_gain: 255,
            ffb_enabled: true,
            log_level: "info".to_string(),
        }
    }
}

impl Config {
    /// Parse from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Parse`] if the TOML is invalid.
    pub fn from_toml(s: &str) -> Result<Self, ConfigError> {
        let mut cfg: Self = toml::from_str(s)?;
        // Rebuild lookup tables that are skipped during deserialization.
        cfg.buttons.rebuild_table();
        cfg.layer.buttons.rebuild_table();
        Ok(cfg)
    }

    /// Load from a file path.
    ///
    /// Validation warnings are logged at WARN level but do not prevent loading.
    /// Out-of-range fields are not automatically corrected — the app uses the
    /// value as given and the behaviour is clamped at the point of use.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Io`] or [`ConfigError::Parse`].
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let s = std::fs::read_to_string(path)?;
        let cfg = Self::from_toml(&s)?;
        for warning in cfg.validate() {
            tracing::warn!("config validation: {warning}");
        }
        Ok(cfg)
    }

    /// Validate all field values and return a list of human-readable warnings.
    ///
    /// Out-of-range values are reported as warnings rather than hard errors.
    /// This method does not modify the configuration or clamp any values;
    /// the caller decides whether to log these warnings or surface them to
    /// the user, and clamping happens at the point of use.
    ///
    /// Returns an empty `Vec` when the config is fully valid.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        self.validate_axes(&mut warnings);
        self.validate_buttons(&mut warnings);
        self.validate_layer(&mut warnings);
        self.validate_hat(&mut warnings);
        self.validate_calibration(&mut warnings);
        warnings
    }

    /// Validate per-axis fields (smoothing, dead zone, scale).
    fn validate_axes(&self, warnings: &mut Vec<String>) {
        for (name, axis) in [
            ("axis_x", &self.axis_x),
            ("axis_y", &self.axis_y),
            ("axis_rz", &self.axis_rz),
            ("axis_slider", &self.axis_slider),
        ] {
            if axis.smoothing == 0 || axis.smoothing > SMOOTHING_MAX {
                warnings.push(format!(
                    "[{name}] smoothing must be between 1 and {SMOOTHING_MAX} (you set {})",
                    axis.smoothing
                ));
            }
            if !(0.0..=0.5).contains(&axis.dead_zone) {
                warnings.push(format!(
                    "[{name}] dead_zone must be between 0.0 and 0.5 (you set {})",
                    axis.dead_zone
                ));
            }
            if !(0.0..=4.0).contains(&axis.scale) {
                warnings.push(format!(
                    "[{name}] scale must be between 0.0 and 4.0 (you set {})",
                    axis.scale
                ));
            }
        }
    }

    /// Validate the layer-1 button map (duplicate virtual targets, out-of-range indices).
    fn validate_buttons(&self, warnings: &mut Vec<String>) {
        let mut seen = std::collections::HashMap::<u8, u8>::new();
        for phys in 0..BUTTON_COUNT_U8 {
            let virt = self.buttons.virtual_for(phys);
            if virt >= 32 && self.buttons.map.contains_key(&phys.to_string()) {
                warnings.push(format!(
                    "[buttons] physical button {phys} maps to virtual button {virt} \
                     which is out of range (max is 31) — this mapping has no effect"
                ));
            }
            // Only warn about shadowing for explicitly remapped buttons.
            // Identity-mapped buttons are never tracked in `seen` so they cannot
            // produce false-positive duplicate warnings against explicit remaps.
            if self.buttons.map.contains_key(&phys.to_string())
                && let Some(prev_phys) = seen.insert(virt, phys)
            {
                warnings.push(format!(
                    "[buttons] physical buttons {prev_phys} and {phys} both map to \
                     virtual button {virt} — one will shadow the other",
                ));
            }
        }
    }

    /// Validate the layer-2 button map and shift button when layers are enabled.
    fn validate_layer(&self, warnings: &mut Vec<String>) {
        if self.layer.shift_button == 0 {
            return;
        }
        if self.layer.shift_button as usize > BUTTON_COUNT {
            warnings.push(format!(
                "[layer] shift_button {} is out of range (max physical button is {})",
                self.layer.shift_button, BUTTON_COUNT,
            ));
        }

        // The shift button itself is consumed by apply_config and never forwarded,
        // so exclude it from the duplicate scan to avoid false-positive warnings.
        let shift_phys = if (1..=BUTTON_COUNT_U8).contains(&self.layer.shift_button) {
            Some(self.layer.shift_button - 1) // convert 1-based to 0-based
        } else {
            None
        };

        let mut seen = std::collections::HashMap::<u8, u8>::new();
        for phys in 0..BUTTON_COUNT_U8 {
            if Some(phys) == shift_phys {
                continue;
            }
            let virt = self.layer.buttons.virtual_for(phys);
            if let Some(prev_phys) = seen.insert(virt, phys) {
                warnings.push(format!(
                    "[layer.buttons] physical buttons {prev_phys} and {phys} both map to \
                     virtual button {virt} — one will shadow the other",
                ));
            }
        }
    }

    /// Validate hat button indices when hat-as-buttons mode is enabled.
    fn validate_hat(&self, warnings: &mut Vec<String>) {
        if !self.hat.as_buttons {
            return;
        }
        for (dir, idx) in [
            ("north", self.hat.north),
            ("north_east", self.hat.north_east),
            ("east", self.hat.east),
            ("south_east", self.hat.south_east),
            ("south", self.hat.south),
            ("south_west", self.hat.south_west),
            ("west", self.hat.west),
            ("north_west", self.hat.north_west),
        ] {
            if idx >= 32 {
                warnings.push(format!(
                    "[hat] {dir} = {idx} is out of range (max virtual button index is 31) \
                     — this direction will not fire any button"
                ));
            }
        }
    }

    /// Validate calibration ranges (min must be less than max on each axis).
    fn validate_calibration(&self, warnings: &mut Vec<String>) {
        for (name, min, max) in [
            (
                "calibration.x",
                self.calibration.x_min,
                self.calibration.x_max,
            ),
            (
                "calibration.y",
                self.calibration.y_min,
                self.calibration.y_max,
            ),
            (
                "calibration.rz",
                self.calibration.rz_min,
                self.calibration.rz_max,
            ),
            (
                "calibration.slider",
                self.calibration.slider_min,
                self.calibration.slider_max,
            ),
        ] {
            if min >= max {
                warnings.push(format!(
                    "[{name}] min ({min}) must be less than max ({max}) — \
                     calibration range is degenerate; run sidewinder-diag calibrate"
                ));
            }
        }
    }

    /// Apply calibration, dead zone, response curve, scale, and invert to a raw axis value.
    ///
    /// Processing order:
    /// 1. Calibration normalisation — stretches the hardware range to full i16
    /// 2. Dead zone — values within ±`dead_zone`/2 of centre snap to 0
    /// 3. Response curve — applied to the normalised value after dead zone
    /// 4. Scale and optional invert
    ///
    /// Smoothing (rolling average) is applied upstream by the caller before
    /// this function, so the `raw` argument is already the smoothed value.
    ///
    /// # Arguments
    ///
    /// * `axis_cfg` — per-axis settings (dead zone, curve, scale, invert)
    /// * `cal_min` / `cal_max` — hardware calibration range for this axis
    /// * `raw` — raw i16 value from the HID report (pre-smoothed by caller)
    #[cfg(any(target_os = "windows", test))]
    #[must_use]
    pub fn apply_axis(axis_cfg: &AxisConfig, cal_min: i16, cal_max: i16, raw: i16) -> i16 {
        // Step 1: calibration normalisation.
        let calibrated = CalibrationConfig::apply(raw, cal_min, cal_max);

        // Clamp to [-1.0, 1.0] — i16::MIN / 32767.0 ≈ -1.00003, which would
        // violate the curve's documented input contract.
        let mut v = (f32::from(calibrated) / 32767.0).clamp(-1.0, 1.0);

        // Step 2: dead zone — values within ±dead_zone/2 snap to 0.
        if v.abs() < axis_cfg.dead_zone / 2.0 {
            v = 0.0;
        }

        // Step 3: response curve.
        v = axis_cfg.curve.apply(v);

        // Step 4: scale and optional invert.
        v *= axis_cfg.scale;
        if axis_cfg.invert {
            v = -v;
        }

        // clamp guarantees the value is in [-32768.0, 32767.0], so the f32→i16 cast is safe.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "clamped to i16 bounds before cast"
        )]
        let result = (v * 32767.0).clamp(-32768.0, 32767.0) as i16;
        result
    }

    /// Generate a fully-documented TOML config string with the default values.
    ///
    /// Every field is present and annotated with inline comments explaining its
    /// purpose, valid range, and default value.  The output is valid TOML that
    /// round-trips through [`Config::from_toml`] to produce a [`Config::default`].
    #[must_use]
    pub fn generate_toml() -> String {
        // Build with explicit string construction so comments are preserved exactly.
        // toml_edit could also work but literal construction keeps the layout stable.
        format!(
            r#"# Sidewinder Force Feedback 2 — configuration file
# Generated by `sidewinder-app config --generate`.
# Edit any value and save; the app hot-reloads changes automatically.

# ── Log level ───────────────────────────────────────────────────────────────────
# Controls how much detail is written to the log.
# Options: "error", "warn", "info", "debug", "trace"
# Default: "info"
log_level = "info"

# ── Force feedback ───────────────────────────────────────────────────────────────
# Global FFB gain applied to all effects (0 = no force, 255 = full force).
# Default: 255
ffb_gain = 255

# ── Calibration ──────────────────────────────────────────────────────────────────
# Physical axis ranges measured by `sidewinder-diag calibrate`.
# The app stretches these values to the full virtual range.
# Run `sidewinder-diag calibrate` to measure your hardware automatically.
[calibration]
x_min = -32768
x_max = 32767
y_min = -32768
y_max = 32767
rz_min = -32768
rz_max = 32767
slider_min = -32768
slider_max = 32767

# ── Axes ─────────────────────────────────────────────────────────────────────────
# Each axis has the same options:
#   invert    — flip the direction (true/false). Default: false.
#   dead_zone — fraction of range snapped to centre (0.0–0.5). Default: 0.0.
#   scale     — output multiplier (0.0–4.0). Default: 1.0.
#   curve     — response shape: "linear", "quadratic", "cubic", "s-curve".
#               Default: "linear".
#   smoothing — rolling-average window for jitter reduction (1–{SMOOTHING_MAX}).
#               1 = no smoothing. Default: 1.

[axis_x]
invert = false
dead_zone = 0.0
scale = 1.0
curve = "linear"
smoothing = 1

[axis_y]
invert = false
dead_zone = 0.0
scale = 1.0
curve = "linear"
smoothing = 1

[axis_rz]
invert = false
dead_zone = 0.0
scale = 1.0
curve = "linear"
smoothing = 1

[axis_slider]
invert = false
dead_zone = 0.0
scale = 1.0
curve = "linear"
smoothing = 1
"#
        )
    }
}

// ── Config CLI helpers ────────────────────────────────────────────────────────

/// Run `sidewinder-app config --validate`: load the config file and print a
/// human-readable validation report to stdout.
///
/// Returns `Ok(true)` when the config is clean, `Ok(false)` when warnings were
/// found (exit code should be non-zero), and `Err` on I/O or parse failures.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
#[expect(
    clippy::print_stdout,
    reason = "CLI binary — println! is correct for user-facing output"
)]
pub fn run_validate(path: &std::path::Path) -> Result<bool, ConfigError> {
    // Use from_toml directly to avoid Config::load's internal validate()+tracing::warn
    // calls, which would log each warning a second time before we print them.
    let s = std::fs::read_to_string(path)?;
    let cfg = Config::from_toml(&s)?;
    let warnings = cfg.validate();

    println!("Config file: {}", path.display());
    println!();

    // Sections that can produce validation warnings — must match the prefixes
    // emitted by Config::validate().  "ffb_gain" is a u8 so it can't be out
    // of range; omit "ffb" to avoid printing a permanently misleading "OK".
    // "layer" also matches "layer.buttons" because starts_with is a prefix check.
    let sections = [
        "axis_x",
        "axis_y",
        "axis_rz",
        "axis_slider",
        "calibration",
        "buttons",
        "layer",
        "hat",
    ];

    // Map warnings to their section for display grouping.
    for section in sections {
        let section_warnings: Vec<&str> = warnings
            .iter()
            .filter(|w| w.starts_with(&format!("[{section}")))
            .map(String::as_str)
            .collect();
        if section_warnings.is_empty() {
            println!("  \u{2713} [{section}] — OK");
        } else {
            for w in section_warnings {
                println!("  ! {w}");
            }
        }
    }

    println!();
    if warnings.is_empty() {
        println!("No warnings. Config is valid.");
        Ok(true)
    } else {
        println!(
            "{} warning{}. The app will use defaults for flagged fields.",
            warnings.len(),
            if warnings.len() == 1 { "" } else { "s" }
        );
        Ok(false)
    }
}

/// Run `sidewinder-app config --generate`: write a documented default config
/// to `path` if it does not already exist.
///
/// Prints the path when a new config is written.  If the file already exists,
/// prints a message and returns success without overwriting it.
///
/// # Errors
///
/// Returns [`ConfigError::Io`] if the file cannot be created or written for
/// reasons other than the destination already existing.
#[expect(
    clippy::print_stdout,
    reason = "CLI binary — println! is correct for user-facing output"
)]
pub fn run_generate(path: &std::path::Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    // Use create_new to atomically fail if the file already exists, avoiding a
    // TOCTOU race between an existence check and the write.
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut f) => {
            use std::io::Write as _;
            f.write_all(Config::generate_toml().as_bytes())?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            println!("Config already exists at:");
            println!("  {}", path.display());
            println!();
            println!("To regenerate, delete the file first.");
            return Ok(());
        }
        Err(e) => return Err(ConfigError::Io(e)),
    }

    println!("Default config written to:");
    println!("  {}", path.display());
    println!();
    println!("Edit it with any text editor, then restart the app.");
    Ok(())
}

// ── Default path ──────────────────────────────────────────────────────────────

/// Return the platform default config file path.
///
/// SYNC: A copy of this function lives in `sidewinder_diag::main`.
/// If you change the paths here, update that copy too.
#[must_use]
pub fn default_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(appdata)
            .join("Sidewinder")
            .join("config.toml")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".config")
            .join("sidewinder")
            .join("config.toml")
    }
}

// ── Hot-reload watcher ────────────────────────────────────────────────────────

/// Spawn a background task that watches `path` for changes and reloads the
/// config into the returned [`watch::Receiver`].
///
/// The initial value is loaded synchronously; if the file does not exist a
/// default [`Config`] is used.  Watcher errors are logged and the watcher task
/// exits, but the returned receiver continues to hold the last known config.
pub fn watch_config(path: &Path) -> watch::Receiver<Config> {
    let initial = match Config::load(path) {
        Ok(cfg) => cfg,
        Err(ConfigError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!("config file not found at {:?}, using defaults", path);
            Config::default()
        }
        Err(e) => {
            tracing::warn!(
                internal_error = %e,
                "Your config file has an error. The default settings will be used."
            );
            Config::default()
        }
    };
    let (tx, rx) = watch::channel(initial);

    let watch_path = path.to_owned();
    tokio::spawn(async move {
        use notify::{Event, RecursiveMode, Watcher};
        use tokio::sync::mpsc;

        let (notify_tx, mut notify_rx) = mpsc::channel::<Result<Event, notify::Error>>(16);

        let mut watcher =
            match notify::recommended_watcher(move |res| match notify_tx.try_send(res) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    tracing::warn!("config watcher: event channel full, dropping event");
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    tracing::debug!("config watcher: event channel closed");
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!(
                        internal_error = %e,
                        "Couldn't watch the config file for changes. Edit the file and restart the app to apply changes."
                    );
                    return;
                }
            };

        let watch_dir = watch_path.parent().unwrap_or(&watch_path);
        if let Err(e) = watcher.watch(watch_dir, RecursiveMode::NonRecursive) {
            tracing::error!(
                internal_error = %e,
                "Couldn't watch the config file for changes. Edit the file and restart the app to apply changes."
            );
            return;
        }

        while let Some(event) = notify_rx.recv().await {
            match event {
                Ok(ev) if ev.paths.contains(&watch_path) => match Config::load(&watch_path) {
                    Ok(cfg) => {
                        tracing::info!("config reloaded from {:?}", watch_path);
                        if tx.send(cfg).is_err() {
                            tracing::debug!("all config receivers dropped; stopping watcher");
                            return;
                        }
                    }
                    Err(e) => tracing::warn!(
                        internal_error = %e,
                        config_path = %watch_path.display(),
                        "Config file has an error — keeping the previous settings. Fix the error and save to apply changes."
                    ),
                },
                Ok(_) => {} // event for a different path — ignore
                Err(e) => tracing::error!(
                    internal_error = %e,
                    "Couldn't watch the config file for changes. Edit the file and restart the app to apply changes."
                ),
            }
        }
    });

    rx
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code — panics and unwraps are acceptable"
)]
mod tests {
    use super::*;

    #[test]
    fn default_config_parses_from_empty_toml() {
        let cfg = Config::from_toml("").unwrap();
        assert_eq!(cfg, Config::default());
    }

    /// The shipped `config/default.toml` must parse without errors.
    ///
    /// This guards against the template falling out of sync with the Rust types.
    #[test]
    fn default_toml_file_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent() // crates/sidewinder-app → crates
            .and_then(|p| p.parent()) // crates → repo root
            .unwrap()
            .join("config")
            .join("default.toml");
        let s = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("could not read {path:?}: {e}"));
        let cfg = Config::from_toml(&s)
            .unwrap_or_else(|e| panic!("config/default.toml failed to parse: {e}"));
        // The file sets explicit defaults; validate must report no warnings.
        let warnings = cfg.validate();
        assert!(
            warnings.is_empty(),
            "config/default.toml has validation warnings: {warnings:?}"
        );
    }

    #[test]
    fn ffb_gain_override() {
        let cfg = Config::from_toml("ffb_gain = 128").unwrap();
        assert_eq!(cfg.ffb_gain, 128);
        // Other fields default
        assert_eq!(cfg.axis_x, AxisConfig::default());
    }

    #[test]
    fn ffb_gain_boundary_values() {
        assert_eq!(Config::from_toml("ffb_gain = 0").unwrap().ffb_gain, 0);
        assert_eq!(Config::from_toml("ffb_gain = 255").unwrap().ffb_gain, 255);
    }

    #[test]
    fn ffb_enabled_explicit_false() {
        let cfg = Config::from_toml("ffb_enabled = false").unwrap();
        assert!(!cfg.ffb_enabled);
    }

    #[test]
    fn ffb_enabled_explicit_true() {
        let cfg = Config::from_toml("ffb_enabled = true").unwrap();
        assert!(cfg.ffb_enabled);
    }

    /// Omitting `ffb_enabled` from a config that has other fields must still
    /// produce the default `true` value via the field-level serde default.
    /// This is a different code path from the struct-level `#[serde(default)]`.
    #[test]
    fn ffb_enabled_absent_defaults_to_true() {
        let cfg = Config::from_toml("ffb_gain = 128").unwrap();
        assert!(
            cfg.ffb_enabled,
            "ffb_enabled must default to true when omitted from TOML"
        );
    }

    #[test]
    fn axis_invert() {
        let cfg = Config::from_toml("[axis_y]\ninvert = true").unwrap();
        assert!(cfg.axis_y.invert);
        assert!(!cfg.axis_x.invert);
    }

    /// Verify that `axis_rz` (Rz/twist) and `axis_slider` (throttle) parse
    /// from their correct TOML keys.  Guard against revert to the old
    /// `axis_z` / `axis_rz` naming (Issue #19).
    #[test]
    fn axis_rz_and_slider_toml_keys() {
        let cfg =
            Config::from_toml("[axis_rz]\ninvert = true\n[axis_slider]\nscale = 0.5").unwrap();
        assert!(cfg.axis_rz.invert, "axis_rz key must control Rz/twist");
        assert!(
            (cfg.axis_slider.scale - 0.5).abs() < f32::EPSILON,
            "axis_slider key must control Slider/throttle"
        );
        assert!(!cfg.axis_x.invert);
        assert!(!cfg.axis_y.invert);
    }

    #[test]
    fn apply_axis_passthrough() {
        let raw = 16000i16;
        let out = Config::apply_axis(&AxisConfig::default(), i16::MIN, i16::MAX, raw);
        assert_eq!(out, raw);
    }

    #[test]
    fn apply_axis_invert() {
        let cfg = AxisConfig {
            invert: true,
            ..Default::default()
        };
        assert_eq!(Config::apply_axis(&cfg, i16::MIN, i16::MAX, 16000), -16000);
    }

    #[test]
    fn apply_axis_dead_zone_snaps_to_zero() {
        let cfg = AxisConfig {
            dead_zone: 0.2,
            ..Default::default()
        };
        // 3000 / 32767 ≈ 0.092 < 0.1 (dead_zone/2) → 0
        assert_eq!(Config::apply_axis(&cfg, i16::MIN, i16::MAX, 3000), 0);
    }

    /// Calibration stretches a narrow hardware range to the full i16 output.
    #[test]
    fn calibration_stretches_range() {
        // Hardware only goes -16000..=16000; the midpoint 0 should map to ~0.
        let mid = CalibrationConfig::apply(0, -16000, 16000);
        assert!(mid.abs() <= 1, "midpoint should map near 0, got {mid}");
        // Hardware max should map near i16::MAX.
        let top = CalibrationConfig::apply(16000, -16000, 16000);
        assert!(
            top >= 32000,
            "hardware max should map near i16::MAX, got {top}"
        );
    }

    /// Default calibration (full i16 range) is a pass-through.
    #[test]
    fn calibration_default_is_passthrough() {
        let cal = CalibrationConfig::default();
        for &raw in &[0i16, 16000, -16000, i16::MIN, i16::MAX] {
            let out = CalibrationConfig::apply(raw, cal.x_min, cal.x_max);
            let diff = (i32::from(out) - i32::from(raw)).abs();
            assert!(
                diff <= 1,
                "default calibration should be a pass-through, got diff {diff} for raw={raw}"
            );
        }
    }

    /// Degenerate calibration (min == max) returns raw unchanged.
    #[test]
    fn calibration_degenerate_returns_raw() {
        assert_eq!(CalibrationConfig::apply(1234, 100, 100), 1234);
    }

    /// Inverted calibration (min > max) is also degenerate — returns raw unchanged.
    #[test]
    fn calibration_inverted_returns_raw() {
        assert_eq!(CalibrationConfig::apply(500, 5000, 1000), 500);
        assert_eq!(CalibrationConfig::apply(-100, 0, -100), -100);
    }

    // ── Property-based tests ─────────────────────────────────────────────────

    #[cfg(test)]
    mod props {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// With dead_zone=0, scale=1.0, invert=false and default (full-range)
            /// calibration the output must be within 1 LSB of the input.
            #[test]
            fn passthrough_within_one_lsb(raw in i16::MIN..=i16::MAX) {
                let out = Config::apply_axis(&AxisConfig::default(), i16::MIN, i16::MAX, raw);
                let diff = (i32::from(out) - i32::from(raw)).abs();
                prop_assert!(diff <= 1, "passthrough drift {diff} for raw={raw}");
            }

            /// Applying invert twice must equal the single passthrough result.
            #[test]
            fn double_invert_is_identity(raw in i16::MIN..=i16::MAX) {
                let invert_cfg = AxisConfig {
                    invert: true,
                    ..AxisConfig::default()
                };
                let once = Config::apply_axis(&invert_cfg, i16::MIN, i16::MAX, raw);
                let twice = Config::apply_axis(&invert_cfg, i16::MIN, i16::MAX, once);
                let passthrough = Config::apply_axis(&AxisConfig::default(), i16::MIN, i16::MAX, raw);

                // Allow ±1 for the two normalizations.
                let diff = (i32::from(twice) - i32::from(passthrough)).abs();
                prop_assert!(
                    diff <= 1,
                    "double-invert drift {diff} for raw={raw}",
                );
            }

            /// The output is always clamped to the signed i16 range.
            #[test]
            fn output_clamped_to_i16_bounds(
                raw in i16::MIN..=i16::MAX,
                scale in 1.0f32..=8.0f32, // large scales exercise the clamp
                invert in proptest::bool::ANY,
            ) {
                let cfg = AxisConfig { invert, dead_zone: 0.0, scale, ..AxisConfig::default() };
                let out = i32::from(Config::apply_axis(&cfg, i16::MIN, i16::MAX, raw));
                prop_assert!(
                    i16::try_from(out).is_ok(),
                    "output {} outside i16 bounds for raw={}, scale={}",
                    out, raw, scale,
                );
            }

            /// Any raw value strictly inside the dead zone must map to zero.
            #[test]
            fn dead_zone_snaps_to_zero(
                dead_zone in 0.1f32..=1.0f32,
                scale in 0.0f32..=2.0f32,
                invert in proptest::bool::ANY,
            ) {
                // clamp guarantees the value is within i16 bounds before the cast.
                #[expect(clippy::cast_possible_truncation, reason = "floor of bounded f32 always fits i16 here")]
                let threshold = ((dead_zone / 2.0) * 32767.0).floor() as i16;
                prop_assume!(threshold > 0);

                for &raw in &[threshold - 1, -(threshold - 1)] {
                    let cfg = AxisConfig { invert, dead_zone, scale, ..AxisConfig::default() };
                    let out = Config::apply_axis(&cfg, i16::MIN, i16::MAX, raw);
                    prop_assert_eq!(
                        out, 0,
                        "raw={} inside dead_zone={} must snap to 0",
                        raw, dead_zone,
                    );
                }
            }

            /// Calibration output is always within i16 bounds regardless of
            /// cal range and raw input.
            #[test]
            fn calibration_output_clamped(
                raw in i16::MIN..=i16::MAX,
                cal_min in i16::MIN..=i16::MAX,
                cal_max in i16::MIN..=i16::MAX,
            ) {
                let out = i32::from(CalibrationConfig::apply(raw, cal_min, cal_max));
                prop_assert!(
                    i16::try_from(out).is_ok(),
                    "calibration output {out} out of i16 bounds"
                );
            }

            /// All response curves are monotonically non-decreasing on [0, 1].
            ///
            /// x1 ≤ x2 in [0, 1] → curve(x1) ≤ curve(x2).
            /// The 1e-5 tolerance covers floating-point rounding on adjacent values.
            #[test]
            fn curve_monotone_on_positive_range(
                x1 in 0.0f32..=1.0f32,
                x2 in 0.0f32..=1.0f32,
            ) {
                let (lo, hi) = if x1 <= x2 { (x1, x2) } else { (x2, x1) };
                for curve in [
                    ResponseCurve::Linear,
                    ResponseCurve::Quadratic,
                    ResponseCurve::Cubic,
                    ResponseCurve::SCurve,
                ] {
                    let out_lo = curve.apply(lo);
                    let out_hi = curve.apply(hi);
                    prop_assert!(
                        out_lo <= out_hi + 1e-5,
                        "{curve:?}: f({lo}) = {out_lo} > f({hi}) = {out_hi} (not monotone)"
                    );
                }
            }
        }
    }

    // ── ResponseCurve tests ───────────────────────────────────────────────────

    /// Linear curve is a no-op.
    #[test]
    fn curve_linear_passthrough() {
        for v in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
            let out = ResponseCurve::Linear.apply(v);
            assert!((out - v).abs() < f32::EPSILON, "linear({v}) = {out}");
        }
    }

    /// All curves map 0 → 0 and ±1 → ±1.
    #[test]
    fn curve_boundary_values() {
        for curve in [
            ResponseCurve::Linear,
            ResponseCurve::Quadratic,
            ResponseCurve::Cubic,
            ResponseCurve::SCurve,
        ] {
            let zero = curve.apply(0.0);
            assert!(zero.abs() < f32::EPSILON, "{curve:?}(0) = {zero}");
            let one = curve.apply(1.0);
            assert!((one - 1.0).abs() < f32::EPSILON, "{curve:?}(1) = {one}");
            let neg = curve.apply(-1.0);
            assert!((neg + 1.0).abs() < f32::EPSILON, "{curve:?}(-1) = {neg}");
        }
    }

    /// All curves are sign-preserving.
    #[test]
    fn curve_sign_preserving() {
        let x = 0.3f32;
        for curve in [
            ResponseCurve::Linear,
            ResponseCurve::Quadratic,
            ResponseCurve::Cubic,
            ResponseCurve::SCurve,
        ] {
            let pos = curve.apply(x);
            let neg = curve.apply(-x);
            assert!(pos > 0.0, "{curve:?}({x}) should be positive, got {pos}");
            assert!(neg < 0.0, "{curve:?}(-{x}) should be negative, got {neg}");
            assert!(
                (pos + neg).abs() < 1e-6,
                "{curve:?}: f(x) + f(-x) should be 0, got {}",
                pos + neg
            );
        }
    }

    /// Quadratic is gentler than linear near the centre (x=0.3: 0.3² = 0.09 < 0.3).
    #[test]
    fn curve_quadratic_gentler_near_centre() {
        let x = 0.3f32;
        let linear = ResponseCurve::Linear.apply(x);
        let quad = ResponseCurve::Quadratic.apply(x);
        assert!(quad < linear, "quadratic should be gentler near centre");
    }

    /// S-curve (smoothstep) has zero derivative at x=0 and x=1, so it produces
    /// the same output as linear at x=0.5 and converges to 1.0 quickly near the
    /// extremes, matching cubic in gentleness near centre.
    ///
    /// Specifically: at x=0.1, s-curve > cubic (smoothstep gentler decay near 0
    /// than x³), showing the "soft centre" property is less extreme than cubic.
    #[test]
    fn curve_scurve_gentler_than_cubic_near_centre() {
        let x = 0.1f32;
        let cubic = ResponseCurve::Cubic.apply(x);
        let scurve = ResponseCurve::SCurve.apply(x);
        // cubic at 0.1 = 0.001; scurve at 0.1 = 3(0.01)-2(0.001) = 0.028
        // scurve > cubic means scurve is LESS extreme (closer to linear) near centre
        assert!(
            scurve > cubic,
            "s-curve should be closer to linear than cubic near centre: scurve={scurve}, cubic={cubic}"
        );
    }

    /// `curve` TOML field round-trips correctly.
    #[test]
    fn curve_toml_roundtrip() {
        let cfg = Config::from_toml("[axis_x]\ncurve = \"s-curve\"").unwrap();
        assert_eq!(cfg.axis_x.curve, ResponseCurve::SCurve);
        let cfg2 = Config::from_toml("[axis_y]\ncurve = \"quadratic\"").unwrap();
        assert_eq!(cfg2.axis_y.curve, ResponseCurve::Quadratic);
    }

    /// `apply_axis` with s-curve produces a smaller absolute value than linear at mid-range.
    #[test]
    fn apply_axis_scurve_gentler_than_linear_at_mid() {
        let raw = 16000i16; // about half deflection
        let linear_cfg = AxisConfig {
            curve: ResponseCurve::Linear,
            ..AxisConfig::default()
        };
        let scurve_cfg = AxisConfig {
            curve: ResponseCurve::SCurve,
            ..AxisConfig::default()
        };
        let linear_out = Config::apply_axis(&linear_cfg, i16::MIN, i16::MAX, raw).abs();
        let scurve_out = Config::apply_axis(&scurve_cfg, i16::MIN, i16::MAX, raw).abs();
        assert!(
            scurve_out < linear_out,
            "s-curve should produce smaller output at mid deflection: linear={linear_out} scurve={scurve_out}"
        );
    }

    // ── Smoothing config tests ────────────────────────────────────────────────

    /// smoothing field parses from TOML.
    #[test]
    fn smoothing_toml_field() {
        let cfg = Config::from_toml("[axis_x]\nsmoothing = 5").unwrap();
        assert_eq!(cfg.axis_x.smoothing, 5);
    }

    /// Default smoothing is 1 (no smoothing).
    #[test]
    fn smoothing_default_is_one() {
        assert_eq!(AxisConfig::default().smoothing, 1);
    }

    // ── Validation tests ──────────────────────────────────────────────────────

    /// Valid config produces no warnings.
    #[test]
    fn validate_clean_config_no_warnings() {
        assert!(Config::default().validate().is_empty());
    }

    /// Smoothing out of range is flagged.
    #[test]
    fn validate_smoothing_out_of_range() {
        let mut cfg = Config::default();
        cfg.axis_x.smoothing = 99;
        let warnings = cfg.validate();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("smoothing"),
            "warning should mention 'smoothing': {}",
            warnings[0]
        );
        assert!(
            warnings[0].contains("99"),
            "warning should include the bad value: {}",
            warnings[0]
        );
    }

    /// Dead zone out of range is flagged.
    #[test]
    fn validate_dead_zone_out_of_range() {
        let mut cfg = Config::default();
        cfg.axis_y.dead_zone = 0.8;
        let warnings = cfg.validate();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("dead_zone"));
    }

    /// Multiple axes with multiple bad fields all produce warnings.
    #[test]
    fn validate_multiple_warnings() {
        let mut cfg = Config::default();
        cfg.axis_x.smoothing = 0;
        cfg.axis_rz.dead_zone = -0.1;
        let warnings = cfg.validate();
        assert_eq!(warnings.len(), 2);
    }

    // ── generate_toml tests ───────────────────────────────────────────────────

    /// Generated TOML parses back to a default Config.
    #[test]
    fn generate_toml_round_trips_to_default() {
        let toml = Config::generate_toml();
        let cfg = Config::from_toml(&toml).expect("generated TOML must parse without errors");
        assert_eq!(cfg, Config::default());
    }

    /// Generated TOML passes validation with no warnings.
    #[test]
    fn generate_toml_is_valid() {
        let toml = Config::generate_toml();
        let cfg = Config::from_toml(&toml).unwrap();
        assert!(
            cfg.validate().is_empty(),
            "generated config must have no validation warnings"
        );
    }

    /// Generated TOML contains inline comments for each section.
    #[test]
    fn generate_toml_has_comments() {
        let toml = Config::generate_toml();
        assert!(toml.contains("# Sidewinder"), "must have header comment");
        assert!(
            toml.contains("[calibration]"),
            "must have calibration section"
        );
        assert!(toml.contains("[axis_x]"), "must have axis_x section");
        assert!(toml.contains("smoothing"), "must document smoothing");
        assert!(toml.contains("dead_zone"), "must document dead_zone");
    }

    // ── run_generate / run_validate tests ─────────────────────────────────────

    /// `run_generate` writes a valid config file when the path does not exist.
    #[test]
    fn run_generate_writes_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        run_generate(&path).expect("run_generate must succeed");
        assert!(path.exists(), "config file must be created");
        let content = std::fs::read_to_string(&path).unwrap();
        Config::from_toml(&content).expect("generated file must be valid TOML");
    }

    /// `run_generate` does not overwrite an existing file.
    #[test]
    fn run_generate_does_not_overwrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# sentinel\n").unwrap();
        run_generate(&path).expect("run_generate must not error on existing file");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("# sentinel"),
            "existing file must not be overwritten"
        );
    }

    /// `run_generate` creates intermediate directories.
    #[test]
    fn run_generate_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a").join("b").join("config.toml");
        run_generate(&path).expect("run_generate must create parent directories");
        assert!(path.exists());
    }

    /// `run_validate` returns `Ok(true)` for a valid config file.
    #[test]
    fn run_validate_clean_config_returns_true() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, Config::generate_toml()).unwrap();
        let ok = run_validate(&path).expect("validate must not error");
        assert!(ok, "clean config must return true");
    }

    /// `run_validate` returns `Ok(false)` for a config with warnings.
    #[test]
    fn run_validate_bad_config_returns_false() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[axis_x]\nsmoothing = 50\n").unwrap();
        let ok = run_validate(&path).expect("validate must not error on parseable config");
        assert!(!ok, "config with warnings must return false");
    }

    /// `run_validate` errors on a missing file.
    #[test]
    fn run_validate_missing_file_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.toml");
        assert!(
            run_validate(&path).is_err(),
            "validate must error when file does not exist"
        );
    }

    // ── Button map config tests ───────────────────────────────────────────────

    /// Default button map is identity (no remapping).
    #[test]
    fn button_map_default_is_identity() {
        let map = ButtonMapConfig::default();
        for phys in 0..9u8 {
            assert_eq!(map.virtual_for(phys), phys);
        }
    }

    /// Button map TOML round-trips correctly.
    #[test]
    fn button_map_toml_roundtrip() {
        // In the TOML, [buttons] keys are 0-based physical button indices.
        let toml = "[buttons]\n0 = 1\n1 = 0\n";
        let cfg = Config::from_toml(toml).unwrap();
        assert_eq!(cfg.buttons.virtual_for(0), 1, "button 0 → virtual 1");
        assert_eq!(cfg.buttons.virtual_for(1), 0, "button 1 → virtual 0");
        assert_eq!(cfg.buttons.virtual_for(2), 2, "button 2 unchanged");
    }

    /// Duplicate virtual button target triggers a validation warning.
    #[test]
    fn validate_button_map_duplicate_warns() {
        // Use a virtual target (12) outside the physical range (0–8) so no
        // identity mapping for buttons 0–8 accidentally creates a second hit.
        let cfg = Config {
            buttons: ButtonMapConfig::from_pairs(&[(0, 12), (1, 12)]),
            ..Default::default()
        };
        let warnings = cfg.validate();
        assert_eq!(warnings.len(), 1, "expected exactly one duplicate warning");
        assert!(
            warnings[0].contains("shadow"),
            "warning should mention shadowing: {}",
            warnings[0]
        );
    }

    // ── Hat config tests ──────────────────────────────────────────────────────

    /// Default hat config has `as_buttons` = false.
    #[test]
    fn hat_config_default_is_pov_mode() {
        assert!(!HatConfig::default().as_buttons);
    }

    /// Hat `as_buttons` TOML field parses correctly.
    #[test]
    fn hat_as_buttons_toml() {
        let cfg = Config::from_toml("[hat]\nas_buttons = true").unwrap();
        assert!(cfg.hat.as_buttons);
    }

    /// Hat `button_for` returns the correct index for each direction.
    #[test]
    fn hat_button_for_all_directions() {
        use sidewinder_hid::input::PovDirection;
        let hat = HatConfig::default();
        assert_eq!(hat.button_for(PovDirection::North), Some(9));
        assert_eq!(hat.button_for(PovDirection::NorthEast), Some(10));
        assert_eq!(hat.button_for(PovDirection::East), Some(11));
        assert_eq!(hat.button_for(PovDirection::SouthEast), Some(12));
        assert_eq!(hat.button_for(PovDirection::South), Some(13));
        assert_eq!(hat.button_for(PovDirection::SouthWest), Some(14));
        assert_eq!(hat.button_for(PovDirection::West), Some(15));
        assert_eq!(hat.button_for(PovDirection::NorthWest), Some(16));
        assert_eq!(hat.button_for(PovDirection::Center), None);
    }

    // ── Layer config tests ────────────────────────────────────────────────────

    /// Default layer config is disabled (`shift_button` = 0).
    #[test]
    fn layer_config_default_disabled() {
        assert_eq!(LayerConfig::default().shift_button, 0);
    }

    /// Layer config TOML parses `shift_button` and layer buttons.
    #[test]
    fn layer_config_toml() {
        let toml = "[layer]\nshift_button = 6\n[layer.buttons]\n0 = 9\n";
        let cfg = Config::from_toml(toml).unwrap();
        assert_eq!(cfg.layer.shift_button, 6);
        assert_eq!(cfg.layer.buttons.virtual_for(0), 9);
        assert_eq!(cfg.layer.buttons.virtual_for(1), 1); // identity for unmapped
    }

    /// Layer 2 duplicate button map triggers a warning when `shift_button` is set.
    #[test]
    fn validate_layer_button_map_duplicate_warns() {
        let cfg = Config {
            layer: LayerConfig {
                shift_button: 6,
                buttons: ButtonMapConfig::from_pairs(&[(0, 12), (1, 12)]),
            },
            ..Default::default()
        };
        let warnings = cfg.validate();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("layer.buttons"));
    }

    /// Layer 2 duplicate warnings are suppressed when `shift_button` = 0 (disabled).
    #[test]
    fn validate_layer_button_map_no_warning_when_disabled() {
        let cfg = Config {
            layer: LayerConfig {
                shift_button: 0, // disabled
                buttons: ButtonMapConfig::from_pairs(&[(0, 12), (1, 12)]),
            },
            ..Default::default()
        };
        // No warnings — layer is disabled, so the map is never used.
        assert!(cfg.validate().is_empty());
    }

    /// `shift_button` beyond the physical button count triggers a warning.
    #[test]
    fn validate_shift_button_out_of_range() {
        let cfg = Config {
            layer: LayerConfig {
                shift_button: 10, // max is 9
                buttons: ButtonMapConfig::default(),
            },
            ..Default::default()
        };
        let warnings = cfg.validate();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("shift_button"),
            "warning should mention shift_button: {}",
            warnings[0]
        );
        assert!(
            warnings[0].contains("10"),
            "warning should include the bad value: {}",
            warnings[0]
        );
    }

    /// Hat button index >= 32 triggers a warning when `as_buttons` is enabled.
    #[test]
    fn validate_hat_button_out_of_range() {
        let cfg = Config {
            hat: HatConfig {
                as_buttons: true,
                north_west: 32, // out of range (max is 31)
                ..Default::default()
            },
            ..Default::default()
        };
        let warnings = cfg.validate();
        assert_eq!(
            warnings.len(),
            1,
            "expected exactly one out-of-range warning"
        );
        assert!(warnings[0].contains("north_west"));
        assert!(warnings[0].contains("32"));
    }

    /// Hat out-of-range warning is suppressed when `as_buttons` is false.
    #[test]
    fn validate_hat_out_of_range_suppressed_in_pov_mode() {
        let cfg = Config {
            hat: HatConfig {
                as_buttons: false, // pov mode — button indices irrelevant
                north_west: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        // No warnings — hat is in POV mode, button indices are unused.
        assert!(cfg.validate().is_empty());
    }
}
