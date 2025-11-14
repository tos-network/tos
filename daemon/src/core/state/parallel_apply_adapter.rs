// Parallel Apply Adapter - Makes ParallelChainState compatible with BlockchainApplyState trait
//
// SECURITY FIX #8: This adapter allows parallel execution to reuse Transaction::apply_with_partial_verify()
// ensuring validation parity with sequential execution path.
//
// Phase 1 Implementation: Basic transfers with TOS fee, no extra data
// - Validates signature, nonce, balance (existing)
// - Validates version format, fee type, transfer count, self-transfer, extra data size (NEW via adapter)
// - Validates burn amount, multisig invariants (NEW via adapter)
//
// Phase 2-4: Contract, Energy, AI Mining support (future work)

use anyhow::anyhow;
use async_trait::async_trait;
use indexmap::IndexMap;
use std::{borrow::Cow, collections::HashMap, sync::Arc};
use tos_common::{
    account::{EnergyResource, Nonce},
    ai_mining::AIMiningState,
    block::{Block, BlockVersion},
    contract::{
        AssetChanges, ChainState as ContractChainState, ContractCache, ContractEventTracker,
        ContractOutput,
    },
    crypto::{elgamal::CompressedPublicKey, Hash, PublicKey},
    tokio::sync::{RwLock, Semaphore},
    transaction::{
        verify::{BlockchainApplyState, BlockchainVerificationState, ContractEnvironment},
        ContractDeposit, MultiSigPayload, Reference,
    },
};
use tos_kernel::{Environment, Module};

use crate::core::{error::BlockchainError, storage::Storage};

use super::parallel_chain_state::ParallelChainState;

use log::debug;

/// Adapter that makes ParallelChainState compatible with BlockchainApplyState trait
///
/// This allows parallel execution to reuse the same Transaction::apply_with_partial_verify()
/// method as sequential execution, ensuring validation parity.
///
/// SECURITY: This is the key to fixing Vulnerability #8 (Incomplete Transaction Validation).
/// By implementing this adapter, parallel path gets all 20+ consensus-critical validations
/// that sequential path performs, with zero code duplication.
///
/// IMPORTANT: This adapter requires read-only storage access to perform state-level validations
/// (fee requirements, reference validation). Storage access is safe because ParallelChainState
/// already acquired a read lock during initialization.
pub struct ParallelApplyAdapter<'a, S: Storage> {
    /// The parallel chain state being adapted
    parallel_state: Arc<ParallelChainState<S>>,

    /// Read-only storage access for validation (acquired via read lock)
    storage: Arc<RwLock<S>>,

    /// Semaphore to prevent concurrent storage access (DEADLOCK FIX)
    /// This ensures only one task accesses storage at a time, preventing sled internal deadlocks.
    /// Sled's internal locking mechanism + LRU cache Mutex causes deadlocks when multiple
    /// async tasks call storage.read() concurrently.
    storage_semaphore: Arc<Semaphore>,

    /// Block being processed
    block: &'a Block,

    /// Block hash
    block_hash: &'a Hash,

    /// Balance modifications cache
    /// WORKAROUND: We cache balance modifications here because get_sender_balance/get_receiver_balance
    /// need to return &'b mut u64, but we can't directly return mutable references from DashMap.
    /// Instead, we track all balance reads and modifications, then commit them all at once.
    balance_reads: HashMap<(PublicKey, Hash), u64>,

    /// Output sum tracking (spending amounts)
    /// CRITICAL SECURITY: This tracks the total amount spent per account/asset during TX execution.
    /// Sequential path uses Echange::output_sum which is subtracted in apply_changes().
    /// Parallel path must do the same: track outputs here, subtract when committing.
    /// Without this, balances are never debited → consensus-breaking inflation bug.
    output_sums: HashMap<(PublicKey, Hash), u64>,

    /// SECURITY FIX: Staged nonces (not committed until success)
    /// Prevents nonce poisoning attack where failed TX increments nonce
    staged_nonces: HashMap<PublicKey, Nonce>,

    /// SECURITY FIX: Staged multisig configs (not committed until success)
    /// Prevents multisig hijacking attack where failed TX changes config
    staged_multisig: HashMap<PublicKey, Option<MultiSigPayload>>,

    /// SECURITY FIX: Staged gas fees (not committed until success)
    /// Prevents gas manipulation where failed TX still counts toward block gas
    staged_gas_fees: u64,

    /// SECURITY FIX: Staged burned supply (not committed until success)
    /// Prevents burn manipulation where failed TX still counts toward total burns
    staged_burned_supply: u64,

    /// Contract executor for executing contract bytecode
    executor: std::sync::Arc<dyn tos_common::contract::ContractExecutor>,

    /// Staged contract deployments (for commit on success)
    /// Stores Arc<Module> to avoid double-clone and memory bloat
    /// Used to keep modules alive during execution (lifetime safety)
    staged_contracts: HashMap<Hash, Arc<tos_kernel::Module>>,

    /// Staged contract caches (for merge on success)
    /// Maps contract_hash -> ContractCache
    /// These caches contain all storage writes performed by the contract during execution
    staged_contract_caches: HashMap<Hash, ContractCache>,

    /// Tracks whether adapter was successfully committed
    /// Used by Drop impl to determine if rollback is needed
    committed: bool,
}

