// Parallel Chain State - Simplified Arc-based architecture for parallel transaction execution
// No lifetimes, DashMap for automatic concurrency control

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    marker::PhantomData,
};
use tokio::sync::{RwLock, Semaphore};
use dashmap::DashMap;
use tos_common::{
    account::EnergyResource,
    ai_mining::AIMiningState,
    block::{Block, BlockVersion, TopoHeight},
    config::TOS_ASSET,
    crypto::{Hash, PublicKey, Hashable},
    transaction::{
        Transaction,
        TransferPayload,
        BurnPayload,
        InvokeContractPayload,
        DeployContractPayload,
        EnergyPayload,
        MultiSigPayload,
    },
};
use tos_environment::Environment;
use crate::core::{
    error::BlockchainError,
    storage::Storage,
};

/// Account state cached in memory for parallel execution
#[derive(Debug, Clone)]
struct AccountState {
    /// Original nonce from storage (for modification tracking)
    original_nonce: u64,
    /// Current nonce
    nonce: u64,
    /// Balances per asset
    balances: HashMap<Hash, u64>,
    /// Original balances from storage (for modification tracking)
    original_balances: HashMap<Hash, u64>,
    /// Original multisig configuration from storage
    original_multisig: Option<MultiSigPayload>,
    /// Multisig configuration
    multisig: Option<MultiSigPayload>,
    /// Original energy resource from storage (for modification tracking)
    original_energy: Option<EnergyResource>,
    /// Energy resource (current state)
    energy: Option<EnergyResource>,
}

/// Contract state cached in memory
#[derive(Debug, Clone)]
struct ContractState {
    /// Contract module (bytecode)
    #[allow(dead_code)]
    module: Option<Arc<tos_vm::Module>>,
    /// Contract storage data
    #[allow(dead_code)]
    data: Vec<u8>,
}

/// Result of transaction execution
#[derive(Debug, Clone)]
pub struct TransactionResult {
    /// Transaction hash
    pub tx_hash: Hash,
    /// Whether execution succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Gas used
    pub gas_used: u64,
}

/// Parallel-execution-ready chain state with no lifetime constraints
/// Uses DashMap for automatic per-account locking and Arc for easy cloning
/// Generic over Storage type to avoid dyn compatibility issues
pub struct ParallelChainState<S: Storage> {
    // Storage reference with RwLock for interior mutability (Solana pattern)
    // Arc<RwLock<S>> enables sharing storage across parallel executors
    storage: Arc<RwLock<S>>,

    // PhantomData to ensure S is used
    _phantom: PhantomData<S>,

    // Environment for contract execution
    #[allow(dead_code)]
    environment: Arc<Environment>,

    // Concurrent account state (automatic locking via DashMap)
    accounts: DashMap<PublicKey, AccountState>,

    // Concurrent contract state
    contracts: DashMap<Hash, ContractState>,

    // Immutable block context
    #[allow(dead_code)]
    stable_topoheight: TopoHeight,
    topoheight: TopoHeight,
    #[allow(dead_code)]
    block_version: BlockVersion,
    block: Block,
    block_hash: Hash,

    // Cached network info (to avoid repeated lock acquisition)
    is_mainnet: bool,

    // Accumulated results (atomic for thread-safety)
    burned_supply: AtomicU64,
    gas_fee: AtomicU64,

    // AI Mining global state (RwLock for shared mutable access)
    // First Option: whether AI mining state is loaded
    // Second Option: the actual AI mining state (None if no miners registered)
    ai_mining_state: Arc<RwLock<Option<Option<AIMiningState>>>>,
    // Original AI mining state from storage (for modification tracking)
    original_ai_mining_state: Arc<RwLock<Option<Option<AIMiningState>>>>,

    // DEADLOCK FIX: Semaphore to serialize storage access during parallel execution
    // This prevents concurrent storage.read() calls that trigger sled internal deadlocks
    storage_semaphore: Arc<Semaphore>,
}

