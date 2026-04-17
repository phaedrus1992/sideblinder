# sideblinder-driver

UMDF2 Windows driver that exposes a virtual HID joystick device to the
operating system.

`sideblinder-app` pushes joystick state and receives FFB reports via custom
IOCTLs. The driver presents a standard HID gamepad to Windows, so any game
that reads DirectInput or XInput joystick input will see the Sidewinder Force
Feedback 2 axes, buttons, and POV hat, and FFB effects created by games are
forwarded back to the physical device.

### Building

Requires the [Windows Driver Kit (WDK)](https://learn.microsoft.com/en-us/windows-hardware/drivers/download-the-wdk).
See the top-level [CONTRIBUTING.md](../../CONTRIBUTING.md) for setup
instructions.

```powershell
cargo make           # uses Makefile.toml, invokes wdk-build
```

### Installation

See [scripts/install.ps1](../../scripts/install.ps1) and the
[README](../../README.md#installation) for instructions.

This driver requires attestation signing for production use. Test-signed
builds require enabling test-signing mode (`bcdedit /set testsigning on`),
which is only appropriate for development machines.

## License

MIT — see [LICENSE](../../LICENSE).