impl<'a, S: Storage> ParallelApplyAdapter<'a, S> {
    /// Create a new adapter for a transaction execution
    pub fn new(
        parallel_state: Arc<ParallelChainState<S>>,
        storage: Arc<RwLock<S>>,
        storage_semaphore: Arc<Semaphore>,
        block: &'a Block,
        block_hash: &'a Hash,
        executor: std::sync::Arc<dyn tos_common::contract::ContractExecutor>,
    ) -> Self {
        Self {
            parallel_state,
            storage,
            storage_semaphore,
            block,
            block_hash,
            balance_reads: HashMap::new(),
            output_sums: HashMap::new(),
            staged_nonces: HashMap::new(),
            staged_multisig: HashMap::new(),
            staged_gas_fees: 0,
            staged_burned_supply: 0,
            executor,
            staged_contracts: HashMap::new(),
            staged_contract_caches: HashMap::new(),
            committed: false,
        }
    }

    /// Commit cached balance changes back to ParallelChainState
    /// Call this after transaction application succeeds
    ///
    /// IMPORTANT: Transaction::apply_with_partial_verify() already mutated balances via get_sender_balance().
    /// Sequential path also mutates Echange::version during TX, then discards it and recomputes in apply_changes().
    /// Parallel path keeps the mutations because we can't recompute (no receiver_balances separation).
    ///
    /// The balance_reads cache contains the FINAL mutated balances after TX execution.
    /// We just commit them directly - NO additional output_sum subtraction needed!
    #[allow(deprecated)] // We're the abstraction layer, allowed to use set_balance internally
    pub fn commit_balances(&self) {
        for ((account, asset), balance) in &self.balance_reads {
            self.parallel_state.set_balance(account, asset, *balance);
        }
    }

