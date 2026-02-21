#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "Usage: $0 <git-commit-args>" >&2
  echo "Example: $0 -m \"feat: add xyz\"" >&2
  exit 1
fi

echo "[commit_with_checks] Running format check..."
"$(dirname "$0")/pre_commit_checks.sh"

echo "[commit_with_checks] Checks passed, committing..."
git commit "$@"
