//! Force feedback effect types and HID output report construction.
//!
//! Covers all 12 HID PID (Physical Interface Device) effect types:
//! Constant Force, Ramp, Square, Sine, Triangle, Sawtooth Up/Down,
//! Spring, Damper, Inertia, Friction, and Custom Force.

// ── Primitive types ──────────────────────────────────────────────────────────

/// Waveform for periodic effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Square,
    Sine,
    Triangle,
    SawtoothUp,
    SawtoothDown,
}

/// Conditional effect type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionType {
    Spring,
    Damper,
    Inertia,
    Friction,
}

/// Attack / fade envelope applied to an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FfbEnvelope {
    /// Level at the start of attack (0 – 32 767).
    pub attack_level: u16,
    /// Duration of the attack phase in ms.
    pub attack_time_ms: u16,
    /// Level at the start of fade (0 – 32 767).
    pub fade_level: u16,
    /// Duration of the fade phase in ms.
    pub fade_time_ms: u16,
}

/// Per-axis parameters for a conditional effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionParams {
    /// Centre-point offset (signed).
    pub center_point_offset: i16,
    /// Coefficient for positive deflection.
    pub positive_coefficient: i16,
    /// Coefficient for negative deflection.
    pub negative_coefficient: i16,
    /// Maximum force on the positive side (0 – 32 767).
    pub positive_saturation: u16,
    /// Maximum force on the negative side (0 – 32 767).
    pub negative_saturation: u16,
    /// Dead-band width around the centre (0 – 32 767).
    pub dead_band: u16,
}

// ── Effect params ─────────────────────────────────────────────────────────────

/// Type-specific parameters for a force feedback effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FfbEffectParams {
    /// Constant directional force.
    ConstantForce { magnitude: i16 },
    /// Force that ramps linearly from `start` to `end`.
    Ramp { start: i16, end: i16 },
    /// Periodic waveform effect.
    Periodic {
        waveform: Waveform,
        /// Peak-to-peak magnitude (0 – 32 767).
        magnitude: i16,
        /// DC offset applied to the waveform.
        offset: i16,
        /// Waveform period in ms.
        period_ms: u16,
        /// Phase shift (0 – 35 999, in hundredths of a degree).
        phase: u16,
    },
    /// Conditional effect (spring, damper, inertia, or friction).
    Condition {
        condition_type: ConditionType,
        /// One entry per axis (index 0 = X, index 1 = Y).
        conditions: [ConditionParams; 2],
    },
    /// Custom force data (sample array sent separately).
    CustomForce {
        sample_count: u16,
        sample_period_ms: u16,
    },
}

// ── Top-level effect ──────────────────────────────────────────────────────────

/// A complete force feedback effect definition.
///
/// Corresponds to a HID PID "Set Effect" report plus any type-specific
/// parameter reports and an optional envelope report.
#[derive(Debug, Clone, PartialEq)]
pub struct FfbEffect {
    /// Effect slot index (1-based, as per HID PID spec; range 1 – 40).
    pub effect_block_index: u8,
    /// Duration in ms.  Use `0xFFFF` for an infinite effect.
    pub duration_ms: u16,
    /// Effect gain (0 – 255).
    pub gain: u8,
    /// Direction in hundredths of a degree (0 – 35 999).
    pub direction: u16,
    /// Delay before the effect starts, in ms.
    pub start_delay_ms: u16,
    /// Button that triggers the effect (0-based), or `None` for no trigger.
    pub trigger_button: Option<u8>,
    /// Re-trigger interval in ms (0 = no auto-repeat).
    pub trigger_repeat_ms: u16,
    /// Optional attack / fade envelope.
    pub envelope: Option<FfbEnvelope>,
    /// Type-specific parameters.
    pub params: FfbEffectParams,
}

// ── Operation commands ────────────────────────────────────────────────────────

/// Commands sent to the device to control effect playback and global state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfbOperation {
    /// Start playing an effect (stops others if `solo` is true).
    Start { effect_block_index: u8, solo: bool },
    /// Stop a specific effect.
    Stop { effect_block_index: u8 },
    /// Stop all active effects.
    StopAll,
    /// Free an effect slot.
    Free { effect_block_index: u8 },
    /// Free all effect slots.
    FreeAll,
    /// Set the device-level gain (0 – 255).
    SetGain { gain: u8 },
    /// Enable or disable the device actuators (physical motors).
    ///
    /// Sends a HID PID Device Control report (report ID `0x1A`).
    /// `true` → Enable Actuators (`0x01`), `false` → Disable Actuators (`0x02`).
    EnableActuators { enable: bool },
}

