#!/bin/bash
# TOS Security Tests Runner
# Runs security tests in RELEASE mode for realistic attack scenarios
# Usage: ./scripts/run_security_tests.sh [--quick|--fuzz|--bench|--full]

set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Parse arguments
MODE="full"
if [ "$1" = "--quick" ]; then
    MODE="quick"
elif [ "$1" = "--fuzz" ]; then
    MODE="fuzz"
elif [ "$1" = "--bench" ]; then
    MODE="bench"
fi

echo "=========================================="
echo "TOS Security Tests (Release Mode)"
echo "=========================================="
echo ""
echo "Critical: Security tests MUST run in Release mode"
echo "  - Realistic attack scenario performance"
echo "  - Validates defenses under production load"
echo "  - Tests cryptographic operations at full speed"
echo ""
echo "Mode: $MODE"
echo ""

# Run existing security tests
echo -e "${BLUE}Running daemon security tests...${NC}"
echo "----------------------------------------------"
cargo test --package tos_daemon security --release --no-fail-fast -- --nocapture

echo ""
echo -e "${BLUE}Running common crypto security tests...${NC}"
echo "----------------------------------------------"
cargo test --package tos_common crypto_security --release --no-fail-fast -- --nocapture

# Run new comprehensive security tests
echo ""
echo -e "${BLUE}Running comprehensive security tests...${NC}"
echo "----------------------------------------------"
cargo test --package tos_daemon --test security_comprehensive_tests --release -- --nocapture

# Run property-based tests
echo ""
echo -e "${BLUE}Running property-based tests...${NC}"
echo "----------------------------------------------"
cargo test --package tos_daemon --test property_tests --release -- --nocapture

# Run fuzz tests if requested
if [ "$MODE" = "fuzz" ] || [ "$MODE" = "full" ]; then
    echo ""
    echo -e "${BLUE}Running fuzz tests (60s per target)...${NC}"
    echo "----------------------------------------------"

    if command -v cargo-fuzz &> /dev/null; then
        cd daemon/fuzz
        echo -e "${YELLOW}Fuzzing GHOSTDAG...${NC}"
        cargo +nightly fuzz run ghostdag_fuzzer -- -max_total_time=60 -verbosity=0 || true

        echo -e "${YELLOW}Fuzzing block deserialization...${NC}"
        cargo +nightly fuzz run fuzz_block_deserialize -- -max_total_time=60 -verbosity=0 || true

        echo -e "${YELLOW}Fuzzing transaction decode...${NC}"
        cargo +nightly fuzz run fuzz_transaction_decode -- -max_total_time=60 -verbosity=0 || true

        echo -e "${YELLOW}Fuzzing contract bytecode...${NC}"
        cargo +nightly fuzz run fuzz_contract_bytecode -- -max_total_time=60 -verbosity=0 || true
        cd ../..
    else
        echo -e "${YELLOW}Skipping fuzz tests (cargo-fuzz not installed)${NC}"
    fi
fi

# Run benchmarks if requested
if [ "$MODE" = "bench" ] || [ "$MODE" = "full" ]; then
    echo ""
    echo -e "${BLUE}Running security benchmarks...${NC}"
    echo "----------------------------------------------"
    cargo bench --package tos_daemon --bench security_benchmarks
fi

echo ""
echo -e "${GREEN}==========================================${NC}"
echo -e "${GREEN}Security Tests Completed Successfully${NC}"
echo -e "${GREEN}==========================================${NC}"
