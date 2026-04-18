# Sideblinder — Project Instructions

## Building

### Windows: `-j 1` required for driver builds

Any `cargo` command that transitively builds `sideblinder-driver` must run with
`-j 1` (or `CARGO_BUILD_JOBS=1`) on Windows. That includes `--workspace` builds,
`cargo clippy --all-targets`, and `cargo test`, since `sideblinder-driver` is a
default workspace member.

```bash
CARGO_BUILD_JOBS=1 cargo build -p sideblinder-driver
# or for any workspace-wide command:
CARGO_BUILD_JOBS=1 cargo build --workspace --locked
```

**Why:** `wdk-macros` 0.5.1 (a transitive dep via `wdk`) races on a shared
`.lock` file inside `target/.../scratch-*/out/wdk_macros_ast_fragments/` during
parallel proc-macro expansion. Under contention, Windows' `LockFileEx` returns
`ERROR_INVALID_FUNCTION (os error 1)` instead of the expected lock-violation
error, and the build fails with `unable to create file lock guard, unable to
obtain file lock, Incorrect function. (os error 1)`. Upstream fix in flight at
[microsoft/windows-drivers-rs#463](https://github.com/microsoft/windows-drivers-rs/pull/463)
(migrates from `fs4` to `std::File::lock()`); revisit `-j 1` once that lands.

The race only happens on the *first* build after `cargo clean` (when the
`cached_function_info_map.json` cache is empty). Once the cache is populated,
subsequent parallel builds are fine. CI always starts clean, so CI uses
`-j 1` unconditionally; see `.github/workflows/ci.yml`.

### Don't build from `\\wsl$\...` / WSL drive mounts

Check out and build the tree on a native NTFS path (e.g. `C:\...`). Building
from `\\wsl$\Ubuntu` or a mapped WSL drive (`W:`, etc.) fails at rustc's own
incremental compilation session lock with the same `ERROR_INVALID_FUNCTION`
error — the WSL 9P filesystem doesn't implement `LockFileEx` at all. Unlike the
wdk-macros bug, this one isn't fixable by `-j 1`; the filesystem itself can't
satisfy the API.

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

# Configure git to use your key (replace KEY_ID with the ID from above).
# Note: --global scope affects all repos on your machine. To limit to this repo,
# run from the sideblinder checkout and omit --global from the second command.
git config --global user.signingkey KEY_ID
git config --global commit.gpgsign true  # or 'git config commit.gpgsign true' per-repo
```

**GitHub key registration:** Export your public key and add it to GitHub so signatures are verified:

```bash
# Export public key
gpg --armor --export KEY_ID

# Copy the output and add it to GitHub:
# GitHub → Settings → SSH and GPG keys → New GPG key
# (Paste the key including the -----BEGIN and -----END lines)

# Verify the email address in your GPG key matches your GitHub commit email.
# If mismatched, GitHub will show the signature as "Unverified".
gpg --list-secret-keys --keyid-format=long --with-colons | grep uid
```

**macOS users:** Install and configure `pinentry-mac` to avoid hangs during `cargo release --execute`:

```bash
# Install pinentry-mac (one-time)
brew install pinentry-mac

# Configure gpg-agent to use it (add to ~/.gnupg/gpg-agent.conf)
echo "pinentry-program /usr/local/bin/pinentry-mac" >> ~/.gnupg/gpg-agent.conf

# Restart gpg-agent
gpg-connect-agent reloadagent /bye
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

## Reference Code

The `reference/` directory contains full source code of related projects for local
study and architectural reference. These are **not dependencies** — they are read-only
reference implementations to learn from.

### When to use reference code

- **Study patterns:** Before designing a feature (e.g., multi-device input handling,
  plugin architecture), search `reference/` to see how established projects solve it
- **Verify design decisions:** When uncertain about an approach, compare against
  reference implementations
- **Understand compatibility:** Check how other drivers/apps interact with the same
  hardware or Windows APIs

### Rules for using reference code

1. **Never copy code directly** — always understand and rewrite in Sideblinder's style
2. **Credit inspiration** — if a reference implementation influences a design decision
   or informs significant logic, note it in code comments (e.g., `// Inspired by vJoy's device state tracking`)
3. **Don't blindly follow patterns** — Sideblinder may have different constraints
   (safety, driver signing, Windows version support). Adapt, don't replicate
4. **Keep reference code in sync** — treat `reference/` as snapshots. If you use a
   reference project's pattern and later find it has a bug or improvement, consider
   investigating the current upstream and updating your code accordingly
5. **Never modify reference code** — if you find bugs in reference projects, report
   them upstream; do not patch `reference/` locally

These boundaries preserve reference code as a **learning resource** while keeping
Sideblinder's codebase clean and original.
