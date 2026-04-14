"""Shared utilities for coverage scripts."""

from __future__ import annotations

import json
import sys
from collections import defaultdict
from pathlib import Path


def load_per_crate_coverage(coverage_path: Path) -> dict[str, float]:
    """Parse cargo-llvm-cov JSON and return per-crate line coverage percent.

    Exits with code 2 if the file cannot be read or the JSON structure is unexpected.
    """
    try:
        data = json.loads(coverage_path.read_text())
        files = data["data"][0]["files"]
    except OSError as exc:
        print(f"ERROR: failed to read {coverage_path}: {exc}", file=sys.stderr)
        sys.exit(2)
    except json.JSONDecodeError as exc:
        print(f"ERROR: {coverage_path} is not valid JSON: {exc}", file=sys.stderr)
        sys.exit(2)
    except (KeyError, IndexError) as exc:
        print(
            f"ERROR: {coverage_path} has unexpected structure: {exc}",
            file=sys.stderr,
        )
        print(
            "Expected cargo-llvm-cov --workspace --json output. "
            "Check that cargo llvm-cov ran successfully.",
            file=sys.stderr,
        )
        sys.exit(2)

    totals: dict[str, dict[str, int]] = defaultdict(lambda: {"count": 0, "covered": 0})
    for i, f in enumerate(files):
        try:
            filename = f["filename"]
            lines = f["summary"]["lines"]
            count = lines["count"]
            covered = lines["covered"]
        except (KeyError, TypeError) as exc:
            print(
                f"ERROR: {coverage_path} has unexpected structure in files[{i}]: {exc}",
                file=sys.stderr,
            )
            print(
                "Expected cargo-llvm-cov --workspace --json output. "
                "Check that cargo llvm-cov ran successfully.",
                file=sys.stderr,
            )
            sys.exit(2)

        # Normalise path separators so this works on both Unix and Windows hosts.
        parts = Path(filename.replace("\\", "/")).parts
        try:
            idx = parts.index("crates")
            crate = parts[idx + 1]
        except (ValueError, IndexError):
            continue
        totals[crate]["count"] += count
        totals[crate]["covered"] += covered

    return {
        crate: (100.0 * d["covered"] / d["count"] if d["count"] else 0.0)
        for crate, d in totals.items()
    }


def to_tenths(pct: float) -> int:
    """Convert a coverage percentage to integer tenths, rounding to nearest.

    Using integer tenths for all comparisons avoids IEEE 754 representation
    surprises (e.g. values that should be equal at 1 decimal comparing as
    slightly less or greater).
    """
    return round(pct * 10)


def serialize_baseline(baseline: dict[str, float]) -> str:
    """Serialize baseline dict to JSON with stable 1-decimal float representation."""
    # Use string formatting to guarantee exactly one decimal place, avoiding
    # IEEE 754 representation surprises (e.g. round(86.35, 1) → 86.35000000001).
    stable = {k: float(f"{v:.1f}") for k, v in baseline.items()}
    return json.dumps(stable, indent=2, sort_keys=True) + "\n"
