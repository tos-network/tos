# Solana Storage Ownership Pattern Analysis

**Date**: October 27, 2025
**Purpose**: Understand how Solana solves storage ownership for parallel execution

---

## Key Findings from Solana (agave)

### 1. Storage Ownership Pattern

**File**: `agave/runtime/src/bank.rs`

```rust
pub struct BankRc {
    /// where all the Accounts are stored
    pub accounts: Arc<Accounts>,  // ← KEY: Arc wrapper!

    /// Previous checkpoint of this bank
    pub(crate) parent: RwLock<Option<Arc<Bank>>>,

    pub(crate) bank_id_generator: Arc<AtomicU64>,
}

impl BankRc {
    pub(crate) fn new(accounts: Accounts) -> Self {
        Self {
            accounts: Arc::new(accounts),  // ← Wrap in Arc at construction
            parent: RwLock<None>,
            bank_id_generator: Arc::new(AtomicU64::new(0)),
        }
    }
}

pub struct Bank {
    /// References to accounts, parent and signature status
    pub rc: BankRc,  // ← Accounts accessed via BankRc

    // ... other fields
}
```

**Key Insight**: Solana doesn't directly own the accounts database. It wraps it in `Arc` from the beginning, making sharing trivial.

### 2. Parallel Execution Access

**File**: `agave/runtime/src/bank.rs:3243`

```rust
pub fn load_and_execute_transactions(
    &self,  // ← Bank is borrowed, not owned
    batch: &TransactionBatch<impl TransactionWithMeta>,
    // ...
) -> LoadAndExecuteTransactionsOutput {
    // Bank delegates to transaction_processor
    let sanitized_output = self
        .transaction_processor
        .load_and_execute_sanitized_transactions(
            self,  // ← Pass Bank reference
            sanitized_txs,
            check_results,
            &processing_environment,
            &processing_config,
        );

    // ... process results
}
```

**File**: `agave/svm/src/transaction_processor.rs`

The TransactionProcessor can access `bank.rc.accounts` (which is `Arc<Accounts>`) and clone the Arc to share with parallel workers.

---

## Application to TOS

### Current TOS Structure

```rust
// daemon/src/core/blockchain.rs
pub struct Blockchain<S: Storage> {
    // ...
    storage: RwLock<S>,  // ← Owned directly
    // ...
}
```

### Problem

Our `ParallelChainState` needs `Arc<S: Storage>`:

```rust
// daemon/src/core/state/parallel_chain_state.rs
impl<S: Storage> ParallelChainState<S> {
    pub async fn new(
        storage: Arc<S>,  // ← Needs Arc
        environment: Arc<Environment>,
        // ...
    ) -> Arc<Self>
}
```

But `Blockchain` owns `RwLock<S>`, not `Arc<S>`.

### Solution Options

#### Option 1: Refactor Blockchain to use Arc (Solana's approach) ⚠️ BREAKING CHANGE

```rust
pub struct Blockchain<S: Storage> {
    storage: Arc<RwLock<S>>,  // ← Change to Arc<RwLock<S>>
    // ... rest unchanged
}
```

**Pros**:
- Clean, matches Solana's pattern
- Easy to share storage with parallel executor
- Future-proof for other parallel features

**Cons**:
- **Breaking change** - requires updating all code that accesses storage
- Need to audit all `self.storage.write()` and `self.storage.read()` calls
- Potential performance impact (Arc has small overhead)

**Estimated work**: 4-6 hours to refactor + test

#### Option 2: Clone Storage for Parallel Execution (If Storage: Clone) ✅ SIMPLE

```rust
// In add_new_block() or similar
if should_use_parallel_execution(txs.len()) {
    // Get read lock and clone storage
    let storage_clone = {
        let storage_guard = self.storage.read().await;
        storage_guard.clone() // Only works if S: Clone
    };

    // Create Arc wrapper
    let storage_arc = Arc::new(storage_clone);

    // Use for parallel execution
    let parallel_state = ParallelChainState::new(
        storage_arc,
        Arc::new(self.environment.clone()),
        // ...
    ).await;

    // Execute in parallel
    let executor = ParallelExecutor::default();
    let results = executor.execute_batch(parallel_state, txs).await;

    // Merge results back
}
```

**Pros**:
- **Zero breaking changes**
- Simple to implement
- Can be done incrementally

