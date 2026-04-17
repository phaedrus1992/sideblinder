# sideblinder-gui

egui-based settings and live-monitor GUI for the Sideblinder app.

Connects to a running `sideblinder-app` instance over the named pipe defined
in `sideblinder-ipc`. If the app is not running, it falls back to reading the
physical device directly via `sideblinder-hid`.

Provides three screens:

- **Dashboard** — real-time axis bars, button state, POV hat, FFB master
  gain slider, and enable/disable controls.
- **Axes** — per-axis response-curve selector, dead-zone and scale sliders,
  smoothing control, invert checkbox, and a live curve preview.
- **Buttons** — click-to-select remap grid, hat-direction-to-button
  assignment, and shift-layer configuration.

Config changes made in the GUI are written back to the config file
immediately and take effect in the running service without a restart.

## License

MIT — see [LICENSE](../../LICENSE).
