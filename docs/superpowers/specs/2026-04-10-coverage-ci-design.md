# Code Coverage CI with Trend Enforcement

**Date:** 2026-04-10
**Status:** Approved

## Summary

Add per-crate code coverage measurement and enforcement to the sideblinder project.
Coverage is measured with `cargo-llvm-cov`, enforced locally via a `prek` pre-push
hook, and verified in GitHub Actions CI as a secondary safety net. Each crate must
maintain ≥80% line coverage, and coverage can only ever increase — it is never
allowed to regress from the checked-in baseline.

## Goals

- Every crate (`sidewinder-hid`, `sidewinder-app`, `sidewinder-diag`) maintains
  ≥80% line coverage at all times.
- Coverage can only go up: any PR that drops a crate's coverage below its current
  baseline is blocked at the pre-push hook and at CI.
- Baseline updates are explicit, deliberate commits — not automatic or silent.
- The workflow is fully reproducible locally on macOS (developer machines) and in
  GitHub Actions Ubuntu runners.

## Non-Goals

- Windows-only code paths (`#[cfg(target_os = "windows")]`) are not covered in CI
  or in local non-Windows runs. This is accepted: the baseline reflects what is
  measurable on the current platform.
- No external coverage services (Codecov, Coveralls). Everything is self-contained
  in the repository.

## Files

```
.github/
  workflows/
    ci.yml                        # build + test + coverage jobs
scripts/
  check-coverage.py               # reads llvm-cov JSON, enforces baseline + 80% floor
  update-coverage-baseline.py     # raises baseline entries, never lowers them
coverage-baseline.json            # per-crate coverage floors; checked in to git
```

## Coverage Baseline Format

`coverage-baseline.json` stores a line-coverage percentage (float) per crate name.
Initial values are set to 80.0. The file is updated manually by running
`scripts/update-coverage-baseline.py` after coverage genuinely improves, then
committing the result.

```json
{
  "sidewinder-hid": 80.0,
  "sidewinder-app": 80.0,
  "sidewinder-diag": 80.0
}
```

Crate names match the `[package] name` field in each crate's `Cargo.toml`.

## Enforcement Logic (`scripts/check-coverage.py`)

**Inputs:** `coverage.json` (from `cargo llvm-cov --workspace --json`),
`coverage-baseline.json`

**For each crate:**

1. Extract actual line coverage percent from the llvm-cov JSON output.
2. Fail (exit 1) if `actual < 80.0` — absolute floor violation.
3. Fail (exit 1) if `actual < baseline[crate]` — regression against stored baseline.
4. Print a non-fatal notice if `actual > baseline[crate] + 1.0` — suggests running
   the update script to lock in the gain.

**Output:** Human-readable per-crate table showing actual vs. baseline, pass/fail
status, and any notices. Exits 0 only if all crates pass.

## Baseline Update Script (`scripts/update-coverage-baseline.py`)

**Inputs:** `coverage.json`, `coverage-baseline.json` (read-modify-write)

- Reads current per-crate coverage from `coverage.json`.
- For each crate, if `actual > baseline[crate]`, updates the baseline entry to
  `actual` (rounded to one decimal place).
- Refuses to lower any baseline entry — treats a decrease as an error (exit 2).
- Prints a summary of which crates were updated and by how much.
- Writes the updated `coverage-baseline.json` in place.
- **Exit codes:** 0 = nothing changed, 1 = one or more entries raised (baseline
  file was modified), 2 = error (attempted decrease or parse failure). The pre-push
  hook uses exit code 1 to detect that a commit is needed before pushing.

The developer reviews the diff, commits `coverage-baseline.json` alongside the
tests that drove the improvement, and pushes.

## prek Pre-Push Hook (Primary Enforcement)

The hook runs on every `git push`. It:

1. Runs `cargo llvm-cov --workspace --json --output-path /tmp/sw-coverage.json`.
2. Runs `python3 scripts/check-coverage.py /tmp/sw-coverage.json coverage-baseline.json`.
   - If this fails, the push is blocked with a message indicating which crates failed.
3. Runs `python3 scripts/update-coverage-baseline.py /tmp/sw-coverage.json coverage-baseline.json`.
   - If any baseline entry was raised, the hook **aborts the push** with a message:
     "Coverage improved — `coverage-baseline.json` updated. Review and commit the
     changes before pushing."
   - This forces the baseline update to be a visible, deliberate commit.
4. If coverage is at or below baseline (no change), the push proceeds normally.

Hook configuration lives in `.pre-commit-config.yaml` at the workspace root.
Developers install it once with `prek install`.

**Prerequisites:** `cargo-llvm-cov` must be installed locally
(`cargo install cargo-llvm-cov --locked`) and `llvm-tools-preview` added to the
active toolchain (`rustup component add llvm-tools-preview`).

## GitHub Actions CI (Secondary Safety Net)

`.github/workflows/ci.yml` defines two jobs, both running on `ubuntu-latest`:

### `build` job

Runs on every PR and push to `main`.

```
- cargo fmt --check
- cargo clippy --all-targets -- -D warnings
- cargo test --workspace
```

### `coverage` job

Depends on `build` (skipped if build fails). Runs on every PR and push to `main`.

```
- rustup component add llvm-tools-preview
- cargo install cargo-llvm-cov --locked (version pinned, cached)
- cargo llvm-cov --workspace --json --output-path coverage.json
- python3 scripts/check-coverage.py coverage.json coverage-baseline.json
```

Fails the job — and blocks PR merge — if any crate is below its baseline or below
80%. Since the pre-push hook enforces this before the push, CI failure here
indicates a hook bypass (`--no-verify`) or a machine without the hook installed.

## Workflow: Improving Coverage

1. Write tests. Run `cargo test --workspace` to confirm they pass.
2. Run `cargo llvm-cov --workspace --json --output-path /tmp/sw-coverage.json`
   to measure coverage.
3. Run `python3 scripts/update-coverage-baseline.py /tmp/sw-coverage.json coverage-baseline.json`
   to raise the baseline.
4. Commit both the new tests and the updated `coverage-baseline.json` together.
5. Push. The pre-push hook re-measures, finds coverage == baseline, and proceeds.

## Workflow: Adding a New Crate

When a new crate is added to the workspace:

1. Add an entry to `coverage-baseline.json` with value `80.0` in the same commit
   that adds the crate.
2. `check-coverage.py` treats a missing baseline entry as a hard failure to prevent
   accidentally unchecked crates.

## Open Questions

None — all design decisions resolved.
