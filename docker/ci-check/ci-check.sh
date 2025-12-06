#!/bin/bash
# TOS CI Check Script
# Simulates GitHub Actions workflow checks locally
#
# Usage:
#   ci-check           # Run all checks
#   ci-check quick     # Run quick checks only (fmt, clippy)
#   ci-check full      # Run all checks including tests

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Track results
PASSED=0
FAILED=0
SKIPPED=0

print_header() {
    echo ""
    echo -e "${BLUE}=========================================="
    echo "$1"
    echo -e "==========================================${NC}"
}

print_pass() {
    echo -e "${GREEN}✅ PASS: $1${NC}"
    ((PASSED++))
}

print_fail() {
    echo -e "${RED}❌ FAIL: $1${NC}"
    ((FAILED++))
}

print_skip() {
    echo -e "${YELLOW}⏭️  SKIP: $1${NC}"
    ((SKIPPED++))
}

run_check() {
    local name="$1"
    local cmd="$2"

    print_header "$name"
    echo "Running: $cmd"
    echo ""

    if eval "$cmd"; then
        print_pass "$name"
        return 0
    else
        print_fail "$name"
        return 1
    fi
}

# Parse arguments
MODE="${1:-all}"

print_header "TOS CI Check - Mode: $MODE"
echo "Starting CI checks..."
echo "Working directory: $(pwd)"
echo ""

# ============================================
# 1. Formatting Check
# ============================================
run_check "Formatting Check (cargo fmt)" \
    "cargo fmt --all -- --check" || true

# ============================================
# 2. Clippy - Critical Lints (from lint job)
# ============================================
run_check "Clippy - Critical Lints" \
    "cargo clippy --workspace --all-targets -- \
        -D clippy::await_holding_lock \
        -D clippy::todo \
        -D clippy::unimplemented \
        -W clippy::all" || true

# ============================================
# 3. Security Clippy - Production Libraries
# ============================================
run_check "Security Clippy - Production Libraries" \
    "cargo clippy \
        --package tos_daemon \
        --package tos_common \
        --package tos_wallet \
        --lib -- \
        -D clippy::unwrap_used \
        -D clippy::expect_used \
        -D clippy::panic \
        -D clippy::disallowed_methods \
        -D warnings" || true

# ============================================
# 4. Security Clippy - Production Binaries
# ============================================
run_check "Security Clippy - Production Binaries" \
    "cargo clippy --package tos_miner --package tos_ai_miner -- \
        -D clippy::unwrap_used \
        -D clippy::expect_used \
        -D clippy::panic \
        -D clippy::disallowed_methods \
        -D warnings" || true

# ============================================
# 5. Build with Strict Warnings
# ============================================
run_check "Build - Strict Warnings (Production Libs)" \
    "RUSTFLAGS='-D warnings' cargo build \
        --package tos_daemon \
        --package tos_common \
        --package tos_wallet \
        --lib" || true

if [ "$MODE" = "quick" ]; then
    print_skip "Unit Tests (quick mode)"
    print_skip "Doc Tests (quick mode)"
    print_skip "Integration Tests (quick mode)"
else
    # ============================================
    # 6. Unit Tests (Debug mode)
    # ============================================
    run_check "Unit Tests (Debug Mode)" \
        "cargo test --workspace --lib --no-fail-fast" || true

    # ============================================
    # 7. Doc Tests
    # ============================================
    run_check "Doc Tests" \
        "cargo test --workspace --doc --no-fail-fast" || true

    if [ "$MODE" = "full" ]; then
        # ============================================
        # 8. Integration Tests (Release mode)
        # ============================================
        run_check "Integration Tests (Release Mode)" \
            "cargo test --workspace --tests --release --no-fail-fast" || true

        # ============================================
        # 9. Parallel Execution Tests
        # ============================================
        run_check "Parallel Execution Tests" \
            "cargo test --package tos_daemon --test parallel_sequential_parity --release --no-fail-fast && \
             cargo test --package tos_daemon --test parallel_execution_parity_tests_rocksdb --release --no-fail-fast && \
             cargo test --package tos_daemon --test parallel_execution_security_tests_rocksdb --release --no-fail-fast" || true

        # ============================================
        # 10. Security Tests
        # ============================================
        run_check "Security Tests" \
            "cargo test --package tos_daemon security --release --no-fail-fast && \
             cargo test --package tos_common crypto_security --release --no-fail-fast" || true

        # ============================================
        # 11. Release Build
        # ============================================
        run_check "Release Build" \
            "cargo build --workspace --release --lib && \
             cargo build --workspace --release --bins" || true
    else
        print_skip "Integration Tests (use 'full' mode)"
        print_skip "Parallel Execution Tests (use 'full' mode)"
        print_skip "Security Tests (use 'full' mode)"
        print_skip "Release Build (use 'full' mode)"
    fi
fi

# ============================================
# Summary
# ============================================
print_header "CI Check Summary"
echo -e "${GREEN}Passed:  $PASSED${NC}"
echo -e "${RED}Failed:  $FAILED${NC}"
echo -e "${YELLOW}Skipped: $SKIPPED${NC}"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}=========================================="
    echo "All checks passed! Ready to push."
    echo -e "==========================================${NC}"
    exit 0
else
    echo -e "${RED}=========================================="
    echo "Some checks failed. Please fix before pushing."
    echo -e "==========================================${NC}"
    exit 1
fi
