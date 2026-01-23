//! ChainClient: Direct blockchain access for deterministic testing.
//!
//! ChainClient provides a high-level API for interacting with a fully
//! functional blockchain instance without network overhead. It is the
//! TOS equivalent of Solana's BanksClient, offering:
//! - Direct state queries (balance, nonce, storage)
//! - Transaction submission with structured results
//! - Transaction simulation (dry-run)
//! - State override for testing edge cases
//! - Block warp for fast chain advancement
//! - Feature gate configuration

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tos_common::crypto::Hash;

use crate::orchestrator::{Clock, PausedClock};
use crate::tier1_component::{TestBlockchain, TestBlockchainBuilder, TestTransaction};

use super::block_warp::{BlockWarp, WarpError, BLOCK_TIME_MS, MAX_WARP_BLOCKS};
use super::chain_client_config::{AutoMineConfig, ChainClientConfig, GenesisAccount};
use super::confirmation::ConfirmationDepth;
use super::features::FeatureSet;
use super::tx_result::{
    CallDeposit, SimulationResult, StateChange, StateDiff, TransactionError, TxResult,
};

/// Transaction type for the multi-signer builder.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum TransactionType {
    /// Native TOS transfer
    Transfer { to: Hash, amount: u64 },
    /// UNO asset transfer
    UnoTransfer { to: Hash, asset: Hash, amount: u64 },
    /// Deploy a new contract
    DeployContract { bytecode: Vec<u8> },
    /// Call an existing contract
    CallContract {
        contract: Hash,
        entry_id: u16,
        data: Vec<u8>,
        deposits: Vec<CallDeposit>,
    },
    /// Freeze TOS for energy
    Freeze { amount: u64 },
    /// Unfreeze TOS
    Unfreeze { amount: u64 },
    /// Delegate frozen TOS
    Delegate { to: Hash, amount: u64 },
    /// Remove delegation
    Undelegate { from: Hash, amount: u64 },
}

/// ChainClient provides direct blockchain access for testing.
///
/// Unlike the Tier 2 TestDaemon (which goes through RPC), ChainClient
/// operates directly on the blockchain state, providing:
/// - Synchronous state access
/// - Deterministic time control
/// - Full transaction tracing
/// - State override capabilities
///
/// # Example
/// ```ignore
/// let client = ChainClient::start(ChainClientConfig {
///     genesis_accounts: vec![GenesisAccount::new(alice, 1_000_000)],
///     ..Default::default()
/// }).await?;
///
/// let result = client.process_transaction(tx).await?;
/// assert!(result.success);
/// ```
pub struct ChainClient {
    /// Underlying blockchain instance
    blockchain: TestBlockchain,
    /// Clock for time control
    clock: Arc<dyn Clock>,
    /// Feature configuration
    features: FeatureSet,
    /// Auto-mine configuration
    auto_mine: AutoMineConfig,
    /// Configuration reference
    config: ChainClientConfig,
    /// Transaction results log (hash -> result)
    tx_log: Arc<RwLock<HashMap<Hash, TxResult>>>,
    /// Current topoheight (cached for fast access)
    topoheight: u64,
    /// Track state diffs flag
    track_state_diffs: bool,
}

impl ChainClient {
    /// Create and start a new ChainClient with the given configuration.
    pub async fn start(config: ChainClientConfig) -> Result<Self, WarpError> {
        let clock: Arc<dyn Clock> = config
            .clock
            .clone()
            .unwrap_or_else(|| Arc::new(PausedClock::new()));

        let mut builder = TestBlockchainBuilder::new().with_clock(clock.clone());

        for account in &config.genesis_accounts {
            builder = builder.with_funded_account(account.address.clone(), account.balance);
        }

        let blockchain = builder.build().await.map_err(|e| {
            WarpError::BlockCreationFailed(format!("Failed to build blockchain: {}", e))
        })?;

        let track_state_diffs = config.track_state_diffs;
        let features = config.features.clone();
        let auto_mine = config.auto_mine.clone();

        Ok(Self {
            blockchain,
            clock,
            features,
            auto_mine,
            config,
            tx_log: Arc::new(RwLock::new(HashMap::new())),
            topoheight: 0,
            track_state_diffs,
        })
    }

