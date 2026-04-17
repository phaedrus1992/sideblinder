# Contributing to Sideblinder

Thank you for your interest in contributing.

## Development setup

### Prerequisites

- [Rust stable](https://rustup.rs/) (see `rust-toolchain.toml` for the
  pinned channel)
- Windows 11 (required for the `sideblinder-driver` UMDF2 crate; all other
  crates build on Linux and macOS too)
- [Windows Driver Kit (WDK)](https://learn.microsoft.com/en-us/windows-hardware/drivers/download-the-wdk)
  if you are working on `sideblinder-driver`

### Build

```bash
# Build all workspace crates (excludes the driver, which needs the WDK)
cargo build --workspace

# Build and test
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Format check
cargo fmt --check --all
```

### Running the app locally

```powershell
# Install the driver first (requires WDK devcon.exe or pnputil on Windows 10 2004+)
.\scripts\install.ps1

# Run the service
cargo run -p sideblinder-app

# Run the GUI (connects to the running service)
cargo run -p sideblinder-gui

# Run diagnostics
cargo run -p sideblinder-diag -- diagnose
```

## Branching

- Branch from `main`: `git checkout -b fix/123-short-description origin/main`
- Branch naming: `fix/`, `feat/`, `docs/`, `chore/`, `test/`, `refactor/`,
  `perf/` followed by the GitHub issue number and a 2–4 word kebab slug.
- Never push directly to `main`. Use pull requests.

## Pull requests

1. One logical change per PR.
2. Reference the GitHub issue in your commit body: `Fixes #N` or `Closes #N`.
3. Add a `CHANGELOG.md` entry under `[Unreleased]` for any user-visible change
   (new feature, bug fix, behaviour change). Docs-only PRs don't need an entry.
4. All CI checks must be green before requesting review.
5. Write tests for new behaviour. See the Testing section below.

## CHANGELOG

Follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) conventions:
- Place new entries under `## [Unreleased]` at the top.
- Use the standard subsection order: `Added`, `Changed`, `Deprecated`,
  `Removed`, `Fixed`, `Security`.
- Write for the **end user**: describe what they can now do or what problem
  is fixed, not which struct changed.

## Testing

- Run `cargo test --workspace` before committing.
- New public API should have unit tests.
- Use property-based tests (`proptest`) for parsers and serialization.
- HID/driver code: mock device tests are acceptable; hardware-in-the-loop
  tests are welcome but not required.

## Code style

See `CLAUDE.md` for the full coding conventions. Key rules:

- `thiserror` for library crates, `anyhow` for binaries.
- `tracing` for logging in library crates; `println!` is fine for
  user-facing CLI output in binary crates.
- Newtypes over primitives for domain values.
- No `unwrap()` or `panic!()` in non-test production paths.
- `unsafe` blocks require a `// SAFETY:` comment.

## Versioning

Each crate is versioned independently. See the [Versioning section of
CLAUDE.md](CLAUDE.md#versioning) for bump rules and the release process.