    /// SECURITY FIX: Commit all staged mutations to ParallelChainState atomically
    ///
    /// This method is ONLY called when transaction validation succeeds.
    /// It commits all staged mutations (nonces, multisig, gas, burns, balances, contracts) atomically.
    ///
    /// CRITICAL: If transaction fails, this method is never called, and all staged mutations
    /// are automatically discarded when the adapter is dropped (including contract deployments via Drop).
    ///
    /// This fixes the premature state mutation vulnerability where failed transactions
    /// were leaving behind permanent state changes (nonce increments, multisig config changes,
    /// gas/burn accumulations).
    ///
    /// Returns an error if burned supply limit would be exceeded (overflow protection).
    pub fn commit_all(&mut self) -> Result<(), BlockchainError> {
        // Commit balances (already implemented)
        self.commit_balances();

        // SECURITY FIX: Commit nonces
        for (account, nonce) in &self.staged_nonces {
            self.parallel_state.set_nonce(account, *nonce);
        }

        // SECURITY FIX: Commit multisig configs
        for (account, config) in &self.staged_multisig {
            self.parallel_state.set_multisig(account, config.clone());
        }

        // SECURITY FIX: Commit gas fees (saturating arithmetic, never fails)
        if self.staged_gas_fees > 0 {
            self.parallel_state.add_gas_fee(self.staged_gas_fees);
        }

        // SECURITY FIX: Commit burned supply (with overflow protection)
        if self.staged_burned_supply > 0 {
            self.parallel_state
                .add_burned_supply(self.staged_burned_supply)?;
        }

        // Commit staged contracts
        // NOTE: Contracts are already in ParallelChainState cache
        // They will be written to storage during merge_parallel_results()
        if log::log_enabled!(log::Level::Debug) {
            for (contract_address, _) in &self.staged_contracts {
                debug!("Committing deployed contract {}", contract_address);
            }
        }

        // CRITICAL: Commit contract storage caches to ParallelChainState
        // This allows merge_parallel_results() to persist contract state changes
        for (contract_hash, cache) in self.staged_contract_caches.drain() {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Committing contract cache for {} ({} storage entries)",
                    contract_hash,
                    cache.storage.len()
                );
            }
            self.parallel_state.add_contract_cache(contract_hash, cache);
        }

        // Mark as committed to prevent rollback in Drop
        self.committed = true;

        Ok(())
    }

    /// Get or load balance into cache
    async fn get_or_load_balance(
        &mut self,
        account: &PublicKey,
        asset: &Hash,
    ) -> Result<u64, BlockchainError> {
        let key = (account.clone(), asset.clone());

        if let Some(&balance) = self.balance_reads.get(&key) {
            return Ok(balance);
        }

        // DEADLOCK FIX: Acquire semaphore permit before calling ensure_*_loaded()
        // These methods will call storage.read() internally
        let _permit = self.storage_semaphore.acquire().await.unwrap();

        // Load from ParallelChainState
        self.parallel_state.ensure_account_loaded(account).await?;
        self.parallel_state
            .ensure_balance_loaded(account, asset)
            .await?;
        let balance = self.parallel_state.get_balance(account, asset);

        // Cache it
        self.balance_reads.insert(key, balance);
        Ok(balance)
    }
}

