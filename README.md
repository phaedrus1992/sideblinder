# Sideblinder

Connect a Microsoft Sidewinder Force Feedback 2 joystick to Windows 11 as if
it were a modern device — including full force-feedback support — without any
mapping files or middleware.

---

## What is this?

This is a from-scratch virtual joystick driver for Windows 11. It reads the
physical Sidewinder FFB2 via its original USB HID protocol, translates the
input to a standard virtual joystick that every game can see in Game Controllers
(`joy.cpl`), and routes force-feedback output from the game back to the physical
stick's motor.

---

## Requirements

- Windows 11 (test-signing mode must be enabled — see installation below)
- Microsoft Sidewinder Force Feedback 2 joystick (USB)
- PowerShell 5+ (comes with Windows)
- No other joystick mapping software required

---

## Installation

### 1. Enable test-signing mode

The UMDF2 driver is currently test-signed. Open PowerShell **as Administrator**
and run:

```powershell
bcdedit /set testsigning on
```

Restart Windows.

> **Why?** The driver is not yet submitted to Microsoft for WHQL certification.
> Test-signing mode allows unsigned drivers to load. This is safe as long as
> you trust the driver binary.

### 2. Install the driver

After restarting, open PowerShell as Administrator in the repo directory and run:

```powershell
.\scripts\install.ps1
```

Restart Windows again to load the driver.

### 3. Plug in the joystick

Connect the Sidewinder FFB2 to any USB port. Windows should recognise it
automatically as an HID device.

### 4. Start the app

Run `sideblinder-app.exe` (or use the start menu shortcut if you created one).
A tray icon appears in the system tray indicating connection status:

| Icon state | Meaning |
|-----------|---------|
| Connected | Joystick found, virtual device active |
| Disconnected | Joystick not found — waiting for reconnect |

---

## Verifying it works

1. Open **Game Controllers**: press Win + R, type `joy.cpl`, press Enter.
2. You should see a "Sidewinder FFB2 Virtual" device listed.
3. Select it and click **Properties** to see live axis and button input.
4. Move the stick — the axes should respond in real time.

---

## Calibration

For best accuracy, run the calibration wizard once after installation:

```powershell
sideblinder-diag calibrate
```

Follow the on-screen instructions to move the stick to its limits. Calibration
values are saved to the config file automatically.

You can also calibrate from the GUI: **Settings → Calibrate** (once the GUI is
available in a future release).

---

## Configuration

The app reads a TOML config file. To generate a documented copy with all
defaults:

```powershell
sideblinder-app config --generate
```

The config file is at `%APPDATA%\Sideblinder\config.toml`. Changes take effect
immediately after saving — no restart needed.

For the full reference of every setting, see **[docs/config.md](docs/config.md)**.

---

## Getting help

**Diagnostics:** If something isn't working, run:

```powershell
sideblinder-diag diagnose
```

This produces a full report of device state, driver state, and active config.

**Troubleshooting guide:** See **[docs/troubleshooting.md](docs/troubleshooting.md)**
for solutions to common problems.

**Bug reports:** [Open an issue](https://github.com/phaedrus/sideblinder/issues/new)
and include the output of `sideblinder-diag diagnose`.

---

## Reference materials

### Hardware specification

[`docs/hw-spec.md`](docs/hw-spec.md) documents the empirically verified USB HID
wire format of the Sidewinder FF2, derived from the original Microsoft driver CD:

- USB identity: VID `0x045E`, PID `0x001B`
- Input report byte layout (axes, buttons, POV hat)
- FFB output report format and supported effects
- Driver architecture notes

The spec was extracted by decompiling `GcKernel_win2k.sys` (the original Microsoft
kernel filter driver from the 2002 SideWinder Force Feedback 2 CD) using Ghidra.
To regenerate the decompilation for your own reference:

```sh
# Requires Ghidra 12+ and Java 21+
JAVA_HOME=/opt/homebrew/opt/openjdk@21 \
  /path/to/ghidra/support/analyzeHeadless \
  /tmp/ghidra_project MyProject \
  -import /path/to/GcKernel_win2k.sys \
  -postScript ExportC /tmp/gckernel_decompiled.c \
  -deleteProject
```

The decompiled C is not included in this repository as it is a derivative of
Microsoft's copyrighted code.

### Reference code and implementations

Full source code of reference projects is available in the `reference/` directory
for local study and comparison without needing to clone external repositories:

- **Joystick Gremlin** — feature-rich joystick input mapper with plugin architecture
  and profile system. Useful for understanding multi-device input handling patterns
  and UI state management.
- **MW5_FFB** — MechWarrior 5 force-feedback plugin. Small focused codebase showing
  FFB effect application and game integration patterns.
- **vJoy** — Virtual joystick driver for Windows. Reference architecture for virtual
  device emulation and driver communication.

Use `reference/` to study:
- Architecture patterns for multi-device input handling
- FFB effect mapping and application
- Virtual device driver design
- Plugin and profile configuration systems

### Reference repositories (external)

For latest versions and updates to reference projects:

- [Joystick Gremlin](https://github.com/WhiteMagic/JoystickGremlin)
- [MW5_FFB](https://github.com/HappyFox/MW5_FFB)
- [vJoy](https://github.com/BrunnerInnovation/vJoy)
