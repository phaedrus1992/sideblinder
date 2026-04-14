//! Shared ratatui rendering helpers for all diagnostic modes.

use std::collections::VecDeque;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
};

use sidewinder_hid::input::{InputState, PovDirection};

// ── Axis bar ──────────────────────────────────────────────────────────────────

/// Render a single axis as a labelled gauge bar with a numeric value label.
///
/// The title is formatted as `"<label>  <value>"` so the raw i16 is visible
/// alongside the proportional bar (issue #22).
pub fn render_axis(f: &mut Frame, area: Rect, label: &str, value: i16) {
    // Map i16 (-32768..32767) to 0..100% for the gauge.
    // i32::from is lossless; value+32768 is always non-negative (0..=65535),
    // and the result of *100/65535 is always in 0..=100, safely fitting in u16.
    let pct_u32 = (i32::from(value) + 32768).cast_unsigned() * 100 / 65535_u32;
    // pct_u32 is always ≤ 100, well within u16::MAX.
    #[expect(clippy::cast_possible_truncation, reason = "pct_u32 is always ≤ 100")]
    let pct = pct_u32 as u16;
    let colour = if value.abs() < 1000 {
        Color::Green
    } else if value.abs() < 20000 {
        Color::Yellow
    } else {
        Color::Red
    };

    let title = format!("{label}  {value:+}");
    let gauge = Gauge::default()
        .block(Block::default().title(title).borders(Borders::ALL))
        .gauge_style(Style::default().fg(colour))
        .percent(pct);

    f.render_widget(gauge, area);
}

// ── Button row ────────────────────────────────────────────────────────────────

/// Render the 9-button bitfield as a row of coloured spans.
pub fn render_buttons(f: &mut Frame, area: Rect, buttons: u16) {
    let spans: Vec<Span> = (0..9u8)
        .map(|i| {
            let pressed = buttons & (1 << i) != 0;
            let label = format!(" B{} ", i + 1);
            if pressed {
                Span::styled(
                    label,
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(label, Style::default().fg(Color::DarkGray))
            }
        })
        .collect();

    let para = Paragraph::new(Line::from(spans))
        .block(Block::default().title("Buttons").borders(Borders::ALL));
    f.render_widget(para, area);
}

// ── POV hat ───────────────────────────────────────────────────────────────────

/// Render the hat switch as a 3×3 compass rose.
#[expect(
    clippy::many_single_char_names,
    reason = "compass-point names are clear in context"
)]
pub fn render_pov(f: &mut Frame, area: Rect, pov: PovDirection) {
    let (nw, n, ne, w, c, e, sw, s, se) = pov_cells(pov);
    let lines = vec![
        Line::from(vec![cell(nw), cell(n), cell(ne)]),
        Line::from(vec![cell(w), cell(c), cell(e)]),
        Line::from(vec![cell(sw), cell(s), cell(se)]),
    ];
    let para = Paragraph::new(lines).block(Block::default().title("POV").borders(Borders::ALL));
    f.render_widget(para, area);
}

fn cell(active: bool) -> Span<'static> {
    if active {
        Span::styled(
            " ● ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" · ", Style::default().fg(Color::DarkGray))
    }
}

/// Returns (NW, N, NE, W, Centre, E, SW, S, SE) — true if that cell is lit.
fn pov_cells(pov: PovDirection) -> (bool, bool, bool, bool, bool, bool, bool, bool, bool) {
    match pov {
        PovDirection::Center => (false, false, false, false, true, false, false, false, false),
        PovDirection::North => (false, true, false, false, false, false, false, false, false),
        PovDirection::NorthEast => (false, false, true, false, false, false, false, false, false),
        PovDirection::East => (false, false, false, false, false, true, false, false, false),
        PovDirection::SouthEast => (false, false, false, false, false, false, false, false, true),
        PovDirection::South => (false, false, false, false, false, false, false, true, false),
        PovDirection::SouthWest => (false, false, false, false, false, false, true, false, false),
        PovDirection::West => (false, false, false, true, false, false, false, false, false),
        PovDirection::NorthWest => (true, false, false, false, false, false, false, false, false),
    }
}

// ── Full input state panel ────────────────────────────────────────────────────

/// Render a complete [`InputState`] into `area` using the standard 3-section
/// layout: axes | buttons | hat.
pub fn render_input_state(f: &mut Frame, area: Rect, state: &InputState, title: &str) {
    let outer = Block::default().title(title).borders(Borders::ALL);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    // Horizontal split: axes (left) | buttons + hat (right)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(inner);

    // Left: 4 axis gauges stacked vertically
    let axis_labels = ["X", "Y", "Rz (Twist)", "Slider (Throttle)"];
    let axis_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
            Constraint::Ratio(1, 4),
        ])
        .split(cols[0]);

    for (i, label) in axis_labels.iter().enumerate() {
        render_axis(f, axis_rows[i], label, state.axes[i]);
    }

    // Right: buttons on top, hat on bottom
    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(5)])
        .split(cols[1]);

    render_buttons(f, right_rows[0], state.buttons);
    render_pov(f, right_rows[1], state.pov);
}

