# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.0] - 2026-04-14

### Added
- GUI configuration app (`sidewinder-gui`): a desktop window that shows live
  joystick state and lets you edit axis curves, dead zones, button remaps, and
  FFB settings without touching the config file directly.  The app connects to
  a running `sidewinder-app` over a named pipe; if the service is not running
  it falls back to reading the device directly.
- Live Dashboard screen: real-time axis bars, button state, POV hat indicator,
  and FFB gain/enable controls — all reflecting the current joystick input.
- Axes screen: per-axis response-curve selector (Linear, Quadratic, Cubic,
  S-Curve), dead-zone and scale sliders, smoothing control, and an invert
  checkbox, each with a live curve preview dot tracking the physical stick.
- Buttons screen: click-to-select remap grid showing pressed state in green,
  hat-switch direction-to-button assignment, and shift-layer configuration.
- IPC protocol (`sidewinder-ipc`): a shared named-pipe frame format that lets
  the GUI receive axis values, button state, POV, and connection status from
  `sidewinder-app` at ~30 Hz with no polling overhead.
- Automatic device reconnect: unplugging and replugging the joystick now
  resumes operation without restarting the app. The bridge retries every
  2 seconds and logs when the device disconnects and reconnects.
- The system tray tooltip now reflects connection state, showing
  "connected" or "disconnected" as the physical joystick is plugged and
  unplugged.
- Button remapping: map any physical button to any virtual button number
  via `[buttons]` in the config file. Useful when a game hard-codes button
  numbers that don't match the joystick.
- Hat-as-buttons: set `[hat] as_buttons = true` to map the 8-way POV hat to
  discrete virtual buttons (buttons 10–17 by default) instead of a POV axis.
  Useful for games that don't recognise hat input.
- Two-layer buttons: set `[layer] shift_button = N` to use one physical button
  as a "shift key". While held, all other buttons report their layer-2 mappings
  from `[layer.buttons]`, and the shift button itself is not forwarded.
- The virtual device now reports up to 32 buttons (up from 16), giving enough
  room for all 9 physical buttons plus all 8 hat-as-buttons directions without
  any conflict.
- Response curves: choose how your stick behaves near centre vs. at the edges.
  Options: `linear` (default), `quadratic`, `cubic`, `s-curve`.
- Axis smoothing: rolling-average filter to reduce jitter on noisy devices.
  Set `smoothing = 5` for light filtering, up to 30 for heavy smoothing.
- Diagnostics tool (`sidewinder-diag`): inspect raw device data and capture
  reports for troubleshooting.
- Config validation: run `sidewinder-app config --validate` to check your config
  file and see a section-by-section report of any out-of-range values.
- Config generation: run `sidewinder-app config --generate` to write a fully
  documented default config with inline comments for every setting.
- Calibration wizard: run `sidewinder-diag calibrate` to walk through each axis
  interactively, measure the physical range, and save it to your config file.

### Changed
- Graceful degradation: if the driver is not installed, the app now starts in
  input-only mode instead of crashing. The tray tooltip explains the limitation
  and a right-click "Install Driver…" menu item opens the scripts directory.
- Startup self-test: the tray tooltip now reflects calibration status —
  "calibration not set (run Calibrate for accuracy)" is shown when no
  calibration data has been recorded, so new users know to run the wizard.
- FFB gain and enable/disable are now wired end-to-end. Changing `ffb_gain`
  (0–255) in the config file scales the perceived force strength; setting
  `ffb_enabled = false` silences all motors and sends a Device Control:
  Disable Actuators report. Both changes take effect immediately without a
  restart via hot-reload.
- Tray tooltip wording updated to be more user-friendly: "Waiting for
  joystick…" when disconnected, and a clear driver-missing message when the
  driver is absent.
- Workspace-level Clippy lints are now enforced: pedantic warnings, `unwrap_used` denied,
  `panic` denied, `exit` denied. All pre-existing violations fixed. Code quality is now
  verified automatically on every build.

### Fixed
- Error messages in the tray log and notifications now use plain English with
  actionable next steps. Raw Rust error type names no longer appear in
  user-visible output — for example, a missing joystick says "Check that it's
  plugged in and try again" instead of showing an internal system error.
- `shift_button` values beyond the physical button count and hat button indices
  out of the virtual range now produce clear warnings instead of silently having
  no effect.
- Invalid config values (e.g. smoothing out of range, calibration min ≥ max)
  now produce a clear warning on startup instead of silently misbehaving.
- Duplicate button mappings (two physical buttons targeting the same virtual
  button) now produce a clear warning on startup.

[Unreleased]: https://github.com/phaedrus/sideblinder/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/phaedrus/sideblinder/commits/v0.7.0
