# sideblinder-ipc

IPC protocol definitions shared between `sideblinder-app` and
`sideblinder-gui`.

Defines the framing format for messages sent over the named pipe
`\\.\pipe\SideblinderGui`. Data flow is server→client only: the app pushes
live joystick state (axes, buttons, POV, connection status) to the GUI at
~30 Hz. Config changes made in the GUI are written directly to the config
file on disk; the app picks them up via its hot-reload watcher.

The protocol is length-prefixed with a fixed header. Every change to the
wire format requires a version bump so mismatched app and GUI versions fail
fast with a clear error.

## License

MIT — see [LICENSE](../../LICENSE).
