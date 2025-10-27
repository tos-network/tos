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
use tokio::sync::RwLock;
use dashmap::DashMap;
use tos_common::{
    block::{BlockVersion, TopoHeight},
    config::TOS_ASSET,
    crypto::{Hash, PublicKey, Hashable},
    transaction::{
        Transaction,
        TransactionType,
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

    // Cached network info (to avoid repeated lock acquisition)
    is_mainnet: bool,

    // Accumulated results (atomic for thread-safety)
    burned_supply: AtomicU64,
    gas_fee: AtomicU64,
}

impl<S: Storage> ParallelChainState<S> {
    /// Create new state for parallel execution
    pub async fn new(
        storage: Arc<RwLock<S>>,
        environment: Arc<Environment>,
        stable_topoheight: TopoHeight,
        topoheight: TopoHeight,
        block_version: BlockVersion,
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
            is_mainnet,
            burned_supply: AtomicU64::new(0),
            gas_fee: AtomicU64::new(0),
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
    async fn ensure_account_loaded(&self, key: &PublicKey) -> Result<(), BlockchainError> {
        use log::trace;

        // Check if already loaded
        if self.accounts.contains_key(key) {
            return Ok(());
        }

        if log::log_enabled!(log::Level::Trace) {
            trace!("Loading account state from storage for {}", key.as_address(self.is_mainnet));
        }

        // Acquire read lock and load nonce from storage
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
    async fn ensure_balance_loaded(
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

    /// Apply single transaction (thread-safe via DashMap)
    pub async fn apply_transaction(
        &self,
        tx: &Transaction,
    ) -> Result<TransactionResult, BlockchainError> {
        use log::debug;

        let source = tx.get_source();
        let tx_hash = tx.hash();

        if log::log_enabled!(log::Level::Debug) {
            debug!("Applying transaction {} from {} at topoheight {}",
                   tx_hash, source.as_address(self.is_mainnet), self.topoheight);
        }

        // Load account state from storage if not cached
        self.ensure_account_loaded(source).await?;

        // Verify nonce
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

        // Apply transaction based on type
        let result = match tx.get_data() {
            TransactionType::Transfers(transfers) => {
                self.apply_transfers(source, transfers).await
            }
            TransactionType::Burn(payload) => {
                self.apply_burn(source, payload).await
            }
            TransactionType::InvokeContract(payload) => {
                self.apply_invoke_contract(source, payload).await
            }
            TransactionType::DeployContract(payload) => {
                self.apply_deploy_contract(source, payload).await
            }
            TransactionType::Energy(payload) => {
                self.apply_energy(source, payload).await
            }
            TransactionType::MultiSig(payload) => {
                self.apply_multisig(source, payload).await
            }
            TransactionType::AIMining(_) => {
                // AI Mining transactions are handled separately
                Ok(())
            }
        };

        match result {
            Ok(_) => {
                // Increment nonce
                self.accounts.get_mut(source).unwrap().nonce += 1;

                // Accumulate fees
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

    /// Apply transfer transactions
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

            // Credit destination (DashMap auto-locks different key, no deadlock)
            self.balances.entry(destination.clone())
                .or_insert_with(HashMap::new)
                .entry(asset.clone())
                .and_modify(|b| *b = b.saturating_add(amount))
                .or_insert(amount);

            if log::log_enabled!(log::Level::Trace) {
                trace!("Transferred {} of asset {} from {} to {}",
                       amount, asset, source.as_address(self.is_mainnet),
                       destination.as_address(self.is_mainnet));
            }
        }

        Ok(())
    }

    /// Apply burn transaction
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

    /// Apply contract invocation
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

    /// Apply contract deployment
    async fn apply_deploy_contract(
        &self,
        _source: &PublicKey,
        _payload: &DeployContractPayload,
    ) -> Result<(), BlockchainError> {
        // TODO: Implement contract deployment logic
        Ok(())
    }

    /// Apply energy transaction
    async fn apply_energy(
        &self,
        _source: &PublicKey,
        _payload: &EnergyPayload,
    ) -> Result<(), BlockchainError> {
        // TODO: Implement energy transaction logic
        Ok(())
    }

    /// Apply multisig transaction
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
    pub fn get_modified_multisigs(&self) -> Vec<(PublicKey, Option<MultiSigPayload>)> {
        self.accounts.iter()
            .filter_map(|entry| {
                if entry.value().multisig.is_some() {
                    Some((entry.key().clone(), entry.value().multisig.clone()))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    // Note: Integration tests for ParallelChainState are in
    // daemon/tests/integration/parallel_execution_tests.rs
    // because they require real Storage implementation and Transaction objects
    // - Test commit to storage
}