impl<S: Storage> ParallelChainState<S> {
    /// Create new state for parallel execution
    pub async fn new(
        storage: Arc<RwLock<S>>,
        environment: Arc<Environment>,
        stable_topoheight: TopoHeight,
        topoheight: TopoHeight,
        block_version: BlockVersion,
        block: Block,
        block_hash: Hash,
    ) -> Arc<Self> {
        // Cache network info to avoid repeated lock acquisition
        let is_mainnet = storage.read().await.is_mainnet();

        Arc::new(Self {
            storage,
            _phantom: PhantomData,
            environment,
            accounts: DashMap::new(),
            contracts: DashMap::new(),
            stable_topoheight,
            topoheight,
            block_version,
            block,
            block_hash,
            is_mainnet,
            burned_supply: AtomicU64::new(0),
            gas_fee: AtomicU64::new(0),
            // AI Mining state (None = not loaded yet)
            ai_mining_state: Arc::new(RwLock::new(None)),
            original_ai_mining_state: Arc::new(RwLock::new(None)),
            // DEADLOCK FIX: Permit only 1 concurrent storage access during parallel execution
            storage_semaphore: Arc::new(Semaphore::new(1)),
        })
    }

    /// Get total burned supply
    pub fn get_burned_supply(&self) -> u64 {
        self.burned_supply.load(Ordering::Relaxed)
    }

    /// Get total gas fees
    pub fn get_gas_fee(&self) -> u64 {
        self.gas_fee.load(Ordering::Relaxed)
    }

    /// Load account state from storage if not already cached
    pub async fn ensure_account_loaded(&self, key: &PublicKey) -> Result<(), BlockchainError> {
        use log::trace;

        // Check if already loaded
        if self.accounts.contains_key(key) {
            return Ok(());
        }

        if log::log_enabled!(log::Level::Trace) {
            trace!("Loading account state from storage for {}", key.as_address(self.is_mainnet));
        }

        // Acquire read lock and load nonce from storage
        // IMPORTANT: Semaphore must be acquired by CALLER before calling this method
        let storage = self.storage.read().await;
        let nonce = match storage.get_nonce_at_maximum_topoheight(key, self.topoheight).await? {
            Some((_, versioned_nonce)) => versioned_nonce.get_nonce(),
            None => 0, // New account
        };

        // Load multisig state from storage (reuse the same lock)
        let multisig = match storage.get_multisig_at_maximum_topoheight_for(key, self.topoheight).await? {
            Some((_, versioned_multisig)) => {
                // Extract the inner Option<MultiSigPayload> from VersionedMultiSig
                // VersionedMultiSig is Versioned<Option<Cow<'a, MultiSigPayload>>>
                versioned_multisig.get().as_ref().map(|cow| cow.clone().into_owned())
            }
            None => None,
        };
        // Drop lock before inserting into cache
        drop(storage);

        // Insert into cache with original values for modification tracking
        self.accounts.insert(key.clone(), AccountState {
            original_nonce: nonce,
            nonce,
            balances: HashMap::new(), // Balances loaded on-demand
            original_balances: HashMap::new(),
            original_multisig: multisig.clone(),
            multisig,
            original_energy: None, // Energy loaded on-demand
            energy: None,
        });

        Ok(())
    }

    /// Load balance from storage if not already cached
    pub async fn ensure_balance_loaded(
        &self,
        account: &PublicKey,
        asset: &Hash,
    ) -> Result<(), BlockchainError> {
        use log::trace;

        // First ensure account is loaded
        self.ensure_account_loaded(account).await?;

        // Check if balance already loaded
        if let Some(account_entry) = self.accounts.get(account) {
            if account_entry.balances.contains_key(asset) {
                return Ok(()); // Already loaded
            }
        }

        if log::log_enabled!(log::Level::Trace) {
            trace!("Loading balance from storage for {} asset {}",
                   account.as_address(self.is_mainnet), asset);
        }

        // Acquire read lock and load balance from storage
        // IMPORTANT: Semaphore must be acquired by CALLER before calling this method
        let storage = self.storage.read().await;
        let balance = match storage.get_balance_at_maximum_topoheight(account, asset, self.topoheight).await? {
            Some((_, versioned_balance)) => versioned_balance.get_balance(),
            None => 0, // No balance for this asset
        };
        // Drop lock before modifying cache
        drop(storage);

        // Insert balance into account's balance map and track original value
        if let Some(mut account_entry) = self.accounts.get_mut(account) {
            account_entry.balances.insert(asset.clone(), balance);
            account_entry.original_balances.insert(asset.clone(), balance);
        }

        Ok(())
    }

