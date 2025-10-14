# Generate Test Accounts Guide

This guide explains how to generate TOS test accounts for API testing.

## Why Manual Generation?

TOS uses **Ristretto255** cryptography (not Ed25519), which lacks mature Python libraries. Therefore, we use the official `tos_wallet` binary to generate account addresses from seeds.

## Prerequisites

Build the wallet binary:
```bash
cd /Users/tomisetsu/tos-network/tos
cargo build --release --bin tos_wallet
```

Binary location: `/Users/tomisetsu/tos-network/tos/target/release/tos_wallet`

## Test Accounts

We have 3 test accounts with predefined seeds:

### 1. Alice [COMPLETE]

**Seed:**
```
tiger eight taxi vexed revamp thorn paddles dosage layout muzzle eggs chlorine sober oyster ecstatic festival banjo behind western segments january behind usage winter paddles
```

**Address:** `tst1g6vj6htms5nykkywsnvs69xt63ev6wx6w9942lpesjrgdghm3vzqqegasvg`

**Status:** ✅ Verified

### 2. Bob [TODO]

**Seed:**
```
ocean swift mountain eagle dancing river frozen sunset golden meadow crystal palace harmony wisdom ancient forest keeper silver dragon mystic lunar phantom voyage
```

**Address:** TO BE GENERATED

**Status:** ⏭ Needs generation

### 3. Charlie [TODO]

**Seed:**
```
cosmic nebula stellar quantum photon aurora borealis cascade thunder lightning plasma fusion reactor galaxy spiral vortex infinite eternal cosmos energy vault nexus
```

**Address:** TO BE GENERATED

**Status:** ⏭ Needs generation

## Generation Steps

For each account (Bob and Charlie):

### Step 1: Start Wallet

```bash
cd /Users/tomisetsu/tos-network/tos
./target/release/tos_wallet --network testnet --offline-mode
```

### Step 2: Recover from Seed

In the wallet prompt:

```
> recover_seed
```

Paste the seed phrase when prompted (e.g., Bob's seed above)

### Step 3: Set Password

```
Password: test123
Confirm: test123
```

### Step 4: Get Address

```
> address
```

Copy the displayed address (starts with `tst1`)

### Step 5: Exit

```
> exit
```

### Step 6: Update Code

Edit `tests/api/lib/wallet_signer.py` and update the corresponding TEST_ACCOUNTS entry:

```python
"bob": WalletAccount(
    name="Bob",
    seed="ocean swift mountain...",
    address="tst1..." # <- PASTE ADDRESS HERE
),
```

## Quick Script

To generate all accounts quickly:

```bash
cd /Users/tomisetsu/tos-network/tos/tests/api

# Bob
echo "Generating Bob..."
./target/release/tos_wallet --network testnet --offline-mode << EOF
recover_seed
ocean swift mountain eagle dancing river frozen sunset golden meadow crystal palace harmony wisdom ancient forest keeper silver dragon mystic lunar phantom voyage
test123
test123
address
exit
EOF

# Charlie
echo "Generating Charlie..."
./target/release/tos_wallet --network testnet --offline-mode << EOF
recover_seed
cosmic nebula stellar quantum photon aurora borealis cascade thunder lightning plasma fusion reactor galaxy spiral vortex infinite eternal cosmos energy vault nexus
test123
test123
address
exit
EOF
```

Look for lines starting with `tst1` in the output.

## Verification

After generating, verify the accounts:

```bash
cd /Users/tomisetsu/tos-network/tos/tests/api
python3 -c "from lib.wallet_signer import TEST_ACCOUNTS; print('\\n'.join(f'{k}: {v.address}' for k,v in TEST_ACCOUNTS.items()))"
```

Expected output:
```
alice: tst1g6vj6htms5nykkywsnvs69xt63ev6wx6w9942lpesjrgdghm3vzqqegasvg
bob: tst1...
charlie: tst1...
```

## Using Test Accounts

In test code:

```python
from lib.wallet_signer import get_test_account

# Get Alice's account
alice = get_test_account("alice")
print(f"Address: {alice.address}")
print(f"Seed: {alice.seed}")

# Use in tests
def test_with_alice(client):
    alice = get_test_account("alice")
    balance = client.call("get_balance", {"address": alice.address})
    assert "balance" in balance
```

## Transaction Signing (Future)

Transaction signing requires either:

**Option A**: Wallet RPC
- Start: `tos_wallet --rpc-bind-address 127.0.0.1:8081 --network testnet`
- Sign via RPC calls

**Option B**: Rust Helper Binary (Recommended long-term)
- Create `tos_test_signer` binary wrapping wallet crypto
- Call from Python for signing

**Option C**: Temporary Wallet (Current approach)
- Create temp wallet from seed when signing needed
- Sign transaction via wallet binary
- Clean up temp wallet

See `WALLET_IMPLEMENTATION_STATUS.md` for details.

---

**Last Updated**: 2025-10-14
**Status**: Alice complete, Bob and Charlie need generation (5 min each)