    /// Start a ChainClient with minimal defaults: one pre-funded account and OnTransaction auto-mine.
    pub async fn start_default() -> Result<Self, WarpError> {
        let default_address = Hash::new([1u8; 32]);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(default_address, 10_000_000))
            .with_auto_mine(AutoMineConfig::OnTransaction);
        Self::start(config).await
    }

    // --- Transaction Operations ---

    /// Process a transaction and return the structured result.
    ///
    /// The transaction is validated, executed, and included in the next block.
    /// If auto-mine is set to `OnTransaction`, a block is mined immediately.
    pub async fn process_transaction(
        &mut self,
        tx: TestTransaction,
    ) -> Result<TxResult, WarpError> {
        let tx_hash = tx.hash.clone();
        let sender = tx.sender.clone();

        // Validate transaction
        if let Some(error) = self.validate_transaction(&tx).await {
            let result = TxResult {
                success: false,
                tx_hash: tx_hash.clone(),
                block_hash: None,
                topoheight: None,
                error: Some(error),
                gas_used: 0,
                events: vec![],
                log_messages: vec![],
                inner_calls: vec![],
                return_data: vec![],
                new_nonce: self.get_nonce(&sender).await.unwrap_or(0),
            };
            self.tx_log.write().await.insert(tx_hash, result.clone());
            return Ok(result);
        }

        // Execute transaction (submit to mempool)
        let submit_result = self.blockchain.submit_transaction(tx.clone()).await;

        let (success, error) = match &submit_result {
            Ok(_) => (true, None),
            Err(e) => (
                false,
                Some(TransactionError::MalformedTransaction {
                    reason: e.to_string(),
                }),
            ),
        };

        // Auto-mine if configured
        let (block_hash, topo) = if success {
            match &self.auto_mine {
                AutoMineConfig::OnTransaction => {
                    let block = self
                        .blockchain
                        .mine_block()
                        .await
                        .map_err(|e| WarpError::BlockCreationFailed(e.to_string()))?;
                    self.topoheight = self.topoheight.saturating_add(1);
                    self.clock.sleep(Duration::from_millis(BLOCK_TIME_MS)).await;
                    (Some(block.hash), Some(self.topoheight))
                }
                _ => (None, None),
            }
        } else {
            (None, None)
        };

        let new_nonce = self.get_nonce(&sender).await.unwrap_or(0);

        let result = TxResult {
            success,
            tx_hash: tx_hash.clone(),
            block_hash,
            topoheight: topo,
            error,
            gas_used: tx.fee, // simplified: fee = gas_used in test environment
            events: vec![],
            log_messages: vec![],
            inner_calls: vec![],
            return_data: vec![],
            new_nonce,
        };

        self.tx_log.write().await.insert(tx_hash, result.clone());
        Ok(result)
    }

    /// Process multiple transactions in a single block.
    ///
    /// All transactions are submitted to the mempool, then a single block is mined.
    pub async fn process_batch(
        &mut self,
        txs: Vec<TestTransaction>,
    ) -> Result<Vec<TxResult>, WarpError> {
        let mut results = Vec::with_capacity(txs.len());
        // Track pending nonces per sender within the batch so that
        // sequential transactions from the same sender validate correctly
        let mut pending_nonces: std::collections::HashMap<Hash, u64> =
            std::collections::HashMap::new();

        for tx in &txs {
            let tx_hash = tx.hash.clone();
            let sender = tx.sender.clone();

            // Validate with batch-aware nonce tracking
            if let Some(error) = self.validate_batch_transaction(tx, &pending_nonces).await {
                results.push(TxResult {
                    success: false,
                    tx_hash: tx_hash.clone(),
                    block_hash: None,
                    topoheight: None,
                    error: Some(error),
                    gas_used: 0,
                    events: vec![],
                    log_messages: vec![],
                    inner_calls: vec![],
                    return_data: vec![],
                    new_nonce: pending_nonces.get(&sender).copied().unwrap_or(0),
                });
                continue;
            }

            let submit_result = self.blockchain.submit_transaction(tx.clone()).await;
            let (success, error) = match &submit_result {
                Ok(_) => (true, None),
                Err(e) => (
                    false,
                    Some(TransactionError::MalformedTransaction {
                        reason: e.to_string(),
                    }),
                ),
            };

            if success {
                // Track the pending nonce for this sender
                pending_nonces.insert(sender.clone(), tx.nonce);
            }

            results.push(TxResult {
                success,
                tx_hash: tx_hash.clone(),
                block_hash: None,
                topoheight: None,
                error,
                gas_used: tx.fee,
                events: vec![],
                log_messages: vec![],
                inner_calls: vec![],
                return_data: vec![],
                new_nonce: if success {
                    tx.nonce
                } else {
                    self.get_nonce(&sender).await.unwrap_or(0)
                },
            });
        }

        // Mine a single block containing all successful transactions
        let block = self
            .blockchain
            .mine_block()
            .await
            .map_err(|e| WarpError::BlockCreationFailed(e.to_string()))?;
        self.topoheight = self.topoheight.saturating_add(1);
        self.clock.sleep(Duration::from_millis(BLOCK_TIME_MS)).await;

        // Update block info for successful transactions
        for result in &mut results {
            if result.success {
                result.block_hash = Some(block.hash.clone());
                result.topoheight = Some(self.topoheight);
            }
        }

        // Store all results
        for result in &results {
            self.tx_log
                .write()
                .await
                .insert(result.tx_hash.clone(), result.clone());
        }

        Ok(results)
    }

    /// Process a transaction and wait for the specified confirmation depth.
    ///
    /// Mines additional empty blocks after the transaction block to reach
    /// the desired confirmation depth.
    pub async fn process_transaction_with_depth(
        &mut self,
        tx: TestTransaction,
        depth: ConfirmationDepth,
    ) -> Result<TxResult, WarpError> {
        let result = self.process_transaction(tx).await?;

        // Mine additional blocks to reach desired depth
        let blocks_to_mine = match depth {
            ConfirmationDepth::Included => 0,
            ConfirmationDepth::Confirmed(n) => n,
            ConfirmationDepth::Stable => 10, // 10 blocks for stability
        };

        if blocks_to_mine > 0 {
            self.mine_blocks(blocks_to_mine).await?;
        }

        Ok(result)
    }

    /// Simulate a transaction without committing state changes.
    pub async fn simulate_transaction(&self, tx: &TestTransaction) -> SimulationResult {
        // Validate first
        if let Some(error) = self.validate_transaction(tx).await {
            return SimulationResult {
                success: false,
                error: Some(error),
                gas_used: 0,
                events: vec![],
                log_messages: vec![],
                inner_calls: vec![],
                return_data: vec![],
                state_diff: None,
            };
        }

        // In a full implementation, this would fork state and execute.
        // For now, we validate and estimate gas.
        let state_diff = if self.track_state_diffs {
            let mut changes = HashMap::new();
            let sender_balance = self.get_balance(&tx.sender).await.unwrap_or(0);
            let recipient_balance = self.get_balance(&tx.recipient).await.unwrap_or(0);

            changes.insert(
                tx.sender.clone(),
                vec![
                    StateChange::BalanceChange {
                        asset: Hash::zero(),
                        before: sender_balance,
                        after: sender_balance.saturating_sub(tx.amount.saturating_add(tx.fee)),
                    },
                    StateChange::NonceChange {
                        before: tx.nonce,
                        after: tx.nonce.saturating_add(1),
                    },
                ],
            );
            changes.insert(
                tx.recipient.clone(),
                vec![StateChange::BalanceChange {
                    asset: Hash::zero(),
                    before: recipient_balance,
                    after: recipient_balance.saturating_add(tx.amount),
                }],
            );
            Some(StateDiff { changes })
        } else {
            None
        };

        SimulationResult {
            success: true,
            error: None,
            gas_used: tx.fee,
            events: vec![],
            log_messages: vec![],
            inner_calls: vec![],
            return_data: vec![],
            state_diff,
        }
    }

    /// Simulate a batch of transactions.
    pub async fn simulate_batch(&self, txs: &[TestTransaction]) -> Vec<SimulationResult> {
        let mut results = Vec::with_capacity(txs.len());
        for tx in txs {
            results.push(self.simulate_transaction(tx).await);
        }
        results
    }

    // --- State Queries ---

    /// Get the native TOS balance of an account.
    pub async fn get_balance(&self, address: &Hash) -> Result<u64, TransactionError> {
        self.blockchain
            .get_balance(address)
            .await
            .map_err(|_| TransactionError::AccountNotFound {
                address: address.clone(),
            })
    }

    /// Get the nonce of an account.
    pub async fn get_nonce(&self, address: &Hash) -> Result<u64, TransactionError> {
        self.blockchain
            .get_nonce(address)
            .await
            .map_err(|_| TransactionError::AccountNotFound {
                address: address.clone(),
            })
    }

    /// Get contract storage value by key.
    pub async fn get_contract_storage(
        &self,
        contract: &Hash,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, TransactionError> {
        // TestBlockchain doesn't have contract storage yet,
        // return None for undeployed contracts
        let _ = (contract, key);
        Ok(None)
    }

    /// Get contract storage and deserialize with borsh.
    pub async fn get_contract_state_borsh<T: borsh::BorshDeserialize>(
        &self,
        contract: &Hash,
        key: &[u8],
    ) -> Result<Option<T>, TransactionError> {
        let data = self.get_contract_storage(contract, key).await?;
        match data {
            None => Ok(None),
            Some(bytes) => {
                let value = T::try_from_slice(&bytes).map_err(|e| {
                    TransactionError::MalformedTransaction {
                        reason: format!("borsh deserialization failed: {}", e),
                    }
                })?;
                Ok(Some(value))
            }
        }
    }

    /// Get the transaction result for a previously processed transaction.
    pub async fn get_tx_result(&self, tx_hash: &Hash) -> Option<TxResult> {
        self.tx_log.read().await.get(tx_hash).cloned()
    }

    /// Get balance at a specific confirmation depth.
    pub async fn get_balance_at_depth(
        &self,
        address: &Hash,
        _depth: ConfirmationDepth,
    ) -> Result<u64, TransactionError> {
        // In single-node ChainClient, all state is immediately stable
        self.get_balance(address).await
    }

    // --- State Override (Test-Only) ---

    /// Force-set the balance of an account (bypasses normal transaction flow).
    pub async fn force_set_balance(
        &mut self,
        address: &Hash,
        balance: u64,
    ) -> Result<(), WarpError> {
        self.blockchain
            .force_set_balance(address, balance)
            .await
            .map_err(|e| WarpError::StateTransition(e.to_string()))
    }

    /// Force-set the nonce of an account.
    pub async fn force_set_nonce(&mut self, address: &Hash, nonce: u64) -> Result<(), WarpError> {
        self.blockchain
            .force_set_nonce(address, nonce)
            .await
            .map_err(|e| WarpError::StateTransition(e.to_string()))
    }

    /// Force-set a contract storage entry.
    pub async fn force_set_contract_storage(
        &mut self,
        _contract: &Hash,
        _key: Vec<u8>,
        _value: Vec<u8>,
    ) -> Result<(), WarpError> {
        // Contract storage override - placeholder until contract VM is integrated
        Ok(())
    }

    // --- Contract Operations ---

    /// Deploy a contract and return its address hash.
    pub async fn deploy_contract(&mut self, bytecode: &[u8]) -> Result<Hash, TransactionError> {
        // Generate contract address from bytecode hash
        let code_hash = Hash::new({
            let mut hasher = [0u8; 32];
            for (i, byte) in bytecode.iter().enumerate() {
                hasher[i % 32] ^= byte;
            }
            hasher
        });
        // In real implementation, this would store bytecode and initialize the contract
        Ok(code_hash)
    }

    /// Call a deployed contract.
    pub async fn call_contract(
        &mut self,
        _contract: Hash,
        _entry_id: u16,
        _data: Vec<u8>,
    ) -> Result<TxResult, WarpError> {
        // Contract call - returns a structured result
        let tx_hash = Hash::new([0u8; 32]); // placeholder
        Ok(TxResult {
            success: true,
            tx_hash,
            block_hash: None,
            topoheight: Some(self.topoheight),
            error: None,
            gas_used: 0,
            events: vec![],
            log_messages: vec![],
            inner_calls: vec![],
            return_data: vec![],
            new_nonce: 0,
        })
    }

    // --- Transaction Builder ---

    /// Build a transaction from a TransactionType specification.
    pub fn build_transaction(
        &self,
        sender: Hash,
        tx_type: TransactionType,
        nonce: u64,
        fee: u64,
    ) -> TestTransaction {
        let (recipient, amount) = match &tx_type {
            TransactionType::Transfer { to, amount } => (to.clone(), *amount),
            TransactionType::UnoTransfer { to, amount, .. } => (to.clone(), *amount),
            TransactionType::Freeze { amount } => (sender.clone(), *amount),
            TransactionType::Unfreeze { amount } => (sender.clone(), *amount),
            TransactionType::Delegate { to, amount } => (to.clone(), *amount),
            TransactionType::Undelegate { from, amount } => (from.clone(), *amount),
            TransactionType::DeployContract { .. } => (Hash::zero(), 0),
            TransactionType::CallContract { contract, .. } => (contract.clone(), 0),
        };

        TestTransaction {
            hash: self.generate_tx_hash(&sender, nonce),
            sender,
            recipient,
            amount,
            fee,
            nonce,
        }
    }

    /// Build a multi-signer transaction (for operations requiring multiple signatures).
    pub fn build_transaction_multi_signer(
        &self,
        signers: &[Hash],
        tx_type: TransactionType,
        nonce: u64,
        fee: u64,
    ) -> TestTransaction {
        let primary_sender = signers.first().cloned().unwrap_or(Hash::zero());
        self.build_transaction(primary_sender, tx_type, nonce, fee)
    }

    // --- Block Operations ---

    /// Mine a single empty block (convenience method).
    pub async fn mine_empty_block(&mut self) -> Result<Hash, WarpError> {
        let block = self
            .blockchain
            .mine_block()
            .await
            .map_err(|e| WarpError::BlockCreationFailed(e.to_string()))?;
        self.topoheight = self.topoheight.saturating_add(1);
        self.clock.sleep(Duration::from_millis(BLOCK_TIME_MS)).await;
        Ok(block.hash)
    }

    /// Mine N empty blocks, advancing the chain.
    pub async fn mine_blocks(&mut self, count: u64) -> Result<Vec<Hash>, WarpError> {
        let mut hashes = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let hash = self.mine_empty_block().await?;
            hashes.push(hash);
        }
        Ok(hashes)
    }

    /// Submit a transaction to the mempool without mining a block.
    ///
    /// The transaction will be included in the next mined block.
    pub async fn submit_to_mempool(&mut self, tx: TestTransaction) -> Result<(), WarpError> {
        if let Some(error) = self.validate_transaction(&tx).await {
            return Err(WarpError::StateTransition(format!(
                "Transaction validation failed: {:?}",
                error
            )));
        }
        self.blockchain
            .submit_transaction(tx)
            .await
            .map(|_| ())
            .map_err(|e| WarpError::StateTransition(e.to_string()))
    }

    /// Mine a block containing all pending mempool transactions.
    pub async fn mine_mempool(&mut self) -> Result<Hash, WarpError> {
        let block = self
            .blockchain
            .mine_block()
            .await
            .map_err(|e| WarpError::BlockCreationFailed(e.to_string()))?;
        self.topoheight = self.topoheight.saturating_add(1);
        self.clock.sleep(Duration::from_millis(BLOCK_TIME_MS)).await;
        Ok(block.hash)
    }

    /// Get the current topoheight of the chain.
    pub fn topoheight(&self) -> u64 {
        self.topoheight
    }

    /// Get a mutable reference to the underlying blockchain.
    pub fn blockchain_mut(&mut self) -> &mut TestBlockchain {
        &mut self.blockchain
    }

    // --- Feature Queries ---

    /// Check if a feature is active at the current topoheight.
    pub fn is_feature_active(&self, feature_id: &str) -> bool {
        self.features.is_active(feature_id, self.topoheight)
    }

    /// Get the current feature set.
    pub fn features(&self) -> &FeatureSet {
        &self.features
    }

    // --- Accessors ---

    /// Get the underlying blockchain reference.
    pub fn blockchain(&self) -> &TestBlockchain {
        &self.blockchain
    }

    /// Get the clock.
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// Get the current configuration.
    pub fn config(&self) -> &ChainClientConfig {
        &self.config
    }

    // --- Private Helpers ---

    /// Validate a transaction before execution.
    async fn validate_transaction(&self, tx: &TestTransaction) -> Option<TransactionError> {
        // Check sender exists and has sufficient balance
        let balance = match self.blockchain.get_balance(&tx.sender).await {
            Ok(b) => b,
            Err(_) => {
                return Some(TransactionError::AccountNotFound {
                    address: tx.sender.clone(),
                })
            }
        };

        let total_cost = tx.amount.checked_add(tx.fee)?;
        if balance < total_cost {
            return Some(TransactionError::InsufficientBalance {
                have: balance,
                need: total_cost,
                asset: Hash::zero(),
            });
        }

        // Check nonce (blockchain expects stored_nonce + 1)
        let stored_nonce = self.blockchain.get_nonce(&tx.sender).await.unwrap_or(0);
        let expected_nonce = stored_nonce.saturating_add(1);
        if tx.nonce != expected_nonce {
            return Some(TransactionError::InvalidNonce {
                expected: expected_nonce,
                provided: tx.nonce,
            });
        }

        // Check amount > 0 for transfers
        if tx.amount == 0 && tx.recipient != tx.sender {
            return Some(TransactionError::MalformedTransaction {
                reason: "transfer amount must be > 0".to_string(),
            });
        }

        None
    }

    /// Validate a transaction within a batch context.
    ///
    /// Uses pending nonces to allow sequential transactions from the same sender
    /// within a single batch (before any block is mined).
    async fn validate_batch_transaction(
        &self,
        tx: &TestTransaction,
        pending_nonces: &std::collections::HashMap<Hash, u64>,
    ) -> Option<TransactionError> {
        // Check sender exists and has sufficient balance
        let balance = match self.blockchain.get_balance(&tx.sender).await {
            Ok(b) => b,
            Err(_) => {
                return Some(TransactionError::AccountNotFound {
                    address: tx.sender.clone(),
                })
            }
        };

        let total_cost = tx.amount.checked_add(tx.fee)?;
        if balance < total_cost {
            return Some(TransactionError::InsufficientBalance {
                have: balance,
                need: total_cost,
                asset: Hash::zero(),
            });
        }

        // Check nonce using pending nonces if available
        let base_nonce = if let Some(&pending) = pending_nonces.get(&tx.sender) {
            pending
        } else {
            self.blockchain.get_nonce(&tx.sender).await.unwrap_or(0)
        };
        let expected_nonce = base_nonce.saturating_add(1);
        if tx.nonce != expected_nonce {
            return Some(TransactionError::InvalidNonce {
                expected: expected_nonce,
                provided: tx.nonce,
            });
        }

        // Check amount > 0 for transfers
        if tx.amount == 0 && tx.recipient != tx.sender {
            return Some(TransactionError::MalformedTransaction {
                reason: "transfer amount must be > 0".to_string(),
            });
        }

        None
    }

    /// Generate a deterministic transaction hash from sender and nonce.
    fn generate_tx_hash(&self, sender: &Hash, nonce: u64) -> Hash {
        let mut bytes = [0u8; 32];
        let sender_bytes = sender.as_bytes();
        let nonce_bytes = nonce.to_le_bytes();
        bytes[..24].copy_from_slice(&sender_bytes[..24]);
        bytes[24..32].copy_from_slice(&nonce_bytes);
        Hash::new(bytes)
    }
}

