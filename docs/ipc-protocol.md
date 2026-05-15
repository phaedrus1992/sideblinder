# IPC Protocol: sideblinder-app ↔ sideblinder-gui

## Overview

The Inter-Process Communication (IPC) protocol carries joystick state from `sideblinder-app` (server) to `sideblinder-gui` (client) via a Windows named pipe at ~30 Hz.

- **Pipe name:** `\\.\pipe\SideblinderGui`
- **Data flow:** Server → Client only
- **Frequency:** ~30 Hz (app updates with every input read)
- **Message format:** Length-prefixed binary frames

**Configuration changes** made in the GUI are NOT sent back over this pipe. Instead, the GUI writes changes directly to the config file (`%APPDATA%\Sideblinder\config.toml`), and the app's `notify` file watcher picks up the changes automatically and reloads.

## Wire Format

### Frame Structure

Each frame consists of a 4-byte length prefix followed by a payload:

```
[0–3]   u32 LE      Length prefix (payload size = 23 bytes)
[4–26]  [u8; 23]    Payload (version + state snapshot)
```

**Total frame size:** 27 bytes

### Payload Structure

The payload begins with a protocol version byte, followed by joystick state fields in little-endian format:

| Offset | Size | Field | Type | Range |
|--------|------|-------|------|-------|
| 0      | 1    | `version` | u8 | 1 (current) |
| 1–16   | 16   | `axes` | [i16; 8] LE | ±32767 |
| 17–18  | 2    | `buttons` | u16 LE | 0–511 (9 buttons) |
| 19     | 1    | `pov` | u8 | 0–7 (N=0, clockwise), 0xFF=centre |
| 20     | 1    | `connected` | u8 | 0 or 1 |
| 21     | 1    | `ffb_enabled` | u8 | 0 or 1 |
| 22     | 1    | `ffb_gain` | u8 | 0–255 |

**Total payload:** 23 bytes

## Version History

### Version 1 (Current)

- **Released:** Sideblinder 1.0 (2026-04-17)
- **Format:** Version byte + axes + buttons + POV + connection status + FFB controls
- **Change from v0:** Added protocol version byte as first byte of payload for forward compatibility

### Version 0 (Deprecated)

- **Format:** Payload without version byte (22 bytes total frame size)
- **Status:** Not supported by Sideblinder 1.0+; mismatch detection will close the connection

## Version Mismatch Behavior

When the GUI reads a frame with a version byte that does not match the expected version (`1`), it must:

1. **Log an error:** The version mismatch error is logged with both the expected and received version numbers
2. **Disconnect:** Close the pipe connection immediately
3. **Display a diagnostic message to the user:** "The app and GUI versions are incompatible. Please update both components to the same version."

This ensures that silent data corruption does not occur due to a mismatch between old and new wire formats.

## Common Scenarios

### Scenario: User updates app but not GUI

1. Old GUI connects and reads a v1 frame expecting v0
2. GUI reads version byte value `1` where it expects axis data
3. GUI detects version mismatch (`got: 1, expected: 0`)
4. GUI closes connection and shows diagnostic message

### Scenario: User updates GUI but not app

1. New GUI connects and reads a v0 frame (no version byte, 22-byte payload)
2. GUI expects 23 bytes but only gets 22 (or reads wrong data due to offset shift)
3. Length prefix validation fails (expected 23, got 22)
4. GUI shows an error and disconnects

## Implementation Notes

- The version byte is the **first byte of the payload**, immediately after the 4-byte length prefix
- The version is checked before any other field deserialization
- If the version check fails, deserialization stops immediately and returns `ProtocolError::VersionMismatch`
- All axis values are signed 16-bit integers in little-endian byte order
- The POV field uses the standard HID hat switch encoding (0=N, 1=NE, 2=E, ..., 7=NW, 0xFF=centred/null)
- The `buttons` field is a 16-bit bitmask where bits 0–8 represent buttons 1–9; bits 9–15 are reserved for future use
- Frame boundaries are determined by the length prefix alone; the pipe is treated as a byte stream
