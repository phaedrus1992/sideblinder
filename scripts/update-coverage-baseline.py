#!/usr/bin/env python3
"""Raise per-crate coverage baseline entries to match current actuals.

Usage:
    update-coverage-baseline.py <coverage.json> <baseline.json>

Exit codes:
    0 — nothing changed (all crates at or below their baseline)
    1 — one or more baseline entries were raised (baseline.json was modified)
    2 — error (e.g., coverage decreased below a baseline entry, or parse error)

The pre-push hook uses exit code 1 to detect that coverage-baseline.json
was modified and a commit is required before pushing.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from coverage_utils import load_per_crate_coverage, serialize_baseline, to_tenths


def main() -> int:
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <coverage.json> <baseline.json>", file=sys.stderr)
        return 2

    coverage_path = Path(sys.argv[1])
    baseline_path = Path(sys.argv[2])

    actual = load_per_crate_coverage(coverage_path)

    try:
        baseline: dict[str, float] = json.loads(baseline_path.read_text())
    except OSError as exc:
        print(f"ERROR: failed to read baseline file {baseline_path}: {exc}", file=sys.stderr)
        return 2
    except json.JSONDecodeError as exc:
        print(
            f"ERROR: failed to parse baseline file {baseline_path}: {exc}", file=sys.stderr
        )
        return 2

    updated = False
    error = False

    for crate, floor in sorted(baseline.items()):
        if crate not in actual:
            print(
                f"ERROR: {crate} is in baseline but not in coverage output",
                file=sys.stderr,
            )
            error = True
            continue

        # Use integer tenths for comparisons to avoid IEEE 754 boundary surprises.
        actual_tenths = to_tenths(actual[crate])
        floor_tenths = to_tenths(floor)
        pct = actual_tenths / 10.0

        if actual_tenths > floor_tenths:
            print(f"{crate}: {floor:.1f}% → {pct:.1f}% (+{pct - floor:.1f}%)")
            baseline[crate] = pct
            updated = True
        elif actual_tenths < floor_tenths:
            # Should not happen if check-coverage.py passed first.
            print(
                f"ERROR: {crate} coverage {pct:.1f}% is below baseline {floor:.1f}%",
                file=sys.stderr,
            )
            print("Run check-coverage.py first to diagnose.", file=sys.stderr)
            error = True
        else:
            print(f"{crate}: {floor:.1f}% (unchanged)")

    if error:
        return 2

    if updated:
        baseline_path.write_text(serialize_baseline(baseline))
        print(f"\nUpdated {baseline_path}")
        print("Review the changes with 'git diff coverage-baseline.json' and commit.")

    return 1 if updated else 0


if __name__ == "__main__":
    sys.exit(main())
