#!/bin/bash
# Run all fuzz targets
#
# Usage: ./run-all.sh [options]
#
# Options:
#   --timeout SECONDS    Time per target (default: 300)
#   --ci                 CI mode - stop on first crash
#   --targets LIST       Comma-separated list of targets

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FUZZ_DIR="$(dirname "$SCRIPT_DIR")"

# Default values
TIMEOUT=300
CI_MODE=false
TARGETS=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --timeout)
            TIMEOUT="$2"
            shift 2
            ;;
        --ci)
            CI_MODE=true
            shift
            ;;
        --targets)
            TARGETS="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

cd "$FUZZ_DIR"

# Get target list
if [ -z "$TARGETS" ]; then
    TARGETS=$(cargo fuzz list 2>/dev/null | tr '\n' ',')
fi

IFS=',' read -ra TARGET_ARRAY <<< "$TARGETS"

echo "=========================================="
echo "Running all fuzz targets"
echo "Targets: ${#TARGET_ARRAY[@]}"
echo "Timeout per target: ${TIMEOUT}s"
echo "CI mode: $CI_MODE"
echo "=========================================="

FAILED=0
PASSED=0

for target in "${TARGET_ARRAY[@]}"; do
    target=$(echo "$target" | xargs)  # Trim whitespace
    [ -z "$target" ] && continue

    echo ""
    echo ">>> Running: $target"

    if cargo fuzz run "$target" -- -max_total_time="$TIMEOUT"; then
        echo "<<< PASSED: $target"
        ((PASSED++))
    else
        echo "<<< FAILED: $target"
        ((FAILED++))

        if [ "$CI_MODE" = true ]; then
            echo "CI mode: Stopping on first failure"
            exit 1
        fi
    fi
done

echo ""
echo "=========================================="
echo "Summary: $PASSED passed, $FAILED failed"
echo "=========================================="

exit $FAILED
