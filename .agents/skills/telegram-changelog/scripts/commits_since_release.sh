#!/usr/bin/env bash
# Read-only history collection. Use Bash explicitly; executable bits are optional.
# Usage: bash commits_since_release.sh [repository-path]
set -euo pipefail

repo="${1:-.}"
if ! git -C "$repo" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "ERROR: '$repo' is not inside a git repository." >&2
  exit 1
fi
cd "$(git -C "$repo" rev-parse --show-toplevel)"

# Walk the main line once, so a stale tag cannot override a newer release commit.
# Tab delimiters work with both BSD and GNU awk (unlike awk's nonportable \x1f).
# Consume the whole stream instead of exiting awk early under pipefail.
boundary=$(git log --first-parent --decorate=short --format='%H%x09%D%x09%s' |
  awk -F '\t' '
    !found && ($2 ~ /(^|, )tag: v?[0-9]+\.[0-9]+\.[0-9]+(, |$)/ ||
               $3 ~ /^chore(\([^)]*\))?: release v?[0-9]+\.[0-9]+\.[0-9]+$/) {
      print $1
      found = 1
    }')

if [ -n "$boundary" ]; then
  boundary_ref=$(git show -s --format='%h (%s)' "$boundary")
  range="${boundary}..HEAD"
  echo "BOUNDARY: release — ${boundary_ref}"
else
  range="HEAD"
  echo "BOUNDARY: none — full history (no version tags or release commits found)"
fi
echo "RANGE: ${range}"

count=$(git log "$range" --no-merges --format='%H' | wc -l | tr -d ' ')
echo "COMMIT COUNT: ${count}"

if [ "$count" = "0" ]; then
  echo "No non-merge commits in this range; changelog completeness is not verified."
  exit 0
fi

echo "COMMITS (newest first; body indented under each subject):"
git log "$range" --no-merges --pretty=format:'%h | %s%n%w(0,4,4)%b'
echo ""
