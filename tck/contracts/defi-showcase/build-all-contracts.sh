#!/bin/bash

# Build script for all DeFi showcase contracts
# Uses custom TOS toolchain for TBPF compatibility

set -e

TOOLCHAIN_PATH="$HOME/tos-network/platform-tools/out/rust/bin"
RUSTC="$TOOLCHAIN_PATH/rustc"
CARGO="$TOOLCHAIN_PATH/cargo"
TARGET="tbpf-tos-tos"
FIXTURES_DIR="$HOME/tos-network/tos/daemon/tests/fixtures"

# Color output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== DeFi Showcase Contracts Build Script ===${NC}"
echo ""

# Check toolchain
if [ ! -f "$RUSTC" ]; then
    echo "Error: Custom toolchain not found at $RUSTC"
    echo "Please rebuild toolchain: cd ~/tos-network/platform-tools && ./build.sh"
    exit 1
fi

echo -e "${GREEN}✓ Custom toolchain found${NC}"
echo ""

# Create fixtures directory if it doesn't exist
mkdir -p "$FIXTURES_DIR"

# Contracts to build
contracts=(
    "usdt-tether"
    "usdc-circle"
    "uniswap-v2-factory"
    "uniswap-v3-pool"
    "aave-v3-pool"
)

# Build each contract
for contract in "${contracts[@]}"; do
    echo -e "${BLUE}Building $contract...${NC}"

    cd "$HOME/tos-network/tako/examples/defi-showcase/$contract"

    # Clean previous build
    rm -rf target

    # Build with custom toolchain
    env RUSTC="$RUSTC" \
        "$CARGO" build \
        --release \
        --target "$TARGET" 2>&1 | grep -v "warning:" || true

    # Get the output binary name (replace - with _)
    binary_name="${contract//-/_}"
    source_file="$HOME/tos-network/tako/examples/target/$TARGET/release/$binary_name.so"
    dest_file="$FIXTURES_DIR/$binary_name.so"

    if [ -f "$source_file" ]; then
        cp "$source_file" "$dest_file"
        size=$(ls -lh "$dest_file" | awk '{print $5}')
        echo -e "${GREEN}✓ $contract built successfully ($size)${NC}"
        echo -e "  Deployed to: $dest_file"
    else
        echo "❌ Failed to build $contract"
        echo "   Expected: $source_file"
        exit 1
    fi

    echo ""
done

echo -e "${GREEN}=== All contracts built successfully ===${NC}"
echo ""
echo "Deployed contracts:"
ls -lh "$FIXTURES_DIR"/*.so | awk '{print "  " $9 " (" $5 ")"}'
