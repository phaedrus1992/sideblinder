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
2. Add an entry to `CHANGELOG.md` under the `[Unreleased]` section.

The workspace-level version in root `Cargo.toml` is bumped at release time
by `cargo release`, not per PR.

### Cutting a release

Install once: `cargo install cargo-release --locked`

**GPG Setup (one-time):** Configure git to sign commits with your GPG key:

```bash
# List available keys
gpg --list-secret-keys --keyid-format=long

# Configure git to use your key (replace KEY_ID with the ID from above)
git config --global user.signingkey KEY_ID
git config --global commit.gpgsign true
```

Run from the workspace root, targeting the crate to release:

```bash
# Dry run first (no --execute = preview only)
cargo release minor -p sideblinder-app

# Actually release (commits and tags are signed and pushed to main)
cargo release minor -p sideblinder-app --execute
```

Each crate is released independently. Do **not** use `--workspace` — it applies
CHANGELOG replacements once per crate and corrupts the file. See `release.toml`
for full configuration.

`cargo release` will:
1. Bump the crate's version in its `Cargo.toml`
2. Rewrite `[Unreleased]` → `[x.y.z] - date` in `CHANGELOG.md`
3. Create and push a signed commit to `main`
4. Create and push a signed annotated tag (`vX.Y.Z`)

The signed commit satisfies branch protection rules (requires verified signatures).
The GitHub Actions `release.yml` workflow triggers on the tag, extracts the
release notes from the CHANGELOG, and creates the GitHub Release.

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
