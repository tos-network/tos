#!/bin/bash

# TOS Wallet Exec mode example script (similar to geth --exec)
WALLET_PATH="my_test_wallet"
PASSWORD="test123"

echo "=== TOS Wallet Exec mode operations (aligned with Geth) ==="

# 1. Query balance - using the new --exec argument
echo "1. Query TOS balance..."
tos_wallet --exec="balance TOS" --wallet-path="$WALLET_PATH" --password="$PASSWORD"

# 1a. Using JSON format (more powerful)
echo "1a. Query TOS balance (JSON)..."
tos_wallet --json='{"command":"balance","params":{"asset":"TOS"}}' \
    --wallet-path="$WALLET_PATH" --password="$PASSWORD"

# 2. Query wallet address - using --exec (simple)
echo "2. Get wallet address..."
tos_wallet --exec="address" --wallet-path="$WALLET_PATH" --password="$PASSWORD"

# 2a. Query wallet address - using JSON (unified)
echo "2a. Get wallet address (JSON)..."
tos_wallet --json='{"command":"address","params":{}}' \
    --wallet-path="$WALLET_PATH" --password="$PASSWORD"

# 3. Set a new nonce - using --exec
echo "3. Set nonce to 100..."
tos_wallet --exec="set_nonce 100" --wallet-path="$WALLET_PATH" --password="$PASSWORD"

# 3a. Set a new nonce - using JSON (type-safe)
echo "3a. Set nonce to 100 (JSON)..."
tos_wallet --json='{"command":"set_nonce","params":{"nonce":100}}' \
    --wallet-path="$WALLET_PATH" --password="$PASSWORD"

# 4. Transfer operation - using --exec (simple command)
echo "4. Execute transfer (--exec)..."
tos_wallet --exec="transfer TOS tos1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq8cczjp 1.0" \
    --wallet-path="$WALLET_PATH" --password="$PASSWORD"

# 4a. Use a JSON file for transfer (complex configuration)
echo "4a. Execute transfer (JSON file)..."
cat > temp_transfer.json << 'EOF'
{
  "command": "transfer",
  "params": {
    "address": "tos1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq8cczjp",
    "amount": "1.0",
    "asset": "TOS",
    "confirm": "yes"
  }
}
EOF


tos_wallet --json-file="temp_transfer.json" \
    --wallet-path="$WALLET_PATH" --password="$PASSWORD"

# Clean up temporary file
rm temp_transfer.json

echo "=== Batch operations completed ==="