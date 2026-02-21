#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "Usage: $0 <git-commit-args>" >&2
  echo "Example: $0 -m \"feat: add xyz\"" >&2
  exit 1
fi

echo "[commit_with_checks] Running format check..."
cargo fmt --all -- --check

echo "[commit_with_checks] Running lint check..."
CLIPPY_JOBS="${CLIPPY_JOBS:-$(nproc)}"
cargo clippy -j "$CLIPPY_JOBS" --workspace --lib --bins --tests -- \
  -D clippy::await_holding_lock \
  -W clippy::all

echo "[commit_with_checks] Running Unit Tests (Debug Mode)..."
TEST_JOBS="${TEST_JOBS:-$(nproc)}"
RUST_TEST_THREADS="${RUST_TEST_THREADS:-16}"
UNIT_TEST_PACKAGES="${UNIT_TEST_PACKAGES:-tos_common}"
TEST_ARGS=()
for pkg in $UNIT_TEST_PACKAGES; do
  TEST_ARGS+=("-p" "$pkg")
done
cargo test -j "$TEST_JOBS" "${TEST_ARGS[@]}" --lib -- --test-threads "$RUST_TEST_THREADS"

echo "[commit_with_checks] Checks passed, committing..."
git commit "$@"
