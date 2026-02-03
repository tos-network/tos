# TOS Failed Transaction Semantics

> Based on actual implementation in `daemon/src/core/blockchain.rs`

## 1. Overview

When a transaction fails, TOS uses an **orphaning strategy** rather than traditional rollback. This document specifies how different failure modes are handled.

## 2. Failure Modes

TOS has three distinct failure modes:

| Mode | When | Fee Charged? | Nonce Advanced? | State Changes? |
|------|------|--------------|-----------------|----------------|
| Nonce Validation | Before execution | NO | NO | None |
| Execution Failure | During execution | YES | YES | Rolled back |
| Block-Level Failure | During verification | NO | NO | None |

## 3. Nonce Validation Failure

**File**: `daemon/src/core/blockchain.rs:4293-4308`

```rust
// Caught BEFORE execution in apply phase
if !nonce_checker.is_valid_nonce(tx.get_nonce()) {
    // TX orphaned immediately
    // NO FEES DEDUCTED
    return;
}
```

**Behavior**:
- Transaction is orphaned (marked as not executed)
- No fees deducted from sender
- No state changes
- Prevents attack where malicious nonce duplicates consume fees

**Trigger Conditions**:
- Nonce already used
- Nonce doesn't match expected value

## 4. Execution Failure

**File**: `daemon/src/core/blockchain.rs:4321-4334`

```rust
// Caught DURING apply_with_partial_verify()
match state.apply_with_partial_verify(...) {
    Ok(_) => { /* success */ }
    Err(e) => {
        // TX orphaned after execution attempt
        // FEES ALREADY DEDUCTED
    }
}
```

**Behavior**:
- Fee is deducted from sender (credited to miner)
- Nonce is advanced
- Other state changes are rolled back
- Transaction marked as orphaned/failed

**Trigger Conditions**:
- Insufficient balance (after fee deduction)
- Contract revert
- Invalid recipient
- Business logic errors

## 5. Block-Level Failure

**File**: `daemon/src/core/blockchain.rs:3814-3831`

```rust
// Entire block rejected during verification
// NO state changes committed
```

**Behavior**:
- Entire block is rejected
- No transactions executed
- No fees charged
- Block not added to chain

**Trigger Conditions**:
- Invalid block header
- Invalid proof-of-work
- Invalid VRF
- Batch signature verification failure

## 6. State Management

**IMPORTANT**: TOS uses orphaning, NOT traditional rollback.

```
┌─────────────────────────────────────────────────────────────────┐
│  Traditional Rollback (NOT used):                                │
│    1. Begin transaction                                          │
│    2. Execute changes                                            │
│    3. On failure: rollback all changes                          │
├─────────────────────────────────────────────────────────────────┤
│  TOS Orphaning Strategy (USED):                                  │
│    1. Fee deducted first (committed)                            │
│    2. Execute transaction                                        │
│    3. On failure: mark TX as orphaned                           │
│    4. Failed TXs left in block, but not counted as executed     │
└─────────────────────────────────────────────────────────────────┘
```

## 7. Fee Handling

### Successful Transaction

```
1. Deduct fee from sender
2. Execute transaction payload
3. Credit fee to miner
4. Increment nonce
```

### Failed Transaction (Execution)

```
1. Deduct fee from sender  ← COMMITTED
2. Attempt execution       ← FAILS
3. Credit fee to miner     ← COMMITTED
4. Increment nonce         ← COMMITTED
5. Mark TX as orphaned
```

### Failed Transaction (Nonce)

```
1. Check nonce             ← FAILS
2. Mark TX as orphaned
3. No fee deducted
4. No nonce change
```

## 8. Phase Separation

**File**: `daemon/src/core/blockchain.rs`

TOS strictly separates verification and execution:

### Verification Phase (Stateless)
- Lines 3814-3831
- Signature verification
- Balance range checks
- Nonce range checks
- **NO STATE CHANGES**

### Execution Phase (Stateful)
- Lines 4268-4470
- Strict nonce check
- Fee deduction
- Transaction execution
- **STATE CHANGES COMMITTED**

This separation prevents consensus failures from state prediction errors.

## 9. Error Recording

Failed transactions are recorded in the execution result:

```rust
// Transaction marked as orphaned with reason
OrphanedTransaction {
    hash: Hash,
    reason: BlockchainError,
}
```

## 10. Mempool State vs Chain State

- Mempool maintains separate state tracking
- Failed TXs in chain don't affect mempool directly
- Mempool removes TXs that conflict with executed chain state

## 11. DAG and Reorgs

When DAG reorders (reorg):

1. Old order blocks may become orphaned
2. Orphaned blocks: all TXs unexecuted
3. State reverted to pre-orphan point
4. New order executed from that point

**File**: `daemon/src/core/blockchain.rs:4034-4127`

## 12. Summary Table

| Scenario | Fee | Nonce | State | TX Status |
|----------|-----|-------|-------|-----------|
| Success | Deducted | +1 | Applied | Executed |
| Nonce fail | None | None | None | Orphaned |
| Balance fail | Deducted | +1 | Reverted | Orphaned |
| Contract revert | Deducted | +1 | Reverted | Orphaned |
| Block invalid | None | None | None | Not in chain |

---

*Document Version: 1.0*
*Based on: TOS Rust implementation*
*Last Updated: 2026-02-04*
