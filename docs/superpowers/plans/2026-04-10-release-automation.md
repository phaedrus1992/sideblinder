# Release Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automatically cut a source-only GitHub Release (CHANGELOG promotion, annotated tag, GitHub Release with notes) whenever a version bump is merged to `main`.

**Architecture:** A single GitHub Actions workflow (`release.yml`) triggers on every push to `main`, detects a version change by diffing `Cargo.toml` against its parent commit, runs an embedded Python script to transform `CHANGELOG.md` in two commits (release snapshot → post-release), creates an annotated tag, and publishes a GitHub Release with the extracted release notes.

**Tech Stack:** GitHub Actions, Python 3 (stdlib only, embedded inline), `gh` CLI (pre-installed on GitHub-hosted runners), `git`.

---

## File Layout

| File | Action | Responsibility |
|------|--------|----------------|
| `.github/workflows/release.yml` | Create | Release pipeline workflow |
| `.github/scripts/transform_changelog.py` | Create | CHANGELOG transformation logic |

---

### Task 1: Scaffold the workflow with version-detection no-op

**Files:**
- Create: `.github/workflows/release.yml`

This task creates the workflow skeleton and proves the no-op exit path works. Nothing else runs yet.

- [ ] **Step 1: Create the workflow file**

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    branches: [main]

permissions:
  contents: write

jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5  # v4.3.1
        with:
          fetch-depth: 2        # need HEAD and HEAD^ for version diff
          persist-credentials: true

      - name: Detect version change
        id: version
        shell: bash
        run: |
          set -euo pipefail
          current=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
          previous=$(git show HEAD^:Cargo.toml 2>/dev/null \
            | grep '^version' | head -1 | sed 's/.*"\(.*\)".*/\1/' \
            || echo "")
          echo "current=$current" >> "$GITHUB_OUTPUT"
          echo "previous=$previous" >> "$GITHUB_OUTPUT"
          if [ "$current" = "$previous" ]; then
            echo "changed=false" >> "$GITHUB_OUTPUT"
          else
            echo "changed=true" >> "$GITHUB_OUTPUT"
          fi

      - name: Skip if no version change
        if: steps.version.outputs.changed == 'false'
        run: |
          echo "Version unchanged (${{ steps.version.outputs.current }}), skipping release."

      - name: Confirm release will proceed
        if: steps.version.outputs.changed == 'true'
        run: |
          echo "Version changed: ${{ steps.version.outputs.previous }} → ${{ steps.version.outputs.current }}"
          echo "Release steps will run."
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add release workflow skeleton with version detection"
```

- [ ] **Step 3: Push and verify the workflow runs without error**

```bash
git push
```

Open the Actions tab on GitHub. The workflow should appear, run, and either print "Version unchanged" (if the version didn't change in this push) or "Version changed: ... → ..." (if it did). Either way it must not fail.

---

### Task 2: Implement the CHANGELOG transformation Python script

**Files:**
- Modify: `.github/workflows/release.yml` (add `Transform CHANGELOG` step)

This task adds the inline Python that does all `CHANGELOG.md` manipulation. It is the most complex part — get it right before wiring up git commits and tagging.

The script must:
1. Read `CHANGELOG.md`
2. Verify `## [Unreleased]` exists and has non-blank, non-heading content
3. Produce two output files:
   - `CHANGELOG_release.md` — `[Unreleased]` replaced with `[x.y.z] - DATE`, footer updated
   - `CHANGELOG_next.md` — `CHANGELOG_release.md` with a fresh empty `[Unreleased]` prepended
4. Print the extracted release notes to `RELEASE_NOTES.md` (used in a later task)
5. Exit non-zero with a clear message on any error condition

- [ ] **Step 1: Add the Transform CHANGELOG step to the workflow**

Add this step after "Confirm release will proceed" (keep `if: steps.version.outputs.changed == 'true'` on all remaining steps):

