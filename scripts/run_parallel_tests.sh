#!/bin/bash
# TOS Parallel Execution Tests Runner
# Runs parallel execution tests in RELEASE mode
# Usage: ./scripts/run_parallel_tests.sh

set -e

echo "=========================================="
echo "TOS Parallel Execution Tests (Release Mode)"
echo "=========================================="
echo ""
echo "Critical: These tests MUST run in Release mode"
echo "  - Tests realistic blockchain performance"
echo "  - Validates parallel transaction execution"
echo "  - Detects race conditions under load"
echo ""

echo "Running parallel execution parity tests..."
echo "----------------------------------------------"
cargo test --package tos_daemon --test parallel_execution_parity_tests --release --no-fail-fast -- --nocapture

echo ""
echo "Running parallel execution RocksDB tests..."
echo "----------------------------------------------"
cargo test --package tos_daemon --test parallel_execution_parity_tests_rocksdb --release --no-fail-fast -- --nocapture

echo ""
echo "Running parallel execution security tests..."
echo "----------------------------------------------"
cargo test --package tos_daemon --test parallel_execution_security_tests --release --no-fail-fast -- --nocapture

echo ""
echo "=========================================="
echo "Parallel Execution Tests Completed Successfully"
echo "=========================================="
