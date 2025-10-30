#!/bin/bash
# TOS Unit Tests Runner
# Runs fast unit tests in Debug mode
# Usage: ./scripts/run_unit_tests.sh

set -e

echo "=========================================="
echo "TOS Unit Tests (Debug Mode)"
echo "=========================================="
echo ""
echo "Running library tests..."
echo "----------------------------------------------"

# Run unit tests for each crate
cargo test --workspace --lib --no-fail-fast

echo ""
echo "Running documentation tests..."
echo "----------------------------------------------"

cargo test --workspace --doc --no-fail-fast

echo ""
echo "=========================================="
echo "Unit Tests Completed Successfully"
echo "=========================================="
