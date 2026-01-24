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

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tos_common::block::{compute_vrf_binding_message, compute_vrf_input, BlockVrfData};
use tos_common::contract::{
    ScheduledExecution, ScheduledExecutionStatus, MAX_SCHEDULED_EXECUTIONS_PER_BLOCK,
    MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK, MAX_SCHEDULING_HORIZON,
};
use tos_common::crypto::{Hash, KeyPair, Signature};
use tos_daemon::vrf::{VrfKeyManager, VrfOutput, VrfProof, VrfPublicKey};

use crate::orchestrator::{Clock, PausedClock};
use crate::tier1_component::{TestBlockchain, TestBlockchainBuilder, TestTransaction};

use super::block_warp::{BlockWarp, WarpError, BLOCK_TIME_MS, MAX_WARP_BLOCKS};
use super::chain_client_config::{AutoMineConfig, ChainClientConfig, GenesisAccount};
use super::confirmation::ConfirmationDepth;
use super::features::FeatureSet;
use super::tx_result::{
    CallDeposit, ContractCallResult, GasBreakdown, InnerCall, SimulationResult, StateChange,
    StateDiff, TransactionError, TxResult,
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

/// Error type for ChainClient operations.
///
/// Provides structured error variants for all ChainClient API methods,
/// enabling precise error handling in test assertions.
#[derive(Debug)]
#[allow(missing_docs)]
pub enum ChainClientError {
    /// Transaction was rejected by the mempool or validation layer
    TransactionRejected(String),
    /// Transaction signature or structure verification failed
    VerificationFailed(String),
    /// Referenced account does not exist
    AccountNotFound(String),
    /// Referenced contract does not exist
    ContractNotFound(String),
    /// Block not found at the specified topoheight
    BlockNotFound(u64),
    /// Warp target is behind current topoheight
    InvalidWarpTarget { target: u64, current: u64 },
    /// Insufficient balance for the requested operation
    InsufficientBalance { have: u64, need: u64 },
    /// Storage layer error
    Storage(String),
    /// Block mining error
    Mining(String),
    /// VRF validation failed
    VrfValidationFailed(String),
}

impl std::fmt::Display for ChainClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TransactionRejected(msg) => write!(f, "transaction rejected: {}", msg),
            Self::VerificationFailed(msg) => write!(f, "verification failed: {}", msg),
            Self::AccountNotFound(msg) => write!(f, "account not found: {}", msg),
            Self::ContractNotFound(msg) => write!(f, "contract not found: {}", msg),
            Self::BlockNotFound(topo) => write!(f, "block not found at topoheight {}", topo),
            Self::InvalidWarpTarget { target, current } => {
                write!(f, "warp target {} not ahead of current {}", target, current)
            }
            Self::InsufficientBalance { have, need } => {
                write!(f, "insufficient balance: have {}, need {}", have, need)
            }
            Self::Storage(msg) => write!(f, "storage error: {}", msg),
            Self::Mining(msg) => write!(f, "mining error: {}", msg),
            Self::VrfValidationFailed(msg) => write!(f, "VRF validation failed: {}", msg),
        }
    }
}

impl std::error::Error for ChainClientError {}

/// A lightweight block representation for ChainClient queries.
#[derive(Debug, Clone)]
pub struct BlockInfo {
    /// Block hash
    pub hash: Hash,
    /// Block topoheight
    pub topoheight: u64,
    /// Transaction hashes included in this block
    pub tx_hashes: Vec<Hash>,
    /// Timestamp (mock clock time when mined)
    pub timestamp: u64,
    /// VRF output data for this block (None if VRF not configured)
    pub vrf_data: Option<BlockVrfData>,
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
    /// Pending nonces for mempool-aware sequential nonce validation.
    /// Maps sender address to their latest pending (unconfirmed) nonce.
    pending_nonces: HashMap<Hash, u64>,
    /// VRF key manager (None if VRF not configured)
    vrf_key_manager: Option<VrfKeyManager>,
    /// Miner keypair for VRF signing (generated at start)
    miner_keypair: KeyPair,
    /// VRF data per block: topoheight -> BlockVrfData
    block_vrf_data: HashMap<u64, BlockVrfData>,
    /// Scheduled execution queue: target_topoheight -> executions
    scheduled_queue: BTreeMap<u64, Vec<ScheduledExecution>>,
    /// Execution results: hash -> (status, execution_topo)
    scheduled_results: HashMap<Hash, (ScheduledExecutionStatus, u64)>,
    /// Miner address for reward tracking
    miner_address: Option<Hash>,
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
        let miner_address = config.miner_address.clone();

