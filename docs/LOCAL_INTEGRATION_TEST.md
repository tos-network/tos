# Local Integration Test Guide

## Test Purpose

This integration test verifies the complete functionality of the TOS blockchain on a local development network (devnet), including:

1. Daemon startup and block production
2. Miner connection and mining operations
3. Wallet creation with correct network addresses
4. Transaction submission and confirmation
5. Balance queries via RPC API
6. Multi-wallet transfer scenarios

## Test Overview

The test simulates a real-world blockchain environment with:
- 1 daemon node (block production)
- 1 miner (proof-of-work)
- 6 wallets (Miner + Wallets A-E)
- Multiple transfer transactions
- Balance verification

**Expected Duration**: 10-15 minutes
**Network**: Devnet (isolated test network)
**Address Prefix**: `tst1` (devnet) vs `tos1` (mainnet)

---

## Prerequisites

### Build Requirements

Ensure all binaries are built before starting:

```bash
cargo build --workspace
```

Expected output: 0 errors, 0 warnings

### Clean Environment

Remove any existing devnet data to start fresh:

```bash
# Kill any running processes
pkill -f tos_daemon
pkill -f tos_miner

# Clean devnet directories
rm -rf ~/tos_devnet/
rm -rf ~/devnet_wallets/
mkdir -p ~/devnet_wallets/

# Clean temporary files
rm -f /tmp/daemon.pid /tmp/miner.pid
rm -f /tmp/*.txt
```

---

## Step 1: Start Devnet Daemon

### Command

```bash
./target/debug/tos_daemon \
  --network devnet \
  --dir-path ~/tos_devnet/ \
  --log-level info \
  --auto-compress-logs \
  > /tmp/devnet_daemon.log 2>&1 &

echo $! > /tmp/daemon.pid
echo "Daemon started with PID $(cat /tmp/daemon.pid)"
```

### Verification

Check daemon is running:

```bash
ps aux | grep tos_daemon | grep -v grep
```

Check logs for successful startup:

```bash
tail -20 /tmp/devnet_daemon.log
```

Expected output:
```
[INFO] Starting TOS Daemon on devnet
[INFO] RPC Server listening on 127.0.0.1:8080
[INFO] P2P Server listening on 0.0.0.0:2125
```

Wait 5-10 seconds for daemon to fully initialize.

---

## Step 2: Create Miner Wallet

### Command

**CRITICAL**: Must use `--network devnet` flag to generate `tst1` address (not `tos1`).

```bash
./target/debug/tos_wallet \
  --network devnet \
  --precomputed-tables-l1 13 \
  --exec "display_address" \
  --wallet-path ~/devnet_wallets/miner \
  --password test123 \
  2>&1 | grep "tst1" | head -1 > /tmp/miner_addr.txt

MINER_ADDR=$(cat /tmp/miner_addr.txt | grep -o "tst1[a-z0-9]*")
echo "Miner Address: $MINER_ADDR"
```

### Expected Output

```
Miner Address: tst1usd3sgut87m6tpywme4yr3ny8ag982660ud3uewtv6zxgn8t6a4qqewldyk
```

**Note**: Address starts with `tst1` (devnet). If it starts with `tos1`, the wallet was created for mainnet and will not work.

---

## Step 3: Start Miner

### Command

Use the miner address from Step 2:

```bash
./target/debug/tos_miner \
  --miner-address tst1usd3sgut87m6tpywme4yr3ny8ag982660ud3uewtv6zxgn8t6a4qqewldyk \
  --daemon-address 127.0.0.1:8080 \
  --num-threads 1 \
  > /tmp/devnet_miner.log 2>&1 &

echo $! > /tmp/miner.pid
echo "Miner started with PID $(cat /tmp/miner.pid)"
```

### Verification

Check miner is running and connected:

```bash
tail -20 /tmp/devnet_miner.log
```

Expected output:
```
[INFO] Connected to daemon at ws://127.0.0.1:8080/getwork/tst1.../worker1
[INFO] Mining with 1 thread(s)
[INFO] Block found! Height: 1, Hash: ...
```

Wait 30-60 seconds for mining to produce blocks (100+ blocks recommended for testing).

Check block height:

```bash
# Create simple RPC test script
cat > /tmp/test_rpc.py << 'EOF'
#!/usr/bin/env python3
import urllib.request
import json

RPC_URL = "http://127.0.0.1:8080/json_rpc"

def call_rpc(method, params=None):
    if params is None:
        params = []

    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    }

    data = json.dumps(payload).encode('utf-8')
    req = urllib.request.Request(
        RPC_URL,
        data=data,
        headers={'Content-Type': 'application/json'}
    )

    try:
        with urllib.request.urlopen(req, timeout=10) as response:
            result = json.loads(response.read().decode('utf-8'))
            print(json.dumps(result, indent=2))
            return result
    except Exception as e:
        print(f"Error: {e}")
        return None

# Test get_info
print("=== Testing get_info ===")
call_rpc("get_info", [])
EOF

python3 /tmp/test_rpc.py
```

---

## Step 4: Create Test Wallets A-E

### Command

Create 5 test wallets with devnet addresses:

```bash
cat > /tmp/create_devnet_wallets.sh << 'EOF'
#!/bin/bash

echo "=== Creating Devnet Wallet A ==="
./target/debug/tos_wallet --network devnet --precomputed-tables-l1 13 --exec "display_address" --wallet-path ~/devnet_wallets/wallet_a --password test123 2>&1 | grep "tst1" | head -1 > /tmp/addr_a.txt
ADDR_A=$(cat /tmp/addr_a.txt | grep -o "tst1[a-z0-9]*")
echo "Wallet A: $ADDR_A"

echo "=== Creating Devnet Wallet B ==="
./target/debug/tos_wallet --network devnet --precomputed-tables-l1 13 --exec "display_address" --wallet-path ~/devnet_wallets/wallet_b --password test123 2>&1 | grep "tst1" | head -1 > /tmp/addr_b.txt
ADDR_B=$(cat /tmp/addr_b.txt | grep -o "tst1[a-z0-9]*")
echo "Wallet B: $ADDR_B"

echo "=== Creating Devnet Wallet C ==="
./target/debug/tos_wallet --network devnet --precomputed-tables-l1 13 --exec "display_address" --wallet-path ~/devnet_wallets/wallet_c --password test123 2>&1 | grep "tst1" | head -1 > /tmp/addr_c.txt
ADDR_C=$(cat /tmp/addr_c.txt | grep -o "tst1[a-z0-9]*")
echo "Wallet C: $ADDR_C"

echo "=== Creating Devnet Wallet D ==="
./target/debug/tos_wallet --network devnet --precomputed-tables-l1 13 --exec "display_address" --wallet-path ~/devnet_wallets/wallet_d --password test123 2>&1 | grep "tst1" | head -1 > /tmp/addr_d.txt
ADDR_D=$(cat /tmp/addr_d.txt | grep -o "tst1[a-z0-9]*")
echo "Wallet D: $ADDR_D"

echo "=== Creating Devnet Wallet E ==="
./target/debug/tos_wallet --network devnet --precomputed-tables-l1 13 --exec "display_address" --wallet-path ~/devnet_wallets/wallet_e --password test123 2>&1 | grep "tst1" | head -1 > /tmp/addr_e.txt
ADDR_E=$(cat /tmp/addr_e.txt | grep -o "tst1[a-z0-9]*")
echo "Wallet E: $ADDR_E"

echo ""
echo "=== Summary ==="
echo "Miner: tst1usd3sgut87m6tpywme4yr3ny8ag982660ud3uewtv6zxgn8t6a4qqewldyk"
echo "A: $ADDR_A"
echo "B: $ADDR_B"
echo "C: $ADDR_C"
echo "D: $ADDR_D"
echo "E: $ADDR_E"
EOF

chmod +x /tmp/create_devnet_wallets.sh
/tmp/create_devnet_wallets.sh
```

### Expected Output

```
Wallet A: tst1dzzh9t3fv8ae3lsmnhda38wk0f787jhpk0h6c39m53mank6dl46sq2l52w5
Wallet B: tst14pkasj002n7hyzheym3fqyd7pm5fmj5x3ve2a06x564gsuf62saqq8mry5u
Wallet C: tst1vj6tj8hyf5x9lwt4a0ujxmd23cawsdykuvry8autedzz439hay8sqjg625u
Wallet D: tst1hzvva4apvuc39f6p5kx9xcz20fdk5fv4ner2c7dkkvlt52qca9qsqlh9fj3
Wallet E: tst1tq3z4edst4mcut9muvwpd56jk4y8wq9g7t3u5mmxl4deytpkxf4sqheqrwc
```

---

## Step 5: Check Initial Balances

### Command

Create balance checking script:

