//! Sidewinder Force Feedback 2 diagnostic tool.
//!
//! TUI-based diagnostics for inspecting raw HID input and force-feedback
//! state.  Press `q` or `Esc` to quit.
//!
//! # Modes
//!
//! - `physical` — polls the physical Sidewinder and shows live axes,
//!   buttons, and hat switch.
//! - `full` — physical input on the left, force-feedback state on the right.
//! - `capture <file>` — record HID reports to a `.swcf` file.
//! - `replay <file>` — play back a `.swcf` file through the parser.
//! - `calibrate` — interactive axis-range wizard that writes calibration to config.

mod calibrate;
mod capture;
mod ui;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
};
use sideblinder_hid::input::InputState;
#[cfg(not(target_os = "windows"))]
use sideblinder_hid::input::PovDirection;
use tracing_subscriber::EnvFilter;

// ── Config path ───────────────────────────────────────────────────────────────

/// Return the platform-default config file path used by `sideblinder-app`.
///
/// SYNC: This must stay identical to `sideblinder_app::config::default_config_path`.
/// If the paths diverge, calibration writes to a location the app never reads.
fn default_config_path() -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        std::path::PathBuf::from(appdata)
            .join("Sideblinder")
            .join("config.toml")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        std::path::PathBuf::from(home)
            .join(".config")
            .join("sideblinder")
            .join("config.toml")
    }
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "sideblinder-diag",
    about = "Diagnostic tool for the Sidewinder FF2 joystick"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show raw physical joystick input (HID reports).
    Physical,
    /// Show physical input + force-feedback state side-by-side.
    Full,
    /// Live hex byte dump of raw HID reports with annotated field overlay.
    ///
    /// Each line shows timestamp, byte count, hex bytes, and a field label
    /// row identifying X, Y, Rz, Slider, Buttons, and POV bytes (Issue #20).
    Raw,
    /// Write a detailed hardware diagnostic log to a file and stdout.
    Diagnose,
    /// Record HID input reports to a capture file (Issue #21).
    ///
    /// Creates a `.swcf` binary file containing up to `--count` raw HID
    /// reports with their capture-relative timestamps.  The file can later
    /// be replayed offline with the `replay` subcommand.
    Capture {
        /// Output file path (e.g. `sideblinder.swcf`).
        file: std::path::PathBuf,
        /// Number of reports to capture before exiting (default: 100).
        #[arg(short, long, default_value_t = 100)]
        count: usize,
    },
    /// Replay a capture file through the input parser (Issue #21).
    ///
    /// Reads each record from a `.swcf` file, passes it through
    /// `parse_input_report`, and prints the decoded [`InputState`] for every
    /// record.  Useful for offline debugging without hardware.
    Replay {
        /// Input file path produced by `sideblinder-diag capture`.
        file: std::path::PathBuf,
    },
    /// Measure axis ranges interactively and save calibration to config (Issue #23).
    ///
    /// Walks through each axis (X, Y, Rz/twist, Throttle/slider), displays a live
    /// bar gauge, and records the min/max reached.  Writes a `[calibration]` section
    /// to the config file without disturbing other settings.
    Calibrate {
        /// Config file to write calibration to (defaults to platform standard location).
        #[arg(long)]
        config: Option<std::path::PathBuf>,
    },
}

// ── Shared state ──────────────────────────────────────────────────────────────

/// Maximum number of entries retained in the scrolling event log.
const EVENT_LOG_CAPACITY: usize = 200;

/// Live state shared between the polling thread and the render loop.
#[derive(Debug, Clone)]
struct DiagState {
    input: InputState,
    ffb_gain: u8,
    active_effects: Vec<u8>,
    error: Option<String>,
    /// Timestamp-tagged event log: `(elapsed_ms, description)`.
    event_log: VecDeque<(u64, String)>,
    /// VID:PID and report byte length, set at open time.
    device_info: Option<String>,
    /// Last raw HID report bytes (for the `raw` subcommand view).
    raw_bytes: Option<(u64, Vec<u8>)>,
}

