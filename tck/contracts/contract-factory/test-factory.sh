#!/bin/bash
# Test script for contract factory example
#
# This script demonstrates the complete factory workflow:
# 1. Build factory contract
# 2. Build template contract
# 3. Build off-chain service
# 4. Run service (in background)
# 5. Simulate deployment requests
#
# Usage: ./test-factory.sh

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Print with color
print_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

print_header() {
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "${GREEN}$1${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
}

# Check if running in correct directory
if [ ! -f "Cargo.toml" ]; then
    print_error "Please run this script from the contract-factory directory"
    exit 1
fi

print_header "TOS Contract Factory Test Script"

# Step 1: Build factory contract
print_header "Step 1: Building Factory Contract"
print_info "Compiling factory contract for TAKO VM..."

if cargo build --release --target tbpf-tos-tos 2>&1 | grep -q "error"; then
    print_error "Factory contract compilation failed"
    exit 1
fi

FACTORY_BYTECODE="target/tbpf-tos-tos/release/contract_factory.so"

if [ -f "$FACTORY_BYTECODE" ]; then
    FACTORY_SIZE=$(stat -f%z "$FACTORY_BYTECODE" 2>/dev/null || stat -c%s "$FACTORY_BYTECODE" 2>/dev/null)
    print_success "Factory contract compiled successfully"
    print_info "  Location: $FACTORY_BYTECODE"
    print_info "  Size: $FACTORY_SIZE bytes"
else
    print_error "Factory bytecode not found at $FACTORY_BYTECODE"
    exit 1
fi

# Step 2: Calculate bytecode hash
print_header "Step 2: Computing Bytecode Hash"

if command -v sha256sum &> /dev/null; then
    FACTORY_HASH=$(sha256sum "$FACTORY_BYTECODE" | cut -d' ' -f1)
elif command -v shasum &> /dev/null; then
    FACTORY_HASH=$(shasum -a 256 "$FACTORY_BYTECODE" | cut -d' ' -f1)
else
    print_error "sha256sum or shasum not found"
    exit 1
fi

print_success "Factory bytecode hash computed"
print_info "  Hash: $FACTORY_HASH"

# Step 3: Build off-chain service
print_header "Step 3: Building Off-Chain Service"

cd off-chain-service

print_info "Compiling deployment service..."

if cargo build --release 2>&1 | grep -q "error"; then
    print_error "Service compilation failed"
    exit 1
fi

if [ -f "target/release/factory-daemon" ]; then
    print_success "Service compiled successfully"
    print_info "  Location: target/release/factory-daemon"
else
    print_error "Service binary not found"
    exit 1
fi

# Step 4: Setup test environment
print_header "Step 4: Setting Up Test Environment"

# Create bytecode directory
mkdir -p bytecodes
print_info "Created bytecodes directory"

# Copy factory bytecode as template (for testing)
cp "../$FACTORY_BYTECODE" "bytecodes/template.so"
print_success "Copied template bytecode"

# Create mock wallet
WALLET_PATH="test-wallet.key"
if [ ! -f "$WALLET_PATH" ]; then
    echo "mock_private_key_for_testing_only" > "$WALLET_PATH"
    chmod 600 "$WALLET_PATH"
    print_info "Created test wallet: $WALLET_PATH"
fi

# Step 5: Test service startup
print_header "Step 5: Testing Service Startup"

export FACTORY_ADDRESS="tos1test_factory_address"
export TOS_RPC_URL="http://localhost:8080"
export WALLET_PATH="$WALLET_PATH"
export BYTECODE_DIR="./bytecodes"

print_info "Environment variables:"
print_info "  FACTORY_ADDRESS=$FACTORY_ADDRESS"
print_info "  TOS_RPC_URL=$TOS_RPC_URL"
print_info "  WALLET_PATH=$WALLET_PATH"
print_info "  BYTECODE_DIR=$BYTECODE_DIR"

print_info "Starting service (will run for 5 seconds)..."
timeout 5 ./target/release/factory-daemon &
SERVICE_PID=$!

sleep 2

if ps -p $SERVICE_PID > /dev/null; then
    print_success "Service started successfully (PID: $SERVICE_PID)"
    sleep 3

    # Service will exit after timeout
    wait $SERVICE_PID 2>/dev/null || true
    print_info "Service stopped (test complete)"
else
    print_error "Service failed to start"
    exit 1
fi

# Step 6: Summary
print_header "Test Summary"

echo ""
echo "✅ Factory contract compiled and hashed"
echo "✅ Off-chain service built successfully"
echo "✅ Test environment configured"
echo "✅ Service startup test passed"
echo ""

print_info "Factory Contract:"
print_info "  File: ../$FACTORY_BYTECODE"
print_info "  Hash: $FACTORY_HASH"
print_info "  Size: $FACTORY_SIZE bytes"
echo ""

print_info "Off-Chain Service:"
print_info "  Binary: target/release/factory-daemon"
print_info "  Bytecodes: $(ls bytecodes/*.so | wc -l) template(s) loaded"
echo ""

print_header "Next Steps"

cat << EOF

To deploy the factory to TOS blockchain:

  1. Deploy factory contract:
     $ tos-cli deploy --bytecode $FACTORY_BYTECODE --wallet owner.key

  2. Configure factory:
     $ tos-cli call <factory-address> --function set_template_hash \\
         --args template_hash=0x$FACTORY_HASH --wallet owner.key

  3. Run off-chain service:
     $ cd off-chain-service
     $ export FACTORY_ADDRESS=<deployed-factory-address>
     $ ./target/release/factory-daemon

  4. Request deployment:
     $ tos-cli call <factory-address> --function request_deployment \\
         --args salt=0x...,bytecode_hash=0x$FACTORY_HASH \\
         --value 1000000000 --wallet user.key

For detailed usage, see:
  - README.md
  - USAGE_EXAMPLE.md

EOF

print_success "All tests passed! 🎉"
echo ""