    /// Load energy resource from storage if not already cached
    pub async fn ensure_energy_loaded(
        &self,
        account: &PublicKey,
    ) -> Result<(), BlockchainError> {
        use log::trace;

        // First ensure account is loaded
        self.ensure_account_loaded(account).await?;

        // Check if energy already loaded
        if let Some(account_entry) = self.accounts.get(account) {
            if account_entry.energy.is_some() || account_entry.original_energy.is_some() {
                return Ok(()); // Already loaded
            }
        }

        if log::log_enabled!(log::Level::Trace) {
            trace!("Loading energy resource from storage for {}",
                   account.as_address(self.is_mainnet));
        }

        // Acquire read lock and load energy from storage
        let storage = self.storage.read().await;
        let energy = storage.get_energy_resource(account).await?;
        drop(storage);

        // Insert energy into account's cache and track original value
        if let Some(mut account_entry) = self.accounts.get_mut(account) {
            account_entry.energy = energy.clone();
            account_entry.original_energy = energy;
        }

        Ok(())
    }

    /// Load AI mining state from storage if not already cached
    pub async fn ensure_ai_mining_loaded(&self) -> Result<(), BlockchainError> {
        use log::trace;

        // Check if already loaded
        {
            let state = self.ai_mining_state.read().await;
            if state.is_some() {
                return Ok(()); // Already loaded
            }
        }

        if log::log_enabled!(log::Level::Trace) {
            trace!("Loading AI mining state from storage");
        }

        // Acquire read lock and load AI mining state from storage
        let storage = self.storage.read().await;
        let ai_state = storage.get_ai_mining_state().await?;
        drop(storage);

        // Cache the loaded state
        *self.ai_mining_state.write().await = Some(ai_state.clone());
        *self.original_ai_mining_state.write().await = Some(ai_state);

        Ok(())
    }

