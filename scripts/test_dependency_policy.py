"""Observable acceptance/rejection tests for the external dependency policy."""
import copy
import datetime
import json
from pathlib import Path
import unittest

from check_dependency_policy import PolicyError, validate_metadata

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
SHA = "1e31e73cd9797790905bbea82cd75adcbe6f1242"
TODAY = datetime.date(2026, 10, 31)


def metadata():
    return {
        "version": 1,
        "packages": [
            {"id": "root", "name": "telegram-connector", "version": "0.22.10", "source": None, "license": None},
            {"id": "dep", "name": "example-dependency", "version": "1.0.0", "source": REGISTRY, "license": "MIT"},
        ],
        "resolve": {"root": "root", "nodes": [
            {"id": "root", "dependencies": ["dep"], "deps": [{"name": "example_dependency", "pkg": "dep", "dep_kinds": [{"kind": None, "target": None}]}]},
            {"id": "dep", "dependencies": [], "deps": []},
        ]},
    }


class DependencyPolicyTests(unittest.TestCase):
    def setUp(self):
        self.policy = json.loads((Path(__file__).resolve().parents[1] / "dependency-policy.json").read_text())
        self.metadata = metadata()

    def check(self):
        return validate_metadata(self.metadata, self.policy, TODAY)

    def assert_code(self, code):
        self.assertIn(code, [issue["code"] for issue in self.check()])

    def test_exact_unlicensed_local_root_and_reviewed_external_are_accepted(self):
        self.assertEqual(self.check(), [])

    def test_root_exemption_cannot_be_retargeted_by_policy(self):
        self.policy["root_package"] = "other-project"
        self.metadata["packages"][0]["name"] = "other-project"
        with self.assertRaises(PolicyError): self.check()

    def test_invalid_license_choice_cannot_be_approved(self):
        self.policy["reviewed_license_choices"]["MIT"] = [True]
        with self.assertRaises(PolicyError): self.check()

    def test_external_missing_license_is_rejected(self):
        self.metadata["packages"][1]["license"] = None
        self.assert_code("dependency_unlicensed")

    def test_unlicensed_path_dependency_is_not_exempt(self):
        self.metadata["packages"][1].update(source=None, license=None)
        self.assert_code("dependency_unlicensed")
        self.assert_code("source_not_allowed")

    def test_root_name_cannot_be_changed_to_gain_exemption(self):
        self.metadata["packages"][0]["name"] = "other-project"
        with self.assertRaises(PolicyError): self.check()

    def test_root_source_must_be_local(self):
        self.metadata["packages"][0]["source"] = REGISTRY
        with self.assertRaises(PolicyError): self.check()

    def test_missing_resolve_graph_is_rejected(self):
        self.metadata["resolve"] = None
        with self.assertRaises(PolicyError): self.check()

    def test_incomplete_graph_is_rejected(self):
        self.metadata["resolve"]["nodes"].pop()
        with self.assertRaises(PolicyError): self.check()

    def test_removing_package_and_node_cannot_leave_a_dangling_dependency(self):
        self.metadata["packages"].pop()
        self.metadata["resolve"]["nodes"].pop()
        with self.assertRaises(PolicyError): self.check()

    def test_dependency_and_named_edge_references_must_agree(self):
        self.metadata["resolve"]["nodes"][0]["deps"] = []
        with self.assertRaises(PolicyError): self.check()

    def test_node_edge_fields_cannot_be_omitted(self):
        del self.metadata["resolve"]["nodes"][0]["dependencies"]
        with self.assertRaises(PolicyError): self.check()

    def test_duplicate_package_ids_are_rejected(self):
        self.metadata["packages"].append(copy.deepcopy(self.metadata["packages"][0]))
        with self.assertRaises(PolicyError): self.check()

    def test_unknown_registry_is_rejected(self):
        self.metadata["packages"][1]["source"] = "registry+https://unreviewed.invalid/index"
        self.assert_code("source_not_allowed")

    def test_official_grammers_immutable_revision_is_accepted(self):
        self.metadata["packages"][1]["source"] = "git+https://codeberg.org/Lonami/grammers?rev=" + SHA + "#" + SHA
        self.assertEqual(self.check(), [])

    def test_git_branch_tracking_is_rejected(self):
        self.metadata["packages"][1]["source"] = "git+https://codeberg.org/Lonami/grammers?branch=master#" + SHA
        self.assert_code("source_not_allowed")

    def test_git_revision_must_match_resolved_commit(self):
        self.metadata["packages"][1]["source"] = "git+https://codeberg.org/Lonami/grammers?rev=" + SHA + "#" + "0" * 40
        self.assert_code("source_not_allowed")

    def test_same_git_repository_must_use_one_revision(self):
        self.metadata["packages"][1]["source"] = "git+https://codeberg.org/Lonami/grammers?rev=" + SHA + "#" + SHA
        other = copy.deepcopy(self.metadata["packages"][1])
        other.update(id="other", name="other-grammers-package", source="git+https://codeberg.org/Lonami/grammers?rev=" + "0" * 40 + "#" + "0" * 40)
        self.metadata["packages"].append(other)
        self.metadata["resolve"]["nodes"].append({"id": "other", "dependencies": [], "deps": []})
        self.assert_code("git_revision_mismatch")

    def test_stale_github_mirror_is_rejected(self):
        self.metadata["packages"][1]["source"] = "git+https://github.com/Lonami/grammers?rev=" + SHA + "#" + SHA
        self.assert_code("source_not_allowed")

    def test_lookalike_git_repository_is_rejected(self):
        self.metadata["packages"][1]["source"] = "git+https://codeberg.org/Lonami/grammers-other?rev=" + SHA + "#" + SHA
        self.assert_code("source_not_allowed")

    def test_unknown_license_or_combined_obligation_is_rejected(self):
        for expression in ["Proprietary", "MIT AND Proprietary", "MIT OR Proprietary"]:
            self.metadata["packages"][1]["license"] = expression
            self.assert_code("license_unreviewed")

    def test_unicode_combined_obligation_is_reviewed_as_a_whole(self):
        self.metadata["packages"][1]["license"] = "(MIT OR Apache-2.0) AND Unicode-3.0"
        self.assertEqual(self.check(), [])

    def test_only_whitespace_is_normalized(self):
        self.metadata["packages"][1]["license"] = "MIT  OR  Apache-2.0"
        self.assertEqual(self.check(), [])

    def test_mpl_exception_is_exact_and_version_scoped(self):
        package = self.metadata["packages"][1]
        package.update(name="option-ext", version="0.2.0", license="MPL-2.0")
        self.assertEqual(self.check(), [])
        package["version"] = "0.2.1"
        self.assert_code("license_unreviewed")
        package.update(name="other-crate", version="0.2.0")
        self.assert_code("license_unreviewed")

    def test_expired_license_exception_is_rejected(self):
        self.metadata["packages"][1].update(name="option-ext", version="0.2.0", license="MPL-2.0")
        self.policy["license_exceptions"][0]["expires"] = "2026-10-30"
        self.assert_code("license_exception_expired")


if __name__ == "__main__": unittest.main()