impl Default for DiagState {
    fn default() -> Self {
        Self {
            input: InputState::default(),
            ffb_gain: 255,
            active_effects: Vec::new(),
            error: None,
            event_log: VecDeque::new(),
            device_info: None,
            raw_bytes: None,
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialise tracing to a file (not stdout, which is taken by the TUI).
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    // Non-TUI subcommands exit before setting up the terminal.
    match &cli.command {
        Command::Diagnose => return run_diagnose(),
        Command::Replay { file } => return run_replay(file),
        Command::Capture { file, count } => {
            return run_capture(file, *count);
        }
        Command::Calibrate { config } => {
            let path = config.clone().unwrap_or_else(default_config_path);
            return calibrate::run_wizard(&path).map_err(|e| {
                Box::<dyn std::error::Error>::from(format!("calibration failed: {e}"))
            });
        }
        Command::Physical | Command::Full | Command::Raw => {}
    }

    let state = Arc::new(Mutex::new(DiagState::default()));

    // Spawn the background polling task.
    spawn_poll_task(state.clone());

    // Enter the TUI.
    run_tui(&cli.command, &state)
}

// ── Diagnose subcommand ───────────────────────────────────────────────────────

/// Run hardware diagnostics, write results to `%TEMP%\sideblinder-diag.log`,
/// and print the log path to stdout.
#[expect(
    clippy::print_stdout,
    reason = "CLI diagnostic binary — println! is the correct tool for user-facing output"
)]
fn run_diagnose() -> Result<(), Box<dyn std::error::Error>> {
    let report = build_diagnostic_report();

    let log_path = std::env::temp_dir().join("sideblinder-diag.log");
    std::fs::write(&log_path, &report)?;

    // Print to stdout as well so the caller can see the path immediately.
    println!("=== Sideblinder diagnostic report ===");
    println!("{report}");
    println!("--- written to: {} ---", log_path.display());
    Ok(())
}

#[cfg(target_os = "windows")]
fn build_diagnostic_report() -> String {
    use sideblinder_hid::enumerate::enumerate_hid_devices;
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(out, "{}", report_header());
    let _ = writeln!(out);
    let _ = writeln!(out, "--- System Info ---");
    let _ = writeln!(out, "Timestamp: {}", human_timestamp());
    let _ = writeln!(out, "Platform:  Windows");
    let _ = writeln!(out);

    // 1 — Enumerate all HID devices.
    let _ = writeln!(out, "--- Device Detection ---");
    match enumerate_hid_devices() {
        Err(e) => {
            let _ = writeln!(out, "Failed to enumerate HID devices: {e}");
            return out;
        }
        Ok(devices) => {
            if devices.is_empty() {
                let _ = writeln!(out, "No HID devices found.");
            } else {
                let _ = writeln!(out, "HID devices found: {}", devices.len());
                for d in &devices {
                    let marker = if d.is_ff2() {
                        " <-- SIDEWINDER FF2"
                    } else {
                        ""
                    };
                    let _ = writeln!(
                        out,
                        "  {:04x}:{:04x}  {}{}",
                        d.vendor_id, d.product_id, d.path, marker
                    );
                }
            }
            let _ = writeln!(out);

            // 2 — Deep-probe the Sidewinder if present.
            let _ = writeln!(out, "--- HID Capabilities ---");
            if let Some(sw) = devices.iter().find(|d| d.is_ff2()) {
                if probe_ff2_device(&mut out, sw).is_err() {
                    return out;
                }
            } else {
                let _ = writeln!(
                    out,
                    "No Sidewinder Force Feedback 2 found (VID=045E PID=001B).\n\
                     Make sure the joystick is plugged in and powered on."
                );
            }
        }
    }

    // 3 — Driver status: probe the kernel device path.
    let _ = writeln!(out);
    let _ = writeln!(out, "--- Driver Status ---");
    let _ = probe_driver_device(&mut out);

    out
}

/// Open the Sidewinder HID device, query its capabilities, and attempt a
/// blocking read.  Writes results into `out`.
///
/// Returns `Err(())` if the device could not be opened at all (caller should
/// stop further probing and return the partial report).
#[cfg(target_os = "windows")]
#[expect(unsafe_code, reason = "Win32 CreateFile/CloseHandle FFI calls")]
fn probe_ff2_device(
    out: &mut String,
    sw: &sideblinder_hid::enumerate::HidDeviceInfo,
) -> Result<(), ()> {
    use std::fmt::Write as _;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    };

    let _ = writeln!(out, "Probing device at: {}", sw.path);
    let wide: Vec<u16> = sw.path.encode_utf16().chain(std::iter::once(0)).collect();

    // Open with GENERIC_READ | GENERIC_WRITE.
    // SAFETY: wide is null-terminated; flags are valid Win32 constants.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0x8000_0000 | 0x4000_0000, // GENERIC_READ | GENERIC_WRITE
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        // SAFETY: called immediately after the failing CreateFileW.
        let code = unsafe { GetLastError() };
        let _ = writeln!(
            out,
            "  Open failed: {} (error {code:#010x})",
            translate_win32_error(code)
        );
        probe_ff2_readonly(out, &wide);
        return Err(());
    }

    let _ = writeln!(out, "  Opened successfully.");
    probe_hid_caps(out, handle);
    probe_raw_read(out, handle);

    // SAFETY: handle was successfully opened above.
    unsafe { CloseHandle(handle) };
    Ok(())
}