```bash
cat > /tmp/check_all_balances.py << 'EOF'
#!/usr/bin/env python3
import urllib.request
import json

RPC_URL = "http://127.0.0.1:8080/json_rpc"
TOS_ASSET = "0000000000000000000000000000000000000000000000000000000000000000"

wallets = {
    "Miner": "tst1usd3sgut87m6tpywme4yr3ny8ag982660ud3uewtv6zxgn8t6a4qqewldyk",
    "Wallet A": "tst1dzzh9t3fv8ae3lsmnhda38wk0f787jhpk0h6c39m53mank6dl46sq2l52w5",
    "Wallet B": "tst14pkasj002n7hyzheym3fqyd7pm5fmj5x3ve2a06x564gsuf62saqq8mry5u",
    "Wallet C": "tst1vj6tj8hyf5x9lwt4a0ujxmd23cawsdykuvry8autedzz439hay8sqjg625u",
    "Wallet D": "tst1hzvva4apvuc39f6p5kx9xcz20fdk5fv4ner2c7dkkvlt52qca9qsqlh9fj3",
    "Wallet E": "tst1tq3z4edst4mcut9muvwpd56jk4y8wq9g7t3u5mmxl4deytpkxf4sqheqrwc",
}

def call_rpc(method, params):
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    }

    data = json.dumps(payload).encode('utf-8')
    req = urllib.request.Request(
        RPC_URL,
        data=data,
        headers={'Content-Type': 'application/json'}
    )

    try:
        with urllib.request.urlopen(req, timeout=10) as response:
            return json.loads(response.read().decode('utf-8'))
    except Exception as e:
        return {"error": str(e)}

print("=== Checking all wallet balances ===\n")
print(f"{'Wallet':<15} {'Address':<70} {'Balance (TOS)':<20}")
print("=" * 110)

for name, address in wallets.items():
    result = call_rpc("get_balance", {"address": address, "asset": TOS_ASSET})

    if "result" in result:
        balance_atomic = result["result"]["balance"]
        balance_tos = balance_atomic / 100_000_000  # TOS has 8 decimals (10^8)
        print(f"{name:<15} {address:<70} {balance_tos:>15.8f} TOS")
    else:
        print(f"{name:<15} {address:<70} {'Error/No balance':>20}")

print("=" * 110)
EOF

python3 /tmp/check_all_balances.py
```

### Expected Output

```
=== Checking all wallet balances ===

Wallet          Address                                                                Balance (TOS)
==============================================================================================================
Miner           tst1usd3sgut87m6tpywme4yr3ny8ag982660ud3uewtv6zxgn8t6a4qqewldyk           500.12345678 TOS
Wallet A        tst1dzzh9t3fv8ae3lsmnhda38wk0f787jhpk0h6c39m53mank6dl46sq2l52w5              0.00000000 TOS
Wallet B        tst14pkasj002n7hyzheym3fqyd7pm5fmj5x3ve2a06x564gsuf62saqq8mry5u              0.00000000 TOS
Wallet C        tst1vj6tj8hyf5x9lwt4a0ujxmd23cawsdykuvry8autedzz439hay8sqjg625u              0.00000000 TOS
Wallet D        tst1hzvva4apvuc39f6p5kx9xcz20fdk5fv4ner2c7dkkvlt52qca9qsqlh9fj3              0.00000000 TOS
Wallet E        tst1tq3z4edst4mcut9muvwpd56jk4y8wq9g7t3u5mmxl4deytpkxf4sqheqrwc         Error/No balance
==============================================================================================================
```

**Note**:
- Miner should have balance from mining rewards
- Wallets A-E should have zero balance initially
- TOS uses **8 decimals**: 1 TOS = 100,000,000 atomic units

---

## Step 6: Execute Transfers

### Test Scenario

Execute the following transfer sequence:
1. Miner → Wallet A: 1.0 TOS
2. Miner → Wallet B: 2.0 TOS
3. Miner → Wallet C: 3.0 TOS
4. Wallet A → Wallet D: 0.05 TOS

### Commands

#### Transfer 1: Miner → Wallet A (1.0 TOS)

```bash
./target/debug/tos_wallet --network devnet --wallet-path ~/devnet_wallets/miner --password test123 --exec "transfer --address tst1dzzh9t3fv8ae3lsmnhda38wk0f787jhpk0h6c39m53mank6dl46sq2l52w5 --amount 1.0"
```

Wait 5-10 seconds for confirmation, then check balance:

```bash
python3 /tmp/check_all_balances.py
```

#### Transfer 2: Miner → Wallet B (2.0 TOS)