**Cons**:
- Requires `S: Clone` trait bound
- Clone overhead (depends on storage implementation)
- Two copies of storage exist during parallel execution

**Estimated work**: 2-3 hours to implement

#### Option 3: Use Storage Reference Pattern (Complex) ❌ NOT RECOMMENDED

Create a wrapper type that implements Storage by delegating to RwLock<S>.

**Cons**:
- Complex trait implementation
- Lifetime and async issues
- Error-prone

**Status**: Attempted, encountered trait compatibility issues (see `parallel_integration.rs` attempt)

---

## Recommended Approach for TOS

### Immediate (Phase 3 Implementation)

Use **Option 2 (Clone Storage)** with fallback:

```rust
// daemon/src/core/blockchain.rs

impl<S: Storage + Clone> Blockchain<S> {
    async fn execute_transactions_parallel(
        &self,
        txs: Vec<Transaction>,
        stable_topoheight: u64,
        topoheight: u64,
        version: BlockVersion,
    ) -> Result<Vec<TransactionResult>, BlockchainError> {
        use crate::config::{PARALLEL_EXECUTION_ENABLED, MIN_TXS_FOR_PARALLEL};
        use crate::core::executor::ParallelExecutor;
        use crate::core::state::ParallelChainState;

        // Only use parallel if enabled and batch is large enough
        if !PARALLEL_EXECUTION_ENABLED || txs.len() < MIN_TXS_FOR_PARALLEL {
            return self.execute_transactions_sequential(txs).await;
        }

        // Clone storage for parallel execution
        let storage_arc = {
            let storage_guard = self.storage.read().await;
            Arc::new(storage_guard.clone())
        };

        // Create parallel state
        let parallel_state = ParallelChainState::new(
            storage_arc,
            Arc::new(self.environment.clone()),
            stable_topoheight,
            topoheight,
            version,
        ).await;

        // Execute in parallel
        let executor = ParallelExecutor::default();
        let results = executor.execute_batch(parallel_state, txs).await;

        Ok(results)
    }
}
```

### Long-term (Future)

Consider **Option 1 (Arc Refactor)** for:
- Better performance (avoid storage cloning)
- Consistency with Solana's proven pattern
- Support for other parallel features (e.g., parallel signature verification)

**Timeline**: After Phase 3 Implementation proves successful

---

## Implementation Status

**Current Branch**: `feature/parallel-transaction-execution-v3`

**Completed**:
- ✅ V3 architecture (ParallelChainState, ParallelExecutor)
- ✅ Storage loading (cache-first, lazy)
- ✅ Testing framework (4/4 tests passing)
- ✅ Integration analysis (Option B: Hybrid Approach)
- ✅ Configuration flags added
- ✅ Getter methods for state merging
- ✅ Solana research complete

**Next Steps**:
1. Verify that SledStorage implements Clone
2. Implement execute_transactions_parallel() with clone approach
3. Add merge logic to combine parallel results with sequential state
4. Test hybrid execution with devnet
5. Measure performance improvement

**Estimated Time to Working Hybrid Execution**: 6-8 hours

---

## Solana Code References

### Key Files Analyzed

1. **agave/runtime/src/bank.rs**
   - Line 277: `BankRc` struct with `Arc<Accounts>`
   - Line 729: `Bank` struct
   - Line 3243: `load_and_execute_transactions` method

2. **agave/svm/src/transaction_processor.rs**
   - Transaction processing with parallel execution support

### Takeaways

1. **Arc is fundamental** - Solana uses Arc extensively for sharing state
2. **No RwLock<Storage>** - Solana wraps in Arc first, then uses interior mutability where needed
3. **Simple delegation** - Bank delegates parallel work to TransactionProcessor
4. **Clean separation** - Storage/Accounts layer is separate from execution layer

---

## Conclusion

Solana's approach is elegant: **wrap storage in Arc from the beginning**. For TOS, we have two practical paths:

1. **Quick path (Clone)**: Works now, enables Phase 3 Implementation immediately
2. **Optimal path (Arc refactor)**: Better long-term, requires more upfront work

**Recommendation**: Start with Clone approach for Phase 3, plan Arc refactor for later.

---

**Last Updated**: October 27, 2025
**Author**: TOS Development Team + Claude Code
**Status**: Analysis Complete ✅