/// Retry opening the device read-only and report the result.
#[cfg(target_os = "windows")]
#[expect(unsafe_code, reason = "Win32 CreateFile/CloseHandle FFI calls")]
fn probe_ff2_readonly(out: &mut String, wide: &[u16]) {
    use std::fmt::Write as _;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    };

    // SAFETY: wide is null-terminated; flags are valid Win32 constants.
    let h2 = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0x8000_0000, // GENERIC_READ only
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if h2 == INVALID_HANDLE_VALUE {
        // SAFETY: called immediately after the failing CreateFileW.
        let code2 = unsafe { GetLastError() };
        let _ = writeln!(
            out,
            "  Read-only open also failed: {} (error {code2:#010x})",
            translate_win32_error(code2),
        );
    } else {
        let _ = writeln!(out, "  Read-only open succeeded (write access denied).");
        // SAFETY: h2 is a valid handle.
        unsafe { CloseHandle(h2) };
    }
}

/// Query HID capabilities from a valid device handle and append to `out`.
#[cfg(target_os = "windows")]
#[expect(unsafe_code, reason = "Win32 HidD/HidP FFI calls")]
fn probe_hid_caps(out: &mut String, handle: windows_sys::Win32::Foundation::HANDLE) {
    use std::fmt::Write as _;
    use windows_sys::Win32::{
        Devices::HumanInterfaceDevice::{
            HIDP_CAPS, HidD_FreePreparsedData, HidD_GetPreparsedData, HidP_GetCaps,
        },
        Foundation::GetLastError,
    };

    const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;

    let mut preparsed: isize = 0;
    // SAFETY: handle is valid; preparsed is out-parameter.
    let ok = unsafe { HidD_GetPreparsedData(handle, &raw mut preparsed) };
    if !ok {
        // SAFETY: called immediately after the failing HidD_GetPreparsedData.
        let code = unsafe { GetLastError() };
        let _ = writeln!(
            out,
            "  HidD_GetPreparsedData failed: {} (error {code:#010x})",
            translate_win32_error(code),
        );
        return;
    }

    // SAFETY: HIDP_CAPS is a POD struct; zeroed is valid before HidP_GetCaps writes it.
    let mut caps: HIDP_CAPS = unsafe { std::mem::zeroed() };
    // SAFETY: preparsed is valid; caps is zeroed.
    let status = unsafe { HidP_GetCaps(preparsed, &raw mut caps) };
    // SAFETY: preparsed was returned by HidD_GetPreparsedData.
    unsafe { HidD_FreePreparsedData(preparsed) };

    if status == HIDP_STATUS_SUCCESS {
        let _ = writeln!(
            out,
            "  InputReportByteLength:  {}",
            caps.InputReportByteLength
        );
        let _ = writeln!(
            out,
            "  OutputReportByteLength: {}",
            caps.OutputReportByteLength
        );
        let _ = writeln!(
            out,
            "  UsagePage: {:#06x}  Usage: {:#06x}",
            caps.UsagePage, caps.Usage
        );
    } else {
        let _ = writeln!(out, "  HidP_GetCaps failed (status={status:#010x})");
    }
}

/// Attempt a single blocking `ReadFile` and append the result to `out`.
#[cfg(target_os = "windows")]
#[expect(unsafe_code, reason = "Win32 ReadFile FFI call")]
#[expect(
    clippy::cast_possible_truncation,
    reason = "buf is 64 bytes, always fits in u32"
)]
fn probe_raw_read(out: &mut String, handle: windows_sys::Win32::Foundation::HANDLE) {
    use std::fmt::Write as _;
    use windows_sys::Win32::{Foundation::GetLastError, Storage::FileSystem::ReadFile};

    let _ = writeln!(out);
    let _ = writeln!(out, "--- Raw Report Sample ---");
    let _ = writeln!(out, "  Attempting ReadFile (64-byte buffer)…");
    let mut buf = vec![0u8; 64];
    let mut bytes_read: u32 = 0;
    // SAFETY: handle is valid; buf is large enough.
    let ok = unsafe {
        ReadFile(
            handle,
            buf.as_mut_ptr(),
            buf.len() as u32,
            &raw mut bytes_read,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        // SAFETY: called immediately after the failing ReadFile.
        let code = unsafe { GetLastError() };
        let _ = writeln!(
            out,
            "  ReadFile failed: {} (error {code:#010x})",
            translate_win32_error(code)
        );
    } else {
        buf.truncate(bytes_read as usize);
        let _ = writeln!(out, "  Read OK — {bytes_read} bytes: {buf:02x?}");
    }
}

/// Try to open `\\.\SideblinderFFB2` (the kernel device created by the driver)
/// and report whether the driver is present and accessible.
#[cfg(target_os = "windows")]
#[expect(
    unsafe_code,
    reason = "Direct Win32 CreateFile/CloseHandle FFI calls for driver device probe"
)]
fn probe_driver_device(out: &mut String) -> Result<(), std::fmt::Error> {
    use std::fmt::Write as _;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    };

    // The driver creates this symbolic link when it loads.
    let path = "\\\\.\\SideblinderFFB2";
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: wide is null-terminated; flags are valid Win32 constants.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0x8000_0000 | 0x4000_0000, // GENERIC_READ | GENERIC_WRITE
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        // SAFETY: called immediately after the failing CreateFileW.
        let code = unsafe { GetLastError() };
        writeln!(
            out,
            "  Driver device not found: {} (error {code:#010x})\n  \
             Make sure the Sidewinder FFB2 driver is installed and running.",
            translate_win32_error(code),
        )?;
    } else {
        writeln!(
            out,
            "  Driver device opened successfully — driver is present and running."
        )?;
        // SAFETY: handle is valid.
        unsafe { CloseHandle(handle) };
    }

    Ok(())
}

