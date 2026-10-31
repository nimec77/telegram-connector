#!/usr/bin/env python3
"""Check complete Cargo metadata against reviewed external-dependency declarations.

This checks license metadata, not source license texts or distribution compliance.
Only this application's exact local root identity is exempt from license checks.
"""
import argparse
import datetime
import json
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import parse_qs, urlsplit, urlunsplit

ROOT_PACKAGE = "telegram-connector"
STANDARD_CHOICES = {"MIT", "Apache-2.0", "BSD-3-Clause", "ISC", "Unicode-3.0"}
PROJECT_ROOT = Path(__file__).resolve().parents[1]


class PolicyError(ValueError):
    """The policy/metadata cannot establish a complete project dependency graph."""


def normalized(expression):
    return " ".join(expression.split())


def allowed_source(source, policy):
    if source in policy["registries"]:
        return True
    if not isinstance(source, str) or not source.startswith("git+"):
        return False
    try:
        parsed = urlsplit(source[4:])
        query = parse_qs(parsed.query, strict_parsing=True)
        base = urlunsplit((parsed.scheme, parsed.netloc, parsed.path, "", ""))
        revision = query.get("rev", [])
        return (
            base in policy["git_repositories"]
            and parsed.username is None and parsed.password is None
            and set(query) == {"rev"} and len(revision) == 1
            and re.fullmatch(r"[0-9a-fA-F]{40}", revision[0]) is not None
            and revision[0].lower() == parsed.fragment.lower()
        )
    except ValueError:
        return False


def validate_metadata(metadata, policy, today=None):
    """Return explicit violations; raise PolicyError for incomplete metadata/policy."""
    today = today or datetime.datetime.now(datetime.timezone.utc).date()
    if policy.get("schema_version") != 1 or policy.get("root_package") != ROOT_PACKAGE:
        raise PolicyError("policy must identify this exact application root")
    choices = policy.get("reviewed_license_choices")
    exceptions = policy.get("license_exceptions", [])
    if not isinstance(choices, dict) or any(
        not isinstance(key, str) or not isinstance(value, list) or not value
        or any(not isinstance(item, str) or item not in STANDARD_CHOICES for item in value)
        for key, value in choices.items()
    ):
        raise PolicyError("reviewed full license declarations require explicit choices")
    if not all(isinstance(policy.get(key), list) for key in ("registries", "git_repositories")):
        raise PolicyError("source allowlists are required")
    reviewed = {normalized(key) for key in choices}
    if not isinstance(exceptions, list):
        raise PolicyError("license exceptions must be a list")
    for exception in exceptions:
        if not all(isinstance(exception.get(key), str) and exception[key] for key in
                   ("name", "version", "expression", "owner", "expires", "rationale")):
            raise PolicyError("license exceptions require exact scope, owner, rationale and expiry")
        try:
            datetime.date.fromisoformat(exception["expires"])
        except ValueError as error:
            raise PolicyError("license exception date must use YYYY-MM-DD") from error

    resolve = metadata.get("resolve")
    packages = metadata.get("packages")
    if metadata.get("version") != 1 or not isinstance(resolve, dict) or not isinstance(packages, list):
        raise PolicyError("complete Cargo metadata with a resolve graph is required")
    root_id = resolve.get("root")
    if not isinstance(root_id, str) or not root_id or not isinstance(resolve.get("nodes"), list):
        raise PolicyError("a single resolved application root is required")
    by_id = {}
    for package in packages:
        if not isinstance(package, dict) or not isinstance(package.get("id"), str):
            raise PolicyError("package identities must be present")
        if package["id"] in by_id:
            raise PolicyError("duplicate package identity")
        if not all(isinstance(package.get(key), str) and package[key] for key in ("name", "version")):
            raise PolicyError("package names and versions must be present")
        by_id[package["id"]] = package
    try:
        node_ids = [node["id"] for node in resolve["nodes"]]
        complete = len(node_ids) == len(set(node_ids)) and set(node_ids) == set(by_id)
    except (KeyError, TypeError) as error:
        raise PolicyError("resolve graph is malformed") from error
    if not complete:
        raise PolicyError("all resolved packages must be present; do not use --no-deps")
    for node in resolve["nodes"]:
        dependencies = node.get("dependencies")
        named_edges = node.get("deps")
        if not isinstance(dependencies, list) or not isinstance(named_edges, list):
            raise PolicyError("resolve nodes require complete dependency edges")
        if any(not isinstance(item, str) or item not in by_id for item in dependencies):
            raise PolicyError("dependency edge references an absent package")
        if any(not isinstance(edge, dict) or not isinstance(edge.get("pkg"), str)
               or edge["pkg"] not in by_id for edge in named_edges):
            raise PolicyError("named dependency edge references an absent package")
        if set(dependencies) != {edge["pkg"] for edge in named_edges}:
            raise PolicyError("dependency edge representations disagree")
    root = by_id.get(root_id)
    if root is None or root["name"] != ROOT_PACKAGE or root.get("source") is not None:
        raise PolicyError("only the exact local telegram-connector root can be exempt")

    issues = []
    git_revisions = {}
    for package_id, package in by_id.items():
        if package_id == root_id:
            continue
        label = package["name"] + "@" + package["version"]
        if not allowed_source(package.get("source"), policy):
            issues.append({"code": "source_not_allowed", "package": label})
        elif package.get("source", "").startswith("git+"):
            parsed = urlsplit(package["source"][4:])
            base = urlunsplit((parsed.scheme, parsed.netloc, parsed.path, "", ""))
            revision = parsed.fragment.lower()
            previous = git_revisions.setdefault(base, revision)
            if previous != revision:
                issues.append({"code": "git_revision_mismatch", "package": label})
        expression = package.get("license")
        if not isinstance(expression, str) or not expression.strip():
            issues.append({"code": "dependency_unlicensed", "package": label})
            continue
        expression = normalized(expression)
        if expression in reviewed:
            continue
        scoped = [exception for exception in exceptions
                  if exception["name"] == package["name"]
                  and exception["version"] == package["version"]
                  and normalized(exception["expression"]) == expression]
        if not scoped:
            issues.append({"code": "license_unreviewed", "package": label})
        elif not any(datetime.date.fromisoformat(item["expires"]) >= today for item in scoped):
            issues.append({"code": "license_exception_expired", "package": label})
    return issues


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", type=Path, help="read previously generated full Cargo metadata")
    parser.add_argument("--policy", type=Path, default=PROJECT_ROOT / "dependency-policy.json")
    parser.add_argument("--offline", action="store_true", help="use cached Cargo metadata dependencies")
    options = parser.parse_args(argv)
    try:
        policy = json.loads(options.policy.read_text())
        if options.metadata:
            metadata = json.loads(options.metadata.read_text())
        else:
            command = ["cargo", "metadata", "--locked", "--all-features", "--format-version", "1",
                       "--manifest-path", str(PROJECT_ROOT / "Cargo.toml")]
            if options.offline:
                command.append("--offline")
            metadata = json.loads(subprocess.check_output(command, text=True))
        issues = validate_metadata(metadata, policy)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print("dependency policy could not establish a valid complete graph: " + str(error), file=sys.stderr)
        return 2
    for issue in issues:
        print(issue["package"] + ": " + issue["code"], file=sys.stderr)
    if issues:
        return 1
    print("External dependency metadata policy passed for " + str(len(metadata["packages"]) - 1)
          + " packages; root licensing remains unassigned.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