    /// Apply single transaction using adapter pattern for full validation
    ///
    /// SECURITY FIX #8: This method now uses ParallelApplyAdapter to ensure validation parity
    /// with sequential execution path. All 20+ consensus-critical validations are performed:
    /// - Signature verification (via Transaction::verify())
    /// - Nonce verification and CAS update
    /// - Fee deduction and balance checks
    /// - Transaction format validation (version, fee type, transfer count, etc.)
    /// - Self-transfer prevention
    /// - Extra data size limits
    /// - Burn amount constraints
    /// - Multisig threshold validation
    /// - And all other validations in Transaction::apply_with_partial_verify()
    ///
    /// Reference: PARALLEL_EXECUTION_ADAPTER_DESIGN.md
    pub async fn apply_transaction(
        self: Arc<Self>,
        tx: Arc<Transaction>,
    ) -> Result<TransactionResult, BlockchainError> {
        use log::debug;
        use crate::core::state::ParallelApplyAdapter;

        let tx_hash = tx.hash();

        if log::log_enabled!(log::Level::Debug) {
            debug!("Applying transaction {} at topoheight {} (adapter-based validation)",
                   tx_hash, self.topoheight);
        }

        // Create adapter for this transaction execution
        // Pass storage for validation (safe read-only access)
        // DEADLOCK FIX: Also pass storage_semaphore to serialize storage access
        let mut adapter = ParallelApplyAdapter::new(
            Arc::clone(&self),
            Arc::clone(&self.storage),
            Arc::clone(&self.storage_semaphore),
            &self.block,
            &self.block_hash,
        );

        // Call Transaction::apply_with_partial_verify() which performs:
        // 1. All format validations (pre_verify)
        // 2. Signature verification
        // 3. Nonce CAS update
        // 4. Balance operations
        // 5. Fee deduction
        // 6. Type-specific application logic
        match tx.apply_with_partial_verify(&tx_hash, &mut adapter).await {
            Ok(()) => {
                // SECURITY FIX: Commit ALL mutations atomically (balances, nonces, multisig, gas, burns)
                // This fixes the premature state mutation vulnerability where failed transactions
                // were leaving behind permanent state changes.
                adapter.commit_all();

                if log::log_enabled!(log::Level::Debug) {
                    debug!("Transaction {} applied successfully (adapter)", tx_hash);
                }

                Ok(TransactionResult {
                    tx_hash,
                    success: true,
                    error: None,
                    gas_used: tx.get_fee(),
                })
            }
            Err(e) => {
                // SECURITY FIX: All staged mutations automatically discarded on failure
                // (nonces, multisig, gas, burns stay unchanged when TX fails)
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Transaction {} validation failed (adapter): {:?}", tx_hash, e);
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

    /// Legacy helper method - no longer used (replaced by adapter pattern)
    #[allow(dead_code)]
    async fn apply_transfers(
        &self,
        source: &PublicKey,
        transfers: &[TransferPayload],
    ) -> Result<(), BlockchainError> {
        use log::{debug, trace};

        if log::log_enabled!(log::Level::Trace) {
            trace!("Applying {} transfers from {}", transfers.len(), source.as_address(self.is_mainnet));
        }

        for transfer in transfers {
            let asset = transfer.get_asset();
            let amount = transfer.get_amount();
            let destination = transfer.get_destination();

            // Load source balance from storage if not cached
            self.ensure_balance_loaded(source, asset).await?;

            // Check and deduct from source balance
            {
                let mut account = self.accounts.get_mut(source).unwrap();
                let src_balance = account.balances.get_mut(asset)
                    .ok_or_else(|| {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Source {} has no balance for asset {}", source.as_address(self.is_mainnet), asset);
                        }
                        BlockchainError::NoBalance(source.as_address(self.is_mainnet))
                    })?;

                if *src_balance < amount {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Insufficient funds: source {} has {} but needs {} for asset {}",
                               source.as_address(self.is_mainnet), src_balance, amount, asset);
                    }
                    return Err(BlockchainError::NoBalance(source.as_address(self.is_mainnet)));
                }

                *src_balance -= amount;
            }

            // SECURITY FIX #2: Load existing receiver balance before applying delta
            // This prevents balance corruption when receiver has existing balance not in cache
            // Reference: SECURITY_AUDIT_PARALLEL_EXECUTION.md - Vulnerability #2
            self.ensure_balance_loaded(destination, asset).await?;

            // Credit destination - MUST update self.accounts (not self.balances DashMap)
            // The loaded balance is in self.accounts, so we must increment it there
            {
                let mut dest_account = self.accounts.get_mut(destination).unwrap();
                let dest_balance = dest_account.balances.get_mut(asset).unwrap();
                *dest_balance = dest_balance.saturating_add(amount);

                if log::log_enabled!(log::Level::Trace) {
                    trace!("Credited {} of asset {} to {} (new balance: {})",
                           amount, asset, destination.as_address(self.is_mainnet), *dest_balance);
                }
            }

            if log::log_enabled!(log::Level::Trace) {
                trace!("Transferred {} of asset {} from {} to {}",
                       amount, asset, source.as_address(self.is_mainnet),
                       destination.as_address(self.is_mainnet));
            }
        }

        Ok(())
    }

    /// Legacy helper method - no longer used (replaced by adapter pattern)
    #[allow(dead_code)]
    async fn apply_burn(
        &self,
        source: &PublicKey,
        payload: &BurnPayload,
    ) -> Result<(), BlockchainError> {
        use log::{debug, trace};

        let asset = &payload.asset;
        let amount = payload.amount;

        if log::log_enabled!(log::Level::Trace) {
            trace!("Burning {} of asset {} from {}", amount, asset, source.as_address(self.is_mainnet));
        }

        // Load source balance from storage if not cached
        self.ensure_balance_loaded(source, asset).await?;

        // Check and deduct from source balance
        {
            let mut account = self.accounts.get_mut(source).unwrap();
            let src_balance = account.balances.get_mut(asset)
                .ok_or_else(|| BlockchainError::NoBalance(source.as_address(self.is_mainnet)))?;

            if *src_balance < amount {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Insufficient funds for burn: source {} has {} but needs {}",
                           source.as_address(self.is_mainnet), src_balance, amount);
                }
                return Err(BlockchainError::NoBalance(source.as_address(self.is_mainnet)));
            }

