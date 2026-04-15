#!/usr/bin/env python3
"""Extract release notes for a version tag from CHANGELOG.md.

Reads VERSION from the environment (with or without a leading 'v').
Looks for either the matching versioned section [x.y.z] or the [Unreleased]
section and writes its body to RELEASE_NOTES.md. The CHANGELOG is not modified.
"""

import re
import os
import sys

raw_version = os.environ.get("VERSION", "")
if not raw_version:
    print("ERROR: VERSION environment variable is required", file=sys.stderr)
    sys.exit(1)

# Accept tags like v0.8.0 or plain 0.8.0
version = raw_version.lstrip("v")

with open("CHANGELOG.md") as f:
    text = f.read()

# Split on section headings (## [).
# Interleaved result: [preamble, "## [", "Unreleased]...", "## [", "0.7.0]..."]
parts = re.split(r"^(## \[)", text, flags=re.MULTILINE)

# Build a list of (heading_suffix, body) pairs from the interleaved split.
sections: list[tuple[str, str]] = []
i = 1
while i + 1 < len(parts):
    chunk = parts[i + 1]          # e.g. "0.8.0] - 2026-04-15\n<body>"
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
for heading, body_raw in sections:
    # Match versioned section: "0.8.0] - 2026-04-15"
    if heading.startswith(f"{version}]"):
        body = clean_body(body_raw)
        break
    # Fall back to [Unreleased] if no versioned section found yet
    if heading.startswith("Unreleased]"):
        body = clean_body(body_raw)
        # Don't break — a versioned section may appear later and takes priority

if not body:
    print(
        f"ERROR: CHANGELOG.md has no section for v{version} or [Unreleased]",
        file=sys.stderr,
    )
    sys.exit(1)

real_content = [
    line for line in body.splitlines()
    if line.strip() and not line.startswith("#")
]
if not real_content:
    print(
        f"ERROR: section for v{version} is empty — nothing to release",
        file=sys.stderr,
    )
    sys.exit(1)

with open("RELEASE_NOTES.md", "w") as f:
    f.write(body.strip() + "\n")

print(f"OK: Extracted release notes for v{version} ({len(real_content)} content lines)")
