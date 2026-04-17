# sideblinder-app

Background service that bridges the physical Microsoft Sidewinder Force
Feedback 2 joystick to the virtual UMDF2 device exposed by
`sideblinder-driver`.

Reads raw HID input via `sideblinder-hid`, applies the axis/button
configuration from `~/.config/sideblinder/config.toml` (dead zones, response
curves, smoothing, button remaps, shift layers), pushes the transformed state
to the virtual device, and forwards FFB output reports from the driver back to
the physical joystick.

Also hosts the named-pipe IPC server so `sideblinder-gui` can receive live
state and push config changes without a restart.

Runs as a tray application on Windows; headless on Linux and macOS.

## License

MIT — see [LICENSE](../../LICENSE).
