# Sideblinder — Project Instructions

## Versioning

This project uses [Semantic Versioning](https://semver.org/). Every PR that includes significant
code changes must include version bumps and a changelog update.

### Version fields

- Each crate carries its own version in `crates/<name>/Cargo.toml` (inheriting or overriding
  `[workspace.package] version` in the root `Cargo.toml`).
- The workspace-level version in the root `Cargo.toml` is bumped to match the highest semver
  change across all crates in that PR.

### Bump rules

| Change type | Version component |
|-------------|-------------------|
| Backwards-incompatible API or behaviour change | MAJOR |
| New functionality, backwards-compatible | MINOR |
| Bug fixes, internal refactors, documentation | PATCH |

### Per-PR checklist

Before opening a PR:
1. Bump the version field of every crate whose public API or behaviour changed.
2. Bump the workspace-level version to the highest bump level across all modified crates.
3. Add an entry to `CHANGELOG.md` under the `[Unreleased]` section.

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
