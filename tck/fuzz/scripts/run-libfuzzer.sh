#!/bin/bash
# Run LibFuzzer on a specific target
#
# Usage: ./run-libfuzzer.sh <target> [options]
#
# Examples:
#   ./run-libfuzzer.sh fuzz_transaction
#   ./run-libfuzzer.sh fuzz_transaction --timeout 3600
#   ./run-libfuzzer.sh fuzz_transaction --jobs 4

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FUZZ_DIR="$(dirname "$SCRIPT_DIR")"

# Default values
TIMEOUT=0
JOBS=1
TARGET=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --timeout)
            TIMEOUT="$2"
            shift 2
            ;;
        --jobs)
            JOBS="$2"
            shift 2
            ;;
        -*)
            echo "Unknown option: $1"
            exit 1
            ;;
        *)
            TARGET="$1"
            shift
            ;;
    esac
done

if [ -z "$TARGET" ]; then
    echo "Usage: $0 <target> [--timeout SECONDS] [--jobs N]"
    echo ""
    echo "Available targets:"
    cargo fuzz list 2>/dev/null || echo "  (run 'cargo fuzz list' to see targets)"
    exit 1
fi

cd "$FUZZ_DIR"

echo "=========================================="
echo "Running fuzzer: $TARGET"
echo "Timeout: ${TIMEOUT}s (0 = unlimited)"
echo "Jobs: $JOBS"
echo "=========================================="

# Build fuzzer arguments
FUZZ_ARGS=()

if [ "$TIMEOUT" -gt 0 ]; then
    FUZZ_ARGS+=("-max_total_time=$TIMEOUT")
fi

if [ "$JOBS" -gt 1 ]; then
    FUZZ_ARGS+=("-jobs=$JOBS" "-workers=$JOBS")
fi

# Run fuzzer
cargo fuzz run "$TARGET" -- "${FUZZ_ARGS[@]}"
