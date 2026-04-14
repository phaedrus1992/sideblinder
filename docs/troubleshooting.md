# Troubleshooting

This guide covers the most common problems and how to fix them.

If none of these solve your problem, run the diagnostics tool and file a bug
report — see [Collecting a diagnostic report](#collecting-a-diagnostic-report)
at the bottom.

---

## App starts but the joystick isn't found

**Symptom:** The tray icon shows "disconnected" or the app logs
"Your joystick wasn't found."

**Causes and fixes:**

1. **Joystick not plugged in** — plug the joystick into a USB port. If it was
   not connected when the app started, restart the app after plugging it in.
2. **USB port not recognised** — try a different USB port, preferably one
   directly on the PC rather than a hub.
3. **Device conflict** — open Device Manager (`devmgmt.msc`) and look for the
   Sidewinder Force Feedback 2 under "Human Interface Devices". A yellow warning
   icon means Windows sees the device but something is wrong; try uninstalling
   the device entry and replugging.

---

## Virtual device not visible in Game Controllers (`joy.cpl`)

**Symptom:** You open Game Controllers but the Sidewinder virtual device is
not listed, even though the app is running.

**Cause:** The UMDF2 driver is not installed or test-signing mode is not enabled.

**Fix:**

1. Open PowerShell as Administrator and enable test-signing:
   ```powershell
   bcdedit /set testsigning on
   ```
2. Restart Windows.
3. After restarting, open PowerShell as Administrator in the repo directory and run
   the install script:
   ```powershell
   .\scripts\install.ps1
   ```
4. Restart Windows again.
5. Start `sideblinder-app` and recheck Game Controllers.

> **Note:** test-signing mode must be enabled before installing the driver so that
> Windows loads it on the next boot.

If the virtual device still does not appear, check Device Manager under
"System Devices" for a "Sidewinder FFB2 Virtual Device" entry with a warning
icon and look at its Properties → Events tab for driver loading errors.

---

## Axes read backwards

**Symptom:** Moving the stick forward moves the in-game axis in the wrong
direction (or vice versa).

**Fix:** Set `invert = true` for the affected axis in your config file:

```toml
[axis_y]
invert = true
```

Run `sideblinder-app config --validate` after editing to confirm the file is
valid. Changes take effect immediately after you save.

---

## Axes feel twitchy or jump around at centre

**Symptom:** Slight stick movements produce larger-than-expected output, or the
axis reads non-zero when you are not touching the stick.

**Fix:**

1. Run the calibration wizard to establish accurate hardware limits:
   ```powershell
   sideblinder-diag calibrate
   ```
2. If the stick drifts at centre, add a small dead zone:
   ```toml
   [axis_x]
   dead_zone = 0.05
   ```
3. If the output feels erratic, add light smoothing:
   ```toml
   [axis_x]
   smoothing = 5
   ```

---

## Buttons fire when moving the stick

**Symptom:** Buttons trigger unexpectedly during stick movement, or button
presses are reported at wrong indices.

**Cause:** This is most likely a parser bug in the HID report parsing code.

**Fix:**

1. Open a terminal and run the raw input viewer:
   ```powershell
   sideblinder-diag raw
   ```
2. Note which bytes change when the problem occurs.
3. [File a bug report](https://github.com/phaedrus/sideblinder/issues/new)
   and include the raw output.

---

## Force feedback not working

**Symptom:** Effects are configured in the game but the joystick does not
vibrate or resist.

**Check in order:**

1. **FFB gain is zero** — check `ffb_gain` in your config file. Set it to `255`
   for full strength:
   ```toml
   ffb_gain = 255
   ```
2. **Game FFB is disabled** — check the in-game FFB settings; most games have a
   separate gain or enable toggle.
3. **Driver not running** — the virtual device must appear in Game Controllers
   before FFB works. See
   [Virtual device not visible in Game Controllers](#virtual-device-not-visible-in-game-controllers-joycpl).

---

## Config changes are not being picked up

**Symptom:** You edit the config file but the app doesn't react, and the log
shows "Couldn't watch the config file for changes."

**Cause:** The filesystem watcher failed to start, usually because the config
directory doesn't exist yet or a permissions issue.

**Fix:**

1. Make sure the config directory exists:
   ```powershell
   New-Item -ItemType Directory -Force "$env:APPDATA\Sideblinder"
   ```
2. Restart the app. If the watcher still fails, set `log_level = "debug"` in
   the config and check the log for details.

---

## Collecting a diagnostic report

When filing a bug, please include a diagnostic report so the developers can
reproduce the problem.

**Step 1 — Run the diagnostics tool:**

```powershell
sideblinder-diag diagnose
```

This prints a structured report of device state, driver state, raw HID bytes,
and the active config. Copy the full output.

**Step 2 — Include in your bug report:**

- The full output of `sideblinder-diag diagnose`
- Your config file (find it at `%APPDATA%\Sideblinder\config.toml`)
- What you expected to happen
- What actually happened, including any error messages from the tray

**File the report at:** https://github.com/phaedrus/sideblinder/issues/new
