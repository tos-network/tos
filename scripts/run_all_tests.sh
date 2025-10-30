#!/bin/bash
# TOS Complete Test Suite Runner
# Runs all tests with appropriate modes
# Usage: ./scripts/run_all_tests.sh

set -e

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

echo "=========================================="
echo "TOS Complete Test Suite"
echo "=========================================="
echo ""
echo "Test execution strategy:"
echo "  ✓ Unit tests:        Debug mode (fast)"
echo "  ✓ Integration tests: Release mode (Kaspa best practice)"
echo "  ✓ Parallel tests:    Release mode (performance critical)"
echo "  ✓ Security tests:    Release mode (realistic scenarios)"
echo ""
echo "Expected duration: 5-15 minutes"
echo ""

# Run each test suite
echo "=========================================="
echo "1/4: Unit Tests"
echo "=========================================="
"${SCRIPT_DIR}/run_unit_tests.sh"

echo ""
echo "=========================================="
echo "2/4: Integration Tests"
echo "=========================================="
"${SCRIPT_DIR}/run_integration_tests.sh"

echo ""
echo "=========================================="
echo "3/4: Parallel Execution Tests"
echo "=========================================="
"${SCRIPT_DIR}/run_parallel_tests.sh"

echo ""
echo "=========================================="
echo "4/4: Security Tests"
echo "=========================================="
"${SCRIPT_DIR}/run_security_tests.sh"

echo ""
echo "=========================================="
echo "ALL TESTS PASSED SUCCESSFULLY!"
echo "=========================================="
echo ""
echo "Test suite completed. Ready for commit."
