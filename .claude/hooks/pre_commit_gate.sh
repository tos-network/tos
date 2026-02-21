#!/usr/bin/env bash
set -euo pipefail

if ! command -v jq >/dev/null 2>&1; then
  echo "[pre-commit gate] jq is required for Claude hooks." >&2
  exit 2
fi

HOOK_INPUT="$(cat)"
TOOL_COMMAND="$(printf '%s' "$HOOK_INPUT" | jq -r '.tool_input.command // empty')"

# Only enforce for git commit commands.
if [[ ! "$TOOL_COMMAND" =~ (^|[[:space:];|&])git([[:space:]]+-C[[:space:]]+[^[:space:]]+)?[[:space:]]+commit([[:space:]]|$) ]]; then
  exit 0
fi

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-}"
if [[ -z "$PROJECT_DIR" ]]; then
  CWD_FROM_HOOK="$(printf '%s' "$HOOK_INPUT" | jq -r '.cwd // empty')"
  if [[ -n "$CWD_FROM_HOOK" ]]; then
    PROJECT_DIR="$(git -C "$CWD_FROM_HOOK" rev-parse --show-toplevel 2>/dev/null || true)"
  fi
fi

if [[ -z "$PROJECT_DIR" || ! -d "$PROJECT_DIR" ]]; then
  echo "[pre-commit gate] Failed to locate project directory." >&2
  exit 2
fi

cd "$PROJECT_DIR"

echo "[pre-commit gate] Running pre-commit checks before git commit..." >&2

if ! "$PROJECT_DIR"/scripts/pre_commit_checks.sh; then
  echo "[pre-commit gate] Blocked: pre-commit checks failed." >&2
  exit 2
fi

exit 0
