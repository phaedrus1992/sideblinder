# Configuration Reference

Sideblinder reads a TOML config file on startup and reloads it automatically
whenever you save changes — no restart needed.

## Config file location

| Platform | Default path |
|----------|-------------|
| Windows  | `%APPDATA%\Sideblinder\config.toml` |

If the file does not exist the app starts with built-in defaults. To generate
a documented copy of the defaults, run:

```powershell
sideblinder-app config --generate
```

To check your file for errors without restarting the app:

```powershell
sideblinder-app config --validate
```

---

## `[axis_x]`, `[axis_y]`, `[axis_rz]`, `[axis_slider]`

Each physical axis has its own section. All four sections share the same fields.
`[axis_x]` documents every field; copy any field into the other sections to
override the default for that axis.

| Axis section | Physical control |
|-------------|-----------------|
| `[axis_x]`      | Left-right stick deflection |
| `[axis_y]`      | Forward-back stick deflection |
| `[axis_rz]`     | Twist / yaw (rotate the stick) |
| `[axis_slider]` | Throttle lever on the base |

### `curve`

Controls how axis output changes relative to how far you move the stick.

| Value | Behaviour |
|-------|-----------|
| `"linear"` | Output moves exactly with stick position. Good starting point. **(default)** |
| `"quadratic"` | Less sensitive near centre, more responsive at extremes. Good for fine aiming. |
| `"cubic"` | Very gentle near centre, very aggressive near extremes. |
| `"s-curve"` | Smooth near centre and near extremes (3x² − 2x³). Good all-around feel. |

**Default:** `"linear"`

**Example:**
```toml
[axis_x]
curve = "s-curve"
```

---

### `dead_zone`

Fraction of the full range around centre to treat as zero. Use this if the stick
drifts when you are not touching it.

**Valid range:** `0.0` to `0.5`  
**Default:** `0.0`

| Value | Effect |
|-------|--------|
| `0.0` | No dead zone — every movement registers |
| `0.05` | 5 % dead zone — good starting point for light centre drift |
| `0.10` | 10 % dead zone — for sticks that don't return to centre |
| `0.20` | 20 % dead zone — for sticks with significant centre play |

**Example:**
```toml
[axis_x]
dead_zone = 0.05
```

---

### `scale`

Multiplies the axis output after dead zone and curve are applied.

**Valid range:** `0.0` to `4.0`  
**Default:** `1.0`

| Value | Effect |
|-------|--------|
| `1.0` | Full range |
| `0.5` | Half range — max deflection maps to 50 % output |
| `2.0` | Amplified — clips at the virtual device limits |

**Example:**
```toml
[axis_y]
scale = 0.8
```

---

### `invert`

Flips the axis direction.

**Default:** `false`

| Value | Effect |
|-------|--------|
| `false` | Normal direction |
| `true`  | Push forward → negative; pull back → positive |

**Example:**
```toml
[axis_y]
invert = true
```

---

### `smoothing`

Number of recent samples to average. Higher values reduce jitter but add a small
amount of input lag.

**Valid range:** `1` to `30`  
**Default:** `1`

| Value | Effect |
|-------|--------|
| `1`  | No smoothing — raw device values |
| `5`  | Light smoothing — barely noticeable lag |
| `15` | Heavy smoothing — visible lag; only for very noisy devices |
| `30` | Maximum smoothing |

**Example:**
```toml
[axis_rz]
smoothing = 5
```

---

## `[calibration]`

Hardware calibration tells the app the actual physical range each axis produces
so it can map correctly to the full virtual output range.

**You should not edit this section manually.** Run the calibration wizard instead:

```powershell
sideblinder-diag calibrate
```

If you haven't calibrated, the defaults cover the full signed 16-bit range
(`-32768` to `32767`), which works but may not use the full virtual output range.

| Field | Description | Default |
|-------|-------------|---------|
| `x_min` / `x_max` | X axis hardware limits | `-32768` / `32767` |
| `y_min` / `y_max` | Y axis hardware limits | `-32768` / `32767` |
| `rz_min` / `rz_max` | Twist axis hardware limits | `-32768` / `32767` |
| `slider_min` / `slider_max` | Throttle hardware limits | `-32768` / `32767` |

---

## `ffb_gain`

Global force feedback strength. `0` is off; `255` is full strength.

The game or simulator also has its own FFB gain setting. `ffb_gain` is a
hardware-level cap applied before the game's value — think of it as a safety
ceiling on how strong effects can ever feel.

**Valid range:** `0` to `255`  
**Default:** `255` (full strength — let the game control gain)

**Example** — cap effects at 70 % to avoid fatigue on long sessions:
```toml
ffb_gain = 178
```

---

## `log_level`

Controls how much the app writes to its log file.

**Default:** `"info"`

| Value | What is logged |
|-------|---------------|
| `"error"` | Errors only (very quiet) |
| `"warn"`  | Errors and warnings |
| `"info"`  | Normal operation messages — good for everyday use |
| `"debug"` | Detailed trace of every event — large log files |
| `"trace"` | Maximum verbosity — very large; for deep debugging only |

**Example** — enable debug logging for a troubleshooting session:
```toml
log_level = "debug"
```

---

## Complete example

```toml
[axis_x]
curve = "s-curve"
dead_zone = 0.05
scale = 1.0
invert = false
smoothing = 3

[axis_y]
curve = "s-curve"
dead_zone = 0.05
scale = 1.0
invert = true      # reversed so push-forward is positive in some sims
smoothing = 3

[axis_rz]
curve = "linear"
dead_zone = 0.10
scale = 0.8
invert = false
smoothing = 1

[axis_slider]
curve = "linear"
dead_zone = 0.0
scale = 1.0
invert = false
smoothing = 1

[calibration]
x_min = -32000
x_max = 32000
y_min = -32000
y_max = 32000
rz_min = -28000
rz_max = 28000
slider_min = -32768
slider_max = 32767

ffb_gain = 200
log_level = "info"
```
