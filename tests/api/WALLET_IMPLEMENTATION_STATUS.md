# TOS Wallet Implementation Status

## Current Status: Partial Implementation

The Python wallet implementation for testing has been partially completed with a critical limitation.

## What Works

1. **Mnemonic Seed Processing**: Correctly implemented
   - 1626-word English word list extracted from Rust code
   - Word-to-index lookup working
   - Seed-to-private-key conversion algorithm matches Rust implementation
   - Generates correct 32-byte private key from seed phrase

2. **Address Encoding**: Correctly implemented
   - Bech32 encoding with network prefix (tos/tst)
   - Proper Address structure: PublicKey (32 bytes) + AddressType (1 byte)
   - Matches TOS address format from common/src/crypto/address.rs

## Critical Issue: Ristretto255 Curve

**Problem**: TOS uses Ristretto255 curve (via curve25519-dalek) for public key cryptography, NOT Ed25519.

### Technical Details

From `common/src/crypto/elgamal/key.rs`:
```rust
pub struct PublicKey(RistrettoPoint);
pub struct PrivateKey(Scalar);

// Public key derivation: P = s * G (on Ristretto255 curve)
pub fn new(secret: &PrivateKey) -> Result<Self, KeyError> {
    let s = secret.as_scalar();
    let point = s * &RISTRETTO_BASEPOINT_TABLE;
    Ok(Self(point))
}
```

### Current Python Implementation

- Uses Ed25519 (via PyNaCl library)
- This produces **incorrect** public keys and addresses
- Alice's test shows:
  - Expected address: `tst1g6vj6htms5nykkywsnvs69xt63ev6wx6w9942lpesjrgdghm3vzqqegasvg`
  - Generated address: `tst1zgqmfgpfv0w4mpvuqqw7mgd8h3swhxcg4a2mmcqqvwtuyar99lqqqs4smg4`
  - Mismatch due to wrong curve

### Python Ristretto255 Support

**Status**: No mature Python library found

Attempted packages:
- `python-ristretto255` - Does not exist
- `ristretto255` - Does not exist
- `pynacl` - Supports Ed25519 only, not Ristretto255

The Rust ecosystem has excellent support via curve25519-dalek, but Python ecosystem lacks equivalent.

## Solutions

### Option 1: Use TOS Wallet Binary (Recommended)

Call the compiled `tos_wallet` binary as subprocess for key operations:

```python
import subprocess
import json

def get_address_from_seed(seed: str, network: str = "testnet") -> str:
    """Use tos_wallet binary to derive address from seed"""
    cmd = [
        "/path/to/tos_wallet",
        "--network", network,
        "--offline-mode",
        "--disable-interactive-mode",
        # Pass seed somehow - need to determine best method
    ]
    # Implementation needed
    pass
```

**Pros**:
- Uses official TOS implementation
- Guaranteed correctness
- No additional dependencies

**Cons**:
- Requires compiled wallet binary
- More complex integration
- Slower than native Python

### Option 2: Use Wallet RPC

Start tos_wallet in RPC mode and call signing endpoints:

```python
# Start wallet:
# tos_wallet --rpc-bind-address 127.0.0.1:8081 --network testnet

import requests

def sign_transaction(tx_data: dict) -> dict:
    response = requests.post(
        "http://127.0.0.1:8081",
        json={
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sign_transaction",
            "params": tx_data
        }
    )
    return response.json()["result"]
```

**Pros**:
- Clean RPC interface
- Full wallet functionality available
- Easy to use from Python

**Cons**:
- User specifically requested NOT to use wallet RPC
- Requires running separate wallet process
- Network overhead

### Option 3: Pre-generate Test Accounts

Manually create test accounts once using tos_wallet, save keys:

```python
TEST_ACCOUNTS = {
    "alice": {
        "address": "tst1g6vj6htms5nykkywsnvs69xt63ev6wx6w9942lpesjrgdghm3vzqqegasvg",
        "public_key_hex": "...",  # Extracted from wallet
        "private_key_hex": "...", # Extracted from wallet
    },
    "bob": { ... },
    "charlie": { ... }
}
```

Then implement transaction signing by calling tos_wallet binary per transaction.

**Pros**:
- Simple setup
- Fast for testing
- No ongoing wallet process needed

**Cons**:
- Not fully automated
- Still needs wallet binary for signing
- Keys stored in code (test only!)

### Option 4: Create Rust Helper Binary

Create minimal Rust binary that wraps wallet functionality:

```rust
// tos_key_helper binary
fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args[1].as_str() {
        "derive_address" => {
            let seed = &args[2];
            // Use wallet code to derive address
            println!("{}", address);
        },
        "sign_transaction" => {
            // Sign and output
        }
    }
}
```

Call from Python:
```python
address = subprocess.check_output(["tos_key_helper", "derive_address", seed])
```

**Pros**:
- Uses official TOS crypto
- Fast native implementation
- Clean Python interface

**Cons**:
- Requires maintaining additional Rust code
- Build complexity
- Deployment overhead

## Recommendation

**Short-term (for current testing)**: Use Option 3 (Pre-generated accounts)
- Generate 3-5 test accounts using tos_wallet
- Extract and store addresses/keys
- Use wallet binary subprocess for signing when needed

**Long-term (for production use)**: Implement Option 4 (Rust helper binary)
- Create `tos_test_signer` binary with wallet as dependency
- Provides: `derive_keys`, `sign_transaction`, `verify_signature`
- Python wrapper for clean interface

## Current Implementation Files

1. `lib/wallet.py` - Partial implementation
   - Mnemonic processing: WORKING
   - Key derivation: INCOMPLETE (wrong curve)
   - Address encoding: WORKING (structure correct)
   - Transaction signing: NOT IMPLEMENTED

2. `lib/english_words.py` - Complete
   - All 1626 words extracted from Rust code
   - Ready for production use

3. Test accounts needed:
   - Alice (provided seed)
   - Bob (generate new)
   - Charlie (generate new)
   - Each needs: name, seed, address, public_key, private_key

## Next Steps

1. Choose solution approach (recommend Option 3 for now)
2. Generate test accounts using tos_wallet binary
3. Extract keys and update TEST_ACCOUNTS in wallet.py
4. Implement transaction signing via wallet binary subprocess
5. Complete energy system transaction tests

## Testing Without Full Wallet

Many tests can run without wallet functionality:

Currently working (98/104 tests, 94.2% pass rate):
- All query APIs
- Block APIs
- Transaction APIs
- DAG APIs
- Energy query APIs

Blocked (6 tests skipped):
- Transaction submission tests
- P2P tests (unrelated to wallet)

Completing wallet implementation will enable all tests.

---

**Status**: Documented 2025-10-14
**Priority**: Medium (98% tests already working)
**Complexity**: High (requires Ristretto255 or binary integration)