```yaml
      - name: Transform CHANGELOG
        if: steps.version.outputs.changed == 'true'
        env:
          VERSION: ${{ steps.version.outputs.current }}
        run: |
          set -euo pipefail
          python3 << 'PYEOF'
          import sys, re, os
          from datetime import date

          version = os.environ["VERSION"]
          today = date.today().isoformat()
          repo = "https://github.com/phaedrus/sideblinder"

          text = open("CHANGELOG.md").read()

          # ── Validate [Unreleased] exists ──────────────────────────────────
          if "## [Unreleased]" not in text:
              print("ERROR: CHANGELOG.md has no [Unreleased] section", file=sys.stderr)
              sys.exit(1)

          # Split on section headings (lines starting with ## [)
          # Produces: [preamble, "Unreleased\n...", "prev version\n...", ...]
          parts = re.split(r'^(## \[)', text, flags=re.MULTILINE)
          # parts[0] = text before first ## [
          # parts[1], parts[2] = "## [", "Unreleased]\n...<content>..."
          # parts[3], parts[4] = "## [", "0.1.0] - ...\n..."  (if present)

          # Find Unreleased body: it's parts[2] when parts[1] == "## ["
          # Actually re.split with a capture group interleaves delimiters:
          # index 0: before first match
          # index 1: delimiter ("## [")
          # index 2: rest after delimiter up to next match
          # index 3: delimiter, index 4: rest, ...
          # So parts[2] starts with "Unreleased]\n" and contains the body
          if len(parts) < 3 or not parts[2].startswith("Unreleased]"):
              print("ERROR: CHANGELOG.md has no [Unreleased] section", file=sys.stderr)
              sys.exit(1)

          unreleased_chunk = parts[2]  # "Unreleased]\n...<body>..."
          body_lines = unreleased_chunk.split("\n", 1)[1] if "\n" in unreleased_chunk else ""

          # Check body has real content (non-blank, non-heading lines)
          real_content = [
              l for l in body_lines.splitlines()
              if l.strip() and not l.startswith("#")
          ]
          if not real_content:
              print("ERROR: [Unreleased] section is empty — nothing to release", file=sys.stderr)
              sys.exit(1)

          # ── Build release notes (body only, no heading) ───────────────────
          release_notes = body_lines.strip()
          open("RELEASE_NOTES.md", "w").write(release_notes)

          # ── Build CHANGELOG_release.md ────────────────────────────────────
          # Replace the Unreleased heading+body with versioned heading+body
          versioned_chunk = f"{version}] - {today}\n{body_lines}"
          release_parts = list(parts)
          release_parts[2] = versioned_chunk

          # Update footer links
          # Old [Unreleased] link (any form) → versioned compare link
          # Also add new [x.y.z] link
          # Detect previous tag from existing versioned footer links
          prev_tag_match = re.search(
              r'^\[([0-9]+\.[0-9]+\.[0-9]+)\]:',
              "".join(release_parts),
              re.MULTILINE
          )
          if prev_tag_match:
              prev_tag = "v" + prev_tag_match.group(1)
              version_link = f"[{version}]: {repo}/compare/{prev_tag}...v{version}"
          else:
              # First release — no previous tag exists
              version_link = f"[{version}]: {repo}/commits/v{version}"

          unreleased_link = f"[Unreleased]: {repo}/compare/v{version}...HEAD"

          release_text = "".join(release_parts)
          # Replace existing [Unreleased]: ... line (any form)
          release_text = re.sub(
              r'^\[Unreleased\]:.*$',
              unreleased_link + "\n" + version_link,
              release_text,
              flags=re.MULTILINE
          )
          open("CHANGELOG_release.md", "w").write(release_text)

          # ── Build CHANGELOG_next.md ───────────────────────────────────────
          # Prepend fresh empty [Unreleased] section
          next_text = f"## [Unreleased]\n\n" + release_text
          open("CHANGELOG_next.md", "w").write(next_text)

          print(f"✓ Prepared CHANGELOG for release v{version}")
          print(f"  Release notes: {len(real_content)} content lines")
          PYEOF
```

- [ ] **Step 2: Add a step to print the release notes for inspection**

```yaml
      - name: Show release notes
        if: steps.version.outputs.changed == 'true'
        run: cat RELEASE_NOTES.md
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add CHANGELOG transformation script to release workflow"
```

- [ ] **Step 4: Push and verify the transformation runs**

```bash
git push
```

In the Actions tab, the "Transform CHANGELOG" step should print `✓ Prepared CHANGELOG for release v0.2.0` and "Show release notes" should display the current `[Unreleased]` body. If you're testing on a branch where the version didn't change vs. its parent, the steps will be skipped — that's correct. To force a test run, temporarily change the condition to `if: always()` locally, but **do not commit that change**.

---

### Task 3: Commit release snapshot and create annotated tag

**Files:**
- Modify: `.github/workflows/release.yml` (add commit + tag steps)

This task makes the release commit (clean CHANGELOG, no `[Unreleased]`) and creates the annotated tag on it.

- [ ] **Step 1: Configure git identity for bot commits**

Add this step before the commit steps (after "Transform CHANGELOG"):

```yaml
      - name: Configure git identity
        if: steps.version.outputs.changed == 'true'
        run: |
          git config user.name  "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
```

- [ ] **Step 2: Add the release commit step**

```yaml
      - name: Commit release CHANGELOG
        if: steps.version.outputs.changed == 'true'
        env:
          VERSION: ${{ steps.version.outputs.current }}
        run: |
          set -euo pipefail
          cp CHANGELOG_release.md CHANGELOG.md
          git add CHANGELOG.md
          git commit -m "chore: release v${VERSION}"
```

- [ ] **Step 3: Add the tag step**

```yaml
      - name: Create annotated tag
        if: steps.version.outputs.changed == 'true'
        env:
          VERSION: ${{ steps.version.outputs.current }}
        run: |
          set -euo pipefail
          if git rev-parse "v${VERSION}" >/dev/null 2>&1; then
            echo "ERROR: Tag v${VERSION} already exists — was this version already released?" >&2
            exit 1
          fi
          git tag -a "v${VERSION}" -m "Release v${VERSION}"
```

- [ ] **Step 4: Push the release commit and tag**

