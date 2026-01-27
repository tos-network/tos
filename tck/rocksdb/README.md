# RocksDB Test Vectors for TOS/Avatar Compatibility

This directory contains test vector generators for verifying binary compatibility
between TOS (Rust) and Avatar (C) RocksDB implementations.

## Overview

These test vectors ensure that:
1. **Serialization** - Both implementations produce identical byte sequences
2. **Key Format** - Database keys are encoded identically
3. **Database Operations** - Both can read/write the same data

## Generating Test Vectors

```bash
cd ~/tos/tck/rocksdb

# Generate all YAML test vectors
cargo run --release --bin gen_serialization_vectors
cargo run --release --bin gen_key_format_vectors
cargo run --release --bin gen_database_vectors
```

## Generated Files

- `serialization.yaml` - Big-endian encoding, Option<T>, Account, VersionedNonce, VersionedBalance
- `key_format.yaml` - Key encoding for all column families
- `database.yaml` - Database operations and expected query results

## Avatar C Side Verification

Build and run the Avatar test:

```bash
cd ~/avatar
make -j
./build/native/bin/test_yaml_vectors
```

Or run the full TOS compatibility test:

```bash
./build/native/bin/test_tos_compat
```

## Test Vector Categories

### Serialization Vectors (serialization.yaml)

| Category | Description |
|----------|-------------|
| big_endian_vectors | u64 big-endian encoding |
| option_u64_vectors | Option<u64> encoding (0x00=None, 0x01+value=Some) |
| account_vectors | Account struct serialization |
| versioned_nonce_vectors | VersionedNonce serialization |
| versioned_balance_vectors | VersionedBalance serialization |

### Key Format Vectors (key_format.yaml)

| Category | Description |
|----------|-------------|
| account_key_vectors | Account lookup keys |
| balance_key_vectors | Balance lookup keys (account + asset + topo) |
| nonce_key_vectors | Nonce lookup keys (account + topo) |
| tx_hash_key_vectors | Transaction hash keys |
| topoheight_key_vectors | Topoheight mapping keys |

### Database Vectors (database.yaml)

| Category | Description |
|----------|-------------|
| account_operations | PUT operations for accounts |
| nonce_operations | PUT operations for nonces |
| balance_operations | PUT operations for balances |
| query_vectors | Expected GET results |

## Encoding Rules

### Big-Endian Integers
All multi-byte integers use network byte order (big-endian):
```
u64 value = 0x123 -> bytes = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x23]
```

### Option<T> Encoding
```
None    -> [0x00]
Some(v) -> [0x01] + serialize(v)
```

### Balance Type
```
Input  = 0
Output = 1
Both   = 2
```

## Adding New Test Vectors

1. Add generator code to the appropriate `gen_*.rs` file
2. Run the generator to update the YAML
3. Update Avatar C test to verify the new vectors
4. Commit both the generator and generated YAML files