// ── Report builders ───────────────────────────────────────────────────────────

/// HID PID report ID for the Device Gain output report.
///
/// Callers that need to identify or filter Device Gain reports (e.g. to scale
/// the gain byte before forwarding) should import this constant rather than
/// hardcoding `0x0D`.
pub const REPORT_ID_DEVICE_GAIN: u8 = 0x0D;

/// HID PID report IDs for output reports sent to the physical device.
mod report_id {
    pub const SET_EFFECT: u8 = 0x01;
    pub const SET_ENVELOPE: u8 = 0x02;
    pub const SET_CONDITION: u8 = 0x03;
    pub const SET_PERIODIC: u8 = 0x04;
    pub const SET_CONSTANT_FORCE: u8 = 0x05;
    pub const SET_RAMP_FORCE: u8 = 0x06;
    pub const EFFECT_OPERATION: u8 = 0x0A;
    pub const DEVICE_GAIN: u8 = super::REPORT_ID_DEVICE_GAIN;
    /// Device Control report — enables or disables actuators.
    pub const DEVICE_CONTROL: u8 = 0x1A;
}

/// Build HID output report bytes for an [`FfbOperation`].
///
/// Returns the bytes to pass to [`crate::hid_transport::HidTransport::write_output_report`].
#[must_use]
pub fn build_operation_report(op: FfbOperation) -> Vec<u8> {
    match op {
        FfbOperation::Start {
            effect_block_index,
            solo,
        } => vec![
            report_id::EFFECT_OPERATION,
            effect_block_index,
            if solo { 0x02 } else { 0x01 }, // 1 = OpStart, 2 = OpStartSolo
            0x01,                           // loop count = 1
        ],
        FfbOperation::Stop { effect_block_index } => {
            vec![report_id::EFFECT_OPERATION, effect_block_index, 0x03, 0x00]
        }
        FfbOperation::StopAll => vec![report_id::EFFECT_OPERATION, 0xFF, 0x03, 0x00],
        FfbOperation::Free { effect_block_index } => {
            vec![report_id::EFFECT_OPERATION, effect_block_index, 0x00, 0x00]
        }
        FfbOperation::FreeAll => vec![report_id::EFFECT_OPERATION, 0xFF, 0x00, 0x00],
        FfbOperation::SetGain { gain } => vec![report_id::DEVICE_GAIN, gain],
        FfbOperation::EnableActuators { enable } => {
            // HID PID Device Control: 0x01 = Enable Actuators, 0x02 = Disable Actuators.
            vec![report_id::DEVICE_CONTROL, if enable { 0x01 } else { 0x02 }]
        }
    }
}

/// Build the sequence of HID output reports needed to define a complete [`FfbEffect`].
///
/// Returns one or more report byte-vectors in the order they must be sent:
/// 1. Set Effect report (mandatory)
/// 2. Type-specific parameter report(s)
/// 3. Set Envelope report (if an envelope is set)
#[must_use]
pub fn build_effect_reports(effect: &FfbEffect) -> Vec<Vec<u8>> {
    let mut reports = Vec::new();
    reports.push(build_set_effect_report(effect));
    push_type_reports(effect, &mut reports);
    if let Some(env) = effect.envelope {
        reports.push(build_envelope_report(effect.effect_block_index, env));
    }
    reports
}

/// Build the mandatory Set Effect (0x01) report.
fn build_set_effect_report(effect: &FfbEffect) -> Vec<u8> {
    let effect_type = effect_type_byte(&effect.params);
    let trigger_btn = effect.trigger_button.map_or(0xFF, |b| b + 1); // 1-based, 0xFF = none
    let [dur_lo, dur_hi] = effect.duration_ms.to_le_bytes();
    let [rep_lo, rep_hi] = effect.trigger_repeat_ms.to_le_bytes();
    let [del_lo, del_hi] = effect.start_delay_ms.to_le_bytes();
    let [dir_lo, dir_hi] = effect.direction.to_le_bytes();
    vec![
        report_id::SET_EFFECT,
        effect.effect_block_index,
        effect_type,
        dur_lo,
        dur_hi,
        rep_lo,
        rep_hi,
        0x00,
        0x00, // sample period (unused for most effects)
        del_lo,
        del_hi,
        effect.gain,
        trigger_btn,
        0x03, // axes enable: X and Y
        0x01, // direction enable
        dir_lo,
        dir_hi,
    ]
}

