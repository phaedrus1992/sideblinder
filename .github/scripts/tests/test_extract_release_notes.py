"""Tests for extract_release_notes.py.

Each test runs the script as a subprocess in a temporary directory with a
synthetic CHANGELOG.md and checks exit code + output file contents.
"""

import os
import subprocess
import sys
from pathlib import Path

SCRIPT = Path(__file__).parent.parent / "extract_release_notes.py"


def run_script(
    tmp_path: Path, changelog: str, version: str | None = None
) -> subprocess.CompletedProcess[str]:
    """Run the extraction script in tmp_path against the given CHANGELOG content.

    Args:
        tmp_path: Temporary directory for the test.
        changelog: CHANGELOG.md content to write before running the script.
        version: VALUE for the VERSION env var. If None, VERSION is omitted
            from the environment (tests the missing-env-var error path).
    """
    (tmp_path / "CHANGELOG.md").write_text(changelog, encoding="utf-8")
    env = {k: v for k, v in os.environ.items() if k != "VERSION"}
    if version is not None:
        env["VERSION"] = version
    return subprocess.run(
        [sys.executable, str(SCRIPT)],
        cwd=tmp_path,
        env=env,
        capture_output=True,
        text=True,
    )


# ── Sample CHANGELOGs ─────────────────────────────────────────────────────────

CHANGELOG_VERSIONED = """\
# Changelog

## [Unreleased]

## [0.8.0] - 2026-04-15

### Added
- New feature A

### Changed
- Behaviour B updated

[Unreleased]: https://github.com/x/y/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/x/y/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/x/y/commits/v0.7.0
"""

CHANGELOG_UNRELEASED_ONLY = """\
# Changelog

## [Unreleased]

### Added
- Work in progress

[Unreleased]: https://github.com/x/y/compare/v0.7.0...HEAD
"""

CHANGELOG_EMPTY_UNRELEASED = """\
# Changelog

## [Unreleased]

[Unreleased]: https://github.com/x/y/compare/v0.7.0...HEAD
"""

CHANGELOG_MULTIPLE_VERSIONS = """\
# Changelog

## [Unreleased]

## [0.8.0] - 2026-04-15

### Changed
- Release 0.8.0 content

## [0.7.0] - 2026-04-14

### Added
- Release 0.7.0 content

[Unreleased]: https://github.com/x/y/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/x/y/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/x/y/commits/v0.7.0
"""


# ── Happy path ────────────────────────────────────────────────────────────────

def test_extracts_versioned_section(tmp_path):
    result = run_script(tmp_path, CHANGELOG_VERSIONED, "v0.8.0")

    assert result.returncode == 0
    notes = (tmp_path / "RELEASE_NOTES.md").read_text(encoding="utf-8")
    assert "New feature A" in notes
    assert "Behaviour B updated" in notes


def test_version_without_v_prefix(tmp_path):
    """VERSION env var without leading 'v' is accepted."""
    result = run_script(tmp_path, CHANGELOG_VERSIONED, "0.8.0")

    assert result.returncode == 0
    notes = (tmp_path / "RELEASE_NOTES.md").read_text(encoding="utf-8")
    assert "New feature A" in notes
    assert "Behaviour B updated" in notes


def test_extracts_correct_version_from_multiple(tmp_path):
    """With multiple versioned sections, only the requested one is extracted."""
    result = run_script(tmp_path, CHANGELOG_MULTIPLE_VERSIONS, "v0.7.0")

    assert result.returncode == 0
    notes = (tmp_path / "RELEASE_NOTES.md").read_text(encoding="utf-8")
    assert "Release 0.7.0 content" in notes
    assert "Release 0.8.0 content" not in notes


def test_strips_reference_definition_footer(tmp_path):
    """Reference-definition lines at the end of a section body are excluded."""
    result = run_script(tmp_path, CHANGELOG_VERSIONED, "v0.8.0")

    assert result.returncode == 0
    notes = (tmp_path / "RELEASE_NOTES.md").read_text(encoding="utf-8")
    assert "[Unreleased]:" not in notes
    assert "[0.8.0]:" not in notes


def test_output_ends_with_newline(tmp_path):
    """RELEASE_NOTES.md always ends with a single newline."""
    result = run_script(tmp_path, CHANGELOG_VERSIONED, "v0.8.0")

    assert result.returncode == 0
    raw = (tmp_path / "RELEASE_NOTES.md").read_bytes()
    assert raw.endswith(b"\n")
    assert not raw.endswith(b"\n\n")


# ── [Unreleased] fallback ─────────────────────────────────────────────────────

def test_falls_back_to_unreleased_when_no_versioned_sections(tmp_path):
    """When CHANGELOG has only [Unreleased], use it regardless of VERSION."""
    result = run_script(tmp_path, CHANGELOG_UNRELEASED_ONLY, "v0.8.0")

    assert result.returncode == 0
    notes = (tmp_path / "RELEASE_NOTES.md").read_text(encoding="utf-8")
    assert "Work in progress" in notes


# ── Error paths ───────────────────────────────────────────────────────────────

def test_error_when_version_not_found_and_versioned_sections_exist(tmp_path):
    """If versioned sections exist but none match the tag, fail — no fallback."""
    result = run_script(tmp_path, CHANGELOG_VERSIONED, "v0.9.0")

    assert result.returncode != 0
    assert not (tmp_path / "RELEASE_NOTES.md").exists()
    assert "versioned sections exist" in result.stderr


def test_error_when_changelog_has_no_sections(tmp_path):
    """A CHANGELOG with no ## [ sections at all fails."""
    changelog = "# Changelog\n\nNothing here yet.\n"
    result = run_script(tmp_path, changelog, "v0.8.0")

    assert result.returncode != 0
    assert not (tmp_path / "RELEASE_NOTES.md").exists()
    assert "no section" in result.stderr or "ERROR" in result.stderr


def test_error_when_matching_section_is_empty(tmp_path):
    """A [x.y.z] section whose body is only reference-definition lines is an error.

    clean_body strips the footer link, leaving an empty body. The empty-body check
    at line 69 of extract_release_notes.py catches this and exits non-zero.
    """
    changelog = """\
# Changelog

## [0.8.0] - 2026-04-15

[0.8.0]: https://github.com/x/y/commits/v0.8.0
"""
    result = run_script(tmp_path, changelog, "v0.8.0")

    assert result.returncode != 0
    assert not (tmp_path / "RELEASE_NOTES.md").exists()


def test_error_when_unreleased_section_is_empty(tmp_path):
    """[Unreleased] fallback also fails if the section body is empty."""
    result = run_script(tmp_path, CHANGELOG_EMPTY_UNRELEASED, "v0.8.0")

    assert result.returncode != 0
    assert not (tmp_path / "RELEASE_NOTES.md").exists()


def test_error_when_version_env_var_missing(tmp_path):
    """Missing VERSION env var causes immediate failure."""
    result = run_script(tmp_path, CHANGELOG_VERSIONED, version=None)

    assert result.returncode != 0
    assert "VERSION" in result.stderr
