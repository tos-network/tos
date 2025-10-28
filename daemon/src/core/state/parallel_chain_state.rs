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
    /// Current nonce
    nonce: u64,
    /// Balances per asset
    balances: HashMap<Hash, u64>,
    /// Multisig configuration
    multisig: Option<MultiSigPayload>,
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

    // Concurrent balance tracking (PublicKey -> Asset -> Balance)
    balances: DashMap<PublicKey, HashMap<Hash, u64>>,

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
            balances: DashMap::new(),
            contracts: DashMap::new(),
            stable_topoheight,
            topoheight,
            block_version,
            block,
            block_hash,
            is_mainnet,
            burned_supply: AtomicU64::new(0),
            gas_fee: AtomicU64::new(0),
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

        // Insert into cache
        self.accounts.insert(key.clone(), AccountState {
            nonce,
            balances: HashMap::new(), // Balances loaded on-demand
            multisig,
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

        // Insert balance into account's balance map
        if let Some(mut account_entry) = self.accounts.get_mut(account) {
            account_entry.balances.insert(asset.clone(), balance);
        }

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
        // TODO: Implement contract invocation logic
        // This will require:
        // 1. Load contract from storage
        // 2. Prepare deposits
        // 3. Execute contract in VM
        // 4. Apply state changes
        Ok(())
    }

    /// Legacy helper method - no longer used (replaced by adapter pattern)
    #[allow(dead_code)]
    async fn apply_deploy_contract(
        &self,
        _source: &PublicKey,
        _payload: &DeployContractPayload,
    ) -> Result<(), BlockchainError> {
        // TODO: Implement contract deployment logic
        Ok(())
    }

    /// Legacy helper method - no longer used (replaced by adapter pattern)
    #[allow(dead_code)]
    async fn apply_energy(
        &self,
        _source: &PublicKey,
        _payload: &EnergyPayload,
    ) -> Result<(), BlockchainError> {
        // TODO: Implement energy transaction logic
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

    /// Commit all changes to storage (single-threaded finalization)
    /// Takes a mutable storage reference to write changes
    pub async fn commit(&self, storage: &mut S) -> Result<(), BlockchainError> {
        use log::{debug, info};

        if log::log_enabled!(log::Level::Info) {
            info!("Committing parallel chain state changes to storage at topoheight {}", self.topoheight);
        }

        // Write all account nonces
        let mut nonce_count = 0;
        for entry in self.accounts.iter() {
            use tos_common::account::VersionedNonce;
            let versioned_nonce = VersionedNonce::new(entry.value().nonce, Some(self.topoheight));
            storage.set_last_nonce_to(entry.key(), self.topoheight, &versioned_nonce).await?;
            nonce_count += 1;
        }

        // Write all balances
        let mut balance_count = 0;
        for entry in self.balances.iter() {
            let account = entry.key();
            for (asset, balance) in entry.value().iter() {
                use tos_common::account::VersionedBalance;
                let versioned_balance = VersionedBalance::new(*balance, Some(self.topoheight));
                storage.set_last_balance_to(account, asset, self.topoheight, &versioned_balance).await?;
                balance_count += 1;
            }
        }

        // Write all contracts
        let mut contract_count = 0;
        for _entry in self.contracts.iter() {
            // TODO: Implement contract state persistence
            contract_count += 1;
        }

        if log::log_enabled!(log::Level::Debug) {
            debug!("Committed {} nonces, {} balances, {} contracts",
                   nonce_count, balance_count, contract_count);
        }

        Ok(())
    }

    /// Reward a miner for the block mined
    pub async fn reward_miner(&self, miner: &PublicKey, reward: u64) -> Result<(), BlockchainError> {
        use log::debug;

        if log::log_enabled!(log::Level::Debug) {
            debug!("Rewarding miner {} with {} TOS at topoheight {}",
                   miner.as_address(self.is_mainnet),
                   tos_common::utils::format_tos(reward),
                   self.topoheight);
        }

        self.balances.entry(miner.clone())
            .or_insert_with(HashMap::new)
            .entry(TOS_ASSET.clone())
            .and_modify(|b| *b = b.saturating_add(reward))
            .or_insert(reward);

        Ok(())
    }

    // Getter methods for merging parallel execution results

    /// Get all modified account nonces
    /// Returns iterator of (PublicKey, new_nonce)
    pub fn get_modified_nonces(&self) -> Vec<(PublicKey, u64)> {
        self.accounts.iter()
            .map(|entry| (entry.key().clone(), entry.value().nonce))
            .collect()
    }

    /// Get all modified balances
    /// Returns iterator of ((PublicKey, Asset), new_balance)
    pub fn get_modified_balances(&self) -> Vec<((PublicKey, Hash), u64)> {
        let mut result = Vec::new();

        // Collect from accounts cache
        for entry in self.accounts.iter() {
            let account = entry.key();
            for (asset, balance) in &entry.value().balances {
                result.push(((account.clone(), asset.clone()), *balance));
            }
        }

        // Collect from balances cache
        for entry in self.balances.iter() {
            let account = entry.key();
            for (asset, balance) in entry.value().iter() {
                result.push(((account.clone(), asset.clone()), *balance));
            }
        }

        result
    }

    /// Get multisig configurations that were modified
    /// SECURITY FIX #7: Return ALL accounts including None (deletions)
    /// Previously filtered out None, causing multisig deletions to be lost
    pub fn get_modified_multisigs(&self) -> Vec<(PublicKey, Option<MultiSigPayload>)> {
        self.accounts.iter()
            .map(|entry| (entry.key().clone(), entry.value().multisig.clone()))
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
