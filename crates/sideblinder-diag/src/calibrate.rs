//! Interactive calibration wizard for `sideblinder-diag calibrate`.
//!
//! Walks the user through moving each axis to its physical limits and
//! measures the min/max values.  Saves the result to the config file
//! using `toml_edit` so existing comments and fields are preserved.

use std::{
    io::Write as _,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    style::{self, Stylize},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use sideblinder_hid::input::InputState;

// ── Axis descriptors ──────────────────────────────────────────────────────────

/// The four axes the wizard measures, in display order.
const AXES: &[AxisDesc] = &[
    AxisDesc {
        label: "X axis (left/right)",
        toml_min: "x_min",
        toml_max: "x_max",
        index: 0,
    },
    AxisDesc {
        label: "Y axis (forward/back)",
        toml_min: "y_min",
        toml_max: "y_max",
        index: 1,
    },
    AxisDesc {
        label: "Rz / twist",
        toml_min: "rz_min",
        toml_max: "rz_max",
        index: 2,
    },
    AxisDesc {
        label: "Throttle / slider",
        toml_min: "slider_min",
        toml_max: "slider_max",
        index: 3,
    },
];

struct AxisDesc {
    label: &'static str,
    toml_min: &'static str,
    toml_max: &'static str,
    /// Index into `InputState::axes`.
    index: usize,
}

// ── Measured result ───────────────────────────────────────────────────────────

/// Min/max measured for a single axis.
#[derive(Debug, Clone, Copy)]
pub struct AxisRange {
    pub min: i16,
    pub max: i16,
}

impl AxisRange {
    fn new(initial: i16) -> Self {
        Self {
            min: initial,
            max: initial,
        }
    }

    fn update(&mut self, value: i16) {
        if value < self.min {
            self.min = value;
        }
        if value > self.max {
            self.max = value;
        }
    }
}

// ── Input source ──────────────────────────────────────────────────────────────

/// Abstraction over real vs. simulated input for testing.
pub trait InputSource: Send + 'static {
    /// Return the most recent axis values, blocking briefly if needed.
    fn poll(&self) -> InputState;
}

/// Live input from the physical device (Windows) or demo sine wave (non-Windows).
pub struct LiveInputSource {
    state: Arc<Mutex<InputState>>,
    /// Set to a human-readable message if the device could not be opened.
    pub error: Arc<Mutex<Option<String>>>,
}

impl LiveInputSource {
    /// Spawn a background thread that continuously updates the shared state.
    pub fn spawn() -> Self {
        let state = Arc::new(Mutex::new(InputState::default()));
        let error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let state_clone = Arc::clone(&state);
        #[cfg(target_os = "windows")]
        let error_clone = Arc::clone(&error);

        std::thread::spawn(move || {
            #[cfg(target_os = "windows")]
            {
                use sideblinder_hid::device::SideblinderDevice;

                match SideblinderDevice::open() {
                    Ok(dev) => loop {
                        if let Ok((_, s)) = dev.poll_raw() {
                            *state_clone
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) = s;
                        }
                    },
                    Err(e) => {
                        tracing::error!("calibrate: failed to open device: {e}");
                        *error_clone
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) =
                            Some(format!("Device not found: {e}"));
                    }
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                // Demo: sine wave on each axis at different frequencies.
                let start = std::time::Instant::now();
                loop {
                    let t = start.elapsed().as_secs_f32();
                    let mut s = InputState::default();
                    // Sine values are bounded to ±32000.0 which fits in i16::MAX (32767).
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "demo values bounded to 32000.0, well within i16 range"
                    )]
                    {
                        s.axes[0] = (t.sin() * 32000.0) as i16;
                        s.axes[1] = ((t * 0.7).cos() * 32000.0) as i16;
                        s.axes[2] = ((t * 0.4).sin() * 32000.0) as i16;
                        s.axes[3] = ((t * 0.3).cos() * 32000.0) as i16;
                    }
                    *state_clone
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = s;
                    std::thread::sleep(Duration::from_millis(16));
                }
            }
        });

        Self { state, error }
    }
}