```yaml
      - name: Push release commit and tag
        if: steps.version.outputs.changed == 'true'
        env:
          VERSION: ${{ steps.version.outputs.current }}
        run: |
          set -euo pipefail
          git push origin main
          git push origin "v${VERSION}"
```

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add release commit and annotated tag steps"
```

---

### Task 4: Commit post-release CHANGELOG and create GitHub Release

**Files:**
- Modify: `.github/workflows/release.yml` (add post-release commit + gh release steps)

This task moves `main` one commit past the tag (adding the fresh `[Unreleased]` section) and then creates the GitHub Release pointed at the tag.

- [ ] **Step 1: Add the post-release commit step**

```yaml
      - name: Commit post-release CHANGELOG
        if: steps.version.outputs.changed == 'true'
        env:
          VERSION: ${{ steps.version.outputs.current }}
        run: |
          set -euo pipefail
          cp CHANGELOG_next.md CHANGELOG.md
          git add CHANGELOG.md
          git commit -m "chore: prepare next development cycle"
          git push origin main
```

- [ ] **Step 2: Add the GitHub Release step**

```yaml
      - name: Create GitHub Release
        if: steps.version.outputs.changed == 'true'
        env:
          VERSION: ${{ steps.version.outputs.current }}
          GH_TOKEN: ${{ github.token }}
        run: |
          set -euo pipefail
          gh release create "v${VERSION}" \
            --title "v${VERSION}" \
            --notes-file RELEASE_NOTES.md \
            --latest
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add post-release commit and GitHub Release creation"
```

- [ ] **Step 4: Push and verify the complete workflow on a real version bump**

The full end-to-end test requires a real version bump merged to `main`. The correct test sequence is:

1. Merge the current branch (which already has version `0.2.0` and content in `[Unreleased]`) to `main`
2. Watch the Actions tab — the workflow should complete all steps
3. Verify:
   - A tag `v0.2.0` exists: `git fetch --tags && git tag --list`
   - A GitHub Release `v0.2.0` exists with the correct release notes: `gh release view v0.2.0`
   - `CHANGELOG.md` on `main` has a fresh empty `## [Unreleased]` section at the top
   - The git log shows two bot commits after the version-bump merge commit: `chore: release v0.2.0` and `chore: prepare next development cycle`
   - `git show v0.2.0:CHANGELOG.md` contains `## [0.2.0]` and no `## [Unreleased]`

---

### Task 5: Wire up the `zizmor` security scan and pin action SHAs

**Files:**
- Modify: `.github/workflows/release.yml` (verify SHA pins, add zizmor scan)

Per global CLAUDE.md: GitHub Actions must have all actions pinned to full SHAs with version comments, and workflows must be scanned with `zizmor` before committing.

- [ ] **Step 1: Verify the `actions/checkout` SHA is current**

Look up the latest SHA for `actions/checkout@v4` at https://github.com/actions/checkout/releases and confirm the SHA used in Task 1 (`11bd71901bbe5b1630ceea73d27597364c9af683`) matches `v4.2.2` or update to the latest patch.

```bash
gh api repos/actions/checkout/git/refs/tags/v4.2.2 --jq '.object.sha'
```

If the SHA has changed (a new patch was released), update the workflow. The comment format is:
```yaml
uses: actions/checkout@<full-40-char-sha>  # v4.2.2
```

- [ ] **Step 2: Run zizmor against the workflow**

```bash
zizmor .github/workflows/release.yml
```

Expected: no findings, or only informational notes. Fix any warnings before proceeding.

Common zizmor findings for release workflows and their fixes:
- `unpinned-uses`: already handled by full SHA pins
- `excessive-permissions`: already scoped to `contents: write` only
- `github-env injection`: the `VERSION` value comes from `Cargo.toml` via `sed` — it contains only digits and dots, so injection is not possible, but zizmor may still flag it; add a validation step if needed:
  ```bash
  if ! [[ "$current" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "ERROR: version '$current' is not a valid semver" >&2; exit 1
  fi
  ```

- [ ] **Step 3: Run actionlint**

```bash
actionlint .github/workflows/release.yml
```

Expected: no errors. Fix any type or syntax issues actionlint reports before committing.

- [ ] **Step 4: Commit any fixes**

```bash
git add .github/workflows/release.yml
git commit -m "ci: verify SHA pins, pass zizmor and actionlint scans"
```

---

## Final State

After all tasks are complete, the workflow file at `.github/workflows/release.yml` is the only new file. Its complete behaviour on a push to `main`:

1. Detect version in `Cargo.toml` vs. parent commit — exit early if equal
2. Run Python script to validate `[Unreleased]` content and produce `CHANGELOG_release.md`, `CHANGELOG_next.md`, `RELEASE_NOTES.md`
3. Commit `CHANGELOG_release.md` as `CHANGELOG.md` → `chore: release vX.Y.Z`
4. Create annotated tag `vX.Y.Z` on that commit and push both
5. Commit `CHANGELOG_next.md` as `CHANGELOG.md` → `chore: prepare next development cycle` and push
6. Create GitHub Release `vX.Y.Z` with `--notes-file RELEASE_NOTES.md`

On every subsequent push to `main` where the version is unchanged, the workflow exits after step 1 with no side effects.
