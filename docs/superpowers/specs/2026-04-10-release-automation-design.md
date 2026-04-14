# Release Automation Design

**Date:** 2026-04-10
**Scope:** Source-only automated release pipeline via GitHub Actions

---

## Overview

A single GitHub Actions workflow automatically cuts a release whenever a version bump is merged to
`main`. No manual steps required beyond merging the PR that bumps the version. The workflow handles
CHANGELOG promotion, git tagging, and GitHub Release creation.

---

## Trigger & Detection

**File:** `.github/workflows/release.yml`

**Trigger:** `push` to `main`.

**Version detection:** Extract the workspace version from `Cargo.toml` on the current commit and
its parent commit using `git show HEAD^:Cargo.toml`. Parse the `version = "x.y.z"` line from each.
If the versions are equal, exit immediately with success (no-op). If they differ, proceed with the
release steps.

**Edge case — initial commit:** If `HEAD^` does not exist (first commit on `main`), treat the
parent version as `""`. This always triggers a release, which is correct for an initial version
commit.

**Re-entrancy:** The two bot commits made during the release process do not touch `Cargo.toml`, so
the version-diff check naturally returns equal and the workflow exits early. No additional skip
logic is needed.

---

## CHANGELOG Transformation

The workflow makes two separate commits to `main`.

### Commit 1 — Release commit

Transform `CHANGELOG.md`:

1. Replace `## [Unreleased]` with `## [x.y.z] - YYYY-MM-DD` (version from `Cargo.toml`, date from
   workflow run date in `YYYY-MM-DD` format).
2. Add a new versioned footer link:
   `[x.y.z]: https://github.com/phaedrus/sideblinder/compare/vPREV...vx.y.z`
   On the first release, where no previous tag exists, use:
   `[x.y.z]: https://github.com/phaedrus/sideblinder/commits/vx.y.z`
3. Replace the `[Unreleased]` footer link (whatever its current form) with:
   `[Unreleased]: https://github.com/phaedrus/sideblinder/compare/vx.y.z...HEAD`
   This normalises the footer to the compare format on first release even if it was previously
   using the `/commits/main` form.

Commit message: `chore: release vx.y.z`

Tag `vx.y.z` (annotated) is created on this commit. At this SHA, `CHANGELOG.md` contains no
`[Unreleased]` section — it is a clean release snapshot.

### Commit 2 — Post-release commit

Prepend a fresh empty `[Unreleased]` section to `CHANGELOG.md`:

```markdown
## [Unreleased]
```

The footer `[Unreleased]` link was already updated in commit 1 to point at `vx.y.z...HEAD`.

Commit message: `chore: prepare next development cycle`

`main` moves to this commit. Development continues with a clean `[Unreleased]` ready.

### Implementation

Both transformation steps are implemented in `.github/scripts/transform_changelog.py`, which is
invoked by `.github/workflows/release.yml`. Python is used rather than `sed` for reliable
multi-line manipulation. The script fails with a non-zero exit code and a human-readable message
on any of the error conditions listed below.

---

## Tagging & GitHub Release

After the release commit is pushed:

1. Create an **annotated** tag `vx.y.z` on the release commit SHA:
   ```
   git tag -a vx.y.z -m "Release vx.y.z"
   git push origin vx.y.z
   ```
   Annotated tags (not lightweight) are used so `git describe` works correctly.

2. Extract the release notes from `CHANGELOG.md` — the block of text between the `## [x.y.z]`
   heading and the next `## [` heading, exclusive of both headings.

3. Create the GitHub Release:
   ```
   gh release create vx.y.z \
     --title "vx.y.z" \
     --notes "<extracted changelog block>" \
     --latest
   ```
   Not marked as pre-release.

---

## Permissions

The workflow requires:

```yaml
permissions:
  contents: write
```

This covers: pushing commits to `main`, creating and pushing tags, creating GitHub Releases.

**Branch protection note:** If the repo has branch protection rules requiring PRs for `main`, the
`github-actions[bot]` must be explicitly allowed to bypass that requirement, or the protection rule
must be turned off for bot pushes. This is a repo configuration concern outside the workflow itself.

---

## Error Handling

All shell steps use `set -euo pipefail`. The Python script exits non-zero on any of the following:

| Condition | Error message |
|-----------|---------------|
| `## [Unreleased]` heading missing from `CHANGELOG.md` | `CHANGELOG.md has no [Unreleased] section` |
| `[Unreleased]` section has no content (empty release) | `[Unreleased] section is empty — nothing to release` |
| Tag `vx.y.z` already exists | `Tag vx.y.z already exists — was this version already released?` |

Any unhandled error stops the run before a partial release can occur (e.g. tag created but release
not created, or first commit pushed but second not).

---

## File Layout

```
.github/
  workflows/
    release.yml              # release pipeline workflow
  scripts/
    transform_changelog.py   # CHANGELOG transformation logic
```

---

## What This Does Not Cover

- Building or uploading binary artifacts (out of scope for source-only releases)
- `cargo publish` to crates.io (not planned)
- Release candidate or pre-release tagging
- Rollback of a partial release (manual intervention required if the workflow fails mid-run)
