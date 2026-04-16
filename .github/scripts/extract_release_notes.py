#!/usr/bin/env python3
"""Extract release notes for a version tag from CHANGELOG.md.

Reads VERSION from the environment (with or without a leading 'v').
Looks for the matching versioned section [x.y.z], falling back to [Unreleased]
only if no versioned sections exist at all. Writes the body to RELEASE_NOTES.md.
The CHANGELOG is not modified.
"""

import os
import re
import sys

raw_version = os.environ.get("VERSION", "")
if not raw_version:
    print("ERROR: VERSION environment variable is required", file=sys.stderr)
    sys.exit(1)

# Accept tags like v0.8.0 or plain 0.8.0
version = raw_version.lstrip("v")

with open("CHANGELOG.md", encoding="utf-8") as f:
    text = f.read()

# Split on section headings (## [).
# Interleaved result: [preamble, "## [", "Unreleased]...", "## [", "0.7.0]..."]
parts = re.split(r"^(## \[)", text, flags=re.MULTILINE)

# Build a list of (heading_suffix, body_raw) pairs from the interleaved split.
sections: list[tuple[str, str]] = []
i = 1
while i + 1 < len(parts):
    chunk = parts[i + 1]  # e.g. "0.8.0] - 2026-04-15\n<body>"
    heading, _, body_raw = chunk.partition("\n")
    sections.append((heading, body_raw))
    i += 2


def clean_body(raw: str) -> str:
    lines = raw.splitlines(keepends=True)
    # Strip trailing reference-definition lines (e.g. "[Unreleased]: https://...")
    while lines and re.match(r"^\[[^\]]+\]:", lines[-1]):
        lines.pop()
    while lines and not lines[-1].strip():
        lines.pop()
    return "".join(lines)


body = ""
unreleased_body = ""
found_versioned = False

for heading, body_raw in sections:
    if heading.startswith("Unreleased]"):
        # Save [Unreleased] as a last resort, but never prefer it over a versioned match.
        unreleased_body = clean_body(body_raw)
    else:
        found_versioned = True
        if heading.startswith(f"{version}]"):
            body = clean_body(body_raw)
            break

# Only fall back to [Unreleased] if there are no versioned sections at all.
# If versioned sections exist but none match, the tag is wrong — don't silently
# release the wrong content.
if not body and not found_versioned:
    body = unreleased_body

if not body:
    print(
        f"ERROR: CHANGELOG.md has no section for v{version}"
        + (" (versioned sections exist; [Unreleased] fallback is disabled)" if found_versioned else ""),
        file=sys.stderr,
    )
    sys.exit(1)

with open("RELEASE_NOTES.md", "w", encoding="utf-8") as f:
    f.write(body.strip() + "\n")

print(f"OK: Extracted release notes for v{version}")