/// Translate a Win32 error code to a short plain-English description.
///
/// Returns a human-readable string for the most common codes seen during
/// Sideblinder diagnostics; falls back to a generic message for others.
#[cfg(target_os = "windows")]
fn translate_win32_error(code: u32) -> &'static str {
    match code {
        0x0000_0000 => "success",
        0x0000_0002 => "device not found",
        0x0000_0003 => "path not found",
        0x0000_0005 => "access denied (try running as administrator)",
        0x0000_0006 => "invalid handle",
        0x0000_0020 => "sharing violation (device already open by another process)",
        0x0000_006E => "broken pipe (device disconnected during read)",
        0x0000_00E8 => "no data available (device not ready)",
        _ => "unknown error",
    }
}

#[cfg(not(target_os = "windows"))]
fn build_diagnostic_report() -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "{}", report_header());
    let _ = writeln!(out, "--- System Info ---");
    let _ = writeln!(out, "Timestamp: {}", human_timestamp());
    let _ = writeln!(out, "Platform:  {}", std::env::consts::OS);
    let _ = writeln!(out);
    let _ = writeln!(out, "--- Device Detection ---");
    let _ = writeln!(
        out,
        "Device access requires Windows with the Sideblinder driver installed."
    );
    let _ = writeln!(
        out,
        "Run this tool on Windows to capture full hardware diagnostics."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "--- Driver Status ---");
    let _ = writeln!(out, "Driver detection requires Windows.");
    out
}

/// Friendly header that appears at the top of every diagnostic report.
fn report_header() -> &'static str {
    "=== Sideblinder Diagnostic Report ===\n\
     This report helps diagnose connection problems.\n\
     Please copy everything below and paste it into your bug report.\n\
     ========================================="
}

/// Format the current time as a human-readable UTC timestamp.
///
/// Uses only `std` — no external crate needed for a basic timestamp.
/// Output: `2026-04-10 14:22:33 UTC`
fn human_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or_else(
        |e| {
            tracing::warn!("system clock before UNIX_EPOCH: {e}");
            0
        },
        |d| d.as_secs(),
    );

    // Manual decomposition of a Unix timestamp into (Y, M, D, H, Min, S).
    // This is intentionally simple and stdlib-only.
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86_400;

    // Gregorian calendar arithmetic (valid for 1970-2099).
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for &md in &month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        month += 1;
    }
    let day = remaining + 1;

    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02} UTC")
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

// ── Replay subcommand ─────────────────────────────────────────────────────────

/// Replay a `.swcf` capture file: parse every record and print its
/// decoded [`InputState`] to stdout.
#[expect(
    clippy::print_stdout,
    reason = "CLI diagnostic binary — println! is the correct tool for user-facing output"
)]
fn run_replay(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use capture::replay_capture;

    println!("Replaying capture file: {}", path.display());
    println!();

    let replayed = replay_capture(path)?;

    if replayed.is_empty() {
        println!("No parseable records found.");
        return Ok(());
    }

    for (i, (rec, state)) in replayed.iter().enumerate() {
        println!(
            "[{i:>4}]  t={:>6}ms  X={:>7}  Y={:>7}  Rz={:>7}  Slider={:>7}  \
             btns={:#05x}  pov={:?}",
            rec.timestamp_ms,
            state.axes[0],
            state.axes[1],
            state.axes[2],
            state.axes[3],
            state.buttons,
            state.pov,
        );
    }

    println!();
    println!("{} records replayed.", replayed.len());
    Ok(())
}

// ── Capture subcommand ────────────────────────────────────────────────────────

/// Record `count` HID input reports to `path`.
///
/// On non-Windows platforms this prints a notice and exits immediately, since
/// there is no physical device to read from.
fn run_capture(path: &std::path::Path, count: usize) -> Result<(), Box<dyn std::error::Error>> {
    capture_reports(path, count)
}

