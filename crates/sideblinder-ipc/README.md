# sideblinder-ipc

IPC protocol definitions shared between `sideblinder-app` and
`sideblinder-gui`.

Defines the framing format for messages sent over the named pipe
`\\.\pipe\SideblinderGui`. Messages carry live joystick state (axes, buttons,
POV, connection status) from the app to the GUI at ~30 Hz, and carry config
patch commands in the opposite direction.

The protocol is length-prefixed with a fixed header. Every change to the
wire format requires a version bump so mismatched app and GUI versions fail
fast with a clear error.

## License

MIT — see [LICENSE](../../LICENSE).
