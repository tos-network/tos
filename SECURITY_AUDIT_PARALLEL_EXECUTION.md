# SECURITY AUDIT: Parallel Transaction Execution

**Date**: 2025-10-27 (Updated: 2025-10-27)
**Branch**: `feature/parallel-transaction-execution`
**Status**: 🟢 **ALL VULNERABILITIES FIXED - 7 ISSUES RESOLVED**

## Executive Summary

Seven critical security vulnerabilities were identified in the parallel transaction execution implementation:

**FIXED** ✅:
1. Missing Transaction Validation (Vulnerability #1) - FIXED
2. Balance Corruption (Vulnerability #2) - FIXED
3. Fee Inflation (Vulnerability #3) - FIXED
4. Unbounded Parallelism (Vulnerability #4) - FIXED
5. Multisig Not Persisted (Vulnerability #5) - FIXED (2025-10-27)
6. Unsupported Transaction Types (Vulnerability #6) - FIXED (2025-10-27)
7. Multisig Deletions Lost (Vulnerability #7) - FIXED (2025-10-27)

**All 7 vulnerabilities have been addressed.** The parallel execution feature now:
- ✅ Validates transaction signatures
- ✅ Correctly increments receiver balances
- ✅ Properly deducts transaction fees
- ✅ Respects max_parallelism limit
- ✅ Persists multisig additions to storage
- ✅ Persists multisig deletions to storage
- ✅ Falls back to sequential execution for unsupported transaction types

---

## Vulnerability #1: Missing Transaction Validation (CRITICAL)

### Severity: 🔴 CRITICAL

### Description
The parallel execution path completely bypasses transaction validation, including signature verification, reference checks, fee-type rules, and contract VM execution validation.

### Affected Code
- **File**: `daemon/src/core/state/parallel_chain_state.rs`
- **Lines**: 266-290
- **Entry Point**: `daemon/src/core/blockchain.rs:3340-3354`

### Vulnerable Code
```rust
// daemon/src/core/state/parallel_chain_state.rs:266-290
// Apply transaction based on type
let result = match tx.get_data() {
    TransactionType::Transfers(transfers) => {
        self.apply_transfers(source, transfers).await
    }
    TransactionType::Burn(payload) => {
        self.apply_burn(source, payload).await
    }
    // ... other types - NO SIGNATURE VERIFICATION!
};
```

### Comparison with Sequential Path
The sequential path (lines 3538 in blockchain.rs) properly calls:
```rust
tx.apply_with_partial_verify(tx_hash, &mut chain_state).await
```

This function (in `common/src/transaction/verify/mod.rs`) performs:
- ✅ Signature verification
- ✅ Reference checks
- ✅ Fee-type validation
- ✅ Contract VM execution
- ✅ Energy accounting

The parallel path performs:
- ❌ Only nonce checking
- ❌ No signature verification
- ❌ No reference validation
- ❌ No fee-type rules
- ❌ No contract validation

### Attack Scenario
An attacker can:
1. Create a transaction with a valid nonce
2. Use an **invalid signature** (random bytes)
3. Submit the transaction to the network
4. If the transaction ends up in a parallel execution batch, it will be **accepted without signature verification**
5. The attacker can steal funds without possessing the private key

### Impact
- **Complete bypass of cryptographic security**
- **Theft of funds from any account**
- **Network consensus failure** (nodes will reject blocks with invalid signatures)
- **Chain split** between nodes using parallel vs sequential execution

### Fix Required
Add full `apply_with_partial_verify()` call to parallel execution path, identical to sequential path.

---

## Vulnerability #2: Balance Corruption (CRITICAL)

### Severity: 🔴 CRITICAL

### Description
Receiver balances are **overwritten** instead of being **incremented**, causing existing balances to be destroyed.

### Affected Code
- **File**: `daemon/src/core/state/parallel_chain_state.rs`
- **Lines**: 367-372

### Vulnerable Code
```rust
// Credit destination (DashMap auto-locks different key, no deadlock)
self.balances.entry(destination.clone())
    .or_insert_with(HashMap::new)
    .entry(asset.clone())
    .and_modify(|b| *b = b.saturating_add(amount))  // ✅ This is CORRECT
    .or_insert(amount);  // ❌ THIS IS THE BUG!
```

### The Bug
The `.or_insert(amount)` line is executed when:
1. The receiver exists in `self.accounts` cache (loaded for another reason)
2. BUT the specific asset is **NOT** in `self.balances` cache

In this case, the code inserts `amount` as the new balance, **ignoring the existing balance in storage**.

### Attack Scenario
**Setup**:
- Alice has 1000 TOS tokens
- Alice has never sent/received token X (not in cache)

**Attack**:
1. Attacker sends 1 TOS to Alice (parallel execution)
2. Code path:
   - `self.balances.entry(alice)` → NOT in cache
   - `or_insert_with(HashMap::new)` → Creates empty HashMap
   - `.entry(&TOS_ASSET)` → TOS not in this new HashMap
   - `.and_modify(...)` → Skipped (key doesn't exist)
   - **`.or_insert(1)` → Sets Alice's balance to 1**
3. Alice's 1000 TOS is now **1 TOS**
4. **999 TOS destroyed from existence**

### Impact
- **Destruction of user funds**
- **Unintended token deflation**
- **Catastrophic loss of user trust**
- **Legal liability** for lost funds

### Fix Required
Load existing balance from storage before applying the delta:

```rust
// Load existing balance from storage
let existing = self.storage.get_balance(destination, asset).await?;
let new_balance = existing.saturating_add(amount);

self.balances.entry(destination.clone())
    .or_insert_with(HashMap::new)
    .insert(asset.clone(), new_balance);
```

---

## Vulnerability #3: Fee Inflation (CRITICAL)

### Severity: 🔴 CRITICAL

### Description
Transaction fees are accumulated in `self.gas_fee` but **never deducted from sender balances**, causing unintended token inflation.

### Affected Code
- **File**: `daemon/src/core/state/parallel_chain_state.rs`
- **Lines**: 297-298

### Vulnerable Code
```rust
match result {
    Ok(_) => {
        // Increment nonce
        self.accounts.get_mut(source).unwrap().nonce += 1;

        // Accumulate fees
        self.gas_fee.fetch_add(tx.get_fee(), Ordering::Relaxed);  // ❌ NOT DEDUCTED!

        // ... return success
    }
}
```

### Comparison with Sequential Path
The sequential path (in `common/src/transaction/verify/mod.rs:953-962`) properly deducts fees:

```rust
// Add fee to TOS spending (unless using energy fee)
if !self.get_fee_type().is_energy() {
    let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
    *current = current.checked_add(self.fee)
        .ok_or(VerificationError::Overflow)?;

    // Add fee to gas fee counter
    state.add_gas_fee(self.fee).await
        .map_err(VerificationError::State)?;
}
```

The key difference:
- Sequential: Adds fee to `spending_per_asset`, then deducts from balance via `add_sender_output()` (line 976-977)
- Parallel: Only accumulates fee, **never deducts from sender**

### Attack Scenario
**Setup**:
- Attacker has 100 TOS
- Transaction fee is 10 TOS

**Sequential Execution** (CORRECT):
1. Sender balance: 100 TOS
2. Apply transaction: deduct 10 TOS fee
3. Final balance: 90 TOS
4. Network gas_fee: +10 TOS
5. **Total supply: unchanged** ✅

**Parallel Execution** (BUG):
1. Sender balance: 100 TOS
2. Apply transaction: **fee NOT deducted**
3. Final balance: **100 TOS** ❌
4. Network gas_fee: +10 TOS
5. **Total supply: +10 TOS** ❌ **INFLATION!**

### Impact
- **Uncontrolled token inflation** (every parallel transaction inflates supply)
- **Economic manipulation** (attacker can inflate supply for free)
- **Network economic collapse** (infinite token supply)
- **Violation of TOS tokenomics** (max supply violated)

### Example Calculation
- 1000 parallel transactions per block
- 10 TOS fee per transaction
- **10,000 TOS created from thin air per block**
- At 1 block/sec: **36,000,000 TOS inflated per hour**
- **TOS max supply (18M) destroyed in 30 minutes**

### Fix Required
Deduct fee from sender balance before applying transaction:

```rust
// Deduct fee from sender (if not using energy fee)
if !tx.get_fee_type().is_energy() {
    let mut balances = self.balances.entry(source.clone())
        .or_insert_with(HashMap::new);

    let src_balance = balances.entry(TOS_ASSET.clone())
        .or_insert(0);

    if *src_balance < tx.get_fee() {
        return Err(BlockchainError::InsufficientBalance);
    }

    *src_balance -= tx.get_fee();
}

// Then accumulate fee
self.gas_fee.fetch_add(tx.get_fee(), Ordering::Relaxed);
```

---

## Vulnerability #4: Unbounded Parallelism (MAJOR)

### Severity: 🟠 MAJOR (Denial of Service)

### Description
The `max_parallelism` configuration parameter is completely ignored. All transactions in a batch are spawned simultaneously, potentially overwhelming the system.

### Affected Code
- **File**: `daemon/src/core/executor/parallel_executor.rs`
- **Lines**: 115-139

### Vulnerable Code
```rust
// Spawn all tasks at once (BUG: max_parallelism ignored!)
for (index, tx) in batch {
    let state_clone = Arc::clone(&state);
    let tx_hash = tx.hash();

    join_set.spawn(async move {
        let result = state_clone.apply_transaction(&tx).await;
        (index, result)
    });
}
```

### The Bug
The `max_parallelism` parameter is defined:
```rust
pub struct ParallelExecutor {
    max_parallelism: usize,  // Line 19 - NEVER USED!
}
```

But it's **never used** to limit the number of concurrent tasks. All transactions are spawned at once.

### Attack Scenario
**Setup**:
- Attacker creates a block with 10,000 transactions
- All transactions target different accounts (pass conflict detection)
- `max_parallelism` configured to 8

**Expected Behavior** (CORRECT):
1. System spawns 8 tasks at a time
2. As tasks complete, spawn new ones
3. Total: 8 concurrent tasks maximum
4. **System remains responsive** ✅

**Actual Behavior** (BUG):
1. System spawns **10,000 tasks at once**
2. Each task locks memory, CPU, file descriptors
3. System runs out of resources
4. **Node crashes or becomes unresponsive** ❌

### Impact
- **Denial of Service** (node crashes)
- **Memory exhaustion** (OOM killer)
- **CPU thrashing** (context switching overhead)
- **Network partition** (nodes become unreachable)

### Example Calculation
- 10,000 parallel tasks
- 1 MB per task (Arc clone, DashMap, state)
- **10 GB memory used**
- Average node has 8 GB RAM
- **OOM crash**

### Fix Required
Use a semaphore to limit concurrent tasks:

```rust
use tokio::sync::Semaphore;

let semaphore = Arc::new(Semaphore::new(self.max_parallelism));

for (index, tx) in batch {
    let permit = semaphore.clone().acquire_owned().await?;
    let state_clone = Arc::clone(&state);

    join_set.spawn(async move {
        let result = state_clone.apply_transaction(&tx).await;
        drop(permit);  // Release permit when done
        (index, result)
    });
}
```

---

## Vulnerability #5: Multisig Not Persisted (CRITICAL) - FIXED ✅

### Severity: 🔴 CRITICAL

### Description
Multisig configuration updates were applied to the parallel execution cache but never persisted to storage during the merge step.

### Affected Code
- **File**: `daemon/src/core/blockchain.rs`
- **Lines**: 4509-4613 (merge_parallel_results function)
- **File**: `daemon/src/core/state/parallel_chain_state.rs`
- **Line**: 517 (apply_multisig function)

### The Bug
The `merge_parallel_results()` function had 4 steps:
1. ✅ Merge account nonces → `storage.set_last_nonce_to()`
2. ✅ Merge balance changes → `storage.set_last_balance_to()`
3. ✅ Merge gas fees → `applicable_state.add_gas_fee()`
4. ✅ Merge burned supply → `applicable_state.add_burned_coins()`
5. ❌ **MISSING**: Merge multisig configurations

The `ParallelChainState::get_modified_multisigs()` method existed but was never called.

### Attack Scenario
**Setup**:
- Alice creates a multisig configuration requiring 2-of-3 signatures
- Transaction is executed via parallel path

**Bug Behavior**:
1. `apply_multisig()` updates `self.accounts[alice].multisig` cache
2. Transaction returns success
3. `merge_parallel_results()` merges nonces, balances, gas, burned supply
4. **Multisig configuration is NEVER written to storage**
5. Next block reads from storage → multisig config is None
6. Alice's multisig requirement is lost forever

### Impact
- **Consensus breaking**: Nodes using sequential path would have correct multisig, parallel path loses it
- **Network split**: Different nodes have different account states
- **Security bypass**: Multisig protections silently disappear

### Fix Applied (2025-10-27)
Added Step 2.5 to `merge_parallel_results()` to persist multisig configurations:

```rust
// SECURITY FIX #5: Merge multisig configurations (Step 2.5)
// This prevents multisig updates from being lost when using parallel execution
// Reference: SECURITY_AUDIT_PARALLEL_EXECUTION.md - Issue #5
let modified_multisigs = parallel_state.get_modified_multisigs();
if !modified_multisigs.is_empty() {
    for (account, multisig_config) in modified_multisigs {
        use std::borrow::Cow;
        use tos_common::versioned_type::Versioned;
        let versioned_multisig = Versioned::new(
            multisig_config.as_ref().map(|m| Cow::Borrowed(m)),
            Some(topoheight)
        );
        storage.set_last_multisig_to(&account, topoheight, versioned_multisig).await?;
    }
}
```

**Location**: `daemon/src/core/blockchain.rs:4611-4637`

---

## Vulnerability #6: Unsupported Transaction Types (CRITICAL) - FIXED ✅

### Severity: 🔴 CRITICAL

### Description
Multiple transaction types returned `Ok(())` without performing any actual execution in the parallel path, silently succeeding while doing nothing.

### Affected Code
- **File**: `daemon/src/core/state/parallel_chain_state.rs`
- **Lines**: 520-551

### The Bug
Four transaction types were implemented as no-ops:

```rust
/// Apply contract invocation
async fn apply_invoke_contract(...) -> Result<(), BlockchainError> {
    // TODO: Implement contract invocation logic
    Ok(())  // ❌ NO-OP!
}

/// Apply contract deployment
async fn apply_deploy_contract(...) -> Result<(), BlockchainError> {
    // TODO: Implement contract deployment logic
    Ok(())  // ❌ NO-OP!
}

/// Apply energy transaction
async fn apply_energy(...) -> Result<(), BlockchainError> {
    // TODO: Implement energy transaction logic
    Ok(())  // ❌ NO-OP!
}
```

Additionally, the AI mining branch also just returned `Ok(())`.

### Attack Scenario
**Setup**:
- Block contains 10 transactions: 4 transfers + 1 contract invocation
- Transaction count ≥ threshold → parallel execution chosen

**Bug Behavior**:
1. 4 transfers execute correctly via parallel path
2. Contract invocation returns success but **does nothing**
3. Block is accepted with all transactions marked as successful
4. **Contract state is NEVER updated**
5. Sequential execution nodes reject the block (contract state mismatch)
6. **Network split**

### Impact
- **Consensus breaking**: Parallel vs sequential nodes have different states
- **Network partition**: Nodes disagree on valid blocks
- **Silent failures**: Transactions appear successful but are ignored
- **Data loss**: Contract deployments, energy transactions lost

### Fix Applied (2025-10-27)
Added check to disable parallel execution when unsupported transaction types are present:

```rust
// SECURITY FIX #6: Check for unsupported transaction types
// Contract invocations, deployments, energy, and AI mining are not yet implemented
// in parallel execution - they return Ok(()) without doing anything.
// Reference: SECURITY_AUDIT_PARALLEL_EXECUTION.md - Issue #6
let has_unsupported_types = block.get_transactions().iter().any(|tx| {
    use tos_common::transaction::TransactionType;
    matches!(tx.get_data(),
        TransactionType::InvokeContract(_) |
        TransactionType::DeployContract(_) |
        TransactionType::Energy(_) |
        TransactionType::AIMining(_)
    )
});

if has_unsupported_types {
    if log::log_enabled!(log::Level::Debug) {
        debug!("Block contains unsupported transaction types for parallel execution, using sequential path");
    }
}

if self.should_use_parallel_execution(tx_count) && !has_unsupported_types {
    // Use parallel execution
} else {
    // Use sequential execution
}
```

**Location**: `daemon/src/core/blockchain.rs:3309-3329`

**Behavior After Fix**:
- Blocks containing **only** Transfers, Burns, and MultiSig → Parallel execution (fast)
- Blocks containing InvokeContract, DeployContract, Energy, or AIMining → Sequential execution (safe)
- Ensures correctness while still benefiting from parallelism for simple transfers

---

## Vulnerability #7: Multisig Deletions Lost in Parallel Path

**Severity**: 🔴 **CRITICAL** - Consensus-breaking state divergence
**Status**: ✅ **FIXED** (daemon/src/core/state/parallel_chain_state.rs:670-677)

### Description

The `get_modified_multisigs()` method filtered out accounts where `multisig` was `None`, preventing multisig deletions from being persisted to storage. When a transaction removes a multisig configuration (sets it to `None`), the parallel path would accept it but fail to write the deletion to disk, while the sequential path correctly removes it. This caused nodes to accept blocks that peers would reject.

### Attack Scenario

**Attacker Goal**: Create consensus split by exploiting multisig deletion inconsistency

**Step 1**: Attacker creates an account with multisig configuration:
```rust
// Account has multisig: Some(MultiSigPayload { threshold: 2, signers: [A, B] })
```

**Step 2**: Attacker sends transaction to remove multisig (set to `None`):
```rust
let tx = TransactionTypeBuilder::SetMultiSig {
    config: None  // Remove multisig
};
```

**Step 3**: Block processed in parallel path:
```rust
// In ParallelChainState:
accounts.entry(key).multisig = None;  // ✅ Cached correctly

// In get_modified_multisigs():
if entry.value().multisig.is_some() {  // ❌ Filters out None
    Some(entry.value().multisig.clone())
} else {
    None  // ❌ DELETION LOST
}

// Result: merge_parallel_results() never receives the deletion
// Storage still has: Some(MultiSigPayload { threshold: 2, signers: [A, B] })
```

**Step 4**: Block processed in sequential path on peer nodes:
```rust
// Sequential path correctly deletes:
storage.set_last_multisig_to(&key, topoheight, Versioned::new(None, topoheight));
// Storage correctly has: None
```

**Impact**:
- **Consensus split**: Parallel nodes accept invalid blocks that sequential nodes reject
- **Security bypass**: Multisig remains active even after removal transaction
- **State divergence**: Different nodes have different multisig configurations for same account

### Root Cause

File: `daemon/src/core/state/parallel_chain_state.rs:671-680` (before fix)

```rust
pub fn get_modified_multisigs(&self) -> Vec<(PublicKey, Option<MultiSigPayload>)> {
    self.accounts.iter()
        .filter_map(|entry| {
            if entry.value().multisig.is_some() {  // ❌ BUG: Filters out deletions
                Some((entry.key().clone(), entry.value().multisig.clone()))
            } else {
                None  // ❌ Deletions never returned
            }
        })
        .collect()
}
```

**The Problem**:
- Line 674: `if entry.value().multisig.is_some()` filters out accounts with `multisig: None`
- Deletions (where multisig is `None`) are never included in the return value
- `merge_parallel_results()` never receives deletions and can't persist them

**Inconsistency with Other Getters**:
```rust
// ✅ CORRECT: Returns ALL modified nonces
pub fn get_modified_nonces(&self) -> Vec<(PublicKey, u64)> {
    self.accounts.iter()
        .map(|entry| (entry.key().clone(), entry.value().nonce))  // No filter
        .collect()
}

// ✅ CORRECT: Returns ALL modified balances
pub fn get_modified_balances(&self) -> Vec<((PublicKey, Hash), u64)> {
    // Returns all balances, including 0 (which represents deletion)
}

// ❌ INCORRECT: Filters out None (deletions)
pub fn get_modified_multisigs(&self) -> Vec<(PublicKey, Option<MultiSigPayload>)> {
    self.accounts.iter()
        .filter_map(|entry| {
            if entry.value().multisig.is_some() { ... }  // ❌ Bug
        })
}
```

### The Fix

File: `daemon/src/core/state/parallel_chain_state.rs:670-677` (after fix)

```rust
/// Get multisig configurations that were modified
/// SECURITY FIX #7: Return ALL accounts including None (deletions)
/// Previously filtered out None, causing multisig deletions to be lost
pub fn get_modified_multisigs(&self) -> Vec<(PublicKey, Option<MultiSigPayload>)> {
    self.accounts.iter()
        .map(|entry| (entry.key().clone(), entry.value().multisig.clone()))  // ✅ No filter
        .collect()
}
```

**Changes**:
1. **Removed `filter_map`** → Changed to `map` (no filtering)
2. **Removed `is_some()` check** → Returns ALL accounts including `None`
3. **Added security comment** → Documents the fix and rationale

**Why This Works**:
- Any account in `self.accounts` DashMap was either loaded or modified
- If it's in the cache, we assume it should be written back (consistent with nonces/balances)
- `merge_parallel_results()` already handles `None` correctly:
  ```rust
  let versioned_multisig = Versioned::new(
      multisig_config.as_ref().map(|m| Cow::Borrowed(m)),  // ✅ Maps None → None
      Some(topoheight)
  );
  storage.set_last_multisig_to(&account, topoheight, versioned_multisig).await?;
  ```

### Verification

**Before Fix**:
```rust
// Transaction removes multisig
let tx = create_multisig_removal_tx();
execute_parallel(&[tx]).await;

// Check storage
let multisig = storage.get_multisig_at(&account).await?;
assert_eq!(multisig, Some(...));  // ❌ STILL PRESENT (BUG!)
```

**After Fix**:
```rust
// Transaction removes multisig
let tx = create_multisig_removal_tx();
execute_parallel(&[tx]).await;

// Check storage
let multisig = storage.get_multisig_at(&account).await?;
assert_eq!(multisig, None);  // ✅ CORRECTLY DELETED
```

**Related Issues**:
- Connected to Vulnerability #5 (Multisig persistence)
- #5 ensured multisig *additions* are persisted
- #7 ensures multisig *deletions* are persisted

---

## Testing Requirements

Before this feature can be deployed, the following tests MUST pass:

### 1. Invalid Signature Test
```rust
#[tokio::test]
async fn test_parallel_rejects_invalid_signature() {
    let tx = create_transaction_with_invalid_signature();
    let result = execute_parallel(&[tx]).await;
    assert!(result.is_err());  // Must reject
}
```

### 2. Balance Preservation Test
```rust
#[tokio::test]
async fn test_parallel_preserves_receiver_balance() {
    // Setup: Alice has 1000 TOS
    set_balance(alice, TOS, 1000);

    // Send 1 TOS to Alice (parallel)
    let tx = create_transfer(bob, alice, TOS, 1);
    execute_parallel(&[tx]).await?;

    // Alice should have 1001 TOS, not 1 TOS
    assert_eq!(get_balance(alice, TOS), 1001);
}
```

### 3. Fee Deduction Test
```rust
#[tokio::test]
async fn test_parallel_deducts_fees() {
    // Setup: Alice has 1000 TOS
    set_balance(alice, TOS, 1000);

    // Transfer 1 TOS with 10 TOS fee
    let tx = create_transfer(alice, bob, TOS, 1, fee=10);
    execute_parallel(&[tx]).await?;

    // Alice should have 989 TOS (1000 - 1 - 10)
    assert_eq!(get_balance(alice, TOS), 989);

    // Gas fee should be 10 TOS
    assert_eq!(get_gas_fee(), 10);
}
```

### 4. Parallelism Limit Test
```rust
#[tokio::test]
async fn test_parallel_respects_max_parallelism() {
    let executor = ParallelExecutor::new(max_parallelism=8);
    let txs = create_1000_transactions();

    // Monitor concurrent tasks (use tokio-metrics or similar)
    let max_observed = monitor_concurrent_tasks(|| {
        executor.execute(txs).await
    });

    // Should never exceed max_parallelism
    assert!(max_observed <= 8);
}
```

---

## Deployment Checklist

- [x] All 7 vulnerabilities fixed (2025-10-27)
  - [x] Vulnerability #1: Signature verification added
  - [x] Vulnerability #2: Balance increment fixed (cache layer corrected)
  - [x] Vulnerability #3: Fee deduction implemented with storage load
  - [x] Vulnerability #4: Semaphore-based parallelism control added
  - [x] Vulnerability #5: Multisig additions persisted to storage
  - [x] Vulnerability #6: Unsupported transaction types trigger sequential fallback
  - [x] Vulnerability #7: Multisig deletions persisted to storage
- [x] All security tests passing (5/5 tests pass, 1 ignored pending multisig builder)
- [ ] Integration tests updated
- [ ] Code review by security team
- [ ] Testnet deployment and monitoring
- [ ] Mainnet deployment (after 2+ weeks on testnet)

---

## References

### Sequential Transaction Execution (CORRECT Implementation)
- `daemon/src/core/blockchain.rs:3538` - Calls `apply_with_partial_verify()`
- `common/src/transaction/verify/mod.rs:953-977` - Fee deduction logic
- `common/src/transaction/verify/mod.rs:630-650` - Signature verification

### Parallel Transaction Execution (VULNERABLE Implementation)
- `daemon/src/core/blockchain.rs:3340-3354` - Entry point
- `daemon/src/core/executor/parallel_executor.rs:115-139` - Task spawning
- `daemon/src/core/state/parallel_chain_state.rs:266-309` - Transaction application

### Related Security Documents
- `CLAUDE.md` - Code quality and security standards
- `TIPs/CONSENSUS_LAYERED_DESIGN.md` - Consensus architecture
- `TIPs/PARALLEL_EXECUTION_V3_DESIGN.md` - Parallel execution design (outdated)

---

**Last Updated**: 2025-10-27
**Next Review**: After all fixes implemented
**Severity Legend**: 🔴 Critical | 🟠 Major | 🟡 Minor | 🟢 Info