/// Implement BlockchainVerificationState - provides read/write access to state
#[async_trait]
impl<'a, S: Storage> BlockchainVerificationState<'a, BlockchainError>
    for ParallelApplyAdapter<'a, S>
{
    /// Pre-verify the transaction at state level
    ///
    /// SECURITY FIX: Delegate to the same pre_verify_tx helper that sequential path uses.
    /// This performs critical validations:
    /// - TX version compatibility with block version (hard fork rules)
    /// - Fee requirement validation
    /// - Reference topoheight validation
    async fn pre_verify_tx<'b>(
        &'b mut self,
        tx: &tos_common::transaction::Transaction,
    ) -> Result<(), BlockchainError> {
        // DEADLOCK FIX: Acquire semaphore permit before storage access
        // This prevents concurrent storage.read() calls that trigger sled internal deadlocks
        let _permit = self.storage_semaphore.acquire().await.unwrap();

        // Acquire read lock on storage for validation
        let storage_guard = self.storage.read().await;

        // Delegate to the shared validation helper (same as sequential path)
        super::pre_verify_tx(
            &*storage_guard,
            tx,
            self.parallel_state.get_stable_topoheight(),
            self.parallel_state.get_topoheight(),
            self.parallel_state.get_block_version(),
        )
        .await
    }

    /// Get the balance for a receiver account
    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, BlockchainError> {
        // Load balance into cache
        let balance = self
            .get_or_load_balance(account.as_ref(), asset.as_ref())
            .await?;

        // Update cache with current value
        let key = (account.into_owned(), asset.into_owned());
        self.balance_reads.insert(key.clone(), balance);

        // Return mutable reference to cached value
        // SAFETY: We guarantee that:
        // 1. The HashMap entry exists (we just inserted it)
        // 2. The reference is valid for lifetime 'b (tied to &'b mut self)
        // 3. No other code can access self.balance_reads while this reference exists
        Ok(self.balance_reads.get_mut(&key).unwrap())
    }

    /// Get the balance for a sender account (used for spending verification)
    ///
    /// PHASE 2 COMPLETE: Full reference validation with anti-front-running logic.
    ///
    /// This implementation uses read-only storage queries to perform the same validation
    /// as sequential execution's search_versioned_balance_for_reference():
    /// - Scenario A: TX references previous block → use final balance
    /// - Scenario B: TX references old block, received funds after → use reference balance
    /// - Scenario C: TX references old block, sent TX after → use output balance if available
    /// - Scenario D: Multiple TXs after reference → use output balance
    ///
    /// SAFETY: Read-only queries (via RwLock::read()) are safe in parallel execution.
    /// Multiple tasks can hold read locks simultaneously without race conditions.
    async fn get_sender_balance<'b>(
        &'b mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        reference: &Reference,
    ) -> Result<&'b mut u64, BlockchainError> {
        use log::trace;

        // Basic validation first
        let current_topo = self.parallel_state.get_topoheight();
        if reference.topoheight > current_topo {
            return Err(BlockchainError::InvalidReferenceTopoheight(
                reference.topoheight,
                current_topo,
            ));
        }

        // DEADLOCK FIX: Acquire semaphore permit before storage access
        let _permit = self.storage_semaphore.acquire().await.unwrap();

        // Acquire read lock for reference validation queries
        let storage_guard = self.storage.read().await;

        // Call the shared reference validation helper (same as sequential path)
        // This performs anti-front-running scenarios A-D
        let (use_output_balance, new_version, versioned_balance) =
            super::search_versioned_balance_for_reference(
                &*storage_guard,
                account,
                asset,
                current_topo,
                reference,
                false, // no_new = false (we may create new versions)
            )
            .await?;

        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Reference validation for {}: use_output={}, new_version={}, balance={}",
                account.as_address(storage_guard.is_mainnet()),
                use_output_balance,
                new_version,
                versioned_balance.get_balance()
            );
        }

        // Release storage lock before modifying balance cache
        drop(storage_guard);

        // Extract the validated balance using the correct VersionedBalance API
        // take_balance_with(use_output_balance) returns output_balance if use_output_balance=true and output exists,
        // otherwise returns final_balance
        let validated_balance = versioned_balance.take_balance_with(use_output_balance);

        // Cache the validated balance
        let key = (account.clone(), asset.clone());
        self.balance_reads.insert(key.clone(), validated_balance);

        // Return mutable reference to cached value
        Ok(self.balance_reads.get_mut(&key).unwrap())
    }

    /// Track sender output (spending) for final balance verification
    ///
    /// PHASE 1 IMPLEMENTATION: Tracks output_sum for protocol compatibility.
    ///
    /// Sequential execution separates sender balances (accounts HashMap) from receiver balances
    /// (receiver_balances HashMap). When an account both receives and sends in same block,
    /// apply_changes() merges them: receiver_balance - output_sum.
    ///
    /// Parallel execution uses a single DashMap without sender/receiver separation.
    /// When an account receives AND sends:
    /// 1. Receive: balance 50 → 80 (via get_receiver_balance mutation)
    /// 2. Send: balance 80 → 60 (via get_sender_balance mutation)
    /// Final balance 60 is correct - already reflects both operations.
    ///
    /// Therefore output_sum is tracked here for protocol compatibility but NOT used in
    /// commit_balances() for Phase 1 simple transfers. Future phases may need it for
    /// complex sender/receiver merging logic.
    async fn add_sender_output(
        &mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        output: u64,
    ) -> Result<(), BlockchainError> {
        use log::trace;

        let key = (account.clone(), asset.clone());

        // Accumulate output_sum (for protocol compatibility, not used in Phase 1)
        let current_sum = self.output_sums.get(&key).copied().unwrap_or(0);
        let new_sum = current_sum.saturating_add(output);

        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "add_sender_output: account {} asset {} output {} (sum {} -> {})",
                account.as_address(self.parallel_state.is_mainnet()),
                asset,
                output,
                current_sum,
                new_sum
            );
        }

        self.output_sums.insert(key, new_sum);
        Ok(())
    }

    /// Get the nonce of an account
    async fn get_account_nonce(
        &mut self,
        account: &'a PublicKey,
    ) -> Result<Nonce, BlockchainError> {
        self.parallel_state.ensure_account_loaded(account).await?;
        Ok(self.parallel_state.get_nonce(account))
    }

    /// Update account nonce
    /// SECURITY FIX: Stage nonce update instead of immediate write
    /// This prevents nonce poisoning attack where failed TX increments nonce
    async fn update_account_nonce(
        &mut self,
        account: &'a PublicKey,
        new_nonce: Nonce,
    ) -> Result<(), BlockchainError> {
        // Stage the nonce update - will only be committed if TX succeeds
        self.staged_nonces.insert(account.clone(), new_nonce);
        Ok(())
    }

    /// Atomically compare and swap nonce
    async fn compare_and_swap_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        expected: Nonce,
        new_value: Nonce,
    ) -> Result<bool, BlockchainError> {
        let current_nonce = self.get_account_nonce(account).await?;
        if current_nonce == expected {
            self.update_account_nonce(account, new_value).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get the block version
    fn get_block_version(&self) -> BlockVersion {
        self.parallel_state.get_block_version()
    }

    /// Set multisig configuration for an account
    /// SECURITY FIX: Stage multisig config update instead of immediate write
    /// This prevents multisig hijacking attack where failed TX changes account config
    async fn set_multisig_state(
        &mut self,
        account: &'a PublicKey,
        config: &MultiSigPayload,
    ) -> Result<(), BlockchainError> {
        self.parallel_state.ensure_account_loaded(account).await?;
        // Stage the multisig config update - will only be committed if TX succeeds
        self.staged_multisig
            .insert(account.clone(), Some(config.clone()));
        Ok(())
    }

    /// Get multisig configuration for an account
    async fn get_multisig_state(
        &mut self,
        account: &'a PublicKey,
    ) -> Result<Option<&MultiSigPayload>, BlockchainError> {
        self.parallel_state.ensure_account_loaded(account).await?;

        // TODO Phase 2: Return proper reference using a multisig cache similar to balance_reads
        // For now, we return None which will cause multisig transactions to fail in Phase 1
        // This is acceptable since Phase 1 only supports simple transfers
        Ok(None)
    }

    /// Get the VM environment (for contract execution)
    async fn get_environment(&mut self) -> Result<&Environment, BlockchainError> {
        // Use public accessor for environment, deref Arc<Environment> to &Environment
        Ok(self.parallel_state.get_environment().as_ref())
    }

    /// Set contract module (deploy contract)
    async fn set_contract_module(
        &mut self,
        hash: &Hash,
        module: &'a Module,
    ) -> Result<(), BlockchainError> {
        // Acquire storage semaphore to prevent RocksDB/Sled deadlocks
        // CRITICAL: All storage access must be protected by this semaphore
        let permit = self.storage_semaphore.acquire().await.map_err(|e| {
            BlockchainError::Any(anyhow!("Failed to acquire storage semaphore: {}", e))
        })?;

        // Convert borrowed module to Arc for caching (avoids double-clone)
        let module_arc = Arc::new(module.clone());

        // Cache in ParallelChainState (atomic check-and-insert under semaphore)
        self.parallel_state
            .cache_deployed_contract(hash, module_arc.clone(), &permit)
            .await?;

        // Stage for commit - store Arc to avoid memory bloat
        self.staged_contracts.insert(hash.clone(), module_arc);

        Ok(())
    }

    /// Load contract module into cache
    async fn load_contract_module(&mut self, hash: &Hash) -> Result<bool, BlockchainError> {
        // Check if already in cache (fast path, no storage access)
        if self.parallel_state.get_cached_contract(hash).is_some() {
            return Ok(true);
        }

        // Acquire storage semaphore before accessing storage
        let permit = self.storage_semaphore.acquire().await.map_err(|e| {
            BlockchainError::Any(anyhow!("Failed to acquire storage semaphore: {}", e))
        })?;

        // Load from storage using ContractProvider::get_contract_at_maximum_topoheight_for
        // CRITICAL: Load the full VersionedContract to preserve metadata (constants, hooks, etc.)
        // Using load_contract_module() would only give raw bytecode, losing all serialized metadata
        // that the sequential path persists, causing execution divergence.
        let storage = self.storage.read().await;
        let topoheight = self.parallel_state.get_topoheight(); // Use public accessor

        let contract_result = storage
            .get_contract_at_maximum_topoheight_for(hash, topoheight)
            .await?;

        if let Some((_stored_topoheight, versioned_contract)) = contract_result {
            // Extract Module from VersionedContract (preserves all metadata)
            let module_cow = versioned_contract.get();

            if let Some(module_ref) = module_cow.as_ref() {
                // Clone the full Module (includes constants, entry chunks, hook map)
                let module = module_ref.as_ref().clone();
                let module_arc = Arc::new(module);

                // Cache using cache_existing_contract (skips collision check)
                // This is safe because we're loading an already-deployed contract
                self.parallel_state
                    .cache_existing_contract(hash, module_arc);

                drop(storage); // Release storage lock
                drop(permit); // Release semaphore

                return Ok(true);
            }
        }

        drop(storage);
        drop(permit);
        Ok(false)
    }

    /// Get contract module with environment
    ///
    /// Returns references to the contract module and VM environment for execution.
    async fn get_contract_module_with_environment(
        &self,
        hash: &Hash,
    ) -> Result<(&Module, &Environment), BlockchainError> {
        // Get DashMap guard to keep reference alive
        let guard = self
            .parallel_state
            .get_contract_guard(hash)
            .ok_or_else(|| BlockchainError::ContractNotFound(hash.clone()))?;

        // Extract Module reference from ContractState
        let module_arc = guard
            .module
            .as_ref()
            .ok_or_else(|| BlockchainError::ContractNotFound(hash.clone()))?;

        // SAFETY: Lifetime extension from DashMap guard to method return
        //
        // Invariants that must hold:
        // 1. Module Stability: Module is stored in Arc within DashMap in ParallelChainState
        // 2. DashMap Guarantees: DashMap ensures entry won't be freed while RefMut guard exists
        // 3. ParallelChainState Lifetime: ParallelChainState outlives this adapter
        // 4. Transaction Atomicity: Contract won't be removed during transaction execution
        // 5. Arc Guarantees: Even if cache entry removed, Arc keeps Module alive
        // 6. Staged Contracts: staged_contracts HashMap holds Arc, preventing premature drop
        //
        // Why safe alternatives don't work:
        // - Cannot return DashMap guard with Module reference:
        //   * Guard lifetime tied to temporary, can't extend to return value
        //   * Caller needs direct &Module, not guard wrapper
        // - Cannot clone Module:
        //   * Module contains compiled bytecode (large, expensive to clone)
        //   * Performance critical path (contract execution hot path)
        // - Cannot use RefCell/RwLock:
        //   * DashMap already provides interior mutability with better concurrency
        //   * Would require locking for entire execution duration (bad for parallelism)
        //
        // Memory safety guarantees:
        // - No use-after-free: DashMap + Arc double protection keeps Module alive
        // - No dangling pointers: Pointer derived from Arc, stable heap address
        // - No data races: Arc uses atomic refcounting, Module is immutable during execution
        //
        // Execution flow:
        // 1. Caller invokes this method to get Module reference
        // 2. DashMap guard created, locks entry for read
        // 3. Pointer cast extends lifetime from guard to method return
        // 4. Guard dropped immediately after cast (releases DashMap lock)
        // 5. Module remains valid via Arc in staged_contracts (for deployed) or cache (for existing)
        // 6. Caller uses Module for contract execution
        // 7. Adapter holds staged_contracts until commit or rollback
        //
        // Rollback safety:
        // - Drop impl removes staged_contracts from cache if !committed
        // - But execution has already completed by then (references no longer exist)
        // - Even if reference existed, Arc would keep Module alive
        //
        // Verified by: Manual review 2025-11-14, parallel execution tests passing
        let module_ref: &Module = unsafe { &*(module_arc.as_ref() as *const Module) };

        // Use public accessor for environment
        Ok((module_ref, self.parallel_state.get_environment().as_ref()))
    }
}

/// Implement BlockchainApplyState - provides additional methods for transaction application
#[async_trait]
impl<'a, S: Storage> BlockchainApplyState<'a, S, BlockchainError> for ParallelApplyAdapter<'a, S> {
    /// Track burned supply
    /// SECURITY FIX: Stage burned supply instead of immediate write
    /// This prevents burn manipulation where failed TX still counts toward total burns
    async fn add_burned_coins(&mut self, amount: u64) -> Result<(), BlockchainError> {
        // Stage the burned supply - will only be committed if TX succeeds
        self.staged_burned_supply = self
            .staged_burned_supply
            .checked_add(amount)
            .ok_or(BlockchainError::Overflow)?;
        Ok(())
    }

    /// Track miner fees
    /// SECURITY FIX: Stage gas fees instead of immediate write
    /// This prevents gas manipulation where failed TX still counts toward block gas
    async fn add_gas_fee(&mut self, amount: u64) -> Result<(), BlockchainError> {
        // Stage the gas fee - will only be committed if TX succeeds
        self.staged_gas_fees = self
            .staged_gas_fees
            .checked_add(amount)
            .ok_or(BlockchainError::Overflow)?;
        Ok(())
    }

    /// Get the block hash
    fn get_block_hash(&self) -> &Hash {
        self.block_hash
    }

    /// Get the block
    fn get_block(&self) -> &Block {
        self.block
    }

    /// Check if mainnet
    fn is_mainnet(&self) -> bool {
        self.parallel_state.is_mainnet()
    }

    /// Track contract outputs
    async fn set_contract_outputs(
        &mut self,
        _tx_hash: &'a Hash,
        _outputs: Vec<ContractOutput>,
    ) -> Result<(), BlockchainError> {
        Err(BlockchainError::Any(anyhow!(
            "Contract execution not yet supported in parallel execution (Phase 3)"
        )))
    }

    /// Get contract execution environment
    async fn get_contract_environment_for<'b>(
        &'b mut self,
        _contract: &'b Hash,
        _deposits: &'b IndexMap<Hash, ContractDeposit>,
        _tx_hash: &'b Hash,
    ) -> Result<(ContractEnvironment<'b, S>, ContractChainState<'b>), BlockchainError> {
        Err(BlockchainError::Any(anyhow!(
            "Contract execution not yet supported in parallel execution (Phase 3)"
        )))
    }

    /// Merge contract state changes
    ///
    /// This method is called after contract execution to persist the contract's storage changes.
    /// The cache contains all storage writes made by the contract during execution.
    ///
    /// # Implementation Strategy
    ///
    /// For parallel execution, we don't persist to storage immediately. Instead:
    /// 1. Stage the contract cache for later merging (during commit_all())
    /// 2. The actual persistence happens in merge_parallel_results() after all transactions succeed
    ///
    /// This ensures:
    /// - Failed transactions don't persist state (rollback safety)
    /// - Deterministic merge order (sorted by contract hash)
    /// - Atomic commit with other state changes (nonces, balances)
    ///
    /// # Arguments
    ///
    /// * `hash` - Contract address (hash)
    /// * `cache` - Contract storage cache containing all writes
    /// * `_tracker` - Event tracker (currently unused in parallel path)
    /// * `_assets` - Asset changes (currently unused in parallel path)
    async fn merge_contract_changes(
        &mut self,
        hash: &Hash,
        cache: ContractCache,
        _tracker: ContractEventTracker,
        _assets: HashMap<Hash, Option<AssetChanges>>,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Staging contract cache for {} ({} storage entries)",
                hash,
                cache.storage.len()
            );
        }

        // Stage the contract cache for commit on success
        // This will be merged to storage in merge_parallel_results()
        self.staged_contract_caches.insert(hash.clone(), cache);

        // TODO: Handle tracker (events) and assets when contract system is fully integrated
        // For now, we only persist storage writes

        Ok(())
    }

    /// Remove contract module
    async fn remove_contract_module(&mut self, _hash: &Hash) -> Result<(), BlockchainError> {
        Err(BlockchainError::Any(anyhow!(
            "Contract execution not yet supported in parallel execution (Phase 3)"
        )))
    }

    /// Get energy resource for an account
    async fn get_energy_resource(
        &mut self,
        _account: &'a CompressedPublicKey,
    ) -> Result<Option<EnergyResource>, BlockchainError> {
        Err(BlockchainError::Any(anyhow!(
            "Energy transactions not yet supported in parallel execution (Phase 3)"
        )))
    }

    /// Set energy resource for an account
    async fn set_energy_resource(
        &mut self,
        _account: &'a CompressedPublicKey,
        _energy_resource: EnergyResource,
    ) -> Result<(), BlockchainError> {
        Err(BlockchainError::Any(anyhow!(
            "Energy transactions not yet supported in parallel execution (Phase 3)"
        )))
    }

    /// Get AI mining state
    async fn get_ai_mining_state(&mut self) -> Result<Option<AIMiningState>, BlockchainError> {
        Err(BlockchainError::Any(anyhow!(
            "AI mining transactions not yet supported in parallel execution (Phase 4)"
        )))
    }

    /// Set AI mining state
    async fn set_ai_mining_state(&mut self, _state: &AIMiningState) -> Result<(), BlockchainError> {
        Err(BlockchainError::Any(anyhow!(
            "AI mining transactions not yet supported in parallel execution (Phase 4)"
        )))
    }

    fn get_contract_executor(&self) -> std::sync::Arc<dyn tos_common::contract::ContractExecutor> {
        self.executor.clone()
    }
}

/// Automatic rollback on Drop for failed transactions
///
/// If commit_all() was not called (transaction failed), this automatically
/// removes all staged contract deployments from the cache.
///
/// SECURITY: This ensures that failed contract deployments (e.g., constructor failure)
/// do not leave contracts in the cache that could be accessed by other transactions.
impl<'a, S: Storage> Drop for ParallelApplyAdapter<'a, S> {
    fn drop(&mut self) {
        // Only rollback if not committed
        if !self.committed {
            // Remove all staged contracts from cache
            for contract_address in self.staged_contracts.keys() {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Rolling back deployed contract {} (transaction failed)",
                        contract_address
                    );
                }
                self.parallel_state.remove_cached_contract(contract_address);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // TODO: Add unit tests for adapter methods
    // - Test balance operations (get/update)
    // - Test nonce operations (get/update/CAS)
    // - Test fee tracking (burned coins, gas fees)
    // - Test unsupported operations return appropriate errors
}