#[async_trait]
impl BlockWarp for ChainClient {
    async fn warp_blocks(&mut self, n: u64) -> Result<u64, WarpError> {
        if n > MAX_WARP_BLOCKS {
            return Err(WarpError::ExceedsMaxWarp {
                requested: n,
                max: MAX_WARP_BLOCKS,
            });
        }

        for _ in 0..n {
            self.clock.sleep(Duration::from_millis(BLOCK_TIME_MS)).await;
            self.blockchain
                .mine_block()
                .await
                .map_err(|e| WarpError::BlockCreationFailed(e.to_string()))?;
            self.topoheight = self.topoheight.saturating_add(1);
        }

        Ok(self.topoheight)
    }

    async fn warp_to_topoheight(&mut self, target: u64) -> Result<(), WarpError> {
        let current = self.current_topoheight();
        if target < current {
            return Err(WarpError::TargetBehindCurrent { target, current });
        }
        let blocks_needed = target.saturating_sub(current);
        self.warp_blocks(blocks_needed).await?;
        Ok(())
    }

    async fn create_block_with_txs(
        &mut self,
        txs: Vec<TestTransaction>,
    ) -> Result<Hash, WarpError> {
        // Submit all transactions to mempool
        for tx in &txs {
            self.blockchain
                .submit_transaction(tx.clone())
                .await
                .map_err(|e| WarpError::BlockCreationFailed(e.to_string()))?;
        }

        // Mine block containing them
        self.clock.sleep(Duration::from_millis(BLOCK_TIME_MS)).await;
        let block = self
            .blockchain
            .mine_block()
            .await
            .map_err(|e| WarpError::BlockCreationFailed(e.to_string()))?;
        self.topoheight = self.topoheight.saturating_add(1);

        Ok(block.hash)
    }

