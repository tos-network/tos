#!/bin/bash
# TOS Local CI Check Script
# Run CI checks locally (without Docker)
#
# Usage:
#   ./scripts/ci-check.sh           # Run all checks
#   ./scripts/ci-check.sh quick     # Run quick checks only (fmt, clippy)
#   ./scripts/ci-check.sh full      # Run all checks including tests
#   ./scripts/ci-check.sh docker    # Run checks in Docker container

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

# If docker mode, build and run in container
if [ "$1" = "docker" ]; then
    echo "Building CI check Docker image..."
    docker build -t tos-ci-check docker/ci-check/

    echo "Running CI checks in Docker..."
    docker run --rm \
        -v "$PROJECT_ROOT:/workspace" \
        -v "$HOME/.cargo/registry:/root/.cargo/registry" \
        -v "$HOME/.cargo/git:/root/.cargo/git" \
        tos-ci-check "${@:2}"
    exit $?
fi

# Run the check script directly
exec "$PROJECT_ROOT/docker/ci-check/ci-check.sh" "$@"
