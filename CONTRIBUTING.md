# Contributing to Sideblinder

Thank you for your interest in contributing.

## Development setup

### Prerequisites

- [Rust stable](https://rustup.rs/) (`rust-toolchain.toml` selects the
  `stable` channel; no specific version is pinned)
- Windows 11 (required for the `sideblinder-driver` UMDF2 crate; all other
  crates build on Linux and macOS too)
- [Windows Driver Kit (WDK)](https://learn.microsoft.com/en-us/windows-hardware/drivers/download-the-wdk)
  if you are working on `sideblinder-driver`

### Build

> **Windows:** every workspace command below must run with `-j 1` /
> `CARGO_BUILD_JOBS=1` because `sideblinder-driver` pulls in `wdk-macros`
> 0.5.1, which has a parallel-proc-macro race that manifests as
> `Incorrect function. (os error 1)`. See `CLAUDE.md` → *Building* for
> the full story and upstream fix tracking. Also: don't check out or
> build from `\\wsl$\...` / mapped WSL drives — the 9P filesystem
> doesn't support `LockFileEx`.

```bash
# Build all workspace crates (includes the driver on Windows; requires WDK)
CARGO_BUILD_JOBS=1 cargo build --workspace --locked

# Build and test
CARGO_BUILD_JOBS=1 cargo test --workspace --locked

# Lint
CARGO_BUILD_JOBS=1 cargo clippy --all-targets --all-features -- -D warnings

# Format check (not yet enforced in CI but recommended locally)
cargo fmt --check --all
```

On Linux/macOS the `-j 1` constraint doesn't apply — the driver crate is
Windows-only, so the bug is never hit.

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

- Run `cargo test --workspace --locked` before committing.
- New public API should have unit tests.
- Use property-based tests (`proptest`) for parsers and serialization.
- HID/driver code: mock device tests are acceptable; hardware-in-the-loop
  tests are welcome but not required.

## Code style

See `CLAUDE.md` for the full coding conventions. Key rules:

- `thiserror` for library crates, `anyhow` for binaries.
- `tracing` for all logging. The workspace lint denies `print_stdout` and
  `print_stderr` — use `tracing` even in binary crates.
- Newtypes over primitives for domain values.
- No `unwrap()` or `panic!()` in non-test production paths.
- `unsafe` blocks require a `// SAFETY:` comment.

## Versioning

Each crate is versioned independently using [Semantic Versioning](https://semver.org/):

| Change type | Bump |
|-------------|------|
| Backwards-incompatible API or behaviour change | MAJOR |
| New functionality, backwards-compatible | MINOR |
| Bug fixes, internal refactors, documentation | PATCH |

Bump every crate whose public API or behaviour changed. Add a `CHANGELOG.md`
entry in the same PR. See `CLAUDE.md` for the full release process
(`cargo release`) and GPG signing requirements.
