# TOS Parallel Execution V3 - Next Steps Roadmap

**Current Status**: Foundation Complete (30%)
**Goal**: Production-Ready Parallel Execution (100%)

---

## Phase 1: Make It Work (Priority 1) - 1-2 Days

### 1.1 Implement Storage Loading (4-6 hours)

**Problem**: Currently creates empty accounts with nonce=0 and empty balances
**Impact**: All transactions will fail because accounts have no balance

**Task**: Load existing state from storage before execution

```rust
// File: daemon/src/core/state/parallel_chain_state.rs

impl<S: Storage> ParallelChainState<S> {
    /// Load account state from storage if not in cache
    async fn ensure_account_loaded(&self, key: &PublicKey) -> Result<(), BlockchainError> {
        if self.accounts.contains_key(key) {
            return Ok(()); // Already loaded
        }

        // Load from storage
        let nonce = self.storage.get_last_nonce(key, self.topoheight)
            .await?
            .map(|v| v.nonce)
            .unwrap_or(0);

        let multisig = self.storage.get_multisig_state(key)
            .await
            .ok();

        // Note: Balances are loaded on-demand in ensure_balance_loaded()

        self.accounts.insert(key.clone(), AccountState {
            nonce,
            balances: HashMap::new(), // Lazy load balances
            multisig,
        });

        Ok(())
    }

    /// Load balance from storage if not in cache
    async fn ensure_balance_loaded(
        &self,
        account: &PublicKey,
        asset: &Hash,
    ) -> Result<(), BlockchainError> {
        // Check if account entry exists
        if let Some(mut entry) = self.accounts.get_mut(account) {
            if entry.balances.contains_key(asset) {
                return Ok(()); // Already loaded
            }

            // Load balance from storage
            let balance = self.storage
                .get_last_balance(account, asset, self.topoheight)
                .await?
                .map(|v| v.balance)
                .unwrap_or(0);

            entry.balances.insert(asset.clone(), balance);
        }

        Ok(())
    }

    /// Modified apply_transaction with storage loading
    pub async fn apply_transaction(
        &self,
        tx: &Transaction,
    ) -> Result<TransactionResult, BlockchainError> {
        use log::{debug, trace};

        let source = tx.get_source();
        let tx_hash = tx.hash();

        if log::log_enabled!(log::Level::Debug) {
            debug!("Applying transaction {} from {} at topoheight {}",
                   tx_hash, source.as_address(self.storage.is_mainnet()), self.topoheight);
        }

        // STEP 1: Load account state from storage
        self.ensure_account_loaded(source).await?;

        // STEP 2: Verify nonce
        let current_nonce = {
            let account = self.accounts.get(source).unwrap();
            account.nonce
        };

        if tx.get_nonce() != current_nonce {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Invalid nonce for transaction {}: expected {}, got {}",
                       tx_hash, current_nonce, tx.get_nonce());
            }
            return Ok(TransactionResult {
                tx_hash,
                success: false,
                error: Some(format!("Invalid nonce: expected {}, got {}", current_nonce, tx.get_nonce())),
                gas_used: 0,
            });
        }

        // STEP 3: Apply transaction based on type
        let result = match tx.get_data() {
            TransactionType::Transfers(transfers) => {
                self.apply_transfers(source, transfers).await
            }
            // ... other types
        };

        // STEP 4: Update nonce and fees on success
        match result {
            Ok(_) => {
                self.accounts.get_mut(source).unwrap().nonce += 1;
                self.gas_fee.fetch_add(tx.get_fee(), Ordering::Relaxed);

                if log::log_enabled!(log::Level::Debug) {
                    debug!("Transaction {} applied successfully", tx_hash);
                }

                Ok(TransactionResult {
                    tx_hash,
                    success: true,
                    error: None,
                    gas_used: tx.get_fee(),
                })
            }
            Err(e) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Transaction {} failed: {:?}", tx_hash, e);
                }
                Ok(TransactionResult {
                    tx_hash,
                    success: false,
                    error: Some(format!("{:?}", e)),
                    gas_used: 0,
                })
            }
        }
    }

    /// Modified apply_transfers with balance loading
    async fn apply_transfers(
        &self,
        source: &PublicKey,
        transfers: &[TransferPayload],
    ) -> Result<(), BlockchainError> {
        use log::{debug, trace};

        if log::log_enabled!(log::Level::Trace) {
            trace!("Applying {} transfers from {}", transfers.len(), source.as_address(self.storage.is_mainnet()));
        }

        for transfer in transfers {
            let asset = transfer.get_asset();
            let amount = transfer.get_amount();
            let destination = transfer.get_destination();

            // LOAD BALANCE FROM STORAGE IF NOT IN CACHE
            self.ensure_balance_loaded(source, asset).await?;

            // Check and deduct from source balance
            {
                let mut account = self.accounts.get_mut(source).unwrap();
                let src_balance = account.balances.get_mut(asset)
                    .ok_or_else(|| {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Source {} has no balance for asset {}", source.as_address(self.storage.is_mainnet()), asset);
                        }
                        BlockchainError::NoBalance(source.as_address(self.storage.is_mainnet()))
                    })?;

                if *src_balance < amount {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Insufficient funds: source {} has {} but needs {} for asset {}",
                               source.as_address(self.storage.is_mainnet()), src_balance, amount, asset);
                    }
                    return Err(BlockchainError::NoBalance(source.as_address(self.storage.is_mainnet())));
                }

                *src_balance -= amount;
            }

            // Credit destination (DashMap auto-locks different key, no deadlock)
            self.balances.entry(destination.clone())
                .or_insert_with(HashMap::new)
                .entry(asset.clone())
                .and_modify(|b| *b = b.saturating_add(amount))
                .or_insert(amount);

            if log::log_enabled!(log::Level::Trace) {
                trace!("Transferred {} of asset {} from {} to {}",
                       amount, asset, source.as_address(self.storage.is_mainnet()),
                       destination.as_address(self.storage.is_mainnet()));
            }
        }

        Ok(())
    }

    /// Similar changes needed for apply_burn
    async fn apply_burn(
        &self,
        source: &PublicKey,
        payload: &BurnPayload,
    ) -> Result<(), BlockchainError> {
        use log::{debug, trace};

        let asset = &payload.asset;
        let amount = payload.amount;

        if log::log_enabled!(log::Level::Trace) {
            trace!("Burning {} of asset {} from {}", amount, asset, source.as_address(self.storage.is_mainnet()));
        }

        // LOAD BALANCE FROM STORAGE
        self.ensure_balance_loaded(source, asset).await?;

        // Check and deduct from source balance
        {
            let mut account = self.accounts.get_mut(source).unwrap();
            let src_balance = account.balances.get_mut(asset)
                .ok_or_else(|| BlockchainError::NoBalance(source.as_address(self.storage.is_mainnet())))?;

            if *src_balance < amount {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Insufficient funds for burn: source {} has {} but needs {}",
                           source.as_address(self.storage.is_mainnet()), src_balance, amount);
                }
                return Err(BlockchainError::NoBalance(source.as_address(self.storage.is_mainnet())));
            }

            *src_balance -= amount;
        }

        // Accumulate burned supply
        self.burned_supply.fetch_add(amount, Ordering::Relaxed);

        if log::log_enabled!(log::Level::Debug) {
            debug!("Burned {} of asset {} from {}", amount, asset, source.as_address(self.storage.is_mainnet()));
        }

        Ok(())
    }
}
```

