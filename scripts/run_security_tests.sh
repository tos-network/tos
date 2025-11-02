#!/bin/bash
# TOS Security Tests Runner
# Runs security tests in RELEASE mode for realistic attack scenarios
# Usage: ./scripts/run_security_tests.sh

set -e

echo "=========================================="
echo "TOS Security Tests (Release Mode)"
echo "=========================================="
echo ""
echo "Critical: Security tests MUST run in Release mode"
echo "  - Realistic attack scenario performance"
echo "  - Validates defenses under production load"
echo "  - Tests cryptographic operations at full speed"
echo ""

echo "Running daemon security tests..."
echo "----------------------------------------------"
cargo test --package tos_daemon security --release --no-fail-fast -- --nocapture

echo ""
echo "Running common crypto security tests..."
echo "----------------------------------------------"
cargo test --package tos_common crypto_security --release --no-fail-fast -- --nocapture

echo ""
echo "=========================================="
echo "Security Tests Completed Successfully"
echo "=========================================="
