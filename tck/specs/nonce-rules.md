# TOS Nonce Handling Specification

> Based on actual implementation in `daemon/src/core/nonce_checker.rs` and `daemon/src/core/blockchain.rs`

## 1. Overview

The nonce is a strictly monotonically increasing counter per account that:
- Prevents transaction replay
- Orders transactions from the same sender
- Enables transaction replacement in mempool

## 2. Nonce Type

**File**: `common/src/account/nonce.rs`

```rust
pub type Nonce = u64;
```

- 8 bytes, big-endian serialization
- Starts at 0 for new accounts
- Increments by 1 after each successful transaction

## 3. NonceChecker

**File**: `daemon/src/core/nonce_checker.rs`

The `NonceChecker` struct maintains strict sequential nonce tracking:

```rust
pub struct NonceChecker {
    expected_nonce: Nonce,
    executed_nonces: HashMap<Nonce, TopoHeight>,
}
```

### Validation Logic (line 37)

```rust
if self.contains_nonce(&nonce) || nonce != self.expected_nonce {
    return false;
}
```

**Rules**:
1. Nonce must equal `expected_nonce` exactly
2. Nonce must not already be used (no duplicates)

## 4. Validation Phases

### Phase 1: Mempool Validation

**File**: `daemon/src/core/blockchain.rs:2592-2602`

```rust
// Nonce must be within valid range
tx.get_nonce() <= cache.get_max() + 1 &&
tx.get_nonce() >= cache.get_min()
```

**Error**: `InvalidTxNonceMempoolCache` if outside range

### Phase 2: Block Verification (Stateless)

**File**: `daemon/src/core/blockchain.rs:3814-3831`

- Checks nonce is within acceptable range
- Does NOT modify state
- Batch verification with multi-threading

### Phase 3: Block Execution (Stateful)

**File**: `daemon/src/core/blockchain.rs:4293-4309`

```rust
// Strict sequential check during execution
if !nonce_checker.is_valid_nonce(tx.get_nonce()) {
    // TX orphaned, NO fee deducted
    return;
}
```

**Critical**: Nonce validation happens BEFORE execution, so failed nonce = no fee charged.

## 5. Nonce Processing Flow

```
┌─────────────────────────────────────────────────────────────────┐
│  Account State: nonce = 5                                        │
├─────────────────────────────────────────────────────────────────┤
│  TX arrives with nonce = 5                                       │
│    → Mempool: Accept (5 >= min, 5 <= max+1)                     │
│    → Verify: Pass (within range)                                 │
│    → Execute: Pass (5 == expected)                               │
│    → Account nonce → 6                                           │
├─────────────────────────────────────────────────────────────────┤
│  TX arrives with nonce = 4                                       │
│    → Mempool: Reject (4 < min)                                  │
│    → Error: InvalidTxNonceMempoolCache                          │
├─────────────────────────────────────────────────────────────────┤
│  TX arrives with nonce = 7                                       │
│    → Mempool: Reject (7 > max+1, gap too large)                 │
│    → Error: InvalidTxNonceMempoolCache                          │
└─────────────────────────────────────────────────────────────────┘
```

## 6. Error Types

**File**: `daemon/src/core/error.rs`

| Error | Code Line | Description |
|-------|-----------|-------------|
| `TxNonceAlreadyUsed(Nonce, Hash)` | 202 | Duplicate nonce with conflicting TX |
| `InvalidTransactionNonce(Nonce, Nonce)` | 241 | Generic nonce mismatch |
| `InvalidTxNonce(Hash, Nonce, Nonce, Address)` | 320-321 | Detailed nonce error |
| `InvalidTxNonceMempoolCache(Nonce, Nonce, Nonce)` | 322-323 | Nonce outside valid range |
| `InvalidNonce(Hash, Nonce, Nonce)` | 371 | Verification layer error |

## 7. Nonce and Fee Relationship

**Critical Behavior**:

| Failure Point | Fee Charged? | Nonce Advanced? |
|---------------|--------------|-----------------|
| Nonce validation fail | NO | NO |
| Execution fail (other) | YES | YES |

This prevents attacks where malicious nonce duplicates consume victim's fees.

## 8. Transaction Replacement

Mempool allows replacing a pending transaction if:
1. Same sender
2. Same nonce
3. Higher fee

The new transaction replaces the old one in mempool.

## 9. Gap Handling

TOS does NOT support nonce gaps. Transactions must be strictly sequential:

```
Account nonce: 5
Expected: TX with nonce 5
Rejected: TX with nonce 6 (gap not allowed in execution)
```

Mempool may temporarily accept slightly ahead nonces, but execution requires strict sequence.

## 10. Nonce in DAG Context

**File**: `daemon/src/core/nonce_checker.rs`

Nonces are tracked with `TopoHeight` to handle DAG reorgs:

```rust
executed_nonces: HashMap<Nonce, TopoHeight>
```

When blocks are orphaned, nonce state is reverted to allow re-execution.

## 11. Nonce Serialization

**File**: `common/src/serializer/`

```rust
// 8 bytes, big-endian
writer.write_u64(&nonce);
```

## 12. Initial Nonce

New accounts start with nonce = 0.

First transaction from account must have nonce = 0.

---

*Document Version: 1.0*
*Based on: TOS Rust implementation*
*Last Updated: 2026-02-04*
