#!/bin/bash
# TOS Integration Tests Runner
# Runs integration tests in RELEASE mode (following Kaspa best practice)
# Usage: ./scripts/run_integration_tests.sh

set -e

echo "=========================================="
echo "TOS Integration Tests (Release Mode)"
echo "=========================================="
echo ""
echo "Following Kaspa best practice:"
echo "  - Debug mode: 10-100x slower"
echo "  - Release mode: Production-like performance"
echo ""
echo "Running workspace integration tests..."
echo "----------------------------------------------"

# Run all integration tests in release mode
cargo test --workspace --tests --release --no-fail-fast

echo ""
echo "Running testing-integration package tests..."
echo "----------------------------------------------"

cargo test --package tos-testing-integration --release --no-fail-fast

echo ""
echo "=========================================="
echo "Integration Tests Completed Successfully"
echo "=========================================="