impl InputSource for LiveInputSource {
    fn poll(&self) -> InputState {
        *self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

// ── Wizard ────────────────────────────────────────────────────────────────────

/// Run the full calibration wizard, writing results to `config_path` on save.
///
/// # Errors
///
/// Returns an error if terminal setup fails or the config file cannot be written.
pub fn run_wizard(config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let source = LiveInputSource::spawn();

    // Brief delay so the background thread has time to attempt device open.
    // On Windows, if the device is missing, the error field will be set within
    // a few milliseconds.  50 ms is imperceptible to the user.
    std::thread::sleep(Duration::from_millis(50));

    if let Some(err) = source
        .error
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_deref()
    {
        return Err(
            format!("{err}\n\nMake sure the Sidewinder is plugged in and try again.").into(),
        );
    }

    run_wizard_with_source(config_path, &source)
}

/// Run the wizard with an injectable input source (enables testing without hardware).
///
/// # Errors
///
/// Returns an error if terminal setup fails or the config file cannot be written.
pub fn run_wizard_with_source<S: InputSource>(
    config_path: &Path,
    source: &S,
) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();

    let result = wizard_loop(&mut stdout, config_path, source);

    // Always restore terminal.
    let _ = disable_raw_mode();
    let _ = execute!(stdout, cursor::Show);

    result
}

#[expect(
    clippy::print_stdout,
    reason = "calibration wizard — println! is correct for terminal user output"
)]
fn wizard_loop(
    stdout: &mut std::io::Stdout,
    config_path: &Path,
    source: &impl InputSource,
) -> Result<(), Box<dyn std::error::Error>> {
    execute!(stdout, cursor::Hide)?;

    // ── Welcome ───────────────────────────────────────────────────────────────
    print_welcome(stdout)?;

    // Block until Enter or Q.
    loop {
        if let Ok(true) = event::poll(Duration::from_millis(50))
            && let Ok(Event::Key(k)) = event::read()
        {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match k.code {
                KeyCode::Enter | KeyCode::Char(' ') => break,
                KeyCode::Char('q') | KeyCode::Esc => {
                    execute!(stdout, cursor::Show)?;
                    println!("\r\nCalibration cancelled.");
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    // ── Per-axis measurement ──────────────────────────────────────────────────
    let mut ranges: [AxisRange; 4] = [AxisRange::new(0); 4];

    for (i, axis) in AXES.iter().enumerate() {
        let range = measure_axis(stdout, axis, source)?;
        if let Some(r) = range {
            ranges[i] = r;
        } else {
            execute!(stdout, cursor::Show)?;
            println!("\r\nCalibration cancelled.");
            return Ok(());
        }
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    let save = print_summary(stdout, &ranges)?;

    if save {
        write_calibration(config_path, &ranges)?;
        execute!(stdout, cursor::Show)?;
        println!("\r\nCalibration saved to:");
        println!("  {}\r", config_path.display());
    } else {
        execute!(stdout, cursor::Show)?;
        println!("\r\nCalibration discarded. Config unchanged.");
    }

    Ok(())
}

// ── Welcome screen ────────────────────────────────────────────────────────────

/// Print a bordered header box and clear the screen.
#[expect(
    clippy::print_stdout,
    reason = "calibration wizard — println! is correct for terminal user output"
)]
fn print_header(
    stdout: &mut std::io::Stdout,
    title: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    execute!(stdout, Clear(ClearType::All), cursor::MoveTo(0, 0))?;
    println!("┌─────────────────────────────────────────────────────┐\r");
    println!("│{title:^55}│\r");
    println!("└─────────────────────────────────────────────────────┘\r");
    Ok(())
}

#[expect(
    clippy::print_stdout,
    reason = "calibration wizard — println!/print! is correct for terminal user output"
)]
fn print_welcome(stdout: &mut std::io::Stdout) -> Result<(), Box<dyn std::error::Error>> {
    print_header(stdout, "Sidewinder FF2 Calibration Wizard")?;
    println!("\r");
    println!("We'll measure how far your joystick can move.\r");
    println!("This takes about 30 seconds.\r");
    println!("\r");
    println!("Follow the instructions for each control:\r");
    println!("  • Move each axis slowly to its full range in both directions.\r");
    println!("  • Press Enter when you are satisfied with the measurement.\r");
    println!("  • Press Q or Esc at any point to cancel without saving.\r");
    println!("\r");
    print!("Press Enter to begin, or Q to cancel: ");
    stdout.flush()?;
    Ok(())
}

// ── Per-axis screen ───────────────────────────────────────────────────────────

/// Measure one axis interactively.  Returns `None` if the user cancelled.
fn measure_axis(
    stdout: &mut std::io::Stdout,
    axis: &AxisDesc,
    source: &impl InputSource,
) -> Result<Option<AxisRange>, Box<dyn std::error::Error>> {
    let initial = source.poll().axes[axis.index];
    let mut range = AxisRange::new(initial);

    loop {
        let state = source.poll();
        let current = state.axes[axis.index];
        range.update(current);

        render_axis_screen(stdout, axis.label, current, range)?;

        if event::poll(Duration::from_millis(16))?
            && let Ok(Event::Key(k)) = event::read()
        {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match k.code {
                KeyCode::Enter | KeyCode::Char(' ') => return Ok(Some(range)),
                KeyCode::Char('q') | KeyCode::Esc => return Ok(None),
                _ => {}
            }
        }
    }
}

#[expect(
    clippy::print_stdout,
    reason = "calibration wizard — println! is correct for terminal user output"
)]
fn render_axis_screen(
    stdout: &mut std::io::Stdout,
    label: &str,
    current: i16,
    range: AxisRange,
) -> Result<(), Box<dyn std::error::Error>> {
    // Gauge bar: map current value from [-32768,32767] to [0, BAR_WIDTH].
    const BAR_WIDTH: usize = 40;

    print_header(stdout, "Sidewinder FF2 Calibration Wizard")?;
    println!("\r");
    println!(
        "Move the {} slowly to its full range in\r",
        style::style(label).bold()
    );
    println!("both directions, then press Enter.\r");
    println!("\r");

    let pos = value_to_bar(current, BAR_WIDTH);
    let min_pos = value_to_bar(range.min, BAR_WIDTH);
    let max_pos = value_to_bar(range.max, BAR_WIDTH);

    // Draw the bar: '=' for tracked min-max range, '*' for cursor.
    let mut bar = vec![' '; BAR_WIDTH];
    for cell in bar.iter_mut().take(max_pos + 1).skip(min_pos) {
        *cell = '─';
    }
    if pos < BAR_WIDTH {
        bar[pos] = '█';
    }
    let bar_str: String = bar.into_iter().collect();

    println!("  ┌{:─<1$}┐\r", "", BAR_WIDTH);
    println!("  │{bar_str}│\r");
    println!("  └{:─<1$}┘\r", "", BAR_WIDTH);
    println!("  Left                                          Right\r");
    println!("\r");
    println!("  Current:  {current:>7}\r");
    println!("  Min:      {:>7}      Max: {:>7}\r", range.min, range.max);
    println!("\r");
    println!("  Enter = accept    Q/Esc = cancel\r");
    Ok(())
}

