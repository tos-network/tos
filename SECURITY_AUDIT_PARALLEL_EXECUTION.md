# SECURITY AUDIT: Parallel Transaction Execution

**Date**: 2025-10-27
**Branch**: `feature/parallel-transaction-execution-v3`
**Status**: 🔴 **CRITICAL VULNERABILITIES FOUND - DO NOT MERGE TO PRODUCTION**

## Executive Summary

Four critical security vulnerabilities have been identified in the parallel transaction execution implementation. These vulnerabilities would allow:
- Execution of transactions with invalid signatures
- Corruption of account balances
- Inflation of token supply
- Denial of service via unbounded parallelism

**All 4 vulnerabilities MUST be fixed before this feature can be deployed.**

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

- [ ] All 4 vulnerabilities fixed
- [ ] All security tests passing
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