#[cfg(target_os = "windows")]
#[expect(
    clippy::print_stdout,
    reason = "CLI diagnostic binary — println! is the correct tool for user-facing output"
)]
fn capture_reports(path: &std::path::Path, count: usize) -> Result<(), Box<dyn std::error::Error>> {
    use capture::CaptureWriter;
    use sideblinder_hid::device::SideblinderDevice;

    let dev = SideblinderDevice::open()?;
    let transport = dev.into_transport();

    let mut writer = CaptureWriter::create(path)?;

    println!("Capturing {count} reports to {} …", path.display());

    for i in 0..count {
        let raw = transport.read_input_report()?;
        writer.write_record(&raw)?;
        if (i + 1) % 10 == 0 || i + 1 == count {
            println!("  {}/{count} reports captured", i + 1);
        }
    }

    writer.finish()?;
    println!("Saved to {}", path.display());
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[expect(
    clippy::print_stdout,
    reason = "CLI diagnostic binary — println! is the correct tool for user-facing output"
)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "stub pending implementation — non-Windows has no device to capture from"
)]
fn capture_reports(
    _path: &std::path::Path,
    _count: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("capture requires a connected Sideblinder device (Windows only).");
    Ok(())
}

// ── Polling task ──────────────────────────────────────────────────────────────

/// Spawn a blocking thread that continuously reads the physical device and
/// updates `state`.  On non-Windows, generates a gentle sine-wave demo.
fn spawn_poll_task(state: Arc<Mutex<DiagState>>) {
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        poll_task_windows(&state);
        #[cfg(not(target_os = "windows"))]
        poll_task_demo(&state);
    });
}

#[cfg(target_os = "windows")]
fn poll_task_windows(state: &Arc<Mutex<DiagState>>) {
    use sideblinder_hid::device::SideblinderDevice;
    use sideblinder_hid::enumerate::find_sideblinder;
    use sideblinder_hid::input::PovDirection;

    match SideblinderDevice::open() {
        Ok(dev) => {
            // Set device info once after a successful open.
            let info_str = find_sideblinder()
                .ok()
                .flatten()
                .map(|d| format!("[{:04X}:{:04X}]", d.vendor_id, d.product_id));
            lock_state(state).device_info = info_str;

            let start = std::time::Instant::now();
            let mut prev_buttons = 0u16;
            let mut prev_pov = PovDirection::Center;

            loop {
                match dev.poll_raw() {
                    Ok((raw, s)) => {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "as_millis() returns u128; saturating at u64::MAX (~585 million years) is acceptable for a diagnostic timestamp"
                        )]
                        let elapsed_ms = start.elapsed().as_millis() as u64;
                        let mut st = lock_state(state);

                        let changed = s.buttons ^ prev_buttons;
                        for bit in 0..9u8 {
                            if changed & (1 << bit) != 0 {
                                let pressed = s.buttons & (1 << bit) != 0;
                                let verb = if pressed { "PRESSED" } else { "released" };
                                push_event(
                                    &mut st.event_log,
                                    elapsed_ms,
                                    format!("Button {b} {verb}", b = bit + 1),
                                );
                            }
                        }

                        if s.pov != prev_pov {
                            push_event(&mut st.event_log, elapsed_ms, format!("POV → {:?}", s.pov));
                        }

                        st.raw_bytes = Some((elapsed_ms, raw));
                        st.input = s;
                        prev_buttons = st.input.buttons;
                        prev_pov = st.input.pov;
                    }
                    Err(e) => {
                        lock_state(state).error = Some(e.to_string());
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!("failed to open Sideblinder device: {e}");
            lock_state(state).error = Some(format!("open failed: {e}"));
        }
    }
}

/// Demo polling loop for non-Windows: sine-wave axes, cycling buttons, rotating hat.
#[cfg(not(target_os = "windows"))]
fn poll_task_demo(state: &Arc<Mutex<DiagState>>) {
    let start = std::time::Instant::now();

    lock_state(state).device_info = Some("[045E:001B] InputReportByteLength=12 (demo)".to_string());

    let mut prev_buttons = 0u16;
    let mut prev_pov = PovDirection::Center;

    loop {
        let (elapsed_ms, t, new_buttons, new_pov) = demo_tick(&start);

        let mut s = lock_state(state);

        let changed = new_buttons ^ prev_buttons;
        for bit in 0..9u8 {
            if changed & (1 << bit) != 0 {
                let pressed = new_buttons & (1 << bit) != 0;
                let verb = if pressed { "PRESSED" } else { "released" };
                push_event(
                    &mut s.event_log,
                    elapsed_ms,
                    format!("Button {b} {verb}", b = bit + 1),
                );
            }
        }

        if new_pov != prev_pov {
            push_event(&mut s.event_log, elapsed_ms, format!("POV → {new_pov:?}"));
        }

        update_demo_input_state(&mut s, t, new_buttons, new_pov);

        let raw = synthesize_demo_report(&s.input);
        s.raw_bytes = Some((elapsed_ms, raw));
        drop(s);

        prev_buttons = new_buttons;
        prev_pov = new_pov;

        std::thread::sleep(Duration::from_millis(16)); // ~60 Hz
    }
}

/// Compute the current demo tick values (elapsed time and derived animation state).
///
/// Casts are intentional: precision loss is acceptable for animation; truncation
/// cycles button/POV state.
#[cfg(not(target_os = "windows"))]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "demo animation — precision loss and truncation are acceptable"
)]
fn demo_tick(start: &std::time::Instant) -> (u64, f32, u16, PovDirection) {
    let elapsed_ms = start.elapsed().as_millis() as u64;
    let t = elapsed_ms as f32 / 1000.0;
    let new_buttons = ((t * 2.0) as u16) & 0x01FF;
    let new_pov = match ((t * 0.5) as u8) % 9 {
        0 => PovDirection::Center,
        1 => PovDirection::North,
        2 => PovDirection::NorthEast,
        3 => PovDirection::East,
        4 => PovDirection::SouthEast,
        5 => PovDirection::South,
        6 => PovDirection::SouthWest,
        7 => PovDirection::West,
        _ => PovDirection::NorthWest,
    };
    (elapsed_ms, t, new_buttons, new_pov)
}