**Files to modify**:
- `daemon/src/core/state/parallel_chain_state.rs` (add 3 methods, modify 3 methods)

**Estimated time**: 4-6 hours

---

### 1.2 Write Basic Integration Test (2-3 hours)

**Goal**: Prove the parallel execution actually works end-to-end

```rust
// File: daemon/src/core/executor/tests/integration_test.rs

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::sync::Arc;
    use tos_common::{
        crypto::KeyPair,
        transaction::*,
        config::TOS_ASSET,
    };
    use tos_environment::Environment;
    use crate::core::{
        executor::ParallelExecutor,
        state::ParallelChainState,
        storage::Storage,
    };

    async fn create_test_storage() -> Arc<impl Storage> {
        // TODO: Use real storage implementation
        // For now, use in-memory storage
        Arc::new(crate::core::storage::InMemoryStorage::new())
    }

    #[tokio::test]
    async fn test_parallel_transfers_no_conflict() {
        // Create test accounts
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let charlie = KeyPair::generate();
        let dave = KeyPair::generate();

        // Setup storage with initial balances
        let storage = create_test_storage().await;
        storage.set_balance(&alice.public_key, &TOS_ASSET, 1000).await.unwrap();
        storage.set_balance(&charlie.public_key, &TOS_ASSET, 1000).await.unwrap();

        // Create environment
        let environment = Arc::new(Environment::default());

        // Create parallel state
        let state = ParallelChainState::new(
            Arc::clone(&storage),
            environment,
            0,  // stable_topoheight
            1,  // topoheight
            BlockVersion::V0,
        ).await;

        // Create non-conflicting transactions
        let tx1 = Transaction::new_transfer(
            &alice,
            0,  // nonce
            vec![TransferPayload::new(TOS_ASSET.clone(), bob.public_key.clone(), 100)],
            100,  // fee
        );

        let tx2 = Transaction::new_transfer(
            &charlie,
            0,  // nonce
            vec![TransferPayload::new(TOS_ASSET.clone(), dave.public_key.clone(), 200)],
            100,  // fee
        );

        // Execute in parallel
        let executor = ParallelExecutor::new();
        let results = executor.execute_batch(
            Arc::clone(&state),
            vec![tx1, tx2],
        ).await;

        // Verify results
        assert_eq!(results.len(), 2);
        assert!(results[0].success, "Alice->Bob transfer should succeed");
        assert!(results[1].success, "Charlie->Dave transfer should succeed");

        // Commit to storage
        let mut storage_mut = Arc::try_unwrap(storage).unwrap();
        state.commit(&mut storage_mut).await.unwrap();

        // Verify final balances
        // Alice: 1000 - 100 - 100(fee) = 800
        // Bob: 0 + 100 = 100
        // Charlie: 1000 - 200 - 100(fee) = 700
        // Dave: 0 + 200 = 200
    }

    #[tokio::test]
    async fn test_parallel_transfers_with_conflict() {
        // Create test accounts
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let charlie = KeyPair::generate();

        // Setup storage
        let storage = create_test_storage().await;
        storage.set_balance(&alice.public_key, &TOS_ASSET, 1000).await.unwrap();

        let environment = Arc::new(Environment::default());
        let state = ParallelChainState::new(
            Arc::clone(&storage),
            environment,
            0,
            1,
            BlockVersion::V0,
        ).await;

        // Create conflicting transactions (same source account)
        let tx1 = Transaction::new_transfer(
            &alice,
            0,  // nonce 0
            vec![TransferPayload::new(TOS_ASSET.clone(), bob.public_key.clone(), 100)],
            100,
        );

        let tx2 = Transaction::new_transfer(
            &alice,
            1,  // nonce 1 (sequential)
            vec![TransferPayload::new(TOS_ASSET.clone(), charlie.public_key.clone(), 200)],
            100,
        );

        // Execute - should be batched sequentially
        let executor = ParallelExecutor::new();
        let results = executor.execute_batch(
            Arc::clone(&state),
            vec![tx1, tx2],
        ).await;

        // Verify results
        assert_eq!(results.len(), 2);
        assert!(results[0].success, "First transfer should succeed");
        assert!(results[1].success, "Second transfer should succeed");

        // Verify nonce incremented correctly
        let account = state.accounts.get(&alice.public_key).unwrap();
        assert_eq!(account.nonce, 2);
    }

    #[tokio::test]
    async fn test_invalid_nonce_rejection() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();

        let storage = create_test_storage().await;
        storage.set_balance(&alice.public_key, &TOS_ASSET, 1000).await.unwrap();
        storage.set_nonce(&alice.public_key, 5).await.unwrap(); // Existing nonce = 5

        let environment = Arc::new(Environment::default());
        let state = ParallelChainState::new(
            Arc::clone(&storage),
            environment,
            0,
            1,
            BlockVersion::V0,
        ).await;

        // Create transaction with wrong nonce
        let tx = Transaction::new_transfer(
            &alice,
            0,  // Wrong nonce! Should be 5
            vec![TransferPayload::new(TOS_ASSET.clone(), bob.public_key.clone(), 100)],
            100,
        );

        let executor = ParallelExecutor::new();
        let results = executor.execute_batch(
            Arc::clone(&state),
            vec![tx],
        ).await;

        // Verify rejection
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].error.is_some());
        assert!(results[0].error.as_ref().unwrap().contains("Invalid nonce"));
    }
}
```

