# TOS Wire Format Specification

> Based on actual implementation in `common/src/transaction/` and `common/src/serializer/`

## 1. Overview

All TOS data structures use **big-endian** encoding for multi-byte integers. This document describes the canonical binary serialization format used for:
- Network transmission
- Transaction ID computation
- Signature generation and verification
- Persistent storage

## 2. Primitive Type Encoding

| Type | Size | Encoding | Code Reference |
|------|------|----------|----------------|
| `u8` | 1 byte | Raw byte | `writer.rs:27` |
| `u16` | 2 bytes | Big-endian | `writer.rs:30` |
| `u32` | 4 bytes | Big-endian | `writer.rs:33` |
| `u64` | 8 bytes | Big-endian | `writer.rs:36` |
| `u128` | 16 bytes | Big-endian | `writer.rs:39` |
| `bool` | 1 byte | `0x00`=false, `0x01`=true | `writer.rs:42` |

**Code Reference** (`common/src/serializer/writer.rs`):
```rust
pub fn write_u64(&mut self, value: &u64) {
    self.bytes.extend(value.to_be_bytes());  // Big-endian
}
```

## 3. Transaction Structure

**File**: `common/src/transaction/mod.rs:885-930`

Serialization order (exact byte sequence):

```
┌─────────────────────────────────────────────────────────────────┐
│  version (1)                                                     │
├─────────────────────────────────────────────────────────────────┤
│  chain_id (1) [only if version >= T1]                           │
├─────────────────────────────────────────────────────────────────┤
│  source (32) - CompressedPublicKey                              │
├─────────────────────────────────────────────────────────────────┤
│  data (variable) - TransactionType                              │
├─────────────────────────────────────────────────────────────────┤
│  fee (8) - u64                                                  │
├─────────────────────────────────────────────────────────────────┤
│  fee_type (1) - u8 enum                                         │
├─────────────────────────────────────────────────────────────────┤
│  nonce (8) - u64                                                │
├─────────────────────────────────────────────────────────────────┤
│  [UNO fields - only for Shield/Unshield/UnoTransfers]           │
│    source_commitments_len (1) + source_commitments (var)        │
│    range_proof (var)                                            │
├─────────────────────────────────────────────────────────────────┤
│  reference (40) - Hash(32) + TopoHeight(8)                      │
├─────────────────────────────────────────────────────────────────┤
│  multisig (variable) - Option<MultiSig>                         │
├─────────────────────────────────────────────────────────────────┤
│  signature (64) - Scalar(32) + Scalar(32)                       │
└─────────────────────────────────────────────────────────────────┘
```

## 4. Transaction Version

**File**: `common/src/transaction/version.rs:39-57`

| Value | Version | Notes |
|-------|---------|-------|
| `0x01` | T1 | Current version, includes chain_id |

## 5. Chain ID

**File**: `common/src/crypto/address.rs`

| Value | Network |
|-------|---------|
| `0x00` | Mainnet |
| `0x01` | Testnet |
| `0x02` | Stagenet |
| `0x03` | Devnet |

## 6. Transaction Type IDs

**File**: `common/src/transaction/mod.rs:516-705`

| ID | Type | Description |
|----|------|-------------|
| 0 | Burn | Destroy tokens |
| 1 | Transfers | Send to multiple recipients |
| 2 | MultiSig | MultiSig operations |
| 3 | InvokeContract | Call contract method |
| 4 | DeployContract | Deploy smart contract |
| 5 | Energy | Buy/refund energy |
| 7 | BindReferrer | Bind referral code |
| 8 | BatchReferralReward | Batch referral rewards |
| 9 | SetKyc | Set KYC verification |
| 10 | RevokeKyc | Revoke KYC |
| 11 | RenewKyc | Renew KYC expiration |
| 12 | BootstrapCommittee | Initialize KYC committee |
| 13 | RegisterCommittee | Register as committee |
| 14 | UpdateCommittee | Update committee params |
| 15 | EmergencySuspend | Emergency suspension |
| 16 | TransferKyc | Transfer KYC |
| 17 | AppealKyc | Appeal KYC decision |
| 18 | UnoTransfers | Private transfers (ZK) |
| 19 | ShieldTransfers | Shield tokens |
| 20 | UnshieldTransfers | Unshield tokens |
| 21 | RegisterName | Register TNS name |
| 22 | EphemeralMessage | Ephemeral messaging |
| 23 | AgentAccount | Agent account ops |
| 24 | CreateEscrow | Create escrow |
| 25 | DepositEscrow | Deposit to escrow |
| 26 | ReleaseEscrow | Release escrow |
| 27 | RefundEscrow | Refund escrow |
| 28 | ChallengeEscrow | Challenge escrow |
| 29 | SubmitVerdict | Arbiter verdict |
| 30 | DisputeEscrow | Dispute escrow |
| 31 | AppealEscrow | Appeal verdict |
| 32 | SubmitVerdictByJuror | Juror verdict |
| 33 | RegisterArbiter | Register as arbiter |
| 34 | UpdateArbiter | Update arbiter |
| 35 | CommitArbitrationOpen | Open arbitration |
| 36 | CommitVoteRequest | Request vote |
| 37 | CommitSelectionCommitment | Selection commitment |
| 38 | CommitJurorVote | Juror vote |
| 44 | SlashArbiter | Slash arbiter stake |
| 45 | RequestArbiterExit | Request exit |
| 46 | WithdrawArbiterStake | Withdraw stake |
| 47 | CancelArbiterExit | Cancel exit |