            *src_balance -= amount;
        }

        // Accumulate burned supply
        self.burned_supply.fetch_add(amount, Ordering::Relaxed);

        if log::log_enabled!(log::Level::Debug) {
            debug!("Burned {} of asset {} from {}", amount, asset, source.as_address(self.is_mainnet));
        }

        Ok(())
    }

    /// Legacy helper method - no longer used (replaced by adapter pattern)
    #[allow(dead_code)]
    async fn apply_invoke_contract(
        &self,
        _source: &PublicKey,
        _payload: &InvokeContractPayload,
    ) -> Result<(), BlockchainError> {
        // TODO [IN DEVELOPMENT]: Contract invocation support
        // Waiting for contract system development to complete
        // Once ready, integrate with parallel execution:
        // 1. Load contract from storage
        // 2. Prepare deposits
        // 3. Execute contract in VM
        // 4. Apply state changes to ParallelChainState
        Ok(())
    }

    /// Legacy helper method - no longer used (replaced by adapter pattern)
    #[allow(dead_code)]
    async fn apply_deploy_contract(
        &self,
        _source: &PublicKey,
        _payload: &DeployContractPayload,
    ) -> Result<(), BlockchainError> {
        // TODO [IN DEVELOPMENT]: Contract deployment support
        // Waiting for contract system development to complete
        // Once ready, integrate with parallel execution:
        // 1. Validate contract bytecode
        // 2. Store contract in ParallelChainState
        // 3. Initialize contract storage
        Ok(())
    }

    /// Legacy helper method - no longer used (replaced by adapter pattern)
    #[allow(dead_code)]
    async fn apply_energy(
        &self,
        _source: &PublicKey,
        _payload: &EnergyPayload,
    ) -> Result<(), BlockchainError> {
        // COMPLETED: Energy system support implemented
        // See get_energy_resource(), set_energy_resource(), ensure_energy_loaded()
        Ok(())
    }

    /// Legacy helper method - no longer used (replaced by adapter pattern)
    #[allow(dead_code)]
    async fn apply_multisig(
        &self,
        _source: &PublicKey,
        payload: &MultiSigPayload,
    ) -> Result<(), BlockchainError> {
        // Update multisig state
        let mut account = self.accounts.get_mut(_source).unwrap();
        if payload.is_delete() {
            account.multisig = None;
        } else {
            account.multisig = Some(payload.clone());
        }

        Ok(())
    }

    /// Legacy commit method - no longer used
    /// State is now merged via merge_parallel_results() in blockchain.rs
    #[allow(dead_code)]
    pub async fn commit(&self, storage: &mut S) -> Result<(), BlockchainError> {
        use log::{debug, info};

        if log::log_enabled!(log::Level::Info) {
            info!("Committing parallel chain state changes to storage at topoheight {}", self.topoheight);
        }

        // Write all account nonces (only modified ones)
        let mut nonce_count = 0;
        for entry in self.accounts.iter() {
            // Only write if nonce was actually modified
            if entry.value().nonce != entry.value().original_nonce {
                use tos_common::account::VersionedNonce;
                let versioned_nonce = VersionedNonce::new(entry.value().nonce, Some(self.topoheight));
                storage.set_last_nonce_to(entry.key(), self.topoheight, &versioned_nonce).await?;
                nonce_count += 1;
            }
        }

        // Write all balances (only modified ones)
        let mut balance_count = 0;
        for entry in self.accounts.iter() {
            let account = entry.key();
            for (asset, balance) in &entry.value().balances {
                // Only write if balance was actually modified
                let original_balance = entry.value().original_balances.get(asset).copied().unwrap_or(0);
                if *balance != original_balance {
                    use tos_common::account::VersionedBalance;
                    let versioned_balance = VersionedBalance::new(*balance, Some(self.topoheight));
                    storage.set_last_balance_to(account, asset, self.topoheight, &versioned_balance).await?;
                    balance_count += 1;
                }
            }
        }

        // Write all contracts
        let mut contract_count = 0;
        for _entry in self.contracts.iter() {
            // TODO [IN DEVELOPMENT]: Implement contract state persistence
            // Waiting for contract system development to complete
            contract_count += 1;
        }

        if log::log_enabled!(log::Level::Debug) {
            debug!("Committed {} nonces, {} balances, {} contracts",
                   nonce_count, balance_count, contract_count);
        }

        Ok(())
    }

    /// Reward a miner for the block mined
    ///
    /// CONSENSUS FIX: This method now loads existing balance from storage and accumulates
    /// the reward on top of it, matching serial execution behavior exactly.
    ///
    /// CRITICAL: Rewards MUST be written to `accounts` cache (not `balances` DashMap)
    /// because transaction execution reads from `accounts`. This ensures:
    /// 1. Miners can immediately spend rewards in the same block (consensus requirement)
    /// 2. Existing balances are preserved (no overwrite bug)
    /// 3. Parallel execution matches serial execution results exactly
    ///
    /// Bug History:
    /// - Old implementation wrote to `balances` DashMap → invisible to transactions
    /// - Old implementation didn't load existing balance → overwrote on merge
    /// - Both bugs caused consensus divergence and fund loss
    pub async fn reward_miner(&self, miner: &PublicKey, reward: u64) -> Result<(), BlockchainError> {
        use log::debug;

        if log::log_enabled!(log::Level::Debug) {
            debug!("Rewarding miner {} with {} TOS at topoheight {}",
                   miner.as_address(self.is_mainnet),
                   tos_common::utils::format_tos(reward),
                   self.topoheight);
        }

        // CONSENSUS FIX: Load existing balance from storage into accounts cache
        // This ensures we accumulate reward on top of existing balance (not overwrite)
        self.ensure_balance_loaded(miner, &TOS_ASSET).await?;

        // CONSENSUS FIX: Update balance in `accounts` cache (same cache used by transactions)
        // This ensures rewards are immediately spendable in the same block
        if let Some(mut entry) = self.accounts.get_mut(miner) {
            let balance = entry.balances.entry(TOS_ASSET.clone()).or_insert(0);
            let old_balance = *balance;
            *balance = balance.saturating_add(reward);

            if log::log_enabled!(log::Level::Debug) {
                debug!("Miner {} balance updated: {} → {} TOS (reward: {})",
                       miner.as_address(self.is_mainnet),
                       tos_common::utils::format_tos(old_balance),
                       tos_common::utils::format_tos(*balance),
                       tos_common::utils::format_tos(reward));
            }
        } else {
            // This should never happen because ensure_balance_loaded creates the entry
            // Use AccountNotFound error with miner's address
            return Err(BlockchainError::AccountNotFound(
                miner.as_address(self.is_mainnet)
            ));
        }

        Ok(())
    }

    // Getter methods for merging parallel execution results

    /// Get all modified account nonces
    /// Returns iterator of (PublicKey, new_nonce)
    /// FIX: Only returns nonces that were actually modified (not just loaded)
    pub fn get_modified_nonces(&self) -> Vec<(PublicKey, u64)> {
        self.accounts.iter()
            .filter(|entry| entry.value().nonce != entry.value().original_nonce)
            .map(|entry| (entry.key().clone(), entry.value().nonce))
            .collect()
    }

    /// Get all modified balances
    /// Returns iterator of ((PublicKey, Asset), new_balance)
    ///
    /// CONSENSUS FIX: Only collect from `accounts` cache and only return actually modified balances.
    /// All balance modifications (including miner rewards) are now tracked in `accounts`.
    /// FIX: Only returns balances that were actually modified (not just loaded).
    pub fn get_modified_balances(&self) -> Vec<((PublicKey, Hash), u64)> {
        let mut result = Vec::new();

        // CONSENSUS FIX: Only collect modified balances from accounts cache
        // All balance changes (transactions + rewards) are tracked here
        for entry in self.accounts.iter() {
            let account = entry.key();
            for (asset, balance) in &entry.value().balances {
                // Only include if balance was actually modified
                let original_balance = entry.value().original_balances.get(asset).copied().unwrap_or(0);
                if *balance != original_balance {
                    result.push(((account.clone(), asset.clone()), *balance));
                }
            }
        }

        result
    }

    /// Get multisig configurations that were modified
    /// SECURITY FIX #7: Return ALL accounts with modified multisig including None (deletions)
    /// FIX: Only returns multisig configurations that were actually modified (not just loaded)
    pub fn get_modified_multisigs(&self) -> Vec<(PublicKey, Option<MultiSigPayload>)> {
        self.accounts.iter()
            .filter(|entry| {
                // Check if multisig was actually modified
                // Compare using serialization since MultiSigPayload doesn't implement PartialEq
                let current_multisig = &entry.value().multisig;
                let original_multisig = &entry.value().original_multisig;

                match (current_multisig, original_multisig) {
                    (None, None) => false, // Both None, not modified
                    (Some(_), None) | (None, Some(_)) => true, // Changed from Some to None or vice versa
                    (Some(current), Some(original)) => {
                        // Compare by serializing both (since MultiSigPayload doesn't impl PartialEq)
                        use tos_common::serializer::Serializer;
                        let current_bytes = current.to_bytes();
                        let original_bytes = original.to_bytes();
                        current_bytes != original_bytes
                    }
                }
            })
            .map(|entry| (entry.key().clone(), entry.value().multisig.clone()))
            .collect()
    }

    /// Get energy resources that were modified
    /// Only returns energy resources that were actually modified (not just loaded)
    pub fn get_modified_energy_resources(&self) -> Vec<(PublicKey, Option<EnergyResource>)> {
        self.accounts.iter()
            .filter(|entry| {
                // Check if energy was actually modified
                let current_energy = &entry.value().energy;
                let original_energy = &entry.value().original_energy;

                match (current_energy, original_energy) {
                    (None, None) => false, // Both None, not modified
                    (Some(_), None) | (None, Some(_)) => true, // Changed from Some to None or vice versa
                    (Some(current), Some(original)) => {
                        // Compare by serializing both (since EnergyResource doesn't impl PartialEq)
                        use tos_common::serializer::Serializer;
                        let current_bytes = current.to_bytes();
                        let original_bytes = original.to_bytes();
                        current_bytes != original_bytes
                    }
                }
            })
            .map(|entry| (entry.key().clone(), entry.value().energy.clone()))
            .collect()
    }

    // Helper methods for ParallelApplyAdapter

    /// Get nonce for an account (must be loaded first)
    pub fn get_nonce(&self, account: &PublicKey) -> u64 {
        self.accounts.get(account)
            .map(|entry| entry.nonce)
            .unwrap_or(0)
    }

    /// Set nonce for an account (must be loaded first)
    pub fn set_nonce(&self, account: &PublicKey, nonce: u64) {
        if let Some(mut entry) = self.accounts.get_mut(account) {
            entry.nonce = nonce;
        }
    }

    /// Get balance for an account and asset (must be loaded first)
    /// Returns 0 if balance not loaded
    pub fn get_balance(&self, account: &PublicKey, asset: &Hash) -> u64 {
        self.accounts.get(account)
            .and_then(|entry| entry.balances.get(asset).copied())
            .unwrap_or(0)
    }

    /// Get mutable reference to balance (must be loaded first)
    /// SAFETY: This returns a mutable reference through DashMap's RefMut
    /// The reference is valid as long as the RefMut guard is held
    pub fn get_balance_mut(&self, account: &PublicKey, asset: &Hash) -> Result<u64, BlockchainError> {
        // This is a workaround for lifetime issues with DashMap
        // We return the value, not a reference, to avoid borrow checker issues
        Ok(self.get_balance(account, asset))
    }

    /// Update balance for an account and asset
    ///
    /// # DEPRECATED - Use adapter pattern instead
    ///
    /// # Safety
    /// This method bypasses proper balance loading and modification tracking.
    /// Only call this after `ensure_balance_loaded()` and understand that
    /// it may cause incorrect modification tracking.
    ///
    /// Prefer using `Transaction::apply_with_partial_verify()` with `ParallelApplyAdapter`
    /// which ensures proper validation and state tracking.
    #[deprecated(note = "Use ParallelApplyAdapter pattern instead")]
    pub fn set_balance(&self, account: &PublicKey, asset: &Hash, balance: u64) {
        if let Some(mut entry) = self.accounts.get_mut(account) {
            entry.balances.insert(asset.clone(), balance);
        }
    }

    /// Get multisig configuration for an account (must be loaded first)
    pub fn get_multisig(&self, account: &PublicKey) -> Option<MultiSigPayload> {
        self.accounts.get(account)
            .and_then(|entry| entry.multisig.clone())
    }

    /// Set multisig configuration for an account (must be loaded first)
    pub fn set_multisig(&self, account: &PublicKey, multisig: Option<MultiSigPayload>) {
        if let Some(mut entry) = self.accounts.get_mut(account) {
            entry.multisig = multisig;
        }
    }

    /// Get energy resource for an account (must be loaded first)
    pub fn get_energy_resource(&self, account: &PublicKey) -> Option<EnergyResource> {
        self.accounts.get(account)
            .and_then(|entry| entry.energy.clone())
    }

    /// Set energy resource for an account (must be loaded first)
    pub fn set_energy_resource(&self, account: &PublicKey, energy: Option<EnergyResource>) {
        if let Some(mut entry) = self.accounts.get_mut(account) {
            entry.energy = energy;
        }
    }

    /// Get AI mining state (must be loaded first)
    pub async fn get_ai_mining_state(&self) -> Option<AIMiningState> {
        self.ai_mining_state.read().await.as_ref().and_then(|s| s.clone())
    }

    /// Set AI mining state (must be loaded first)
    pub async fn set_ai_mining_state(&self, state: Option<AIMiningState>) {
        *self.ai_mining_state.write().await = Some(state);
    }

    /// Get modified AI mining state
    /// Returns Some(state) if modified, None if not loaded or not modified
    pub async fn get_modified_ai_mining_state(&self) -> Option<AIMiningState> {
        let current = self.ai_mining_state.read().await;
        let original = self.original_ai_mining_state.read().await;

        // Check if state was actually modified
        match (current.as_ref(), original.as_ref()) {
            (Some(curr), Some(orig)) => {
                // Both loaded, check if different
                if curr != orig {
                    curr.clone()
                } else {
                    None // Not modified
                }
            }
            (Some(curr), None) => {
                // Current loaded but no original means this is a new state (modification)
                curr.clone()
            }
            _ => None, // Not loaded or no current state
        }
    }

    /// Add to burned supply (atomic)
    pub fn add_burned_supply(&self, amount: u64) {
        self.burned_supply.fetch_add(amount, Ordering::Relaxed);
    }

    /// Add to gas fee (atomic)
    pub fn add_gas_fee(&self, amount: u64) {
        self.gas_fee.fetch_add(amount, Ordering::Relaxed);
    }

    /// Get topoheight
    pub fn get_topoheight(&self) -> TopoHeight {
        self.topoheight
    }

    /// Get stable topoheight (for validation)
    pub fn get_stable_topoheight(&self) -> TopoHeight {
        self.stable_topoheight
    }

    /// Get block version
    pub fn get_block_version(&self) -> BlockVersion {
        self.block_version
    }

    /// Check if mainnet
    pub fn is_mainnet(&self) -> bool {
        self.is_mainnet
    }

    /// Get storage reference (for adapter)
    pub fn get_storage(&self) -> &Arc<RwLock<S>> {
        &self.storage
    }

    /// Get environment reference (for adapter)
    pub fn get_environment(&self) -> &Arc<Environment> {
        &self.environment
    }
}

#[cfg(test)]
mod tests {
    // Note: Integration tests for ParallelChainState are in
    // daemon/tests/integration/parallel_execution_tests.rs
    // because they require real Storage implementation and Transaction objects
    // - Test commit to storage
}
