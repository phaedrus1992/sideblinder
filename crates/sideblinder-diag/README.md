# sideblinder-diag

Diagnostics and calibration CLI for the Microsoft Sidewinder Force Feedback 2
joystick.

Subcommands:

| Command | Description |
|---------|-------------|
| `diagnose` | Run a self-test: check driver presence, device enumeration, pipe connectivity, and config validity. |
| `physical` | Show live raw axis and button values from the physical device. |
| `full` | Show live processed values (after calibration and mapping) from the virtual device. |
| `raw` | Dump raw HID input reports as hex for low-level debugging. |
| `capture` | Record a session of input reports to a file for later replay or analysis. |
| `replay` | Replay a captured report file to the virtual device. |
| `calibrate` | Interactive wizard: move each axis to its limits, measure the physical range, and write it to the config file. |

## License

MIT — see [LICENSE](../../LICENSE).
