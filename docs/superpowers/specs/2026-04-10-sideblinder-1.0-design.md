# Sideblinder 1.0 — Design Spec

**Date:** 2026-04-10
**Status:** Approved

## Overview

Sideblinder makes a Microsoft Sidewinder Force Feedback 2 work as a modern virtual joystick
on Windows 11, with full force feedback passthrough. The target user is a non-technical gamer
who owns the hardware and wants it to Just Work — not a developer.

This spec covers the delta from the current state to 1.0 across three milestones.

## UX Principles

These apply to every user-facing surface: config files, error messages, calibration flows,
the GUI, and the diagnostic tool.

1. **Plain language over technical language.** "Your joystick wasn't found — is it plugged
   in?" not "TransportError::NotOpen."
2. **Guide, don't require knowledge.** Wizards over manual config edits for first-time setup.
   The config file should be human-readable with inline comments explaining every field.
3. **Fail loud and helpfully.** Every error state tells the user what went wrong and what to
   do next. No silent failures, no cryptic codes.
4. **Diagnostics are first-class.** The `diagnose` subcommand output should be self-contained
   and copy-pasteable for bug reports. Non-technical users should be able to run it and paste
   the result into a GitHub issue.
5. **Nothing requires the terminal for normal use.** The tray app and GUI cover all normal
   operations. The TUI and CLI are for power users and troubleshooting.

---

## Milestone 1 — Correct

*Goal: the physical device reports are parsed accurately and the virtual device mirrors them
faithfully. Nothing ships to users until this milestone is solid.*

### 1.1 Input parser bug fixes

The current parser has two known symptoms:
- Moving the joystick fires spurious button events (byte offset misalignment)
- Throttle and other axes read incorrectly (wrong axis order or signed/unsigned treatment)

**Fix approach:**
- Add a `sidewinder-diag raw` subcommand (see §1.2) to capture literal report bytes
- Use captured bytes to write regression tests with real device data before changing code
- Verify the exact byte layout: X (0-1), Y (2-3), Rz/twist (4-5), Slider/throttle (6-7),
  buttons (8-9), POV nibble (10). Signed i16 LE throughout.
- Fix `parse_input_report` to match verified layout
- Fix `bridge.rs::apply_config` which currently only maps 4 axes — wire all 4 physical axes
  correctly to the virtual device

### 1.2 Diagnostic tooling additions

These tools exist to make the parsing bugs reproducible and fixable. They also become the
primary support tool for non-technical users filing bug reports.

**`sidewinder-diag raw`**
- Dump raw HID report bytes to the terminal in hex as they arrive, one line per report
- Format: `[timestamp_ms] len=N  01 23 45 67 ...`
- Also show annotated field overlay: byte ranges labeled with field names
- Example output:
  ```
  [  142ms] len=11  00 00  80 00  ff 7f  00 80  05 00  0f
                    ├─X──┘ ├─Y──┘ ├─Rz─┘ ├─Sl─┘ ├─Btn┘ └POV
  ```
- Press `s` to save a capture file, `q` to quit

**`sidewinder-diag capture <file> [--count N]`**
- Record N input reports (default: 100) to a binary file for offline analysis
- File format: simple header + length-prefixed report records
- Use case: user runs this, attaches the file to a bug report

**`sidewinder-diag replay <file>`**
- Play back a capture file through the parser and display the decoded state
- Makes captured device data usable as offline test fixtures

**TUI improvements**
- Add numeric i16 value next to each axis gauge bar
- Add a scrolling event log: every button press/release, POV change, and axis threshold
  cross (+/- 10% of full range) logged with millisecond timestamp
- Show `InputReportByteLength` and device path in the status bar at startup

**`sidewinder-diag diagnose` improvements**
- Add friendly preamble: "This report helps diagnose connection problems. Please paste it
  into your bug report."
- Add human-readable timestamp (not unix epoch)
- Include OS version, driver version if detectable

### 1.3 Axis range calibration

The physical device may not use the full signed i16 range. Calibration records the observed
min/max per axis and normalises output to the full virtual range.

**Calibration wizard — `sidewinder-diag calibrate`**

Step-by-step TUI wizard:
1. Welcome screen: "We'll measure how far your joystick can move. Follow the instructions."
2. For each axis in turn: "Move the [X axis / throttle / etc.] to its full range, then
   press Enter."
3. Progress is shown live as the user moves the control.
4. On completion: shows a summary of detected ranges, asks "Save calibration? [Y/n]"
5. Writes calibration to the config file under `[calibration]`

**Calibration wizard — GUI (see §3.1)**
- Same flow, accessible from tray → "Calibrate Joystick..."
- Visualises axis movement live as user moves controls