```bash
./target/debug/tos_wallet --network devnet --wallet-path ~/devnet_wallets/miner --password test123 --exec "transfer --address tst14pkasj002n7hyzheym3fqyd7pm5fmj5x3ve2a06x564gsuf62saqq8mry5u --amount 2.0"
```

Wait 5-10 seconds, then check balance.

#### Transfer 3: Miner → Wallet C (3.0 TOS)

```bash
./target/debug/tos_wallet --network devnet --wallet-path ~/devnet_wallets/miner --password test123 --exec "transfer --address tst1vj6tj8hyf5x9lwt4a0ujxmd23cawsdykuvry8autedzz439hay8sqjg625u --amount 3.0"
```

Wait 5-10 seconds, then check balance.

#### Transfer 4: Wallet A → Wallet D (0.05 TOS)

```bash
./target/debug/tos_wallet --network devnet --wallet-path ~/devnet_wallets/wallet_a --password test123 --exec "transfer --address tst1hzvva4apvuc39f6p5kx9xcz20fdk5fv4ner2c7dkkvlt52qca9qsqlh9fj3 --amount 0.05"
```

Wait 5-10 seconds, then check balance.

### Expected Transfer Output

Each transfer should show:

```
Transaction submitted successfully!
TX Hash: 1234567890abcdef...
```

If you see "Insufficient balance" or other errors, wait longer for blocks to confirm and retry.

---

## Step 7: Verify Final Balances

### Command

```bash
python3 /tmp/check_all_balances.py
```

### Expected Output

```
=== Checking all wallet balances ===

Wallet          Address                                                                Balance (TOS)
==============================================================================================================
Miner           tst1usd3sgut87m6tpywme4yr3ny8ag982660ud3uewtv6zxgn8t6a4qqewldyk          1021.67358712 TOS
Wallet A        tst1dzzh9t3fv8ae3lsmnhda38wk0f787jhpk0h6c39m53mank6dl46sq2l52w5             0.94885000 TOS
Wallet B        tst14pkasj002n7hyzheym3fqyd7pm5fmj5x3ve2a06x564gsuf62saqq8mry5u             2.00000000 TOS
Wallet C        tst1vj6tj8hyf5x9lwt4a0ujxmd23cawsdykuvry8autedzz439hay8sqjg625u             3.00000000 TOS
Wallet D        tst1hzvva4apvuc39f6p5kx9xcz20fdk5fv4ner2c7dkkvlt52qca9qsqlh9fj3             0.05000000 TOS
Wallet E        tst1tq3z4edst4mcut9muvwpd56jk4y8wq9g7t3u5mmxl4deytpkxf4sqheqrwc            Error/No balance
==============================================================================================================
```

### Balance Verification

- ✅ **Miner**: Should have mining rewards minus sent amounts (1.0 + 2.0 + 3.0 = 6.0 TOS)
- ✅ **Wallet A**: 1.0 TOS received - 0.05 TOS sent - fees ≈ 0.948 TOS
- ✅ **Wallet B**: Exactly 2.00000000 TOS
- ✅ **Wallet C**: Exactly 3.00000000 TOS
- ✅ **Wallet D**: Exactly 0.05000000 TOS
- ⚠️ **Wallet E**: No balance (no transfers)

**Transaction Fees**: Notice Wallet A has less than 0.95 TOS due to transaction fees when sending to Wallet D.

---

## Step 8: Cleanup

### Stop All Processes

```bash
# Stop miner
kill $(cat /tmp/miner.pid) 2>/dev/null

# Stop daemon
kill $(cat /tmp/daemon.pid) 2>/dev/null

# Verify processes stopped
ps aux | grep -E "tos_daemon|tos_miner" | grep -v grep
```

### Optional: Clean All Data

```bash
# Remove devnet data
rm -rf ~/tos_devnet/
rm -rf ~/devnet_wallets/

# Remove temporary files
rm -f /tmp/daemon.pid /tmp/miner.pid
rm -f /tmp/devnet_*.log
rm -f /tmp/*.txt
rm -f /tmp/*.py
rm -f /tmp/*.sh
```

---

## Common Issues and Solutions

### Issue 1: Miner Cannot Connect (400 Bad Request)

**Error Message**:
```
ERROR tos_miner > Error while connecting to ws://127.0.0.1:8080, got an unexpected response: 400 Bad Request
```

**Root Cause**: Miner wallet was created with mainnet address (`tos1`) instead of devnet address (`tst1`).

**Solution**:
1. Delete miner wallet: `rm -rf ~/devnet_wallets/miner`
2. Recreate with `--network devnet` flag (see Step 2)
3. Restart miner with new address