/// Append the type-specific parameter report(s) for an effect.
fn push_type_reports(effect: &FfbEffect, reports: &mut Vec<Vec<u8>>) {
    let idx = effect.effect_block_index;
    match &effect.params {
        FfbEffectParams::ConstantForce { magnitude } => {
            let [lo, hi] = magnitude.to_le_bytes();
            reports.push(vec![report_id::SET_CONSTANT_FORCE, idx, lo, hi]);
        }
        FfbEffectParams::Ramp { start, end } => {
            let [s_lo, s_hi] = start.to_le_bytes();
            let [e_lo, e_hi] = end.to_le_bytes();
            reports.push(vec![report_id::SET_RAMP_FORCE, idx, s_lo, s_hi, e_lo, e_hi]);
        }
        FfbEffectParams::Periodic {
            waveform: _,
            magnitude,
            offset,
            period_ms,
            phase,
        } => {
            let [m_lo, m_hi] = magnitude.to_le_bytes();
            let [o_lo, o_hi] = offset.to_le_bytes();
            let [p_lo, p_hi] = period_ms.to_le_bytes();
            let [ph_lo, ph_hi] = phase.to_le_bytes();
            reports.push(vec![
                report_id::SET_PERIODIC,
                idx,
                m_lo,
                m_hi,
                o_lo,
                o_hi,
                p_lo,
                p_hi,
                ph_lo,
                ph_hi,
            ]);
        }
        FfbEffectParams::Condition {
            condition_type: _,
            conditions,
        } => {
            for (axis_idx, cond) in conditions.iter().enumerate() {
                reports.push(build_condition_report(idx, axis_idx, cond));
            }
        }
        FfbEffectParams::CustomForce { .. } => {
            // Custom force data is streamed separately — no parameter report here.
        }
    }
}

/// Build a Set Condition (0x03) report for one axis.
fn build_condition_report(
    effect_block_index: u8,
    axis_idx: usize,
    cond: &ConditionParams,
) -> Vec<u8> {
    let [cp_lo, cp_hi] = cond.center_point_offset.to_le_bytes();
    let [pc_lo, pc_hi] = cond.positive_coefficient.to_le_bytes();
    let [nc_lo, nc_hi] = cond.negative_coefficient.to_le_bytes();
    let [ps_lo, ps_hi] = cond.positive_saturation.to_le_bytes();
    let [ns_lo, ns_hi] = cond.negative_saturation.to_le_bytes();
    let [db_lo, db_hi] = cond.dead_band.to_le_bytes();
    // axis_idx is always 0 or 1 (the conditions array has exactly 2 entries)
    #[expect(
        clippy::cast_possible_truncation,
        reason = "axis_idx is always 0 or 1 (two-element array), fits in u8"
    )]
    let axis = axis_idx as u8;
    vec![
        report_id::SET_CONDITION,
        effect_block_index,
        axis,
        cp_lo,
        cp_hi,
        pc_lo,
        pc_hi,
        nc_lo,
        nc_hi,
        ps_lo,
        ps_hi,
        ns_lo,
        ns_hi,
        db_lo,
        db_hi,
    ]
}

/// Build the Set Envelope (0x02) report.
fn build_envelope_report(effect_block_index: u8, env: FfbEnvelope) -> Vec<u8> {
    let [al_lo, al_hi] = env.attack_level.to_le_bytes();
    let [fl_lo, fl_hi] = env.fade_level.to_le_bytes();
    let [at_lo, at_hi] = env.attack_time_ms.to_le_bytes();
    let [ft_lo, ft_hi] = env.fade_time_ms.to_le_bytes();
    vec![
        report_id::SET_ENVELOPE,
        effect_block_index,
        al_lo,
        al_hi,
        fl_lo,
        fl_hi,
        at_lo,
        at_hi,
        ft_lo,
        ft_hi,
    ]
}

