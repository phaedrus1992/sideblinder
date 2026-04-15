# Sideblinder — Project Instructions

## Versioning

This project uses [Semantic Versioning](https://semver.org/). Every PR that includes significant
code changes must include version bumps and a changelog update.

### Version fields

- Each crate carries its own independent version in `crates/<name>/Cargo.toml`. Crates are
  versioned separately; a change to one crate does not require bumping others.
- The workspace-level version in the root `Cargo.toml` is also managed independently. It
  represents the overall project release and is bumped based on the severity of any changes
  anywhere in the project (not necessarily matching any single crate's version).

### Bump rules

| Change type | Version component |
|-------------|-------------------|
| Backwards-incompatible API or behaviour change | MAJOR |
| New functionality, backwards-compatible | MINOR |
| Bug fixes, internal refactors, documentation | PATCH |

Apply these rules independently to each affected crate and to the workspace root.

### Per-PR checklist

Before opening a PR:
1. Bump the version of every crate whose public API or behaviour changed.
2. Bump the workspace-level version based on the highest-severity change anywhere in the PR.
3. Add an entry to `CHANGELOG.md` under the `[Unreleased]` section.

### Cutting a release

Use `cargo-release` (install: `cargo install cargo-release --version 1.1.2 --locked`):

```bash
# Dry run first (no --execute = preview only)
cargo release minor

# Actually release
cargo release minor --execute
```

`cargo release` will:
1. Bump the version in the root `Cargo.toml`
2. Rewrite `[Unreleased]` → `[x.y.z] - date` in `CHANGELOG.md`
3. Commit the changes locally
4. Create and push an annotated tag (`v0.8.0`)

The GitHub Actions `release.yml` workflow triggers on that tag, extracts the
release notes from the tagged CHANGELOG, and creates the GitHub Release.
The tag push does **not** require a PR — tags bypass branch protection.

## Changelog

All notable changes are recorded in `CHANGELOG.md` at the repo root, following the conventions
from [Keep a Changelog](https://keepachangelog.com/).

Rules:
- Keep an `[Unreleased]` section at the top for changes not yet tagged.
- Group entries under: `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`.
- Every PR with a semver-worthy change **must** update `CHANGELOG.md` as part of the same commit.
- When a release is tagged, move `[Unreleased]` entries to a new `[x.y.z] - YYYY-MM-DD` section.
- Do not leave `CHANGELOG.md` with only a version bump and no entries — describe what changed.
- Write for the **end user**, not the developer. Describe what the user can now do or what
  problem is fixed — not which struct was added, which crate changed, or how the code works.
  Bad: "`SmoothingBuffer` wired into bridge input loop with hot-reload support"
  Good: "Axis smoothing: reduce jitter by averaging recent inputs."

## Naming Convention

The project is named **Sideblinder**. All our own artifacts must use this name.

- Crate names: `sideblinder-hid`, `sideblinder-app`, `sideblinder-diag`, `sideblinder-gui`,
  `sideblinder-ipc`, `sideblinder-driver`
- Binary names: `sideblinder-app`, `sideblinder-diag`, `sideblinder-gui`
- Rust module paths: `sideblinder_hid`, `sideblinder_app`, `sideblinder_ipc`
- Type names: `SideblinderDevice`, etc.
- Runtime artifacts: named pipe `\\.\pipe\SideblinderGui`, device symlink `\\.\SideblinderFFB2`,
  tray class `SideblinderTray`, config directory `%APPDATA%\Sideblinder` (Windows) /
  `~/.config/sideblinder` (Linux/macOS)
- Windows PnP: device node hardware ID `Root\SideblinderFFB2`, driver INF file `sideblinder.inf`

**Exception:** References to the actual hardware device ("Microsoft Sidewinder Force Feedback 2",
"Sidewinder FF2", VID/PID comments) must remain unchanged — those are hardware product names, not
our artifacts. When in doubt: if it refers to the physical joystick, leave it; if it refers to our
software, rename it.

Do not introduce any new `sidewinder` identifiers for our own artifacts.

## Issue Tracking

All task tracking uses **GitHub Issues** (`gh issue`), not `yx` / Yaks.

Before writing any code, check for an existing GitHub issue:
```bash
gh issue list --state open
gh issue view <number>
```

When completing work, reference the issue number in commit messages:
```
git commit -m "fix: description

Closes #N"
```

Do not create Yaks tasks (`yx add`, `yx state`, etc.) for this project.