### Issue 2: RPC 404 Not Found

**Error Message**:
```
HTTP Error 404: Not Found
```

**Root Cause**: RPC endpoint is `/json_rpc`, not root path.

**Solution**: Use `http://127.0.0.1:8080/json_rpc` in all RPC calls.

### Issue 3: Incorrect Balance Display

**Symptom**: Balances show 10x too small (e.g., 0.1 TOS instead of 1.0 TOS).

**Root Cause**: Using 9 decimals (10^9) instead of 8 decimals (10^8).

**Solution**: TOS uses **8 decimals**. Divide atomic units by `100_000_000` (not `1_000_000_000`).

```python
# CORRECT
balance_tos = balance_atomic / 100_000_000  # 8 decimals

# INCORRECT
balance_tos = balance_atomic / 1_000_000_000  # 9 decimals (wrong!)
```

### Issue 4: Transfer Shows "Insufficient Balance"

**Root Cause**: Transaction not yet confirmed, or miner not running.

**Solution**:
1. Check miner is running: `ps aux | grep tos_miner`
2. Wait 10-20 seconds for blocks to confirm
3. Check balance is updated: `python3 /tmp/check_all_balances.py`
4. Retry transfer

### Issue 5: Wallet Command Format Error

**Error Message**:
```
error: unexpected argument '' found
```

**Root Cause**: Shell escaping issues with multiline commands.

**Solution**: Use single-line command format:

```bash
# CORRECT (single line)
./target/debug/tos_wallet --network devnet --wallet-path ~/devnet_wallets/miner --password test123 --exec "transfer --address tst1... --amount 1.0"
```

---

## Test Checklist

Use this checklist to verify test completion:

- [ ] Daemon started successfully (PID saved to `/tmp/daemon.pid`)
- [ ] Miner wallet created with `tst1` prefix
- [ ] Miner connected and mining blocks (100+ blocks)
- [ ] All 5 test wallets created (A-E) with `tst1` prefix
- [ ] Miner has non-zero balance from mining
- [ ] Transfer 1: Miner → Wallet A (1.0 TOS) confirmed
- [ ] Transfer 2: Miner → Wallet B (2.0 TOS) confirmed
- [ ] Transfer 3: Miner → Wallet C (3.0 TOS) confirmed
- [ ] Transfer 4: Wallet A → Wallet D (0.05 TOS) confirmed
- [ ] Final balances match expected values
- [ ] Transaction fees deducted from Wallet A
- [ ] All processes cleaned up

---

## Technical Reference

### TOS Coin Parameters

From `common/src/config.rs`:

```rust
// 8 decimals numbers
pub const COIN_DECIMALS: u8 = 8;
// 100 000 000 to represent 1 TOS
pub const COIN_VALUE: u64 = 10u64.pow(COIN_DECIMALS as u32);
// 184M full coin
pub const MAXIMUM_SUPPLY: u64 = 184_000_000 * COIN_VALUE;
pub const TOS_ASSET: Hash = Hash::zero();
```

### RPC API Reference

**Endpoint**: `http://127.0.0.1:8080/json_rpc`

**Method**: `get_balance`

**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "get_balance",
  "params": {
    "address": "tst1...",
    "asset": "0000000000000000000000000000000000000000000000000000000000000000"
  }
}
```

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "balance": 100000000,
    "topoheight": 123
  }
}
```

### Network Address Prefixes

| Network | Prefix | Example |
|---------|--------|---------|
| Mainnet | `tos1` | `tos1abc...` |
| Testnet | `tos1` | `tos1abc...` |
| Devnet  | `tst1` | `tst1abc...` |

**CRITICAL**: Always use `--network devnet` when creating wallets for devnet testing.

---

## Automation Script

For convenience, here's a complete automation script:

