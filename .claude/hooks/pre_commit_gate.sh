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

echo "[pre-commit gate] Running format + lint checks before git commit..." >&2

if ! cargo fmt --all -- --check; then
  echo "[pre-commit gate] Blocked: format check failed. Run: cargo fmt --all" >&2
  exit 2
fi

CLIPPY_JOBS="${CLIPPY_JOBS:-$(nproc)}"
if ! cargo clippy -j "$CLIPPY_JOBS" --workspace --lib --bins --tests -- \
  -D clippy::await_holding_lock \
  -W clippy::all; then
  echo "[pre-commit gate] Blocked: lint check failed. Fix clippy issues before commit." >&2
  exit 2
fi

TEST_JOBS="${TEST_JOBS:-$(nproc)}"
RUST_TEST_THREADS="${RUST_TEST_THREADS:-16}"
UNIT_TEST_PACKAGES="${UNIT_TEST_PACKAGES:-tos_common}"
TEST_ARGS=()
for pkg in $UNIT_TEST_PACKAGES; do
  TEST_ARGS+=("-p" "$pkg")
done
if ! cargo test -j "$TEST_JOBS" "${TEST_ARGS[@]}" --lib -- --test-threads "$RUST_TEST_THREADS"; then
  echo "[pre-commit gate] Blocked: Unit Tests (Debug Mode) failed." >&2
  exit 2
fi

exit 0