/// Map a raw i16 value to a `0..bar_width` position (left=0, right=bar_width-1).
fn value_to_bar(value: i16, bar_width: usize) -> usize {
    // Shift to u32 to avoid overflow: value range is [-32768, 32767].
    // i32::from is lossless; adding 32768 yields 0..=65535 (always non-negative),
    // so cast_unsigned() is safe here.
    let shifted = (i32::from(value) + 32768).cast_unsigned(); // 0..=65535
    let scaled = (shifted as usize * (bar_width - 1)) / 65535;
    scaled.min(bar_width - 1)
}

// ── Summary screen ────────────────────────────────────────────────────────────

/// Print the summary and ask whether to save.  Returns `true` to save.
#[expect(
    clippy::print_stdout,
    reason = "calibration wizard — println!/print! is correct for terminal user output"
)]
fn print_summary(
    stdout: &mut std::io::Stdout,
    ranges: &[AxisRange; 4],
) -> Result<bool, Box<dyn std::error::Error>> {
    print_header(stdout, "Calibration Summary")?;
    println!("\r");
    for (axis, range) in AXES.iter().zip(ranges.iter()) {
        println!(
            "  {:20}  {:>7} to {:>7}\r",
            axis.label, range.min, range.max
        );
    }
    println!("\r");
    print!("Save calibration to config? [Y/n]: ");
    stdout.flush()?;

    // Wait for Y or N.
    loop {
        if event::poll(Duration::from_millis(50))?
            && let Ok(Event::Key(k)) = event::read()
        {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match k.code {
                KeyCode::Enter | KeyCode::Char('y' | 'Y') => return Ok(true),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => return Ok(false),
                _ => {}
            }
        }
    }
}