**Files to create**:
- `daemon/src/core/executor/tests/integration_test.rs`
- `daemon/src/core/executor/tests/mod.rs`

**Estimated time**: 2-3 hours

---

### 1.3 Blockchain Integration (3-4 hours)

**Goal**: Connect parallel executor to actual block execution in blockchain.rs

```rust
// File: daemon/src/core/blockchain.rs

impl<S: Storage> Blockchain<S> {
    /// Execute transactions in a block using parallel execution
    pub async fn execute_transactions_parallel(
        &self,
        block: &Block,
        transactions: Vec<Transaction>,
    ) -> Result<Vec<TransactionResult>, BlockchainError> {
        use log::info;
        use crate::core::{
            executor::ParallelExecutor,
            state::ParallelChainState,
        };

        if log::log_enabled!(log::Level::Info) {
            info!("Executing {} transactions in parallel for block {} at height {}",
                  transactions.len(),
                  block.hash(),
                  block.get_height());
        }

        // Create parallel state
        let state = ParallelChainState::new(
            Arc::new(self.storage.clone()), // FIXME: Storage ownership issue
            Arc::new(self.environment.clone()),
            block.get_stable_topoheight(),
            block.get_topoheight(),
            block.get_version(),
        ).await;

        // Execute in parallel
        let executor = ParallelExecutor::new();
        let results = executor.execute_batch(
            Arc::clone(&state),
            transactions,
        ).await;

        // Commit changes to storage
        state.commit(&mut self.storage).await?;

        Ok(results)
    }

    /// Modified apply_block to optionally use parallel execution
    pub async fn apply_block(
        &mut self,
        block: Block,
        use_parallel: bool,  // Configuration flag
    ) -> Result<(), BlockchainError> {
        // ... existing validation code ...

        let transactions = self.get_transactions_for_block(&block).await?;

        let results = if use_parallel && transactions.len() > 10 {
            // Use parallel execution for large batches
            self.execute_transactions_parallel(&block, transactions).await?
        } else {
            // Use sequential execution for small batches
            self.execute_transactions_sequential(&block, transactions).await?
        };

        // ... rest of block application ...
    }
}
```