    fn current_topoheight(&self) -> u64 {
        self.topoheight
    }
}

#[cfg(test)]
mod tests {
    use super::super::chain_client_config::GenesisAccount;
    use super::*;

    fn sample_hash(byte: u8) -> Hash {
        Hash::new([byte; 32])
    }

    #[tokio::test]
    async fn test_chain_client_start() {
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000));

        let client = ChainClient::start(config).await.unwrap();
        assert_eq!(client.current_topoheight(), 0);

        let balance = client.get_balance(&sample_hash(1)).await.unwrap();
        assert_eq!(balance, 1_000_000);
    }

    #[tokio::test]
    async fn test_warp_blocks() {
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000));

        let mut client = ChainClient::start(config).await.unwrap();
        assert_eq!(client.current_topoheight(), 0);

        let new_topo = client.warp_blocks(10).await.unwrap();
        assert_eq!(new_topo, 10);
        assert_eq!(client.current_topoheight(), 10);
    }

    #[tokio::test]
    async fn test_warp_to_topoheight() {
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000));

        let mut client = ChainClient::start(config).await.unwrap();
        client.warp_to_topoheight(50).await.unwrap();
        assert_eq!(client.current_topoheight(), 50);

        // Cannot warp backwards
        let err = client.warp_to_topoheight(30).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn test_process_transaction() {
        let alice = sample_hash(1);
        let bob = sample_hash(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_account(GenesisAccount::new(bob.clone(), 0))
            .with_auto_mine(AutoMineConfig::OnTransaction);

        let mut client = ChainClient::start(config).await.unwrap();

        let tx = TestTransaction {
            hash: sample_hash(99),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 5000,
            fee: 10,
            nonce: 1, // First valid nonce is stored_nonce + 1 = 0 + 1 = 1
        };

        let result = client.process_transaction(tx).await.unwrap();
        assert!(result.success);
        assert_eq!(result.gas_used, 10);

        // Verify balances changed
        let alice_balance = client.get_balance(&alice).await.unwrap();
        let bob_balance = client.get_balance(&bob).await.unwrap();
        assert_eq!(alice_balance, 1_000_000 - 5000 - 10);
        assert_eq!(bob_balance, 5000);
    }

    #[tokio::test]
    async fn test_insufficient_balance() {
        let alice = sample_hash(1);
        let bob = sample_hash(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 100))
            .with_account(GenesisAccount::new(bob.clone(), 0));

        let mut client = ChainClient::start(config).await.unwrap();

        let tx = TestTransaction {
            hash: sample_hash(99),
            sender: alice,
            recipient: bob,
            amount: 200,
            fee: 10,
            nonce: 0,
        };

        let result = client.process_transaction(tx).await.unwrap();
        assert!(!result.success);
        assert!(matches!(
            result.error,
            Some(TransactionError::InsufficientBalance { .. })
        ));
    }

    #[tokio::test]
    async fn test_invalid_nonce() {
        let alice = sample_hash(1);
        let bob = sample_hash(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_account(GenesisAccount::new(bob.clone(), 0));

        let mut client = ChainClient::start(config).await.unwrap();

        let tx = TestTransaction {
            hash: sample_hash(99),
            sender: alice,
            recipient: bob,
            amount: 100,
            fee: 10,
            nonce: 5, // wrong nonce
        };

        let result = client.process_transaction(tx).await.unwrap();
        assert!(!result.success);
        assert!(matches!(
            result.error,
            Some(TransactionError::InvalidNonce { .. })
        ));
    }

    #[tokio::test]
    async fn test_simulate_transaction() {
        let alice = sample_hash(1);
        let bob = sample_hash(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_account(GenesisAccount::new(bob.clone(), 500))
            .with_state_diff_tracking();

        let client = ChainClient::start(config).await.unwrap();

        let tx = TestTransaction {
            hash: sample_hash(99),
            sender: alice.clone(),
            recipient: bob,
            amount: 1000,
            fee: 10,
            nonce: 1, // First valid nonce is stored_nonce + 1
        };

        let sim = client.simulate_transaction(&tx).await;
        assert!(sim.is_success());
        assert!(sim.state_diff.is_some());

        let diff = sim.state_diff.unwrap();
        assert_eq!(diff.affected_accounts(), 2);

        // Original state unchanged (simulation only)
        let alice_balance = client.get_balance(&alice).await.unwrap();
        assert_eq!(alice_balance, 1_000_000);
    }

    #[tokio::test]
    async fn test_force_set_balance() {
        let alice = sample_hash(1);
        let config =
            ChainClientConfig::default().with_account(GenesisAccount::new(alice.clone(), 1000));

        let mut client = ChainClient::start(config).await.unwrap();

        client.force_set_balance(&alice, 999_999).await.unwrap();
        let balance = client.get_balance(&alice).await.unwrap();
        assert_eq!(balance, 999_999);
    }

    #[tokio::test]
    async fn test_feature_gate() {
        let config = ChainClientConfig::default()
            .with_features(FeatureSet::empty().activate_at("nft_v2", 100));

        let mut client = ChainClient::start(config).await.unwrap();

        assert!(!client.is_feature_active("nft_v2"));

        client.warp_to_topoheight(100).await.unwrap();
        assert!(client.is_feature_active("nft_v2"));
    }

    #[tokio::test]
    async fn test_build_transaction() {
        let alice = sample_hash(1);
        let bob = sample_hash(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000));

        let client = ChainClient::start(config).await.unwrap();

        let tx = client.build_transaction(
            alice.clone(),
            TransactionType::Transfer {
                to: bob.clone(),
                amount: 5000,
            },
            0,
            10,
        );

        assert_eq!(tx.sender, alice);
        assert_eq!(tx.recipient, bob);
        assert_eq!(tx.amount, 5000);
        assert_eq!(tx.fee, 10);
        assert_eq!(tx.nonce, 0);
    }

    #[tokio::test]
    async fn test_create_block_with_txs() {
        let alice = sample_hash(1);
        let bob = sample_hash(2);
        let charlie = sample_hash(3);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_account(GenesisAccount::new(bob.clone(), 0))
            .with_account(GenesisAccount::new(charlie.clone(), 0));

        let mut client = ChainClient::start(config).await.unwrap();

        let txs = vec![
            TestTransaction {
                hash: sample_hash(90),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 10,
                nonce: 1, // First valid nonce
            },
            TestTransaction {
                hash: sample_hash(91),
                sender: alice,
                recipient: charlie,
                amount: 2000,
                fee: 10,
                nonce: 2, // Second tx from same sender
            },
        ];

        let block_hash = client.create_block_with_txs(txs).await.unwrap();
        assert_ne!(block_hash, Hash::zero());
        assert_eq!(client.current_topoheight(), 1);

        let bob_balance = client.get_balance(&bob).await.unwrap();
        assert_eq!(bob_balance, 1000);
    }

    #[tokio::test]
    async fn test_max_warp_limit() {
        let config = ChainClientConfig::default();
        let mut client = ChainClient::start(config).await.unwrap();

        let err = client.warp_blocks(MAX_WARP_BLOCKS + 1).await;
        assert!(matches!(err, Err(WarpError::ExceedsMaxWarp { .. })));
    }

    #[tokio::test]
    async fn test_start_default() {
        let client = ChainClient::start_default().await.unwrap();
        assert_eq!(client.topoheight(), 0);

        // Default address is [1u8; 32] with 10_000_000 balance
        let default_address = Hash::new([1u8; 32]);
        let balance = client.get_balance(&default_address).await.unwrap();
        assert_eq!(balance, 10_000_000);
    }

    #[tokio::test]
    async fn test_mine_blocks() {
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000));

        let mut client = ChainClient::start(config).await.unwrap();
        assert_eq!(client.topoheight(), 0);

        let hashes = client.mine_blocks(5).await.unwrap();
        assert_eq!(hashes.len(), 5);
        assert_eq!(client.topoheight(), 5);

        // All hashes should be unique
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                assert_ne!(hashes[i], hashes[j]);
            }
        }
    }

    #[tokio::test]
    async fn test_mine_blocks_zero() {
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000));

        let mut client = ChainClient::start(config).await.unwrap();
        let hashes = client.mine_blocks(0).await.unwrap();
        assert!(hashes.is_empty());
        assert_eq!(client.topoheight(), 0);
    }

    #[tokio::test]
    async fn test_process_batch() {
        let alice = sample_hash(1);
        let bob = sample_hash(2);
        let charlie = sample_hash(3);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_account(GenesisAccount::new(bob.clone(), 0))
            .with_account(GenesisAccount::new(charlie.clone(), 0));

        let mut client = ChainClient::start(config).await.unwrap();

        let txs = vec![
            TestTransaction {
                hash: sample_hash(90),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 1000,
                fee: 10,
                nonce: 1,
            },
            TestTransaction {
                hash: sample_hash(91),
                sender: alice.clone(),
                recipient: charlie.clone(),
                amount: 2000,
                fee: 10,
                nonce: 2,
            },
        ];

        let results = client.process_batch(txs).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].success);
        assert!(results[1].success);

        // Both should have the same block hash
        assert_eq!(results[0].block_hash, results[1].block_hash);
        assert!(results[0].block_hash.is_some());

        // Verify balances
        let bob_balance = client.get_balance(&bob).await.unwrap();
        assert_eq!(bob_balance, 1000);
        let charlie_balance = client.get_balance(&charlie).await.unwrap();
        assert_eq!(charlie_balance, 2000);
    }

    #[tokio::test]
    async fn test_process_batch_with_invalid_tx() {
        let alice = sample_hash(1);
        let bob = sample_hash(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 100))
            .with_account(GenesisAccount::new(bob.clone(), 0));

        let mut client = ChainClient::start(config).await.unwrap();

        let txs = vec![
            // This one should fail: amount + fee > balance
            TestTransaction {
                hash: sample_hash(90),
                sender: alice.clone(),
                recipient: bob.clone(),
                amount: 200,
                fee: 10,
                nonce: 1,
            },
        ];

        let results = client.process_batch(txs).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert!(results[0].block_hash.is_none());
    }

    #[tokio::test]
    async fn test_submit_to_mempool() {
        let alice = sample_hash(1);
        let bob = sample_hash(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000))
            .with_account(GenesisAccount::new(bob.clone(), 0));

        let mut client = ChainClient::start(config).await.unwrap();

        let tx = TestTransaction {
            hash: sample_hash(99),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 5000,
            fee: 10,
            nonce: 1,
        };

        // Submit without mining
        client.submit_to_mempool(tx).await.unwrap();

        // Balance should not have changed yet (no block mined)
        let bob_balance = client.get_balance(&bob).await.unwrap();
        assert_eq!(bob_balance, 0);

        // Now mine the mempool
        let hash = client.mine_mempool().await.unwrap();
        assert_ne!(hash, Hash::zero());

        // Balance should now reflect the transfer
        let bob_balance = client.get_balance(&bob).await.unwrap();
        assert_eq!(bob_balance, 5000);
    }

    #[tokio::test]
    async fn test_submit_to_mempool_invalid_tx() {
        let alice = sample_hash(1);
        let bob = sample_hash(2);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 100))
            .with_account(GenesisAccount::new(bob.clone(), 0));

        let mut client = ChainClient::start(config).await.unwrap();

        let tx = TestTransaction {
            hash: sample_hash(99),
            sender: alice,
            recipient: bob,
            amount: 200,
            fee: 10,
            nonce: 1,
        };

        // Should fail validation
        let result = client.submit_to_mempool(tx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mine_mempool() {
        let alice = sample_hash(1);
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(alice.clone(), 1_000_000));

        let mut client = ChainClient::start(config).await.unwrap();
        assert_eq!(client.topoheight(), 0);

        // Mining an empty mempool should still produce a block
        let hash = client.mine_mempool().await.unwrap();
        assert_ne!(hash, Hash::zero());
        assert_eq!(client.topoheight(), 1);
    }
}
