#!/bin/bash
# TAKO Smart Contract Build Script
# Builds all smart contracts using TOS platform-tools

set -e

# Configuration
TOS_PLATFORM_TOOLS_VERSION="${TOS_PLATFORM_TOOLS_VERSION:-v1.54}"
PLATFORM_TOOLS_DIR="$HOME/.cache/tos/$TOS_PLATFORM_TOOLS_VERSION"
CARGO="$PLATFORM_TOOLS_DIR/rust/bin/cargo"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check platform-tools installation
if [ ! -f "$CARGO" ]; then
    echo -e "${RED}Error: TOS platform-tools $TOS_PLATFORM_TOOLS_VERSION not found${NC}"
    echo "Expected location: $PLATFORM_TOOLS_DIR"
    echo ""
    echo "Please install platform-tools from:"
    echo "  https://github.com/tos-network/platform-tools/releases"
    exit 1
fi

# Export environment variables
export TOS_PLATFORM_TOOLS_VERSION
export PATH="$PLATFORM_TOOLS_DIR/rust/bin:$PLATFORM_TOOLS_DIR/llvm/bin:$PATH"

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Contract directories to build
CONTRACTS=(
    "alloc-oom"
    "counter"
    "token"
    "test-environment"
    "test-balance-transfer"
    "test-events"
    "test-code-ops"
    "cpi-callee"
    "cpi-caller"
    "cpi-e2e-callee"
    "cpi-e2e-caller"
    "reentrancy-guard"
    "proxy_contract"
    "environment_info"
    "erc20-openzeppelin"
    "erc721-openzeppelin"
    "erc1155-openzeppelin"
    "ownable"
    "pausable"
    "vrf-branching"
    "vrf-lottery"
    "vrf-prediction"
    "vrf-random"
    "scheduler"
)

# Parse arguments
BUILD_ALL=false
BUILD_SINGLE=""
PROFILE="release"

while [[ $# -gt 0 ]]; do
    case $1 in
        --all)
            BUILD_ALL=true
            shift
            ;;
        --debug)
            PROFILE="debug"
            shift
            ;;
        --contract)
            BUILD_SINGLE="$2"
            shift 2
            ;;
        -h|--help)
            echo "TAKO Smart Contract Build Script"
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --all          Build all contracts"
            echo "  --contract DIR Build a specific contract"
            echo "  --debug        Build in debug mode (default: release)"
            echo "  -h, --help     Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0 --all                    # Build all contracts"
            echo "  $0 --contract counter       # Build only counter contract"
            echo "  $0 --contract token --debug # Build token in debug mode"
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            exit 1
            ;;
    esac
done

# Default to building all if no specific contract
if [ -z "$BUILD_SINGLE" ]; then
    BUILD_ALL=true
fi

echo -e "${GREEN}TAKO Smart Contract Builder${NC}"
echo "================================"
echo "Platform Tools: $TOS_PLATFORM_TOOLS_VERSION"
echo "Cargo: $CARGO"
echo "Profile: $PROFILE"
echo ""

build_contract() {
    local contract_dir="$1"
    local contract_path="$SCRIPT_DIR/$contract_dir"

    if [ ! -d "$contract_path" ]; then
        echo -e "${YELLOW}Skipping $contract_dir (not found)${NC}"
        return 0
    fi

    echo -e "${GREEN}Building $contract_dir...${NC}"
    cd "$contract_path"

    if [ "$PROFILE" = "release" ]; then
        "$CARGO" tako build --release 2>&1
    else
        "$CARGO" tako build 2>&1
    fi

    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓ $contract_dir built successfully${NC}"
    else
        echo -e "${RED}✗ $contract_dir build failed${NC}"
        return 1
    fi
    echo ""
}

# Build contracts
FAILED=0
BUILT=0

if [ -n "$BUILD_SINGLE" ]; then
    build_contract "$BUILD_SINGLE" || FAILED=$((FAILED + 1))
    BUILT=1
elif [ "$BUILD_ALL" = true ]; then
    for contract in "${CONTRACTS[@]}"; do
        build_contract "$contract" || FAILED=$((FAILED + 1))
        BUILT=$((BUILT + 1))
    done
fi

# Summary
echo "================================"
echo -e "${GREEN}Build Summary${NC}"
echo "  Total: $BUILT"
echo "  Failed: $FAILED"
echo ""

if [ $FAILED -gt 0 ]; then
    echo -e "${RED}Some builds failed!${NC}"
    exit 1
else
    echo -e "${GREEN}All builds successful!${NC}"
    echo ""
    echo "Output directory: $SCRIPT_DIR/target/tbpfv3-tos-tos/$PROFILE/"

    # Copy .so files to fixtures directory
    FIXTURES_DIR="$SCRIPT_DIR/../tests/fixtures"
    if [ -d "$FIXTURES_DIR" ]; then
        echo ""
        echo "Copying .so files to fixtures..."
        cp "$SCRIPT_DIR/target/tbpfv3-tos-tos/$PROFILE/"*.so "$FIXTURES_DIR/"
        echo -e "${GREEN}Copied to: $FIXTURES_DIR${NC}"
    fi
fi