**Files to modify**:
- `daemon/src/core/blockchain.rs` (add 2 methods, modify 1 method)

**Challenges**:
- Storage ownership/borrowing issues (Arc vs &mut)
- Environment cloning
- Backward compatibility with sequential execution

**Estimated time**: 3-4 hours

---

## Phase 2: Make It Fast (Priority 2) - 1 Day

### 2.1 Performance Benchmarking (3-4 hours)

**Goal**: Measure actual speedup vs sequential execution

```rust
// File: daemon/benches/parallel_vs_sequential.rs

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use tos_daemon::core::{
    executor::ParallelExecutor,
    state::ParallelChainState,
};

fn benchmark_parallel_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("transaction_execution");

    for num_txs in [10, 50, 100, 500, 1000] {
        // Benchmark sequential
        group.bench_with_input(
            BenchmarkId::new("sequential", num_txs),
            &num_txs,
            |b, &n| {
                b.iter(|| {
                    // Execute n transactions sequentially
                });
            },
        );

        // Benchmark parallel (0% conflict)
        group.bench_with_input(
            BenchmarkId::new("parallel_0pct_conflict", num_txs),
            &num_txs,
            |b, &n| {
                b.iter(|| {
                    // Execute n non-conflicting transactions in parallel
                });
            },
        );

        // Benchmark parallel (50% conflict)
        group.bench_with_input(
            BenchmarkId::new("parallel_50pct_conflict", num_txs),
            &num_txs,
            |b, &n| {
                b.iter(|| {
                    // Execute n transactions with 50% conflict rate
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_parallel_execution);
criterion_main!(benches);
```