/// Update the demo input state fields from the current animation time `t`.
#[cfg(not(target_os = "windows"))]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "demo values: axes bounded to 32000 (< i16::MAX), ffb_gain wraps intentionally"
)]
fn update_demo_input_state(s: &mut DiagState, t: f32, new_buttons: u16, new_pov: PovDirection) {
    s.input.axes[0] = (t.sin() * 32000.0) as i16;
    s.input.axes[1] = ((t * 0.7).cos() * 32000.0) as i16;
    s.input.axes[2] = ((t * 0.4).sin() * 32000.0) as i16;
    s.input.axes[3] = ((t * 0.3).cos() * 32000.0) as i16;
    s.input.buttons = new_buttons;
    s.input.pov = new_pov;
    s.ffb_gain = ((t * 20.0) as u8).wrapping_add(128);
    s.active_effects = if (t as u32).is_multiple_of(3) {
        vec![1]
    } else {
        vec![]
    };
}

// ── Raw report synthesis (non-Windows demo) ───────────────────────────────────

/// Build a synthetic 11-byte HID report from the current demo input state.
///
/// Layout matches the real Sidewinder FF2 format used by the parser:
/// bytes 0-1 = X (little-endian i16), 2-3 = Y, 4-5 = Rz, 6-7 = Slider,
/// 8-9 = button bitfield (u16 LE), 10 = POV nibble.
#[cfg(not(target_os = "windows"))]
fn synthesize_demo_report(input: &InputState) -> Vec<u8> {
    let mut buf = vec![0u8; 11];
    let (x, y, rz, slider) = (input.axes[0], input.axes[1], input.axes[2], input.axes[3]);
    buf[0..2].copy_from_slice(&x.to_le_bytes());
    buf[2..4].copy_from_slice(&y.to_le_bytes());
    buf[4..6].copy_from_slice(&rz.to_le_bytes());
    buf[6..8].copy_from_slice(&slider.to_le_bytes());
    buf[8..10].copy_from_slice(&input.buttons.to_le_bytes());
    buf[10] = pov_to_nibble(input.pov);
    buf
}

/// Encode a [`PovDirection`] as the 4-bit value used in the HID report.
#[cfg(not(target_os = "windows"))]
fn pov_to_nibble(pov: PovDirection) -> u8 {
    match pov {
        PovDirection::Center => 0x0F,
        PovDirection::North => 0x00,
        PovDirection::NorthEast => 0x01,
        PovDirection::East => 0x02,
        PovDirection::SouthEast => 0x03,
        PovDirection::South => 0x04,
        PovDirection::SouthWest => 0x05,
        PovDirection::West => 0x06,
        PovDirection::NorthWest => 0x07,
    }
}

// ── Status helpers ────────────────────────────────────────────────────────────

/// Build the status bar text, prepending device info when available.
///
/// Format: `[VID:PID] InputReportByteLength=N | <mode>   q/Esc = quit`
fn build_status(s: &DiagState, mode: &str) -> String {
    if let Some(ref e) = s.error {
        return format!(" [ERROR: {e}]  q/Esc = quit");
    }
    let device_prefix = s
        .device_info
        .as_deref()
        .map(|info| format!("{info} | "))
        .unwrap_or_default();
    format!(" {device_prefix}{mode}   q/Esc = quit")
}

// ── State lock helper ─────────────────────────────────────────────────────────

/// Acquire the state mutex, recovering from a poisoned lock.
///
/// A poisoned mutex means the previous lock-holder panicked — we recover the
/// inner value rather than propagating the poison, since the TUI should keep
/// rendering even if a background thread panicked.
fn lock_state(state: &std::sync::Mutex<DiagState>) -> std::sync::MutexGuard<'_, DiagState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ── Event log ─────────────────────────────────────────────────────────────────

