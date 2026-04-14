#!/usr/bin/env python3
"""Check per-crate line coverage against a stored baseline.

Usage:
    check-coverage.py <coverage.json> <baseline.json>

Exit codes:
    0 — all crates pass (at or above baseline; warnings printed but non-fatal)
    1 — one or more crates below baseline, or untracked crates in coverage output
    2 — usage error or malformed input
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from coverage_utils import load_per_crate_coverage, to_tenths


ABSOLUTE_FLOOR = 80.0
NOTICE_THRESHOLD = 1.0  # suggest update if this many points above baseline


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
            f"ERROR: failed to parse baseline JSON from {baseline_path}: {exc}", file=sys.stderr
        )
        return 2

    # Ensure every baseline crate appears in coverage output.
    missing = set(baseline) - set(actual)
    if missing:
        print(
            f"ERROR: crates in baseline not found in coverage output: {', '.join(sorted(missing))}"
        )
        print("Did you run 'cargo llvm-cov --workspace'?")
        return 1

    # Crates in coverage output but not in baseline must be explicitly added.
    # Silently skipping them would let new crates escape enforcement.
    extra = set(actual) - set(baseline)
    if extra:
        for crate in sorted(extra):
            print(
                f"ERROR: {crate} has coverage data but no baseline entry — "
                f"add it to {baseline_path.name}"
            )
        return 1

    if not baseline:
        print("WARNING: baseline is empty — nothing to check")
        return 0

    failed = False
    rows = []

    floor_tenths = int(to_tenths(ABSOLUTE_FLOOR))

    for crate in sorted(baseline):
        # Use integer tenths for all comparisons to avoid IEEE 754 boundary surprises.
        actual_tenths = to_tenths(actual[crate])
        floor = baseline[crate]
        floor_t = to_tenths(floor)
        pct = actual_tenths / 10.0
        status_parts = []

        if actual_tenths < floor_t:
            # Regression against stored baseline — always a hard failure.
            status_parts.append(f"FAIL regression ({pct:.1f}% < baseline {floor:.1f}%)")
            failed = True
        elif actual_tenths < floor_tenths:
            # Below the 80% target. Hard failure only once the baseline itself
            # has reached 80% (i.e., the crate has previously passed the floor).
            if floor_t >= floor_tenths:
                status_parts.append(f"FAIL below 80% floor ({pct:.1f}%)")
                failed = True
            else:
                status_parts.append(f"WARN below 80% target ({pct:.1f}%)")
        else:
            status_parts.append("OK")

        # Notice to update baseline if coverage has improved meaningfully.
        if actual_tenths > floor_t + to_tenths(NOTICE_THRESHOLD):
            status_parts.append(f"(+{pct - floor:.1f}% — run update-coverage-baseline.py)")

        rows.append((crate, pct, floor, " ".join(status_parts)))

    # Print aligned table.
    col_w = max(len(r[0]) for r in rows)
    print(f"\n{'Crate':<{col_w}}  {'Actual':>7}  {'Baseline':>8}  Status")
    print("-" * (col_w + 32))
    for crate, pct, floor, status in rows:
        print(f"{crate:<{col_w}}  {pct:>6.1f}%  {floor:>7.1f}%  {status}")
    print()

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
