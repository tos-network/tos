#!/bin/bash
# Generate test accounts from seeds using tos_wallet binary

WALLET_BIN="$HOME/tos-network/tos/target/release/tos_wallet"
NETWORK="testnet"
WALLET_DIR="/tmp/tos_test_wallets"

# Create temp directory for wallets
mkdir -p "$WALLET_DIR"

echo "=== TOS Test Account Generator ==="
echo ""

# Test account seeds
declare -A ACCOUNTS
ACCOUNTS[alice]="tiger eight taxi vexed revamp thorn paddles dosage layout muzzle eggs chlorine sober oyster ecstatic festival banjo behind western segments january behind usage winter paddles"
ACCOUNTS[bob]="ocean swift mountain eagle dancing river frozen sunset golden meadow crystal palace harmony wisdom ancient forest keeper silver dragon mystic lunar phase"
ACCOUNTS[charlie]="cosmic nebula stellar quantum photon aurora borealis cascade thunder lightning plasma fusion reactor galaxy spiral vortex infinite eternal cosmic ray burst"

# Generate each account
for name in "${!ACCOUNTS[@]}"; do
    echo "Generating account: $name"
    echo "Seed: ${ACCOUNTS[$name]}"

    # Create wallet directory for this account
    account_dir="$WALLET_DIR/$name"
    rm -rf "$account_dir"
    mkdir -p "$account_dir"

    # Create expect script to automate wallet recovery
    expect_script="/tmp/recover_${name}.exp"
    cat > "$expect_script" << EOF
#!/usr/bin/expect -f
set timeout 120

spawn $WALLET_BIN --network $NETWORK --wallet-path $account_dir --offline-mode --disable-ascii-art

expect "Available commands:"
send "recover_seed\\r"

expect "Please enter your seed"
send "${ACCOUNTS[$name]}\\r"

expect "Please enter a password"
send "test123\\r"

expect "Please confirm your password"
send "test123\\r"

expect "Wallet recovered"
send "address\\r"

expect "tst1"
set address \$expect_out(buffer)

send "seed\\r"
expect "Password"
send "test123\\r"

expect "Your seed is"

send "exit\\r"
expect eof
EOF

    chmod +x "$expect_script"

    # Run expect script
    output=$("$expect_script" 2>&1)

    # Extract address from output
    address=$(echo "$output" | grep -o "tst1[a-z0-9]*" | head -1)

    echo "  Address: $address"
    echo ""

    # Clean up
    rm -f "$expect_script"
done

echo "=== Account Generation Complete ==="
echo ""
echo "Wallet files stored in: $WALLET_DIR"
echo ""
echo "To extract keys, use:"
echo "  $WALLET_BIN --wallet-path $WALLET_DIR/alice --network testnet --offline-mode"
