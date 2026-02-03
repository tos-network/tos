# Hash Algorithms Specification

This document specifies which hash algorithms are used for each purpose in the TOS protocol. All clients MUST use the exact algorithms specified here.

## 1. Hash Algorithm Selection

| Purpose | Algorithm | Output Size | Rationale |
|---------|-----------|-------------|-----------|
| Transaction ID (txid) | BLAKE3 | 32 bytes | Fast, modern, parallelizable |
| Signature Hash (sig_hash) | SHA3-512 | 64 bytes | High security margin for signatures |
| Address Hash | SHA256 + RIPEMD160 | 20 bytes | Bitcoin-compatible, compact |
| State Root | SHA3-256 | 32 bytes | Merkle tree root |
| Block Hash | BLAKE3 | 32 bytes | Same as txid for consistency |

## 2. Transaction ID Computation

The transaction ID (txid) uniquely identifies a transaction:

```
txid = BLAKE3(envelope_without_signature)
```

**Input**: Transaction envelope bytes from offset 0 to end of payload (excluding signature)

**Example**:
```
Envelope (76 bytes, no sig): 01 01 01 ... (transfer)
txid = BLAKE3(envelope)
     = 7a3b4c5d... (32 bytes)
```

**CRITICAL**: The signature is NOT included in txid computation. This allows signature malleability without changing txid.

## 3. Signature Hash Computation

The signature hash (sig_hash) is what gets signed:

```
sig_hash = SHA3-512(envelope_without_signature)
```

**Input**: Same as txid (envelope without signature)

**Why SHA3-512?**
- 512-bit output provides extra security margin
- Different algorithm than txid prevents related-key attacks
- SHA3 is more conservative choice for cryptographic commitments

## 4. Address Derivation

Account addresses are derived from public keys:

```
address = type_byte || RIPEMD160(SHA256(compressed_pubkey))
```

**Steps**:
1. Compress public key to 33 bytes (02/03 prefix + X coordinate)
2. SHA256 hash (32 bytes output)
3. RIPEMD160 hash (20 bytes output)
4. Prepend account type byte (1 byte)
5. Total: 21 bytes

**Example**:
```
Public key (uncompressed): 04 + X (32 bytes) + Y (32 bytes) = 65 bytes
Compressed:                02 + X (32 bytes) = 33 bytes (if Y is even)
                           03 + X (32 bytes) = 33 bytes (if Y is odd)
SHA256:                    e3b0c44298fc1c14... (32 bytes)
RIPEMD160:                 b472a266d0bd89c1... (20 bytes)
Address:                   01 b472a266d0bd89c1... (21 bytes)
```

## 5. State Root Computation

The state root is a Merkle tree root of account states:

```
state_root = SHA3-256(MerkleTree(sorted_accounts))
```

**Account state hash**:
```
account_hash = SHA3-256(address || balance || nonce || energy || data_hash)
```

**Merkle tree construction**:
1. Accounts sorted by address (lexicographic byte order)
2. Binary Merkle tree with SHA3-256
3. Leaves are account hashes
4. Empty tree root = SHA3-256("")

**Merkle tree algorithm**:
```
function merkle_root(leaves):
    if leaves is empty:
        return SHA3-256("")
    if leaves has 1 element:
        return leaves[0]

    # Pad to even number if needed
    if len(leaves) is odd:
        leaves.append(leaves[-1])

    # Build parent level
    parents = []
    for i in range(0, len(leaves), 2):
        parent = SHA3-256(leaves[i] || leaves[i+1])
        parents.append(parent)

    return merkle_root(parents)
```

## 6. Block Hash Computation

Block hash uses BLAKE3 for consistency with transaction IDs:

```
block_hash = BLAKE3(block_header_bytes)
```

**Block header bytes** include:
- Version (u8)
- Height (u64)
- Timestamp (u64)
- Parent hashes (variable)
- Transaction root (32 bytes)
- State root (32 bytes)
- Difficulty (variable)
- Nonce (u64)

## 7. Algorithm Implementation Requirements

### BLAKE3
- Reference: https://github.com/BLAKE3-team/BLAKE3
- Output: 32 bytes (256 bits)
- No key, no context (standard hash mode)

### SHA3-256
- Standard: FIPS 202
- Output: 32 bytes (256 bits)
- Use standard Keccak-f[1600] permutation

### SHA3-512
- Standard: FIPS 202
- Output: 64 bytes (512 bits)
- Use standard Keccak-f[1600] permutation

### SHA256
- Standard: FIPS 180-4
- Output: 32 bytes (256 bits)

### RIPEMD160
- Standard: ISO/IEC 10118-3
- Output: 20 bytes (160 bits)

## 8. Test Vectors

Test vectors for hash algorithms are located in:
- `tck/vectors/crypto/blake3.yaml`
- `tck/vectors/crypto/sha3-256.yaml`
- `tck/vectors/crypto/sha3-512.yaml`
- `tck/vectors/crypto/sha256.yaml`

---

*Document Version: 1.0*
*Last Updated: 2026-02-03*
*Reference: MULTI_CLIENT_ALIGNMENT_SCHEME.md Section 2.B*
