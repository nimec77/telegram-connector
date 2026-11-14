"""Behavior checks for release metadata consistency; no Cargo/network required."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("check_version_docs.py")


class VersionDocsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.write("Cargo.toml", '[package]\nname = "telegram-connector"\nversion = "0.26.0"\n')
        self.write("Cargo.lock", 'version = 4\n\n[[package]]\nname = "telegram-connector"\nversion = "0.26.0"\n\n[[package]]\nname = "dependency"\nversion = "3.8.0"\n')
        self.write("CHANGELOG.md", "# Changelog\n\n## [Unreleased]\n\n## [0.26.0] - 2026-11-14\n\n### Added\n\n- Sending.\n\n## [0.25.0] - 2026-11-02\n\n- Artifacts.\n")
        self.write("README.md", "# Telegram MCP Connector\n\nCurrent package version: [0.26.0](CHANGELOG.md#0260---2026-11-14).\n\nSince version 0.23.0, tools use JSON outputs.\n")

    def write(self, name, content):
        (self.root / name).write_text(content, encoding="utf-8")

    def replace(self, name, old, new):
        self.write(name, (self.root / name).read_text(encoding="utf-8").replace(old, new))

    def run_check(self, expected_code, diagnostic="", *extra_args):
        before = {p.name: p.read_bytes() for p in self.root.iterdir()}
        result = subprocess.run(
            [sys.executable, str(SCRIPT), str(self.root), *extra_args],
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(result.returncode, expected_code, result.stdout + result.stderr)
        self.assertIn(diagnostic, result.stdout + result.stderr)
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.root.iterdir()})

    def test_consistent_metadata_accepts_historical_references(self):
        self.run_check(0, "0.26.0")

    def test_stale_root_lock_version_fails(self):
        self.replace("Cargo.lock", 'version = "0.26.0"', 'version = "0.25.0"')
        self.run_check(1, "Cargo.lock")

    def test_missing_readme_version_fails(self):
        self.write("README.md", "# Telegram MCP Connector\n\nSince version 0.23.0, tools use JSON outputs.\n")
        self.run_check(1, "README.md")

    def test_stale_readme_version_fails(self):
        self.replace("README.md", "[0.26.0]", "[0.25.0]")
        self.run_check(1, "README.md")

    def test_broken_readme_anchor_fails(self):
        self.replace("README.md", "0260---2026-11-14", "0260---2026-11-13")
        self.run_check(1, "README.md")

    def test_duplicate_readme_marker_fails(self):
        self.write("README.md", (self.root / "README.md").read_text() * 2)
        self.run_check(1, "README.md")

    def test_missing_release_section_fails(self):
        self.replace("CHANGELOG.md", "## [0.26.0] - 2026-11-14", "## [0.24.0] - 2026-11-14")
        self.run_check(1, "CHANGELOG.md")

    def test_current_release_must_be_first_dated_section(self):
        self.replace("CHANGELOG.md", "## [0.26.0]", "## [0.27.0] - 2026-11-15\n\n## [0.26.0]")
        self.run_check(1, "CHANGELOG.md")

    def test_explicit_downgrade_can_select_existing_historical_release(self):
        self.replace("CHANGELOG.md", "## [0.26.0]", "## [0.27.0] - 2026-11-15\n\n## [0.26.0]")
        self.run_check(0, "0.26.0", "--allow-older-release")

    def test_downgrade_flag_still_requires_matching_release(self):
        self.replace("CHANGELOG.md", "## [0.26.0]", "## [0.24.0]")
        self.run_check(1, "CHANGELOG.md", "--allow-older-release")

    def test_duplicate_current_release_fails(self):
        self.replace("CHANGELOG.md", "## [0.25.0] - 2026-11-02", "## [0.26.0] - 2026-11-13")
        self.run_check(1, "CHANGELOG.md")

    def test_missing_unreleased_fails(self):
        self.replace("CHANGELOG.md", "## [Unreleased]\n\n", "")
        self.run_check(1, "Unreleased")

    def test_invalid_release_date_fails(self):
        self.replace("CHANGELOG.md", "2026-11-14", "2026-02-30")
        self.run_check(1, "date")

    def test_unreleased_work_does_not_break_current_version_check(self):
        self.replace("CHANGELOG.md", "## [Unreleased]\n", "## [Unreleased]\n\n### Fixed\n\n- Next release work.\n")
        self.run_check(0, "0.26.0")

    def test_prerelease_build_metadata_uses_github_anchor(self):
        for name in ("Cargo.toml", "Cargo.lock", "CHANGELOG.md", "README.md"):
            self.replace(name, "0.26.0", "0.27.0-rc.1+build.2")
        self.replace("README.md", "0260---", "0270-rc1build2---")
        self.run_check(0, "0.27.0-rc.1+build.2")

    def test_missing_file_has_actionable_diagnostic(self):
        (self.root / "CHANGELOG.md").unlink()
        self.run_check(1, "CHANGELOG.md")

    def test_old_python_requests_supported_interpreter(self):
        legacy_python = Path("/usr/bin/python3")
        if not legacy_python.exists():
            self.skipTest("No system Python available for compatibility diagnostic")
        probe = subprocess.run(
            [str(legacy_python), "-c", "import sys; sys.exit(sys.version_info >= (3, 11))"],
            capture_output=True, check=False,
        )
        if probe.returncode:
            self.skipTest("System Python already supports tomllib")
        result = subprocess.run(
            [str(legacy_python), str(SCRIPT), str(self.root)],
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("requires Python 3.11+", result.stderr)
        self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