**Calibration in config:**
```toml
# Calibration values are set automatically by the calibration wizard.
# You can also set them manually if you know your joystick's exact range.
[calibration]
x_min   = -32000   # Minimum value the X axis ever reports
x_max   =  32000   # Maximum value the X axis ever reports
rz_min  = -28000
rz_max  =  28000
# ... etc.
```

### 1.4 Device reconnect

The input bridge currently errors out permanently if the device is unplugged. Fix:
- On read error, log "Joystick disconnected — waiting for reconnect..." (INFO level)
- Retry open every 2 seconds
- On reconnect, resume without user action
- Tray icon changes to yellow during disconnect, green on reconnect

---

## Milestone 2 — Configurable

*Goal: non-technical users can customise axis feel, button layout, and hat behaviour through
the GUI and config file. Everything is documented in plain English.*

### 2.1 Response curves

Axis values are transformed through a curve before forwarding to the virtual device.

**Supported curves:**
- `linear` — 1:1 passthrough (default)
- `quadratic` — gentle around centre, more responsive at extremes
- `cubic` — aggressive centre deadening
- `s-curve` — soft centre and soft extremes, most responsive at mid-range
- `custom` — user-defined control points (GUI only for 1.0)

**Implementation:** piecewise cubic Hermite spline evaluated at the normalised axis value.
The Joystick Gremlin reference implementation (`gremlin/spline.py`) uses this approach.

**Config:**
```toml
[axes.x]
# Response curve shape.
# Options: "linear", "quadratic", "cubic", "s-curve"
# Default: "linear" (no transformation applied)
curve = "s-curve"
```

**GUI:** Live preview showing the curve shape and a dot tracking the current axis position.

### 2.2 Axis smoothing

Apply a rolling average to reduce jitter without introducing noticeable lag.

```toml
[axes.x]
# Number of samples to average (1 = no smoothing, 5 = light smoothing, 15 = heavy).
# Higher values reduce noise but add a small amount of lag.
# Default: 1
smoothing = 5
```

### 2.3 Button remapping

Map any physical button to any virtual button number.

```toml
[buttons]
# Remap physical button numbers to virtual button numbers.
# Physical buttons are numbered 1-9 as labelled on the joystick.
# Virtual buttons are what games see.
# Default: each button maps to itself (1→1, 2→2, etc.)
#
# Example: swap buttons 1 and 2
# 1 = 2
# 2 = 1
```

### 2.4 Hat → buttons

Treat the 8-way POV hat as 8 discrete buttons. Useful for games that don't recognise
the hat switch natively.

```toml
[hat]
# Set to true to map the hat switch to virtual buttons instead of a POV hat.
# The hat directions map to buttons 10-17 (North=10, NE=11, E=12, ... NW=17).
# Default: false (hat reported as a POV hat, which most games prefer)
as_buttons = false
```

### 2.5 Button layer

A single "shift" button that gives access to a second set of button mappings while held.

```toml
[layer]
# Button to use as the "shift" key for the second layer.
# While this button is held, all other buttons report their layer-2 mappings.
# Set to 0 to disable layers.
# Default: 0 (disabled)
shift_button = 6

# Layer 2 button mappings (same format as [buttons], applied when shift is held).
[layer.buttons]
# 1 = 10   # Physical button 1 + shift = virtual button 10
```

### 2.6 Config file UX

The generated default config file ships with every field present and documented:
- Section headers explain the purpose of each section
- Every field has a comment explaining what it does, what the valid values are, and
  what the default means in practical terms
- Values outside valid ranges produce a clear error: "smoothing must be between 1 and 30
  (you set 999)"
- Unknown fields produce a warning, not a silent ignore

**`sidewinder-app config --validate`**
- Validates the config file and prints a human-readable report
- Useful for users who edit the file manually

