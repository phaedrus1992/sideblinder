# sideblinder-hid

Low-level HID communication library for the Microsoft Sidewinder Force
Feedback 2 joystick.

Handles device enumeration (by VID/PID), blocking input-report reads, input
parsing (axes, buttons, POV hat), and Force Feedback output-report
construction for all effect types defined by the HID PID spec (constant
force, ramp, periodic waveforms, condition effects, custom force). Used by
`sideblinder-app` as the device access layer.

This crate is synchronous — no async runtime dependency.

## License

MIT — see [LICENSE](../../LICENSE).