/// Append an entry to the event log, capping the log at [`EVENT_LOG_CAPACITY`].
fn push_event(log: &mut VecDeque<(u64, String)>, elapsed_ms: u64, msg: String) {
    if log.len() >= EVENT_LOG_CAPACITY {
        log.pop_front();
    }
    log.push_back((elapsed_ms, msg));
}

// ── TUI loop ──────────────────────────────────────────────────────────────────

fn run_tui(
    command: &Command,
    state: &Arc<Mutex<DiagState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = tui_loop(&mut terminal, command, state);

    // Always attempt both cleanup steps regardless of individual failures.
    // Collect errors so LeaveAlternateScreen is never skipped.
    let raw_result: Result<(), Box<dyn std::error::Error>> =
        disable_raw_mode().map_err(|e| Box::new(e) as _);
    let leave_result: Result<(), Box<dyn std::error::Error>> =
        execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|e| Box::new(e) as _);
    let cursor_result: Result<(), Box<dyn std::error::Error>> =
        terminal.show_cursor().map_err(|e| Box::new(e) as _);

    // Surface the first error encountered (loop result takes priority).
    result.and(raw_result).and(leave_result).and(cursor_result)
}

fn tui_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    command: &Command,
    state: &Arc<Mutex<DiagState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Render
        terminal.draw(|f| {
            let s = lock_state(state).clone();

            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(f.area());

            let body = outer[0];
            let status_area = outer[1];

            match command {
                Command::Physical => {
                    // Split body: top = input panel, bottom = event log.
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(10), Constraint::Length(8)])
                        .split(body);

                    ui::render_input_state(f, rows[0], &s.input, "Physical Device");
                    ui::render_event_log(f, rows[1], &s.event_log);

                    let status = build_status(&s, "Physical device mode — live HID input");
                    ui::render_status_bar(f, status_area, &status);
                }
                Command::Full => {
                    let cols = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                        .split(body);

                    ui::render_input_state(f, cols[0], &s.input, "Physical Input");
                    ui::render_ffb_state(f, cols[1], s.ffb_gain, &s.active_effects);

                    let status = build_status(&s, "Full pipeline mode — input + FFB");
                    ui::render_status_bar(f, status_area, &status);
                }
                Command::Raw => {
                    let raw = s.raw_bytes.as_ref().map(|(ms, b)| (*ms, b.as_slice()));
                    ui::render_raw_report(f, body, raw);
                    let status = build_status(&s, "Raw HID byte dump");
                    ui::render_status_bar(f, status_area, &status);
                }
                // All of the following are handled before entering the TUI loop.
                Command::Diagnose
                | Command::Capture { .. }
                | Command::Replay { .. }
                | Command::Calibrate { .. } => {}
            }
        })?;

        // Handle input events with a 16 ms timeout (~60 fps).
        if event::poll(Duration::from_millis(16))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                _ => {}
            }
        }
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// `human_timestamp` must produce a string that matches the documented
    /// format: `YYYY-MM-DD HH:MM:SS UTC`.
    #[test]
    fn test_human_timestamp_format() {
        let ts = human_timestamp();
        // Must end with " UTC"
        assert!(ts.ends_with(" UTC"), "expected ' UTC' suffix, got: {ts}");
        // Must contain date and time separators at fixed positions.
        // e.g. "2026-04-10 14:22:33 UTC" (19 chars before " UTC")
        let body: Vec<char> = ts.chars().take(19).collect();
        // Indices: 0123456789012345678
        //          YYYY-MM-DD HH:MM:SS
        assert_eq!(body[4], '-', "year-month separator at index 4");
        assert_eq!(body[7], '-', "month-day separator at index 7");
        assert_eq!(body[10], ' ', "date-time space at index 10");
        assert_eq!(body[13], ':', "hour-minute separator at index 13");
        assert_eq!(body[16], ':', "minute-second separator at index 16");
    }

    /// `is_leap` must correctly identify leap years per the Gregorian calendar.
    #[test]
    fn test_is_leap() {
        assert!(is_leap(2000), "2000 is a leap year (divisible by 400)");
        assert!(is_leap(2024), "2024 is a leap year");
        assert!(
            !is_leap(1900),
            "1900 is not a leap year (divisible by 100, not 400)"
        );
        assert!(!is_leap(2023), "2023 is not a leap year");
    }

    /// `report_header` must mention "Diagnostic Report" and have the paste
    /// instructions line.
    #[test]
    fn test_report_header_content() {
        let h = report_header();
        assert!(
            h.contains("Diagnostic Report"),
            "header must name the document"
        );
        assert!(h.contains("bug report"), "header must mention bug report");
    }

    /// The non-Windows diagnostic report must include section headers and
    /// the platform hint.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_non_windows_report_has_sections() {
        let report = build_diagnostic_report();
        assert!(
            report.contains("--- System Info ---"),
            "must have System Info section"
        );
        assert!(
            report.contains("--- Device Detection ---"),
            "must have Device Detection section"
        );
        assert!(
            report.contains("--- Driver Status ---"),
            "must have Driver Status section"
        );
        assert!(report.contains("Timestamp:"), "must include a timestamp");
    }

    /// `push_event` must cap the log at `EVENT_LOG_CAPACITY` entries.
    #[test]
    fn test_push_event_caps_at_capacity() {
        let mut log = VecDeque::new();
        for i in 0..=EVENT_LOG_CAPACITY + 5 {
            push_event(&mut log, i as u64, format!("event {i}"));
        }
        assert_eq!(
            log.len(),
            EVENT_LOG_CAPACITY,
            "log must not exceed capacity"
        );
        // Oldest events should have been evicted; the last entry is the most recent.
        assert!(
            log.back().map(|(_, m)| m.as_str())
                == Some(&format!("event {}", EVENT_LOG_CAPACITY + 5)),
            "most recent event should be last"
        );
    }

    /// `build_status` includes `device_info` when present.
    #[test]
    fn test_build_status_with_device_info() {
        let s = DiagState {
            device_info: Some("[045E:001B] InputReportByteLength=12".to_string()),
            ..DiagState::default()
        };
        let status = build_status(&s, "Physical device mode");
        assert!(
            status.contains("[045E:001B]"),
            "status must include device info"
        );
        assert!(
            status.contains("Physical device mode"),
            "status must include mode text"
        );
    }

    /// `build_status` shows error when error is set.
    #[test]
    fn test_build_status_error_takes_priority() {
        let s = DiagState {
            error: Some("device not found".to_string()),
            device_info: Some("[045E:001B] InputReportByteLength=12".to_string()),
            ..DiagState::default()
        };
        let status = build_status(&s, "Physical device mode");
        assert!(status.contains("ERROR"), "error must show in status");
        assert!(
            status.contains("device not found"),
            "error message must appear"
        );
    }

    /// `pov_to_nibble` must cover all 9 `PovDirection` variants with the correct
    /// nibble values, and each nibble must round-trip through the parser.
    #[cfg(not(target_os = "windows"))]
    #[expect(
        clippy::panic,
        reason = "test code — panic on parse failure is the correct behaviour"
    )]
    #[test]
    fn test_pov_to_nibble_all_variants() {
        use sideblinder_hid::input::{InputState, PovDirection, parse_input_report};

        let cases: &[(PovDirection, u8)] = &[
            (PovDirection::Center, 0x0F),
            (PovDirection::North, 0x00),
            (PovDirection::NorthEast, 0x01),
            (PovDirection::East, 0x02),
            (PovDirection::SouthEast, 0x03),
            (PovDirection::South, 0x04),
            (PovDirection::SouthWest, 0x05),
            (PovDirection::West, 0x06),
            (PovDirection::NorthWest, 0x07),
        ];

        for &(pov, expected_nibble) in cases {
            assert_eq!(
                pov_to_nibble(pov),
                expected_nibble,
                "pov_to_nibble({pov:?}) should be {expected_nibble:#04x}"
            );

            // Round-trip: synthesize → parse → same PovDirection.
            let state = InputState {
                axes: [0; 8],
                buttons: 0,
                pov,
            };
            let bytes = synthesize_demo_report(&state);
            let parsed = parse_input_report(&bytes)
                .unwrap_or_else(|e| panic!("parse failed for {pov:?}: {e}"));
            assert_eq!(parsed.pov, pov, "round-trip failed for {pov:?}");
        }
    }

    /// `synthesize_demo_report` must produce an 11-byte report whose fields
    /// round-trip through the parser to the same input state.
    #[cfg(not(target_os = "windows"))]
    #[expect(
        clippy::expect_used,
        reason = "test code — expect on parse failure is the correct behaviour"
    )]
    #[test]
    fn test_synthesize_demo_report_round_trip() {
        use sideblinder_hid::input::{InputState, PovDirection, parse_input_report};

        let state = InputState {
            axes: [1234, -5678, 9000, -3000, 0, 0, 0, 0],
            buttons: 0b0_0001_0101,
            pov: PovDirection::NorthEast,
        };
        let bytes = synthesize_demo_report(&state);
        assert_eq!(bytes.len(), 11, "report must be 11 bytes");

        let parsed = parse_input_report(&bytes).expect("synthesis must produce valid report bytes");
        assert_eq!(parsed.axes[0], state.axes[0], "X round-trip");
        assert_eq!(parsed.axes[1], state.axes[1], "Y round-trip");
        assert_eq!(parsed.axes[2], state.axes[2], "Rz round-trip");
        assert_eq!(parsed.axes[3], state.axes[3], "Slider round-trip");
        assert_eq!(parsed.buttons, state.buttons, "buttons round-trip");
        assert_eq!(parsed.pov, state.pov, "POV round-trip");
    }
}