**Expected results**:
- 0% conflict: 6-8x speedup
- 25% conflict: 3-4x speedup
- 50% conflict: 2-3x speedup
- 100% conflict: 1x (sequential)

**Estimated time**: 3-4 hours

---

### 2.2 Optimization Tuning (2-3 hours)

**Areas to optimize**:

1. **Batch size tuning**
   ```rust
   // Current: Process all non-conflicting transactions together
   // Better: Limit batch size to avoid task overhead
   const MAX_BATCH_SIZE: usize = 100;
   ```

2. **Thread pool sizing**
   ```rust
   // Current: num_cpus::get()
   // Better: Configurable with reasonable defaults
   let optimal_threads = std::cmp::min(
       num_cpus::get(),
       transactions.len() / 10,
   ).max(1);
   ```

3. **Pre-loading optimization**
   ```rust
   // Pre-load all accounts in batch before parallel execution
   async fn preload_accounts(&self, txs: &[Transaction]) {
       let unique_accounts: HashSet<_> = txs.iter()
           .flat_map(|tx| extract_accounts(tx))
           .collect();

       // Load all in parallel using join_all
       let futures: Vec<_> = unique_accounts.iter()
           .map(|acc| self.ensure_account_loaded(acc))
           .collect();

       join_all(futures).await;
   }
   ```

**Estimated time**: 2-3 hours

---

## Phase 3: Make It Complete (Priority 3) - 2-3 Days

### 3.1 Contract Execution Support (1-2 days)

**Challenge**: Contracts can have unpredictable side effects

**Approach**: Conservative - treat all contract invocations as conflicting

```rust
async fn apply_invoke_contract(
    &self,
    source: &PublicKey,
    payload: &InvokeContractPayload,
) -> Result<(), BlockchainError> {
    // Load contract module
    let contract_hash = &payload.contract;
    let module = self.load_contract_module(contract_hash).await?;

    // Prepare deposits (load balances)
    for deposit in &payload.deposits {
        self.ensure_balance_loaded(source, &deposit.asset).await?;
    }

    // Execute in VM
    let mut vm_context = VMContext::new(
        self.environment.as_ref(),
        source,
        contract_hash,
        self.topoheight,
    );

    let result = module.invoke(
        &payload.method,
        &payload.parameters,
        &mut vm_context,
    )?;

    // Apply state changes from VM
    self.apply_vm_state_changes(vm_context.get_changes()).await?;

    Ok(())
}
```

**Issues to solve**:
- VM state isolation (each parallel task needs separate VM instance)
- Contract storage conflicts (contracts touching same storage keys)
- Gas metering in parallel context

**Estimated time**: 1-2 days

---

### 3.2 Error Recovery and Rollback (4-6 hours)

**Problem**: If transaction fails mid-execution, partial state changes remain

**Solution A**: Transaction-local state (copy-on-write)
```rust
struct TransactionLocalState {
    account_deltas: HashMap<PublicKey, AccountDelta>,
    balance_deltas: HashMap<(PublicKey, Hash), i64>,
}

// On success: Merge deltas into main state
// On failure: Discard deltas
```