fn effect_type_byte(params: &FfbEffectParams) -> u8 {
    match params {
        FfbEffectParams::ConstantForce { .. } => 0x01,
        FfbEffectParams::Ramp { .. } => 0x02,
        FfbEffectParams::Periodic { waveform, .. } => match waveform {
            Waveform::Square => 0x03,
            Waveform::Sine => 0x04,
            Waveform::Triangle => 0x05,
            Waveform::SawtoothUp => 0x06,
            Waveform::SawtoothDown => 0x07,
        },
        FfbEffectParams::Condition { condition_type, .. } => match condition_type {
            ConditionType::Spring => 0x08,
            ConditionType::Damper => 0x09,
            ConditionType::Inertia => 0x0A,
            ConditionType::Friction => 0x0B,
        },
        FfbEffectParams::CustomForce { .. } => 0x0C,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_constant_effect() -> FfbEffect {
        FfbEffect {
            effect_block_index: 1,
            duration_ms: 1000,
            gain: 200,
            direction: 180,
            start_delay_ms: 0,
            trigger_button: None,
            trigger_repeat_ms: 0,
            envelope: None,
            params: FfbEffectParams::ConstantForce { magnitude: 5000 },
        }
    }

    #[test]
    fn operation_report_start() {
        let bytes = build_operation_report(FfbOperation::Start {
            effect_block_index: 3,
            solo: false,
        });
        assert_eq!(bytes[0], 0x0A); // EFFECT_OPERATION report ID
        assert_eq!(bytes[1], 3); // block index
        assert_eq!(bytes[2], 0x01); // OpStart
    }

    #[test]
    fn operation_report_solo_start() {
        let bytes = build_operation_report(FfbOperation::Start {
            effect_block_index: 1,
            solo: true,
        });
        assert_eq!(bytes[2], 0x02); // OpStartSolo
    }

    #[test]
    fn operation_report_set_gain() {
        let bytes = build_operation_report(FfbOperation::SetGain { gain: 128 });
        assert_eq!(bytes[0], 0x0D); // DEVICE_GAIN
        assert_eq!(bytes[1], 128);
    }

    #[test]
    fn operation_report_enable_actuators_true() {
        // HID PID: report ID 0x1A, sub-command 0x01 = Enable Actuators.
        let bytes = build_operation_report(FfbOperation::EnableActuators { enable: true });
        assert_eq!(bytes.len(), 2);
        assert_eq!(bytes[0], 0x1A); // DEVICE_CONTROL report ID
        assert_eq!(bytes[1], 0x01); // Enable Actuators
    }

    #[test]
    fn operation_report_enable_actuators_false() {
        // HID PID: report ID 0x1A, sub-command 0x02 = Disable Actuators.
        // 0x01 and 0x02 are physically opposite commands — this must not be 0x01.
        let bytes = build_operation_report(FfbOperation::EnableActuators { enable: false });
        assert_eq!(bytes.len(), 2);
        assert_eq!(bytes[0], 0x1A); // DEVICE_CONTROL report ID
        assert_eq!(bytes[1], 0x02); // Disable Actuators
    }

    #[test]
    fn constant_force_produces_two_reports() {
        let effect = minimal_constant_effect();
        let reports = build_effect_reports(&effect);
        // Set Effect + Set Constant Force = 2 reports
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0][0], 0x01); // SET_EFFECT
        assert_eq!(reports[0][1], 1); // block index
        assert_eq!(reports[0][2], 0x01); // effect type = ConstantForce
        assert_eq!(reports[1][0], 0x05); // SET_CONSTANT_FORCE
    }

    #[test]
    fn constant_force_with_envelope_produces_three_reports() {
        let mut effect = minimal_constant_effect();
        effect.envelope = Some(FfbEnvelope {
            attack_level: 0,
            attack_time_ms: 200,
            fade_level: 0,
            fade_time_ms: 200,
        });
        let reports = build_effect_reports(&effect);
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[2][0], 0x02); // SET_ENVELOPE
    }

    #[test]
    fn sine_effect_type_byte() {
        let params = FfbEffectParams::Periodic {
            waveform: Waveform::Sine,
            magnitude: 8000,
            offset: 0,
            period_ms: 200,
            phase: 0,
        };
        assert_eq!(effect_type_byte(&params), 0x04);
    }

    #[test]
    fn spring_condition_produces_two_param_reports() {
        let cond = ConditionParams {
            center_point_offset: 0,
            positive_coefficient: 3000,
            negative_coefficient: 3000,
            positive_saturation: 10000,
            negative_saturation: 10000,
            dead_band: 0,
        };
        let effect = FfbEffect {
            effect_block_index: 2,
            duration_ms: 0xFFFF,
            gain: 255,
            direction: 0,
            start_delay_ms: 0,
            trigger_button: None,
            trigger_repeat_ms: 0,
            envelope: None,
            params: FfbEffectParams::Condition {
                condition_type: ConditionType::Spring,
                conditions: [cond, cond],
            },
        };
        let reports = build_effect_reports(&effect);
        // Set Effect + 2 × Set Condition (one per axis)
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[1][0], 0x03); // SET_CONDITION axis 0
        assert_eq!(reports[1][2], 0); // axis index = 0
        assert_eq!(reports[2][2], 1); // axis index = 1
    }
}