// ── FFB state panel ───────────────────────────────────────────────────────────

/// Render a list of active FFB effects and device gain.
pub fn render_ffb_state(f: &mut Frame, area: Rect, gain: u8, active_effects: &[u8]) {
    let effect_text: Vec<Line> = if active_effects.is_empty() {
        vec![Line::from(Span::styled(
            " (no active effects)",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        active_effects
            .iter()
            .map(|id| {
                Line::from(Span::styled(
                    format!(" Effect #{id} — playing"),
                    Style::default().fg(Color::Green),
                ))
            })
            .collect()
    };

    // Round to nearest percent: add 127 (half of 255) before dividing.
    let gain_pct = (u16::from(gain) * 100 + 127) / 255;
    let gain_line = Line::from(vec![
        Span::raw(" Gain: "),
        Span::styled(
            format!("{gain_pct}%"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let mut lines = vec![gain_line, Line::raw("")];
    lines.extend(effect_text);

    let para = Paragraph::new(lines).block(
        Block::default()
            .title("Force Feedback")
            .borders(Borders::ALL),
    );
    f.render_widget(para, area);
}

// ── Raw report view ───────────────────────────────────────────────────────────

/// Render the live hex byte dump for `sidewinder-diag raw` (issue #20).
///
/// Shows each incoming report as:
/// ```text
/// [  142ms] len=11  00 00  80 00  ff 7f  00 80  05 00  0f
///                   ├─X──┘ ├─Y──┘ ├─Rz─┘ ├─Sl─┘ ├─Btn┘ └POV
/// ```
pub fn render_raw_report(f: &mut Frame, area: Rect, report: Option<(u64, &[u8])>) {
    let block = Block::default()
        .title("Raw HID Report")
        .borders(Borders::ALL);

    let lines: Vec<Line> = match report {
        None => {
            vec![Line::from(Span::styled(
                " Waiting for device data…",
                Style::default().fg(Color::DarkGray),
            ))]
        }
        Some((elapsed_ms, bytes)) => {
            // Build the hex dump line.
            let hex: String = bytes
                .chunks(2)
                .map(|chunk| {
                    chunk
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join("  ");

            // Build a label row for the known Sidewinder FF2 field layout.
            let label_row = if bytes.len() >= 11 {
                "                  ├─X──┘ ├─Y──┘ ├─Rz─┘ ├─Sl─┘ ├─Btn┘ └POV"
            } else {
                "                  (unknown layout)"
            };

            // Interpret values for the right-hand side summary.
            let summary = if bytes.len() >= 11 {
                let x = i16::from_le_bytes([bytes[0], bytes[1]]);
                let y = i16::from_le_bytes([bytes[2], bytes[3]]);
                let rz = i16::from_le_bytes([bytes[4], bytes[5]]);
                let sl = i16::from_le_bytes([bytes[6], bytes[7]]);
                let btn = u16::from_le_bytes([bytes[8], bytes[9]]);
                let pov = bytes[10];
                format!("  X={x:+}  Y={y:+}  Rz={rz:+}  Sl={sl:+}  Btn={btn:#05x}  POV={pov:#04x}")
            } else {
                String::new()
            };

            vec![
                Line::from(vec![
                    Span::styled(
                        format!("[{elapsed_ms:>6}ms] len={:>2}  ", bytes.len()),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(hex, Style::default().fg(Color::Cyan)),
                ]),
                Line::from(Span::styled(
                    label_row,
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(summary, Style::default().fg(Color::Yellow))),
            ]
        }
    };

    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, area);
}

// ── Event log ────────────────────────────────────────────────────────────────

/// Render a scrolling event log showing the most recent entries.
///
/// Each entry is a `(elapsed_ms, message)` pair.  The log shows the last N
/// lines that fit the available area (issue #22).
pub fn render_event_log(f: &mut Frame, area: Rect, log: &VecDeque<(u64, String)>) {
    let inner_height = area.height.saturating_sub(2) as usize; // subtract border rows
    let skip = log.len().saturating_sub(inner_height);

    let lines: Vec<Line> = log
        .iter()
        .skip(skip)
        .map(|(ms, msg)| {
            Line::from(vec![
                Span::styled(
                    format!("[{ms:>6}ms] "),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(msg.as_str()),
            ])
        })
        .collect();

    let para =
        Paragraph::new(lines).block(Block::default().title("Event Log").borders(Borders::ALL));
    f.render_widget(para, area);
}

// ── Status bar ────────────────────────────────────────────────────────────────

/// Render a one-line status/help bar at the bottom of the screen.
pub fn render_status_bar(f: &mut Frame, area: Rect, msg: &str) {
    let para = Paragraph::new(msg).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(para, area);
}