**Solution B**: Snapshots (simpler but slower)
```rust
// Take snapshot before execution
let snapshot = state.snapshot();

// Execute transaction
match state.apply_transaction(tx).await {
    Ok(_) => { /* Keep changes */ }
    Err(_) => state.restore(snapshot), // Rollback
}
```

**Recommendation**: Start with Solution B for correctness, optimize to Solution A later

**Estimated time**: 4-6 hours

---

## Phase 4: Make It Production-Ready (Priority 4) - 1 Week

### 4.1 Monitoring and Metrics (1 day)

Add Prometheus metrics:
```rust
use prometheus::{IntCounter, Histogram};

lazy_static! {
    static ref PARALLEL_TX_TOTAL: IntCounter = register_int_counter!(
        "tos_parallel_transactions_total",
        "Total transactions executed in parallel"
    ).unwrap();

    static ref PARALLEL_BATCH_SIZE: Histogram = register_histogram!(
        "tos_parallel_batch_size",
        "Size of parallel execution batches"
    ).unwrap();

    static ref PARALLEL_SPEEDUP: Histogram = register_histogram!(
        "tos_parallel_speedup_ratio",
        "Speedup ratio vs sequential execution"
    ).unwrap();
}
```

### 4.2 Configuration System (1 day)

```rust
pub struct ParallelExecutionConfig {
    /// Enable parallel execution
    pub enabled: bool,
    /// Minimum transactions to trigger parallel execution
    pub min_batch_size: usize,
    /// Maximum parallel tasks
    pub max_parallelism: usize,
    /// Enable contract parallel execution (risky)
    pub enable_contract_parallel: bool,
}

impl Default for ParallelExecutionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_batch_size: 10,
            max_parallelism: num_cpus::get(),
            enable_contract_parallel: false, // Conservative default
        }
    }
}
```

### 4.3 Comprehensive Testing (2-3 days)

1. **Unit tests** - Each component in isolation
2. **Integration tests** - End-to-end scenarios
3. **Stress tests** - Large batches (10k+ transactions)
4. **Chaos tests** - Random failures, panics, timeouts
5. **Regression tests** - Compare results with sequential execution

### 4.4 Documentation (1 day)

1. **API documentation** - Rustdoc comments
2. **Architecture guide** - How it works
3. **Performance guide** - When to use parallel vs sequential
4. **Troubleshooting guide** - Common issues

---

## Summary: Recommended Action Plan

### This Week (Priority 1)
1. ✅ **Day 1-2**: Implement storage loading (让代码能真正执行交易)
2. ✅ **Day 3**: Write basic integration tests (验证正确性)
3. ✅ **Day 4-5**: Blockchain integration (接入真实区块链)

### Next Week (Priority 2)
4. ✅ **Day 6**: Performance benchmarking (测量实际加速比)
5. ✅ **Day 7**: Optimization tuning (优化性能)

### Week 3-4 (Priority 3)
6. Contract execution support (完整功能)
7. Error recovery (生产级可靠性)

### Week 4+ (Priority 4)
8. Monitoring, configuration, testing, documentation (生产就绪)

---

## Decision Point: What Should We Do Next?

我建议的**最优先**行动（选一个）：

### Option A: 实用主义路线 (推荐)
**立即做**: Implement storage loading (4-6 hours)
**为什么**: 不做这个，代码无法真正运行。这是从"能编译"到"能执行"的关键一步。

### Option B: 验证路线
**立即做**: Write integration test first (2-3 hours)
**为什么**: 先写测试，再实现存储加载，TDD方式确保正确性。

### Option C: 展示路线
**立即做**: Performance benchmark skeleton (2 hours)
**为什么**: 先搭建性能测试框架，为后续优化打基础，也可以展示并行执行的价值。

---

**我的推荐**: **Option A (存储加载)**

理由：
1. 当前代码只是"能编译"，但无法真正处理有余额的交易
2. 存储加载是后续所有工作的前提
3. 4-6小时就能完成，投入产出比高
4. 完成后可以用devnet测试真实交易

**你想选哪个？或者有其他想法？**
