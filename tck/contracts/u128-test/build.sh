#!/bin/bash
set -e

# TOS Contract Build Script
# Compiles Rust contracts to eBPF bytecode using Rust nightly

# Configuration
TARGET="bpfel-unknown-none"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if nightly is installed
if ! rustup toolchain list | grep -q nightly; then
    echo -e "${YELLOW}Installing Rust nightly toolchain...${NC}"
    rustup toolchain install nightly --component rust-src
fi

# Build
echo -e "${GREEN}Building hello-world contract...${NC}"
echo ""

# Use cargo to build (simpler than manual rustc)
echo -e "${YELLOW}[1/2]${NC} Compiling with cargo build..."

# Build with nightly and Z build-std for no_std target
cargo +nightly build \
    --release \
    --target $TARGET \
    -Z build-std=core \
    -Z build-std-features=panic_immediate_abort

if [ $? -ne 0 ]; then
    echo -e "${RED}Compilation failed${NC}"
    exit 1
fi

# Copy output
echo -e "${YELLOW}[2/2]${NC} Copying output binary..."
cp target/$TARGET/release/libhello_world.rlib hello_world.so

# Success
echo ""
echo -e "${GREEN}✓ Build successful!${NC}"
echo ""
echo "Output: hello_world.so"
ls -lh hello_world.so

# Show file info
echo ""
echo "File info:"
file hello_world.so || true

# Show size
SIZE=$(stat -f%z hello_world.so 2>/dev/null || stat -c%s hello_world.so 2>/dev/null)
echo "Size: $SIZE bytes"