**`sidewinder-app config --generate`**
- Writes a fully-commented default config to the standard location
- Safe to run repeatedly (won't overwrite existing config)

---

## Milestone 3 — Reliable & Polished

*Goal: the full experience a non-technical user needs, including GUI, FFB quality, startup
self-test, and installer guidance.*

### 3.1 egui GUI (`sidewinder-gui` crate)

A new `sidewinder-gui` crate using `egui` + `eframe`. Launched from the tray ("Open
Settings...") and also directly from the installer shortcut.

**Screens:**

**Dashboard** (home screen)
- Connection status (joystick + driver, with icons)
- Live axis visualisation (4 bars with values)
- Button state (9 indicators)
- FFB on/off toggle + global gain slider
- Quick links: "Calibrate...", "Open Config File", "View Diagnostics"

**Axes screen**
- Per-axis panel: curve selector, deadzone slider, smoothing slider, invert toggle
- Live curve preview with current axis position dot
- "Reset to defaults" per axis

**Buttons screen**
- Grid of physical → virtual mappings
- Hat-as-buttons toggle
- Layer config: shift button selector + layer-2 mapping grid

**Calibration wizard**
- Full-screen modal wizard (same flow as §1.3 TUI wizard)
- Live axis visualisation as user moves controls
- Shows min/max detected in real time

**Diagnostics screen**
- Embeds the raw byte dump view
- "Generate Report" button → runs diagnose, opens result in a scrollable view with a
  "Copy to Clipboard" button

**About screen**
- Version, links to GitHub and documentation

### 3.2 FFB quality

**FFB gain respected end-to-end**
- `ffb_gain` from config is wired to the bridge loop
- Global gain slider in GUI + tray menu updates config and hot-reloads

**FFB enable/disable end-to-end**
- Tray "Disable Force Feedback" toggle is plumbed through the bridge to stop forwarding
  FFB packets to the physical device

**FFB telemetry in TUI and GUI**
- Show active effect type (not just slot ID): "Effect #1 — Sine (periodic)"
- Show magnitude and direction for active effects

### 3.3 Startup self-test

On launch, the app runs a quick self-test and reports status clearly:

```
✓ Joystick found (045E:001B — Microsoft Sidewinder Force Feedback 2)
✓ Driver loaded (\\.\SidewinderFFB2)
✓ Virtual device visible to Windows
! Calibration not set — using full range defaults (run Calibrate to improve accuracy)
```

If either check fails, the tray icon is red and a notification explains the problem in
plain language with a suggested fix.

### 3.4 Graceful degradation

- If the driver is missing: run in input-only mode (virtual device unavailable), show
  yellow icon, display "Driver not installed — force feedback and virtual device
  unavailable. See the installation guide."
- If the joystick is missing: wait for it, show grey icon, no crash

### 3.5 Error messages

Audit all error paths and replace debug strings with user-facing messages:

| Internal error | User-facing message |
|---|---|
| `TransportError::NotOpen` | "Your joystick wasn't found. Check that it's plugged in and try again." |
| Driver IOCTL failure | "Couldn't talk to the driver. Try reinstalling it using the install script." |
| Config parse error | "Your config file has an error on line N: [description]. The default settings will be used." |
| Report too short | "Received an unexpected response from the joystick. If this keeps happening, run Diagnose." |

### 3.6 Documentation

- **README.md**: complete quickstart — install, plug in, run, calibrate, configure
- **docs/config.md**: full config file reference with examples
- **docs/troubleshooting.md**: common problems, how to run diagnose, how to file a bug
- Config file ships with all fields documented inline (see §2.6)

---

## Architecture additions

### New crate: `sidewinder-gui`

```
crates/
  sidewinder-gui/   # egui/eframe GUI app
    src/
      main.rs       # eframe entry point
      app.rs        # top-level App state + screen routing
      dashboard.rs
      axes.rs
      buttons.rs
      calibrate.rs
      diagnostics.rs
      about.rs
```

Shares `sidewinder-hid`, `sidewinder-app` config types, and IPC layer with the existing
crates. Does not duplicate bridge logic — communicates with a running `sidewinder-app`
instance via named pipe or starts its own if one isn't running.

### Capture file format (`sidewinder-diag capture`)

```
Header (16 bytes):
  magic:    [u8; 4]  = b"SWCF"
  version:  u8       = 1
  reserved: [u8; 11]

Records (repeated):
  timestamp_ms: u32   (milliseconds since capture start)
  len:          u8
  data:         [u8; len]
```

### Config additions summary

```toml
# Full documented default config — all fields shown with explanations

[calibration]
# Set by the calibration wizard. Controls how raw axis values are
# normalised to the full virtual range.

[axes.x]        # and .y, .rz, .slider
curve     = "linear"   # Shape of the response curve
deadzone  = 0.0        # Fraction of range to treat as centre (0.0–1.0)
scale     = 1.0        # Output multiplier
invert    = false      # Flip the axis direction
smoothing = 1          # Rolling average window size (1 = off)

[buttons]       # Physical → virtual button number remaps

[hat]
as_buttons = false

[layer]
shift_button = 0   # 0 = disabled

[layer.buttons]

[ffb]
enabled    = true
gain       = 255   # 0–255; 255 = full strength

[log]
level = "info"   # "error", "warn", "info", "debug"
```

---

## Issue structure

Issues are organized into three GitHub milestones matching the milestones above.
See individual issues for implementation details.

**Milestone 1 — Correct:** ~8 issues (parser fix, raw dump, capture/replay, TUI improvements,
calibration wizard TUI, device reconnect, diagnose polish)

**Milestone 2 — Configurable:** ~9 issues (response curves, smoothing, button remap, hat
buttons, button layer, config validation, config generate, documented default config)

**Milestone 3 — Reliable:** ~8 issues (egui GUI crate + 6 screens, FFB wiring, startup
self-test, graceful degradation, error message audit, documentation)