```bash
#!/bin/bash
# File: scripts/devnet_integration_test.sh

set -e

echo "=== TOS Devnet Integration Test ==="
echo ""

# Step 1: Clean environment
echo "Step 1: Cleaning environment..."
pkill -f tos_daemon || true
pkill -f tos_miner || true
rm -rf ~/tos_devnet/
rm -rf ~/devnet_wallets/
mkdir -p ~/devnet_wallets/

# Step 2: Start daemon
echo "Step 2: Starting daemon..."
./target/debug/tos_daemon --network devnet --dir-path ~/tos_devnet/ --log-level info --auto-compress-logs > /tmp/devnet_daemon.log 2>&1 &
echo $! > /tmp/daemon.pid
sleep 5

# Step 3: Create miner wallet
echo "Step 3: Creating miner wallet..."
./target/debug/tos_wallet --network devnet --precomputed-tables-l1 13 --exec "display_address" --wallet-path ~/devnet_wallets/miner --password test123 2>&1 | grep "tst1" | head -1 > /tmp/miner_addr.txt
MINER_ADDR=$(cat /tmp/miner_addr.txt | grep -o "tst1[a-z0-9]*")
echo "Miner Address: $MINER_ADDR"

# Step 4: Start miner
echo "Step 4: Starting miner..."
./target/debug/tos_miner --miner-address $MINER_ADDR --daemon-address 127.0.0.1:8080 --num-threads 1 > /tmp/devnet_miner.log 2>&1 &
echo $! > /tmp/miner.pid
echo "Waiting 60 seconds for mining..."
sleep 60

# Step 5: Create test wallets
echo "Step 5: Creating test wallets A-E..."
./target/debug/tos_wallet --network devnet --precomputed-tables-l1 13 --exec "display_address" --wallet-path ~/devnet_wallets/wallet_a --password test123 2>&1 | grep "tst1" | head -1 > /tmp/addr_a.txt
ADDR_A=$(cat /tmp/addr_a.txt | grep -o "tst1[a-z0-9]*")

./target/debug/tos_wallet --network devnet --precomputed-tables-l1 13 --exec "display_address" --wallet-path ~/devnet_wallets/wallet_b --password test123 2>&1 | grep "tst1" | head -1 > /tmp/addr_b.txt
ADDR_B=$(cat /tmp/addr_b.txt | grep -o "tst1[a-z0-9]*")

./target/debug/tos_wallet --network devnet --precomputed-tables-l1 13 --exec "display_address" --wallet-path ~/devnet_wallets/wallet_c --password test123 2>&1 | grep "tst1" | head -1 > /tmp/addr_c.txt
ADDR_C=$(cat /tmp/addr_c.txt | grep -o "tst1[a-z0-9]*")

./target/debug/tos_wallet --network devnet --precomputed-tables-l1 13 --exec "display_address" --wallet-path ~/devnet_wallets/wallet_d --password test123 2>&1 | grep "tst1" | head -1 > /tmp/addr_d.txt
ADDR_D=$(cat /tmp/addr_d.txt | grep -o "tst1[a-z0-9]*")

./target/debug/tos_wallet --network devnet --precomputed-tables-l1 13 --exec "display_address" --wallet-path ~/devnet_wallets/wallet_e --password test123 2>&1 | grep "tst1" | head -1 > /tmp/addr_e.txt
ADDR_E=$(cat /tmp/addr_e.txt | grep -o "tst1[a-z0-9]*")

echo "Wallet A: $ADDR_A"
echo "Wallet B: $ADDR_B"
echo "Wallet C: $ADDR_C"
echo "Wallet D: $ADDR_D"
echo "Wallet E: $ADDR_E"

# Step 6: Execute transfers
echo "Step 6: Executing transfers..."
./target/debug/tos_wallet --network devnet --wallet-path ~/devnet_wallets/miner --password test123 --exec "transfer --address $ADDR_A --amount 1.0"
sleep 10

./target/debug/tos_wallet --network devnet --wallet-path ~/devnet_wallets/miner --password test123 --exec "transfer --address $ADDR_B --amount 2.0"
sleep 10

./target/debug/tos_wallet --network devnet --wallet-path ~/devnet_wallets/miner --password test123 --exec "transfer --address $ADDR_C --amount 3.0"
sleep 10

./target/debug/tos_wallet --network devnet --wallet-path ~/devnet_wallets/wallet_a --password test123 --exec "transfer --address $ADDR_D --amount 0.05"
sleep 10

# Step 7: Check final balances
echo "Step 7: Checking final balances..."
python3 /tmp/check_all_balances.py

echo ""
echo "=== Test Complete ==="
echo "Daemon PID: $(cat /tmp/daemon.pid)"
echo "Miner PID: $(cat /tmp/miner.pid)"
echo ""
echo "To stop: pkill -f tos_daemon && pkill -f tos_miner"
```

---

## Document Metadata

- **Created**: 2025-11-02
- **Version**: 1.0
- **Purpose**: Integration testing guide for TOS devnet
- **Maintainer**: TOS Development Team
- **Related**: `CLAUDE.md`, `API_REFERENCE.md`

---

**End of Document**
