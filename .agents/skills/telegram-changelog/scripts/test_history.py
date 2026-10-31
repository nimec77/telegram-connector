"""Exercise release-boundary discovery using isolated, disposable Git histories."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("commits_since_release.sh")

def git(repo, *args):
    return subprocess.check_output(
        ["git", "-c", f"core.hooksPath={os.devnull}", "-c", "tag.gpgSign=false",
         "-C", str(repo), *args],
        text=True,
        stderr=subprocess.STDOUT,
    ).strip()

class HistoryTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="telegram history ")
        self.repo = Path(self.tmp.name)
        git(self.repo, "init", "-q")
        git(self.repo, "config", "user.name", "Fixture")
        git(self.repo, "config", "user.email", "fixture@example.invalid")
        self.commit("feat: initial")
    def tearDown(self):
        self.tmp.cleanup()
    def commit(self, subject):
        git(self.repo, "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", subject)
        return git(self.repo, "rev-parse", "HEAD")
    def run_helper(self):
        return subprocess.check_output(
            ["bash", str(SCRIPT), str(self.repo)], cwd=tempfile.gettempdir(), text=True
        )
    def test_no_boundary_includes_history(self):
        output = self.run_helper()
        self.assertIn("COMMIT COUNT: 1", output)
        self.assertIn("feat: initial", output)
    def test_tag_excludes_released_history(self):
        git(self.repo, "tag", "v0.1.0")
        self.commit("fix: new")
        output = self.run_helper()
        self.assertIn("COMMIT COUNT: 1", output)
        self.assertIn("fix: new", output)
        self.assertNotIn("feat: initial", output.split("COMMITS (", 1)[-1])
    def test_newer_release_commit_beats_stale_tag(self):
        git(self.repo, "tag", "v0.1.0")
        self.commit("feat: released")
        self.commit("chore: release v0.2.0")
        self.commit("fix: unreleased")
        output = self.run_helper()
        self.assertIn("COMMIT COUNT: 1", output)
        self.assertIn("fix: unreleased", output)
        self.assertNotIn("feat: released", output)
    def test_newer_tag_beats_old_release_commit(self):
        self.commit("chore: release v0.1.0")
        self.commit("feat: released in next version")
        git(self.repo, "tag", "v0.2.0")
        self.commit("fix: next")
        output = self.run_helper()
        self.assertIn("COMMIT COUNT: 1", output)
        self.assertIn("fix: next", output)
    def test_non_version_tag_is_not_release(self):
        git(self.repo, "tag", "checkpoint")
        self.commit("fix: next")
        output = self.run_helper()
        self.assertIn("COMMIT COUNT: 2", output)
    def test_release_commit_without_tag(self):
        self.commit("chore: release v0.1.0")
        self.commit("feat: next")
        output = self.run_helper()
        self.assertIn("COMMIT COUNT: 1", output)
        self.assertIn("feat: next", output)
    def test_zero_does_not_claim_changelog_complete(self):
        git(self.repo, "tag", "v0.1.0")
        output = self.run_helper()
        self.assertIn("COMMIT COUNT: 0", output)
        self.assertNotIn("already up to date", output)
    def test_release_mentioned_only_in_body_is_not_boundary(self):
        git(self.repo, "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", "docs: explain", "-m", "chore: release v0.1.0")
        output = self.run_helper()
        self.assertIn("COMMIT COUNT: 2", output)
    def test_non_repository_rejected(self):
        with tempfile.TemporaryDirectory() as outside:
            result=subprocess.run(["bash", str(SCRIPT), outside], capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not inside a git repository", result.stderr)

if __name__ == "__main__":
    unittest.main()
