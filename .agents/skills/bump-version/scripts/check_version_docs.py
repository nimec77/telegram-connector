"""Check this crate's four version surfaces without modifying files (Python 3.11+)."""

import argparse
from datetime import date
from pathlib import Path
import re
import sys

if sys.version_info < (3, 11):
    sys.exit("Version documentation check requires Python 3.11+; select an installed newer interpreter.")

import tomllib


def check(root: Path, allow_older_release: bool = False) -> list[str]:
    manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    package = manifest["package"]
    version = package["version"]
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    entries = [p for p in lock.get("package", []) if p["name"] == package["name"]]
    errors = []
    if len(entries) != 1 or entries[0]["version"] != version:
        errors.append(f"Cargo.lock: expected one root package {package['name']} at {version}")

    changelog = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    headings = re.findall(r"^## \[([^\]\n]+)\](?: - ([^\n]+))?$", changelog, re.MULTILINE)
    if not headings or headings[0] != ("Unreleased", "") or sum(
        name == "Unreleased" for name, _ in headings
    ) != 1:
        errors.append("CHANGELOG.md: expected one leading ## [Unreleased] section")
    releases = [(name, day) for name, day in headings if name != "Unreleased"]
    current = [day for name, day in releases if name == version]
    if len(current) != 1:
        errors.append(f"CHANGELOG.md: expected exactly one dated section for {version}")
        return errors
    if not allow_older_release and releases[0][0] != version:
        errors.append(f"CHANGELOG.md: expected {version} as the first release section")
        return errors

    release_date = current[0]
    try:
        if date.fromisoformat(release_date).isoformat() != release_date:
            raise ValueError("noncanonical date")
    except ValueError:
        errors.append(f"CHANGELOG.md: invalid YYYY-MM-DD release date for {version}: {release_date!r}")
        return errors

    # GitHub heading anchors remove the dots, brackets and plus in Cargo SemVer.
    anchor = version.lower().replace(".", "").replace("+", "") + "---" + release_date
    expected = f"Current package version: [{version}](CHANGELOG.md#{anchor})."
    readme = (root / "README.md").read_text(encoding="utf-8")
    markers = re.findall(r"^Current package version:.*$", readme, re.MULTILINE)
    if markers != [expected]:
        errors.append(f"README.md: expected exactly one line:\n  {expected}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", type=Path, default=Path.cwd())
    parser.add_argument(
        "--allow-older-release", action="store_true",
        help="Allow an existing historical section for an explicitly requested downgrade",
    )
    args = parser.parse_args()
    try:
        errors = check(args.root, args.allow_older_release)
    except (OSError, KeyError, TypeError, tomllib.TOMLDecodeError) as exc:
        print(f"Version documentation check failed: {exc}", file=sys.stderr)
        return 1
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    package = tomllib.loads((args.root / "Cargo.toml").read_text(encoding="utf-8"))["package"]
    print(f"Version documentation is consistent: {package['name']} {package['version']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
