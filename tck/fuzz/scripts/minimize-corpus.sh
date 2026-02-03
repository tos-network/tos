#!/bin/bash
# Minimize corpus for a fuzz target
#
# Usage: ./minimize-corpus.sh <target>

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FUZZ_DIR="$(dirname "$SCRIPT_DIR")"

if [ -z "$1" ]; then
    echo "Usage: $0 <target>"
    echo ""
    echo "Available targets:"
    cargo fuzz list 2>/dev/null || echo "  (run 'cargo fuzz list' to see targets)"
    exit 1
fi

TARGET="$1"

cd "$FUZZ_DIR"

# Determine corpus directory
CORPUS_DIR="corpus/$TARGET"
if [ ! -d "$CORPUS_DIR" ]; then
    # Try to find corpus based on target name
    case "$TARGET" in
        fuzz_transaction)
            CORPUS_DIR="corpus/transaction"
            ;;
        fuzz_block_header)
            CORPUS_DIR="corpus/block"
            ;;
        fuzz_rpc_json)
            CORPUS_DIR="corpus/rpc"
            ;;
        fuzz_syscall)
            CORPUS_DIR="corpus/syscall"
            ;;
        *)
            echo "Warning: No corpus directory found for $TARGET"
            exit 0
            ;;
    esac
fi

if [ ! -d "$CORPUS_DIR" ]; then
    echo "Corpus directory not found: $CORPUS_DIR"
    exit 1
fi

BEFORE=$(find "$CORPUS_DIR" -type f | wc -l)

echo "Minimizing corpus for: $TARGET"
echo "Corpus directory: $CORPUS_DIR"
echo "Files before: $BEFORE"

# Run corpus minimization
cargo fuzz cmin "$TARGET" "$CORPUS_DIR"

AFTER=$(find "$CORPUS_DIR" -type f | wc -l)

echo "Files after: $AFTER"
echo "Reduced by: $((BEFORE - AFTER)) files"
