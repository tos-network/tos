#!/bin/bash
set -e

# Build script for CPI E2E Callee contract
# This script builds the contract using the TOS Rust toolchain

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo "Building CPI E2E Callee contract..."

# Check if toolchain is installed
if ! rustup toolchain list | grep -q "tbpf"; then
    echo -e "${RED}Error: tbpf toolchain not found${NC}"
    echo "Please run: ../../setup-toolchain.sh"
    exit 1
fi

# Build the contract
cargo build --release

# Check if build was successful
if [ -f "target/tbpf-tos-tos/release/libcpi_e2e_callee.so" ]; then
    cp target/tbpf-tos-tos/release/libcpi_e2e_callee.so cpi_e2e_callee.so
    echo -e "${GREEN}✓ Build successful: cpi_e2e_callee.so${NC}"
else
    echo -e "${RED}✗ Build failed${NC}"
    exit 1
fi
