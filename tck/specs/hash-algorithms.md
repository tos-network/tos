# TOS Hash Algorithm Specification

> Based on actual implementation in `common/src/crypto/`

## 1. Overview

TOS uses different hash algorithms for different purposes. This document specifies which algorithm is used where.

## 2. Hash Algorithm Summary

| Purpose | Algorithm | Output Size | Code Reference |
|---------|-----------|-------------|----------------|
| Transaction ID (txid) | BLAKE3 | 32 bytes | `hash.rs:62-65` |
| Block Hash | BLAKE3 | 32 bytes | `header.rs:407-413` |
| Signature Hash | SHA3-512 | 64 bytes | `signature.rs:94-106` |
| Contract Address | BLAKE3 | 32 bytes | `hash.rs:201-216` |
| Node Identity | SHA3-256 | 32 bytes | `ed25519.rs:160` |

## 3. Transaction ID (txid)

**Algorithm**: BLAKE3
**Output**: 32 bytes
**File**: `common/src/crypto/hash.rs:62-65`

```rust
pub fn hash(value: &[u8]) -> Hash {
    let result: [u8; HASH_SIZE] = blake3_hash(value).into();
    Hash(result)
}
```

**Input**: Serialized transaction bytes (see wire-format.md)

**Usage**:
- Transaction implements `Hashable` trait (`transaction/mod.rs:1042`)
- Used for transaction identification and deduplication
- Stored in block's `txs_hashes` field

## 4. Block Hash

**Algorithm**: BLAKE3
**Output**: 32 bytes
**File**: `common/src/block/header.rs:407-413`

```rust
impl Hashable for BlockHeader {
    fn hash(&self) -> Hash {
        hash(&self.get_serialized_header())
    }
}
```

**Input**: Serialized block header bytes

**Usage**:
- Block identification
- Parent references in `tips` field
- Proof-of-work target comparison

## 5. Signature Hash (Schnorr)

**Algorithm**: SHA3-512
**Output**: 64 bytes (used as Scalar)
**File**: `common/src/crypto/elgamal/signature.rs:94-106`

```rust
pub fn hash_and_point_to_scalar(
    key: &CompressedPublicKey,
    message: &[u8],
    point: &RistrettoPoint,
) -> Scalar {
    let mut hasher = Sha3_512::new();
    hasher.update(key.as_bytes());
    hasher.update(message);
    hasher.update(point.compress().as_bytes());

    let hash = hasher.finalize();
    Scalar::from_bytes_mod_order_wide(&hash.into())
}
```

**Input**: `public_key || message || commitment_point`

**Why SHA3-512?**
- 512-bit output provides full scalar domain for Ristretto
- Different algorithm from txid prevents related-hash attacks
- Converted to Scalar via `from_bytes_mod_order_wide`

## 6. Contract Address (Deterministic)

**Algorithm**: BLAKE3 (double hash)
**Output**: 32 bytes
**File**: `common/src/crypto/hash.rs:201-216`

```rust
pub fn compute_deterministic_contract_address(
    deployer: &CompressedPublicKey,
    bytecode: &[u8],
) -> Hash {
    let code_hash = hash(bytecode);  // BLAKE3

    let mut data = Vec::with_capacity(1 + 32 + 32);
    data.push(0xff);  // Prefix byte
    data.extend_from_slice(deployer.as_bytes());
    data.extend_from_slice(code_hash.as_bytes());

    hash(&data)  // BLAKE3
}
```

**Input**: `0xff || deployer_pubkey(32) || code_hash(32)`

**Usage**:
- CREATE2-style deterministic contract addresses
- Same deployer + same bytecode = same address

## 7. Address Encoding

**Algorithm**: Bech32 (NOT cryptographic hash)
**File**: `common/src/crypto/address.rs:140-181`

Addresses are NOT derived via cryptographic hashing. They use Bech32 encoding:

```rust
pub fn as_string(&self) -> Result<String, Bech32Error> {
    let bits = convert_bits(&self.compress(), 8, 5, true)?;
    let hrp = if self.is_mainnet() {
        PREFIX_ADDRESS   // "tos"
    } else {
        TESTNET_PREFIX_ADDRESS  // "tst"
    };
    encode(hrp.to_owned(), &bits)
}
```

**Human-readable prefixes**:
- Mainnet: `tos1...`
- Testnet: `tst1...`

## 8. Node Identity (P2P Discovery)

**Algorithm**: SHA3-256
**Output**: 32 bytes
**File**: `common/src/crypto/ed25519.rs:160`

Used for Ed25519 node identity in discv6 discovery protocol only.

## 9. Dependencies

**File**: `common/Cargo.toml`

```toml
blake3 = "1.5.1"
sha3 = "0.10.8"
```

## 10. Hash Constants

**File**: `common/src/crypto/hash.rs`

```rust
pub const HASH_SIZE: usize = 32;  // 256 bits
```

## 11. Hashable Trait

**File**: `common/src/crypto/hash.rs:39-43`

```rust
pub trait Hashable {
    fn hash(&self) -> Hash;
}
```

All hashable types (Transaction, Block, etc.) implement this trait.

---

*Document Version: 1.0*
*Based on: TOS Rust implementation*
*Last Updated: 2026-02-04*
