#!/usr/bin/env python3
"""Transform CHANGELOG.md for a release.

Reads VERSION from the environment. Writes three files:
- RELEASE_NOTES.md: body of the [Unreleased] section (no heading)
- CHANGELOG_release.md: CHANGELOG with [Unreleased] renamed to the versioned heading
- CHANGELOG_next.md: CHANGELOG_release.md with a fresh empty [Unreleased] section prepended
"""

import sys
import re
import os
from datetime import date

version = os.environ.get("VERSION")
if not version:
    print("ERROR: VERSION environment variable is required (e.g. VERSION=0.2.0)", file=sys.stderr)
    sys.exit(1)

today = date.today().isoformat()
repo = "https://github.com/phaedrus/sideblinder"

with open("CHANGELOG.md") as f:
    text = f.read()

# Split on section headings (lines starting with ## [).
# Interleaved result: [preamble, "## [", "Unreleased]...", "## [", "0.1.0]...", ...]
parts = re.split(r'^(## \[)', text, flags=re.MULTILINE)

if len(parts) < 3 or not parts[2].startswith("Unreleased]"):
    print("ERROR: CHANGELOG.md has no [Unreleased] section", file=sys.stderr)
    sys.exit(1)

unreleased_chunk = parts[2]  # "Unreleased]\n...<body>..."
raw_body = unreleased_chunk.split("\n", 1)[1] if "\n" in unreleased_chunk else ""

# Strip trailing reference-definition lines (e.g. "[Unreleased]: https://...").
# When [Unreleased] is the only section, re.split gives us the entire remainder of
# the file as parts[2], including any footer link definitions. Those belong to the
# document footer, not to the section body.
body_lines = raw_body.splitlines(keepends=True)
while body_lines and re.match(r'^\[[^\]]+\]:', body_lines[-1]):
    body_lines.pop()
# Also strip trailing blank lines so body is clean section content only
while body_lines and not body_lines[-1].strip():
    body_lines.pop()
body = "".join(body_lines) + ("\n" if body_lines else "")

# Check body has real content (non-blank, non-heading lines)
real_content = [
    line for line in body.splitlines()
    if line.strip() and not line.startswith("#")
]
if not real_content:
    print("ERROR: [Unreleased] section is empty — nothing to release", file=sys.stderr)
    sys.exit(1)

# ── Build release notes (body only, no heading) ───────────────────
release_notes = body.strip()
with open("RELEASE_NOTES.md", "w") as f:
    f.write(release_notes)

# ── Build CHANGELOG_release.md ────────────────────────────────────
# Replace the Unreleased heading+body with versioned heading+body
versioned_chunk = f"{version}] - {today}\n{body}"
release_parts = list(parts)
release_parts[2] = versioned_chunk

# Detect previous tag from existing versioned footer links in the original text.
# re.search finds the first match; Keep a Changelog convention is newest-first,
# so the first match is the immediately preceding release — the correct compare base.
prev_tag_match = re.search(
    r'^\[([0-9]+\.[0-9]+\.[0-9]+)\]:',
    text,
    re.MULTILINE,
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
release_text, n_subs = re.subn(
    r'^\[Unreleased\]:.*$',
    unreleased_link + "\n" + version_link,
    release_text,
    flags=re.MULTILINE,
)
if n_subs == 0:
    release_text = (
        release_text.rstrip("\n") + "\n\n" + unreleased_link + "\n" + version_link + "\n"
    )
with open("CHANGELOG_release.md", "w") as f:
    f.write(release_text)

# ── Build CHANGELOG_next.md ───────────────────────────────────────
# Insert fresh empty [Unreleased] section after the preamble.
# release_parts[0] is the preamble (text before the first ## [ heading).
# release_text starts with that same preamble, so slicing by its length
# gives everything after the preamble in the modified document.
preamble = release_parts[0]
next_text = preamble + "## [Unreleased]\n\n" + release_text[len(preamble):]
with open("CHANGELOG_next.md", "w") as f:
    f.write(next_text)

print(f"OK: Prepared CHANGELOG for release v{version}")
print(f"  Release notes: {len(real_content)} content lines")
