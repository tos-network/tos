#!/bin/bash
#
# TOS Pre-commit / PR Check Script
# Run this before committing or creating a PR to catch issues early.
#
# Usage:
#   ./pr.sh              # Run full checks (default) - includes tests
#   ./pr.sh quick        # Run quick checks only (fmt + clippy)
#   ./pr.sh standard     # Run standard checks (no tests)
#

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Counters
PASSED=0
FAILED=0

# Print colored output
print_header() {
    echo ""
    echo -e "${BLUE}===========================================${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}===========================================${NC}"
}

print_step() {
    echo -e "${YELLOW}>>> $1${NC}"
}

print_success() {
    echo -e "${GREEN}[PASS]${NC} $1"
    PASSED=$((PASSED + 1))
}

print_error() {
    echo -e "${RED}[FAIL]${NC} $1"
    FAILED=$((FAILED + 1))
}

print_skip() {
    echo -e "${YELLOW}[SKIP]${NC} $1"
}

# Check mode (default: full)
MODE="${1:-full}"

print_header "TOS Pre-commit Checks (mode: $MODE)"

# ===========================================
# Step 1: Code Formatting
# ===========================================
print_step "Step 1: Code Formatting"

echo "Running cargo fmt --all..."
cargo fmt --all

echo "Verifying formatting..."
if cargo fmt --all -- --check; then
    print_success "Code formatting"
else
    print_error "Code formatting - run 'cargo fmt --all' to fix"
    exit 1
fi

# ===========================================
# Step 2: Clippy Linting (Critical)
# ===========================================
print_step "Step 2: Clippy Linting (Critical)"

echo "Running clippy (critical lints as errors, others as warnings)..."
echo "  - await_holding_lock: error (prevents deadlocks)"
echo "  - other lints: warnings only"
if cargo clippy --workspace --lib --bins --tests -- \
    -D clippy::await_holding_lock \
    -W clippy::all 2>&1; then
    print_success "Clippy critical linting"
else
    print_error "Clippy critical linting"
    exit 1
fi

# ===========================================
# Step 3: Security Clippy (Production Code)
# ===========================================
print_step "Step 3: Security Clippy (Production Code)"

echo "Checking daemon, common, wallet for unwrap/expect/panic..."
if cargo clippy \
    --package tos_daemon \
    --package tos_common \
    --package tos_wallet \
    --lib -- \
    -D clippy::unwrap_used \
    -D clippy::expect_used \
    -D clippy::panic \
    -D warnings 2>&1; then
    print_success "Security Clippy (libs)"
else
    print_error "Security Clippy (libs) - no unwrap()/expect()/panic!() in production code"
    exit 1
fi

echo "Checking miner binary..."
if cargo clippy \
    --package tos_miner -- \
    -D clippy::unwrap_used \
    -D clippy::expect_used \
    -D clippy::panic \
    -D warnings 2>&1; then
    print_success "Security Clippy (miner)"
else
    print_error "Security Clippy (miner)"
    exit 1
fi

# Quick mode stops here
if [ "$MODE" = "quick" ]; then
    print_header "Quick Check Summary"
    echo -e "${GREEN}Passed: $PASSED${NC}"
    echo -e "${RED}Failed: $FAILED${NC}"
    echo ""
    echo "Quick checks completed. Run './pr.sh' for full checks."
    exit 0
fi

# ===========================================
# Step 4: Build Verification
# ===========================================
print_step "Step 4: Build Verification"

echo "Building workspace..."
if cargo build --workspace --lib 2>&1; then
    print_success "Build (libs)"
else
    print_error "Build (libs)"
    exit 1
fi

if cargo build --workspace --bins 2>&1; then
    print_success "Build (bins)"
else
    print_error "Build (bins)"
    exit 1
fi

# ===========================================
# Step 5: Build with Strict Warnings
# ===========================================
print_step "Step 5: Build with Strict Warnings (-D warnings)"

echo "Building production crates with -D warnings..."
if RUSTFLAGS="-D warnings" cargo build \
    --package tos_daemon \
    --package tos_common \
    --package tos_wallet \
    --package tos_miner \
    --lib 2>&1; then
    print_success "Strict build"
else
    print_error "Strict build"
    exit 1
fi

# Standard mode stops here
if [ "$MODE" = "standard" ]; then
    print_header "Standard Check Summary"
    echo -e "${GREEN}Passed: $PASSED${NC}"
    echo -e "${RED}Failed: $FAILED${NC}"
    echo ""
    echo "Standard checks completed. Run './pr.sh' (or './pr.sh full') to include tests."
    exit 0
fi

# ===========================================
# Step 6: Tests (full mode only)
# ===========================================
print_step "Step 6: Running Tests"

echo "Running unit tests..."
if cargo test --workspace --lib 2>&1; then
    print_success "Unit tests"
else
    print_error "Unit tests"
    exit 1
fi

echo "Running doc tests..."
if cargo test --workspace --doc 2>&1; then
    print_success "Doc tests"
else
    print_error "Doc tests"
    exit 1
fi

# ===========================================
# Summary
# ===========================================
print_header "Full Check Summary"
echo -e "${GREEN}Passed: $PASSED${NC}"
echo -e "${RED}Failed: $FAILED${NC}"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}All checks passed! Ready to commit.${NC}"
    exit 0
else
    echo -e "${RED}Some checks failed. Please fix before committing.${NC}"
    exit 1
fi