## 7. Fee Type

**File**: `common/src/transaction/mod.rs:145-165`

| Value | Type | Description |
|-------|------|-------------|
| `0x00` | TOS | Traditional TOS fee |
| `0x01` | Energy | Energy-based fee (Transfers only) |
| `0x02` | UNO | UNO fee (UnoTransfers only, burned) |

## 8. Cryptographic Primitives

### CompressedPublicKey (32 bytes)
**File**: `common/src/crypto/elgamal/compressed.rs:208-220`

Ristretto compressed point (32 bytes).

### Signature (64 bytes)
**File**: `common/src/crypto/elgamal/signature.rs:127-150`

```
[s_scalar: 32 bytes][e_scalar: 32 bytes]
```

Schnorr signature with two scalars.

### Hash (32 bytes)
**File**: `common/src/crypto/hash.rs:92-105`

BLAKE3 hash output (32 bytes).

## 9. Reference Structure (40 bytes)

**File**: `common/src/transaction/reference.rs:31-46`

```
[hash: 32 bytes][topoheight: 8 bytes]
```

References a specific block by hash and topological height.

## 10. Optional Fields

**File**: `common/src/serializer/defaults.rs:355-367`

```
[has_value: 1 byte][value: T size if has_value=1]
```

- `0x00` = None
- `0x01` + data = Some(data)

## 11. Array/Collection Encoding

For Transfers, UnoTransfers, etc:

```
[type_id: 1 byte][count: 2 bytes u16][elements...]
```

- Max 500 transfers per transaction (`MAX_TRANSFER_COUNT`)
- Count is big-endian u16

## 12. Transfer Payload Example

**File**: `common/src/transaction/payload/transfer.rs:74-79`

```
[asset: 32 bytes Hash]
[destination: 32 bytes CompressedPublicKey]
[amount: 8 bytes u64]
[extra_data: Option<DataElement>]
```

## 13. Complete Transaction Example

Minimal single Transfer transaction:

```
Offset | Field              | Size | Value
-------|--------------------| -----|------
0      | version            | 1    | 0x01
1      | chain_id           | 1    | 0x00 (Mainnet)
2-33   | source             | 32   | CompressedPublicKey
34     | type_id            | 1    | 0x01 (Transfers)
35-36  | transfer_count     | 2    | 0x0001
37-68  | asset              | 32   | Hash
69-100 | destination        | 32   | CompressedPublicKey
101-108| amount             | 8    | u64
109    | extra_data         | 1    | 0x00 (None)
110-117| fee                | 8    | u64
118    | fee_type           | 1    | 0x00 (TOS)
119-126| nonce              | 8    | u64
127-158| reference.hash     | 32   | Hash
159-166| reference.topo     | 8    | u64
167    | multisig           | 1    | 0x00 (None)
168-231| signature          | 64   | s(32) + e(32)
-------|--------------------| -----|------
Total  |                    | 232  | bytes
```

## 14. Signing Bytes

**File**: `common/src/transaction/mod.rs:461-483`

The signing data includes all fields EXCEPT multisig and signature:

```rust
fn get_signing_bytes(&self) -> Vec<u8> {
    // version + chain_id + source + data + fee +
    // fee_type + nonce + reference
    // (excludes multisig and signature)
}
```

## 15. MultiSig Structure

**File**: `common/src/transaction/multisig.rs:76-102`

```
[signature_count: 1 byte u8]
[signatures: SignatureId[]]

SignatureId:
  [signer_id: 1 byte u8]
  [signature: 64 bytes]
```

Max 255 signatures per MultiSig.

---

*Document Version: 1.0*
*Based on: TOS Rust implementation*
*Last Updated: 2026-02-04*