        // Initialize VRF key manager if configured
        let vrf_key_manager = if let Some(ref hex) = config.vrf.secret_key_hex {
            Some(VrfKeyManager::from_hex(hex).map_err(|e| {
                WarpError::BlockCreationFailed(format!("Failed to load VRF key: {}", e))
            })?)
        } else {
            None
        };

        let miner_keypair = KeyPair::new();

        Ok(Self {
            blockchain,
            clock,
            features,
            auto_mine,
            config,
            tx_log: Arc::new(RwLock::new(HashMap::new())),
            topoheight: 0,
            track_state_diffs,
            pending_nonces: HashMap::new(),
            vrf_key_manager,
            miner_keypair,
            block_vrf_data: HashMap::new(),
            scheduled_queue: BTreeMap::new(),
            scheduled_results: HashMap::new(),
            miner_address,
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
                gas_refunded: 0,
                exit_code: None,
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
            gas_refunded: 0,
            exit_code: None,
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
                    gas_refunded: 0,
                    exit_code: None,
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
                gas_refunded: 0,
                exit_code: None,
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

    /// Call a deployed contract entry point.
    ///
    /// Submits a contract call transaction, mines a block, and returns
    /// the structured result with gas breakdown.
    pub async fn call_contract(
        &mut self,
        contract: &Hash,
        entry_id: u16,
        params: Vec<u8>,
        deposits: Vec<CallDeposit>,
        max_gas: u64,
    ) -> Result<ContractCallResult, WarpError> {
        let _ = (&deposits, max_gas); // reserved for future contract VM integration
        let tx_hash = self.generate_tx_hash(contract, self.topoheight);
        let gas_used = max_gas.min(1000); // simplified gas estimation

        let tx_result = TxResult {
            success: true,
            tx_hash,
            block_hash: None,
            topoheight: Some(self.topoheight),
            error: None,
            gas_used,
            gas_refunded: 0,
            exit_code: Some(0),
            events: vec![],
            log_messages: vec![],
            inner_calls: vec![InnerCall {
                caller: Hash::zero(),
                callee: contract.clone(),
                entry_id,
                data: params.clone(),
                deposits: deposits.clone(),
                gas_used,
                success: true,
                depth: 0,
                return_data: vec![],
                events: vec![],
            }],
            return_data: vec![],
            new_nonce: 0,
        };

        let gas_breakdown = GasBreakdown {
            total_used: gas_used,
            burned: gas_used.saturating_mul(self.config.fee_config.burn_percent) / 100,
            miner_fee: gas_used
                .saturating_sub(gas_used.saturating_mul(self.config.fee_config.burn_percent) / 100),
            refunded: 0,
        };

        Ok(ContractCallResult {
            tx_result,
            decoded_return: None,
            gas_breakdown,
        })
    }

    /// Simulate a contract call without committing state changes.
    ///
    /// Useful for gas estimation and checking if a call would succeed.
    pub async fn simulate_contract_call(
        &self,
        contract: &Hash,
        entry_id: u16,
        params: Vec<u8>,
        max_gas: u64,
    ) -> Result<ContractCallResult, WarpError> {
        let _ = max_gas; // reserved for future VM integration
        let tx_hash = self.generate_tx_hash(contract, self.topoheight);
        let gas_used = max_gas.min(1000); // simplified gas estimation

        let tx_result = TxResult {
            success: true,
            tx_hash,
            block_hash: None,
            topoheight: None,
            error: None,
            gas_used,
            gas_refunded: 0,
            exit_code: Some(0),
            events: vec![],
            log_messages: vec![],
            inner_calls: vec![InnerCall {
                caller: Hash::zero(),
                callee: contract.clone(),
                entry_id,
                data: params,
                deposits: vec![],
                gas_used,
                success: true,
                depth: 0,
                return_data: vec![],
                events: vec![],
            }],
            return_data: vec![],
            new_nonce: 0,
        };

        let gas_breakdown = GasBreakdown {
            total_used: gas_used,
            burned: 0,
            miner_fee: 0,
            refunded: 0,
        };

        Ok(ContractCallResult {
            tx_result,
            decoded_return: None,
            gas_breakdown,
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

        // Produce VRF data if configured
        self.produce_vrf_for_block(&block.hash);

        // Process scheduled executions at this topoheight
        self.process_scheduled_at_topoheight(self.topoheight);

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
    /// Supports sequential nonces from the same sender (e.g., nonce 1, 2, 3)
    /// without requiring a mine between each submission.
    pub async fn submit_to_mempool(&mut self, tx: TestTransaction) -> Result<(), WarpError> {
        if let Some(error) = self
            .validate_batch_transaction(&tx, &self.pending_nonces.clone())
            .await
        {
            return Err(WarpError::StateTransition(format!(
                "Transaction validation failed: {:?}",
                error
            )));
        }
        let sender = tx.sender.clone();
        let nonce = tx.nonce;
        self.blockchain
            .submit_transaction(tx)
            .await
            .map_err(|e| WarpError::StateTransition(e.to_string()))?;
        self.pending_nonces.insert(sender, nonce);
        Ok(())
    }

    /// Mine a block containing all pending mempool transactions.
    ///
    /// Clears pending nonce tracking after mining, since all pending
    /// transactions are now confirmed in the blockchain state.
    pub async fn mine_mempool(&mut self) -> Result<Hash, WarpError> {
        let block = self
            .blockchain
            .mine_block()
            .await
            .map_err(|e| WarpError::BlockCreationFailed(e.to_string()))?;
        self.topoheight = self.topoheight.saturating_add(1);
        self.pending_nonces.clear();
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

    // --- Additional State Queries ---

    /// Check if an account exists in the blockchain state.
    ///
    /// Returns true only if the account has been explicitly created
    /// (via genesis funding or receiving a transfer).
    pub async fn account_exists(&self, address: &Hash) -> Result<bool, ChainClientError> {
        Ok(self.blockchain.account_exists(address).await)
    }

    /// Get native TOS balance (convenience alias for get_balance with native asset).
    pub async fn get_tos_balance(&self, address: &Hash) -> Result<u64, ChainClientError> {
        self.blockchain
            .get_balance(address)
            .await
            .map_err(|_| ChainClientError::AccountNotFound(format!("{}", address)))
    }

    /// Get the DAG tips (blocks with no children).
    pub async fn get_tips(&self) -> Result<Vec<Hash>, ChainClientError> {
        self.blockchain
            .get_tips()
            .await
            .map_err(|e| ChainClientError::Storage(e.to_string()))
    }

    /// Get the stable (finalized) topoheight.
    ///
    /// In the single-node ChainClient, all blocks are immediately stable.
    pub fn get_stable_topoheight(&self) -> u64 {
        self.topoheight
    }

    /// Get block information at a specific topoheight.
    pub async fn get_block_at_topoheight(&self, topo: u64) -> Result<BlockInfo, ChainClientError> {
        if topo > self.topoheight {
            return Err(ChainClientError::BlockNotFound(topo));
        }
        let block = self
            .blockchain
            .get_block_at_height(topo)
            .await
            .map_err(|e| ChainClientError::Storage(e.to_string()))?
            .ok_or(ChainClientError::BlockNotFound(topo))?;
        let vrf_data = self.block_vrf_data.get(&topo).cloned();
        Ok(BlockInfo {
            hash: block.hash,
            topoheight: block.topoheight,
            tx_hashes: block
                .transactions
                .iter()
                .map(|tx| tx.hash.clone())
                .collect(),
            timestamp: topo.saturating_mul(self.config.block_time_ms),
            vrf_data,
        })
    }

    /// Get block information by hash.
    pub async fn get_block(&self, hash: &Hash) -> Result<BlockInfo, ChainClientError> {
        let block = self
            .blockchain
            .get_block_by_hash(hash)
            .await
            .map_err(|e| ChainClientError::Storage(e.to_string()))?
            .ok_or_else(|| ChainClientError::Storage(format!("block {} not found", hash)))?;
        let vrf_data = self.block_vrf_data.get(&block.topoheight).cloned();
        Ok(BlockInfo {
            hash: block.hash,
            topoheight: block.topoheight,
            tx_hashes: block
                .transactions
                .iter()
                .map(|tx| tx.hash.clone())
                .collect(),
            timestamp: block.topoheight.saturating_mul(self.config.block_time_ms),
            vrf_data,
        })
    }

    /// Get all contract storage entries.
    ///
    /// Returns an empty vec until the contract VM is integrated.
    pub async fn get_all_contract_storage(
        &self,
        _contract: &Hash,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, ChainClientError> {
        // Contract storage enumeration not yet implemented
        Ok(vec![])
    }

    /// Get a previously processed transaction result by hash.
    pub async fn get_transaction(&self, tx_hash: &Hash) -> Result<TxResult, ChainClientError> {
        self.tx_log
            .read()
            .await
            .get(tx_hash)
            .cloned()
            .ok_or_else(|| ChainClientError::Storage(format!("transaction {} not found", tx_hash)))
    }

    // --- Builder Utilities ---

    /// Build a simple transfer transaction.
    ///
    /// Automatically fetches the current nonce for the sender.
    pub async fn build_transfer(
        &self,
        from: &Hash,
        to: &Hash,
        amount: u64,
        fee: u64,
    ) -> Result<TestTransaction, ChainClientError> {
        let nonce = self
            .blockchain
            .get_nonce(from)
            .await
            .map_err(|_| ChainClientError::AccountNotFound(format!("{}", from)))?;
        let next_nonce = nonce.saturating_add(1);
        Ok(TestTransaction {
            hash: self.generate_tx_hash(from, next_nonce),
            sender: from.clone(),
            recipient: to.clone(),
            amount,
            fee,
            nonce: next_nonce,
        })
    }

    /// Get the most recent block hash (useful for transaction construction).
    pub async fn get_recent_block_hash(&self) -> Result<Hash, ChainClientError> {
        let tips = self
            .blockchain
            .get_tips()
            .await
            .map_err(|e| ChainClientError::Storage(e.to_string()))?;
        tips.into_iter()
            .next()
            .ok_or_else(|| ChainClientError::Storage("no blocks exist".to_string()))
    }

    /// Get the default payer address (first genesis account or default).
    pub fn payer(&self) -> Hash {
        self.config
            .genesis_accounts
            .first()
            .map(|a| a.address.clone())
            .unwrap_or_else(|| Hash::new([1u8; 32]))
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

    // --- VRF Queries ---

    /// Get VRF data for a block at the given topoheight.
    pub fn get_block_vrf_data(&self, topo: u64) -> Option<&BlockVrfData> {
        self.block_vrf_data.get(&topo)
    }

    /// Validate VRF data for a block.
    /// Returns Ok(()) if valid, Err if VRF is configured but data is missing/invalid.
    pub fn validate_block_vrf(&self, block: &BlockInfo) -> Result<(), ChainClientError> {
        // If VRF not configured, any block is valid
        if self.vrf_key_manager.is_none() {
            return Ok(());
        }

        // Check feature gate
        if !self.features.is_active("vrf_block_data", block.topoheight) {
            return Ok(());
        }

        // VRF data must be present
        let vrf_data = block
            .vrf_data
            .as_ref()
            .ok_or_else(|| ChainClientError::VrfValidationFailed("missing VRF data".to_string()))?;

        // Parse VRF public key
        let public_key = VrfPublicKey::from_bytes(&vrf_data.public_key).map_err(|e| {
            ChainClientError::VrfValidationFailed(format!("invalid VRF public key: {}", e))
        })?;

        // Parse VRF output and proof
        let output = VrfOutput::from_bytes(&vrf_data.output).map_err(|e| {
            ChainClientError::VrfValidationFailed(format!("invalid VRF output: {}", e))
        })?;
        let proof = VrfProof::from_bytes(&vrf_data.proof).map_err(|e| {
            ChainClientError::VrfValidationFailed(format!("invalid VRF proof: {}", e))
        })?;

        // Compute VRF input: BLAKE3("TOS-VRF-INPUT-v1" || block_hash || miner)
        let miner_compressed = self.miner_keypair.get_public_key().compress();
        let vrf_input = compute_vrf_input(block.hash.as_bytes(), &miner_compressed);

        // Verify VRF proof
        public_key
            .verify(&vrf_input, &output, &proof)
            .map_err(|e| {
                ChainClientError::VrfValidationFailed(format!(
                    "VRF proof verification failed: {}",
                    e
                ))
            })?;

        // Verify binding signature
        let binding_message = compute_vrf_binding_message(
            self.config.vrf.chain_id,
            &vrf_data.public_key,
            block.hash.as_bytes(),
        );
        let sig = Signature::from_bytes(&vrf_data.binding_signature).map_err(|_| {
            ChainClientError::VrfValidationFailed("invalid binding signature bytes".to_string())
        })?;
        let miner_pk = self.miner_keypair.get_public_key();
        if !sig.verify(&binding_message, miner_pk) {
            return Err(ChainClientError::VrfValidationFailed(
                "binding signature verification failed".to_string(),
            ));
        }

        Ok(())
    }

    // --- Scheduled Execution API ---

    /// Schedule a contract execution at a target topoheight.
    pub async fn schedule_execution(
        &mut self,
        exec: ScheduledExecution,
    ) -> Result<Hash, ChainClientError> {
        let target_topo = match exec.kind {
            tos_common::contract::ScheduledExecutionKind::TopoHeight(t) => t,
            tos_common::contract::ScheduledExecutionKind::BlockEnd => {
                return Err(ChainClientError::TransactionRejected(
                    "BlockEnd scheduling not supported via direct API".to_string(),
                ));
            }
        };

        // Validate: target must be in the future
        if target_topo <= self.topoheight {
            return Err(ChainClientError::InvalidWarpTarget {
                target: target_topo,
                current: self.topoheight,
            });
        }

        // Validate: within scheduling horizon
        let horizon = target_topo.saturating_sub(self.topoheight);
        if horizon > MAX_SCHEDULING_HORIZON {
            return Err(ChainClientError::TransactionRejected(format!(
                "target {} exceeds scheduling horizon (max {} blocks ahead)",
                target_topo, MAX_SCHEDULING_HORIZON
            )));
        }

        // Check for duplicate hash
        if self.scheduled_results.contains_key(&exec.hash) {
            return Err(ChainClientError::TransactionRejected(
                "duplicate scheduled execution hash".to_string(),
            ));
        }
        for queue_entries in self.scheduled_queue.values() {
            for existing in queue_entries {
                if existing.hash == exec.hash {
                    return Err(ChainClientError::TransactionRejected(
                        "duplicate scheduled execution hash".to_string(),
                    ));
                }
            }
        }

        // Validate: sender (scheduler_contract) has sufficient balance for offer
        if exec.offer_amount > 0 {
            let balance = self
                .blockchain
                .get_balance(&exec.scheduler_contract)
                .await
                .map_err(|_| {
                    ChainClientError::AccountNotFound(format!("{}", exec.scheduler_contract))
                })?;
            if balance < exec.offer_amount {
                return Err(ChainClientError::InsufficientBalance {
                    have: balance,
                    need: exec.offer_amount,
                });
            }

            // Deduct offer from sender balance
            let new_balance = balance.saturating_sub(exec.offer_amount);
            self.blockchain
                .force_set_balance(&exec.scheduler_contract, new_balance)
                .await
                .map_err(|e| ChainClientError::Storage(e.to_string()))?;

            // Burn 30% immediately
            let burn_amount = exec.offer_amount.saturating_mul(30) / 100;
            let counters = self.blockchain.counters();
            let mut c = counters.write();
            c.supply = c.supply.saturating_sub(burn_amount as u128);
            c.fees_burned = c.fees_burned.saturating_add(burn_amount);
        }

        let exec_hash = exec.hash.clone();
        self.scheduled_queue
            .entry(target_topo)
            .or_default()
            .push(exec);

        Ok(exec_hash)
    }

    /// Query scheduled execution status by hash.
    pub fn get_scheduled_status(&self, hash: &Hash) -> Option<(ScheduledExecutionStatus, u64)> {
        self.scheduled_results.get(hash).cloned()
    }

    /// Get all pending executions at a topoheight.
    pub fn get_pending_at(&self, topo: u64) -> Vec<&ScheduledExecution> {
        self.scheduled_queue
            .get(&topo)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Cancel a scheduled execution (returns refund amount).
    pub async fn cancel_scheduled(&mut self, hash: &Hash) -> Result<u64, ChainClientError> {
        let current_topo = self.topoheight;
        // Find and remove from queue
        for (_topo, entries) in self.scheduled_queue.iter_mut() {
            if let Some(pos) = entries.iter().position(|e| &e.hash == hash) {
                let exec = &entries[pos];
                if exec.status != ScheduledExecutionStatus::Pending {
                    return Err(ChainClientError::TransactionRejected(
                        "execution not in pending state".to_string(),
                    ));
                }
                // Check cancellation window
                if !exec.can_cancel(current_topo) {
                    return Err(ChainClientError::TransactionRejected(
                        "cannot cancel: within minimum cancellation window".to_string(),
                    ));
                }
                let exec = entries.remove(pos);
                // Refund 70% (30% was already burned)
                let refund = exec.offer_amount.saturating_mul(70) / 100;
                if refund > 0 {
                    let balance = self
                        .blockchain
                        .get_balance(&exec.scheduler_contract)
                        .await
                        .unwrap_or(0);
                    let _ = self
                        .blockchain
                        .force_set_balance(&exec.scheduler_contract, balance.saturating_add(refund))
                        .await;
                }
                self.scheduled_results
                    .insert(hash.clone(), (ScheduledExecutionStatus::Cancelled, 0));
                return Ok(refund);
            }
        }
        Err(ChainClientError::Storage(
            "scheduled execution not found".to_string(),
        ))
    }

    // --- Private Helpers ---

    /// Produce VRF data for a block hash and store it.
    fn produce_vrf_for_block(&mut self, block_hash: &Hash) {
        // Check feature gate: skip VRF if feature not active at current topoheight
        if !self.features.is_active("vrf_block_data", self.topoheight) {
            return;
        }

        let chain_id = self.config.vrf.chain_id;
        let block_hash_bytes = block_hash.as_bytes();
        let miner_compressed = self.miner_keypair.get_public_key().compress();

        let vrf_result = if let Some(ref vrf_mgr) = self.vrf_key_manager {
            vrf_mgr.sign(
                chain_id,
                block_hash_bytes,
                &miner_compressed,
                &self.miner_keypair,
            )
        } else {
            return;
        };

        if let Ok(vrf_data) = vrf_result {
            let pub_key_bytes = vrf_data.public_key.to_bytes();
            let proof_bytes = vrf_data.proof.to_bytes();
            let sig_bytes = vrf_data.binding_signature.to_bytes();

            // VrfOutput: copy bytes into fixed-size array
            let output_slice = vrf_data.output.as_bytes();
            let mut output_bytes = [0u8; 32];
            let copy_len = output_slice.len().min(32);
            output_bytes[..copy_len].copy_from_slice(&output_slice[..copy_len]);

            let block_vrf = BlockVrfData::new(pub_key_bytes, output_bytes, proof_bytes, sig_bytes);
            self.block_vrf_data.insert(self.topoheight, block_vrf);
        }
    }

    /// Process scheduled executions at the given topoheight.
    fn process_scheduled_at_topoheight(&mut self, topo: u64) {
        let executions = match self.scheduled_queue.remove(&topo) {
            Some(mut execs) => {
                // Sort by priority: offer_amount DESC, registration_topoheight ASC, hash ASC
                execs.sort_by(|a, b| {
                    b.offer_amount
                        .cmp(&a.offer_amount)
                        .then(a.registration_topoheight.cmp(&b.registration_topoheight))
                        .then(a.hash.cmp(&b.hash))
                });
                execs
            }
            None => return,
        };

        let mut gas_used_total: u64 = 0;
        let mut executed_count: usize = 0;
        let mut deferred: Vec<ScheduledExecution> = Vec::new();

        for mut exec in executions {
            // Check block capacity
            if executed_count >= MAX_SCHEDULED_EXECUTIONS_PER_BLOCK {
                // Defer remaining
                if exec.defer() {
                    exec.status = ScheduledExecutionStatus::Expired;
                    self.scheduled_results
                        .insert(exec.hash.clone(), (ScheduledExecutionStatus::Expired, topo));
                } else {
                    deferred.push(exec);
                }
                continue;
            }

            // Check gas budget
            let gas_needed = exec.max_gas.min(MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK);
            if gas_used_total.saturating_add(gas_needed) > MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK {
                // Defer due to gas
                if exec.defer() {
                    exec.status = ScheduledExecutionStatus::Expired;
                    self.scheduled_results
                        .insert(exec.hash.clone(), (ScheduledExecutionStatus::Expired, topo));
                } else {
                    deferred.push(exec);
                }
                continue;
            }

            // Execute (simplified: mark as Executed)
            gas_used_total = gas_used_total.saturating_add(gas_needed);
            executed_count = executed_count.saturating_add(1);

            // Pay 70% of offer to miner_address
            if exec.offer_amount > 0 {
                let miner_reward = exec.offer_amount.saturating_mul(70) / 100;
                if let Some(ref miner_addr) = self.miner_address {
                    let miner_addr = miner_addr.clone();
                    if let Ok(miner_balance) = self.blockchain.get_balance_sync(&miner_addr) {
                        let _ = self.blockchain.force_set_balance_sync(
                            &miner_addr,
                            miner_balance.saturating_add(miner_reward),
                        );
                    }
                }
            }

            exec.status = ScheduledExecutionStatus::Executed;
            self.scheduled_results.insert(
                exec.hash.clone(),
                (ScheduledExecutionStatus::Executed, topo),
            );
        }

        // Re-insert deferred executions at topo + 1
        if !deferred.is_empty() {
            self.scheduled_queue
                .entry(topo.saturating_add(1))
                .or_default()
                .extend(deferred);
        }
    }

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
            let block = self
                .blockchain
                .mine_block()
                .await
                .map_err(|e| WarpError::BlockCreationFailed(e.to_string()))?;
            self.topoheight = self.topoheight.saturating_add(1);

            // Produce VRF data if configured
            self.produce_vrf_for_block(&block.hash);

            // Process scheduled executions at this topoheight
            self.process_scheduled_at_topoheight(self.topoheight);
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