// ── Config writer ─────────────────────────────────────────────────────────────

/// Write the `[calibration]` section to `path`, preserving all other fields.
///
/// If the file does not exist a new one is created with only the calibration
/// section.  If the file exists, the `[calibration]` table is updated in-place
/// so comments and unrelated fields are not disturbed.
///
/// # Errors
///
/// Returns an error if the file cannot be read or written.
pub fn write_calibration(
    path: &Path,
    ranges: &[AxisRange; 4],
) -> Result<(), Box<dyn std::error::Error>> {
    // Read existing config, tolerating missing file without a TOCTOU-prone
    // exists() pre-check.
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Box::new(e)),
    };

    let mut doc: toml_edit::DocumentMut = existing.parse()?;

    // Ensure [calibration] table exists and is rendered explicitly in the output.
    if !doc.contains_table("calibration") {
        doc["calibration"] = toml_edit::table();
    }

    if let Some(t) = doc["calibration"].as_table_mut() {
        t.set_implicit(false);
    }

    let cal = doc["calibration"]
        .as_table_mut()
        .ok_or("calibration is not a table")?;

    for (axis, range) in AXES.iter().zip(ranges.iter()) {
        cal[axis.toml_min] = toml_edit::value(i64::from(range.min));
        cal[axis.toml_max] = toml_edit::value(i64::from(range.max));
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, doc.to_string())?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code — panicking on failure is the correct behaviour"
)]
mod tests {
    use super::*;

    #[test]
    fn test_value_to_bar_bounds() {
        // Minimum maps to 0, maximum maps to bar_width-1.
        assert_eq!(value_to_bar(i16::MIN, 40), 0);
        assert_eq!(value_to_bar(i16::MAX, 40), 39);
        // Centre (0) maps near the middle.
        let mid = value_to_bar(0, 40);
        assert!(
            (19..=20).contains(&mid),
            "centre should be near middle, got {mid}"
        );
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "v is in i16 range by construction"
    )]
    fn test_value_to_bar_monotone() {
        // Increasing values produce non-decreasing bar positions.
        let width = 40;
        let mut prev = 0;
        for v in (-32768i32..=32767).step_by(512) {
            let pos = value_to_bar(v as i16, width);
            assert!(pos >= prev, "bar should be monotone: {v} → {pos} < {prev}");
            prev = pos;
        }
    }

    #[test]
    fn test_axis_range_update() {
        let mut r = AxisRange::new(0);
        r.update(1000);
        r.update(-500);
        r.update(200);
        assert_eq!(r.min, -500);
        assert_eq!(r.max, 1000);
    }

    #[test]
    fn test_write_calibration_creates_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let ranges = [
            AxisRange {
                min: -31000,
                max: 30000,
            },
            AxisRange {
                min: -32000,
                max: 32000,
            },
            AxisRange {
                min: -28000,
                max: 27500,
            },
            AxisRange {
                min: -32767,
                max: 32767,
            },
        ];
        write_calibration(&path, &ranges).expect("write must succeed");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("x_min = -31000"));
        assert!(content.contains("x_max = 30000"));
        assert!(content.contains("slider_max = 32767"));
    }

    #[test]
    fn test_write_calibration_preserves_existing_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        // Write a config with an unrelated field.
        std::fs::write(&path, "log_level = \"debug\"\n\n[calibration]\nx_min = 0\n").unwrap();
        let ranges = [
            AxisRange {
                min: -1000,
                max: 1000,
            },
            AxisRange {
                min: -2000,
                max: 2000,
            },
            AxisRange {
                min: -3000,
                max: 3000,
            },
            AxisRange {
                min: -4000,
                max: 4000,
            },
        ];
        write_calibration(&path, &ranges).expect("write must succeed");
        let content = std::fs::read_to_string(&path).unwrap();
        // Unrelated field must survive.
        assert!(content.contains("log_level"), "log_level must be preserved");
        // Calibration values must be updated.
        assert!(content.contains("x_min = -1000"), "x_min must be updated");
        assert!(content.contains("y_min = -2000"), "y_min must be updated");
    }

    #[test]
    fn test_write_calibration_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("subdir").join("config.toml");
        let ranges = [AxisRange { min: 0, max: 1 }; 4];
        write_calibration(&path, &ranges).expect("must create parent dirs");
        assert!(path.exists());
    }
}
