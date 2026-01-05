mod direction;

use super::{default_true_value, DataElement, RPCContractOutput, RPCTransaction};
use crate::{
    account::{Nonce, VersionedBalance, VersionedNonce, VersionedUnoBalance},
    block::{Algorithm, BlockVersion, TopoHeight, EXTRA_NONCE_SIZE},
    crypto::{Address, Hash},
    difficulty::{CumulativeDifficulty, Difficulty},
    network::Network,
    time::{TimestampMillis, TimestampSeconds},
    transaction::extra_data::{SharedKey, UnknownExtraDataFormat},
};
use indexmap::IndexSet;
use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    net::SocketAddr,
};
use tos_kernel::ValueCell;

pub use direction::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlockType {
    Sync,
    Side,
    Orphaned,
    Normal,
}

// Serialize the extra nonce in a hexadecimal string
pub fn serialize_extra_nonce<S: Serializer>(
    extra_nonce: &Cow<'_, [u8; EXTRA_NONCE_SIZE]>,
    s: S,
) -> Result<S::Ok, S::Error> {
    s.serialize_str(&hex::encode(extra_nonce.as_ref()))
}

// Deserialize the extra nonce from a hexadecimal string
pub fn deserialize_extra_nonce<'de, 'a, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Cow<'a, [u8; EXTRA_NONCE_SIZE]>, D::Error> {
    let mut extra_nonce = [0u8; EXTRA_NONCE_SIZE];
    let hex = String::deserialize(deserializer)?;
    let decoded = hex::decode(hex).map_err(Error::custom)?;
    extra_nonce.copy_from_slice(&decoded);
    Ok(Cow::Owned(extra_nonce))
}

// Structure used to map the public key to a human readable address
#[derive(Serialize, Deserialize)]
pub struct RPCBlockResponse<'a> {
    pub hash: Cow<'a, Hash>,
    pub topoheight: Option<TopoHeight>,
    pub block_type: BlockType,
    pub difficulty: Cow<'a, Difficulty>,
    pub supply: Option<u64>,
    // Reward can be split into two parts
    pub reward: Option<u64>,
    // Miner reward (the one that found the block)
    pub miner_reward: Option<u64>,
    // And Dev Fee reward if enabled
    pub dev_reward: Option<u64>,
    pub cumulative_difficulty: Cow<'a, CumulativeDifficulty>,
    pub total_fees: Option<u64>,
    pub total_size_in_bytes: usize,
    pub version: BlockVersion,
    pub tips: Cow<'a, IndexSet<Hash>>,
    pub timestamp: TimestampMillis,
    pub height: u64,
    pub nonce: Nonce,
    #[serde(serialize_with = "serialize_extra_nonce")]
    #[serde(deserialize_with = "deserialize_extra_nonce")]
    pub extra_nonce: Cow<'a, [u8; EXTRA_NONCE_SIZE]>,
    pub miner: Cow<'a, Address>,
    pub txs_hashes: Cow<'a, IndexSet<Hash>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transactions: Vec<RPCTransaction<'a>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetMempoolParams {
    pub maximum: Option<usize>,
    pub skip: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolTransactionSummary<'a> {
    // TX hash
    pub hash: Cow<'a, Hash>,
    // The current sender
    pub source: Address,
    // Fees expected to be paid
    pub fee: u64,
    // First time seen in the mempool
    pub first_seen: TimestampSeconds,
    // Size of the TX
    pub size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionSummary<'a> {
    // TX hash
    pub hash: Cow<'a, Hash>,
    // The current sender
    pub source: Address,
    // Fees expected to be paid
    pub fee: u64,
    // Size of the TX
    pub size: usize,
}

#[derive(Serialize, Deserialize)]
pub struct GetMempoolResult<'a> {
    // The range of transactions requested
    pub transactions: Vec<TransactionResponse<'a>>,
    // How many TXs in total available in mempool
    pub total: usize,
}

#[derive(Serialize, Deserialize)]
pub struct GetMempoolSummaryResult<'a> {
    // The range of transactions requested
    pub transactions: Vec<MempoolTransactionSummary<'a>>,
    // How many TXs in total available in mempool
    pub total: usize,
}

pub type BlockResponse = RPCBlockResponse<'static>;

#[derive(Serialize, Deserialize)]
pub struct GetTopBlockParams {
    #[serde(default)]
    pub include_txs: bool,
}

#[derive(Serialize, Deserialize)]
pub struct GetBlockAtTopoHeightParams {
    pub topoheight: TopoHeight,
    #[serde(default)]
    pub include_txs: bool,
}

#[derive(Serialize, Deserialize)]
pub struct GetBlocksAtHeightParams {
    pub height: u64,
    #[serde(default)]
    pub include_txs: bool,
}

#[derive(Serialize, Deserialize)]
pub struct GetBlockByHashParams<'a> {
    pub hash: Cow<'a, Hash>,
    #[serde(default)]
    pub include_txs: bool,
}

#[derive(Serialize, Deserialize)]
pub struct GetBlockTemplateParams<'a> {
    pub address: Cow<'a, Address>,
}

#[derive(Serialize, Deserialize)]
pub struct GetMinerWorkParams<'a> {
    // Block Template in hexadecimal format
    pub template: Cow<'a, String>,
    // Address of the miner, if empty, it will use the address from template
    pub address: Option<Cow<'a, Address>>,
}

#[derive(Serialize, Deserialize)]
pub struct GetBlockTemplateResult {
    // block_template is Block Header in hexadecimal format
    // miner jobs can be created from it
    pub template: String,
    // Algorithm to use for the POW challenge
    pub algorithm: Algorithm,
    // Blockchain height
    pub height: u64,
    // Topoheight of the daemon
    pub topoheight: TopoHeight,
    // Difficulty target for the POW challenge
    pub difficulty: Difficulty,
}

#[derive(Serialize, Deserialize, PartialEq)]
pub struct GetMinerWorkResult {
    // algorithm to use
    pub algorithm: Algorithm,
    // template is miner job in hex format
    pub miner_work: String,
    // block height
    pub height: u64,
    // difficulty required for valid block POW
    pub difficulty: Difficulty,
    // topoheight of the daemon
    // this is for visual purposes only
    pub topoheight: TopoHeight,
}

#[derive(Serialize, Deserialize)]
pub struct SubmitMinerWorkParams {
    // hex: represent block miner in hexadecimal format
    // NOTE: alias block_template is used for backward compatibility < 1.9.4
    #[serde(alias = "miner_work", alias = "block_template")]
    pub miner_work: String,
}

#[derive(Serialize, Deserialize)]
pub struct SubmitBlockParams {
    // hex: represent the BlockHeader (Block)
    pub block_template: String,
    // optional miner work to apply to the block template
    pub miner_work: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct GetBalanceParams<'a> {
    pub address: Cow<'a, Address>,
    pub asset: Cow<'a, Hash>,
}

#[derive(Serialize, Deserialize)]
pub struct HasBalanceParams<'a> {
    pub address: Cow<'a, Address>,
    pub asset: Cow<'a, Hash>,
    #[serde(default)]
    pub topoheight: Option<TopoHeight>,
}

#[derive(Serialize, Deserialize)]
pub struct HasBalanceResult {
    pub exist: bool,
}

#[derive(Serialize, Deserialize)]
pub struct GetBalanceAtTopoHeightParams<'a> {
    pub address: Cow<'a, Address>,
    pub asset: Cow<'a, Hash>,
    pub topoheight: TopoHeight,
}

#[derive(Serialize, Deserialize)]
pub struct GetNonceParams<'a> {
    pub address: Cow<'a, Address>,
}

#[derive(Serialize, Deserialize)]
pub struct HasNonceParams<'a> {
    pub address: Cow<'a, Address>,
    #[serde(default)]
    pub topoheight: Option<TopoHeight>,
}

#[derive(Serialize, Deserialize)]
pub struct GetNonceAtTopoHeightParams<'a> {
    pub address: Cow<'a, Address>,
    pub topoheight: TopoHeight,
}

#[derive(Serialize, Deserialize)]
pub struct GetNonceResult {
    pub topoheight: TopoHeight,
    #[serde(flatten)]
    pub version: VersionedNonce,
}

#[derive(Serialize, Deserialize)]
pub struct HasNonceResult {
    pub exist: bool,
}

#[derive(Serialize, Deserialize)]
pub struct GetBalanceResult {
    pub balance: u64,
    pub topoheight: TopoHeight,
}

// Response type for get_balance_at_topoheight RPC endpoint
// Returns the full VersionedBalance structure with version history and output balance tracking
pub type GetBalanceAtTopoHeightResult = VersionedBalance;

// Response type for get_uno_balance RPC endpoint
// Returns the encrypted UNO balance with topoheight
#[derive(Serialize, Deserialize)]
pub struct GetUnoBalanceResult {
    pub version: VersionedUnoBalance,
    pub topoheight: TopoHeight,
}

// Response type for get_uno_balance_at_topoheight RPC endpoint
// Returns the full VersionedUnoBalance structure at a specific topoheight
pub type GetUnoBalanceAtTopoHeightResult = VersionedUnoBalance;

// Response type for has_uno_balance RPC endpoint
#[derive(Serialize, Deserialize)]
pub struct HasUnoBalanceResult {
    pub exist: bool,
}

#[derive(Serialize, Deserialize)]
pub struct GetStableBalanceResult {
    pub balance: u64,
    pub stable_topoheight: TopoHeight,
    pub stable_block_hash: Hash,
}

#[derive(Serialize, Deserialize)]
pub struct GetInfoResult {
    pub height: u64,
    pub topoheight: TopoHeight,
    pub stableheight: u64,
    pub stable_topoheight: TopoHeight,
    pub pruned_topoheight: Option<TopoHeight>,
    pub top_block_hash: Hash,
    // Current TOS circulating supply
    // This is calculated by doing
    // emitted_supply - burned_supply
    pub circulating_supply: u64,
    // Burned TOS supply
    #[serde(default)]
    pub burned_supply: u64,
    // Emitted TOS supply
    #[serde(default)]
    pub emitted_supply: u64,
    // Maximum supply of TOS
    pub maximum_supply: u64,
    // Current difficulty at tips
    pub difficulty: Difficulty,
    // Expected block time in milliseconds
    pub block_time_target: u64,
    // Average block time of last 50 blocks
    // in milliseconds
    pub average_block_time: u64,
    pub block_reward: u64,
    pub dev_reward: u64,
    pub miner_reward: u64,
    // count how many transactions are present in mempool
    pub mempool_size: usize,
    // software version on which the daemon is running
    pub version: String,
    // Network state (mainnet, testnet, devnet)
    pub network: Network,
    // Current block version enabled
    // Always returned by the daemon
    // But for compatibility with previous nodes
    // it is set to None
    pub block_version: Option<BlockVersion>,
}

#[derive(Serialize, Deserialize)]
pub struct SubmitTransactionParams {
    pub data: String, // should be in hex format
}

#[derive(Serialize, Deserialize)]
pub struct GetTransactionParams<'a> {
    pub hash: Cow<'a, Hash>,
}

pub type GetTransactionExecutorParams<'a> = GetTransactionParams<'a>;

#[derive(Serialize, Deserialize)]
pub struct GetTransactionExecutorResult<'a> {
    pub block_topoheight: TopoHeight,
    pub block_timestamp: TimestampMillis,
    pub block_hash: Cow<'a, Hash>,
}

#[derive(Serialize, Deserialize)]
pub struct GetPeersResponse<'a> {
    // Peers that are connected and allows to be displayed
    pub peers: Vec<PeerEntry<'a>>,
    // All peers connected
    pub total_peers: usize,
    // Peers that asked to not be listed
    pub hidden_peers: usize,
}

#[derive(Serialize, Deserialize)]
pub struct PeerEntry<'a> {
    pub id: u64,
    pub addr: Cow<'a, SocketAddr>,
    pub local_port: u16,
    pub tag: Cow<'a, Option<String>>,
    pub version: Cow<'a, String>,
    pub top_block_hash: Cow<'a, Hash>,
    pub topoheight: TopoHeight,
    pub height: u64,
    pub last_ping: TimestampSeconds,
    pub pruned_topoheight: Option<TopoHeight>,
    pub peers: Cow<'a, HashMap<SocketAddr, TimedDirection>>,
    pub cumulative_difficulty: Cow<'a, CumulativeDifficulty>,
    pub connected_on: TimestampSeconds,
    pub bytes_sent: usize,
    pub bytes_recv: usize,
}

#[derive(Serialize, Deserialize)]
pub struct P2pStatusResult<'a> {
    pub peer_count: usize,
    pub max_peers: usize,
    pub tag: Cow<'a, Option<String>>,
    pub our_topoheight: TopoHeight,
    pub best_topoheight: TopoHeight,
    pub median_topoheight: TopoHeight,
    pub peer_id: u64,
}

#[derive(Serialize, Deserialize)]
pub struct GetTopoHeightRangeParams {
    pub start_topoheight: Option<TopoHeight>,
    pub end_topoheight: Option<TopoHeight>,
}

#[derive(Serialize, Deserialize)]
pub struct GetHeightRangeParams {
    pub start_height: Option<u64>,
    pub end_height: Option<u64>,
}

#[derive(Serialize, Deserialize)]
pub struct GetTransactionsParams {
    pub tx_hashes: Vec<Hash>,
}

#[derive(Serialize, Deserialize)]
pub struct TransactionResponse<'a> {
    // in which blocks it was included
    pub blocks: Option<HashSet<Hash>>,
    // in which blocks it was executed
    pub executed_in_block: Option<Hash>,
    // if it is in mempool
    pub in_mempool: bool,
    // if its a mempool tx, we add the timestamp when it was added
    #[serde(default)]
    pub first_seen: Option<TimestampSeconds>,
    #[serde(flatten)]
    pub data: RPCTransaction<'a>,
}

fn default_tos_asset() -> Hash {
    crate::config::TOS_ASSET
}

#[derive(Serialize, Deserialize)]
pub struct GetAccountHistoryParams {
    pub address: Address,
    #[serde(default = "default_tos_asset")]
    pub asset: Hash,
    pub minimum_topoheight: Option<TopoHeight>,
    pub maximum_topoheight: Option<TopoHeight>,
    // Any incoming funds tracked
    #[serde(default = "default_true_value")]
    pub incoming_flow: bool,
    // Any outgoing funds tracked
    #[serde(default = "default_true_value")]
    pub outgoing_flow: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountHistoryType {
    DevFee {
        reward: u64,
    },
    Mining {
        reward: u64,
    },
    Burn {
        amount: u64,
    },
    Outgoing {
        to: Address,
    },
    Incoming {
        from: Address,
    },
    MultiSig {
        participants: Vec<Address>,
        threshold: u8,
    },
    InvokeContract {
        contract: Hash,
        entry_id: u16,
    },
    // Contract hash is already stored
    // by the parent struct
    DeployContract,
    FreezeTos {
        amount: u64,
        duration: String,
    },
    UnfreezeTos {
        amount: u64,
    },
    BindReferrer {
        referrer: Address,
    },
}

#[derive(Serialize, Deserialize)]
pub struct AccountHistoryEntry {
    pub topoheight: TopoHeight,
    pub hash: Hash,
    #[serde(flatten)]
    pub history_type: AccountHistoryType,
    pub block_timestamp: TimestampMillis,
}

// ============================================================================
// AI Mining History API
// ============================================================================

/// Filter by AI Mining transaction type
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AIMiningTransactionType {
    /// User published a task
    PublishTask,
    /// User submitted an answer to a task
    SubmitAnswer,
    /// User validated an answer
    ValidateAnswer,
    /// User registered as a miner
    RegisterMiner,
}

/// Request parameters for get_ai_mining_history RPC
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetAIMiningHistoryParams {
    /// The miner/participant address to query
    pub address: Address,

    /// Filter by task difficulty level (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<crate::ai_mining::DifficultyLevel>,

    /// Filter by transaction type (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_type: Option<AIMiningTransactionType>,

    /// Filter by specific task_id (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Hash>,

    /// Minimum topoheight (block height)
    pub minimum_topoheight: Option<TopoHeight>,

    /// Maximum topoheight (block height)
    pub maximum_topoheight: Option<TopoHeight>,

    /// Include published tasks (default: true)
    #[serde(default = "default_true_value")]
    pub include_published_tasks: bool,

    /// Include submitted answers (default: true)
    #[serde(default = "default_true_value")]
    pub include_submitted_answers: bool,

    /// Include validations performed (default: true)
    #[serde(default = "default_true_value")]
    pub include_validations: bool,

    /// Pagination: skip N entries
    pub skip: Option<usize>,

    /// Pagination: maximum entries to return (default 100, max 1000)
    pub maximum: Option<usize>,
}

/// AI Mining transaction history entry
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AIMiningHistoryEntry {
    /// Block topoheight when transaction occurred
    pub topoheight: TopoHeight,

    /// Transaction hash
    pub tx_hash: Hash,

    /// Block hash containing the transaction
    pub block_hash: Hash,

    /// Timestamp when block was mined (milliseconds)
    pub block_timestamp: TimestampMillis,

    /// The specific transaction details
    #[serde(flatten)]
    pub transaction: AIMiningHistoryType,
}

/// Union type for all AI Mining transaction variants in history
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AIMiningHistoryType {
    /// Task published by this address
    PublishTask {
        task_id: Hash,
        reward_amount: u64,
        difficulty: crate::ai_mining::DifficultyLevel,
        deadline: u64,
        description: String,
    },

    /// Answer submitted by this address
    SubmitAnswer {
        task_id: Hash,
        answer_id: Hash,
        stake_amount: u64,
        answer_hash: Hash,
    },

    /// Validation performed by this address
    ValidateAnswer {
        task_id: Hash,
        answer_id: Hash,
        validation_score: u8,
    },

    /// Miner registration by this address
    RegisterMiner { registration_fee: u64 },
}

/// Summary statistics for AI Mining participant
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AIMiningUserSummary {
    /// Total tasks published by this address
    pub total_tasks_published: u32,

    /// Total answers submitted
    pub total_answers_submitted: u32,

    /// Total validations performed
    pub total_validations_performed: u32,

    /// Current reputation score (0-10000)
    pub reputation_score: u64,

    /// Total rewards earned (nanoTOS)
    pub total_rewards_earned: u64,

    /// Total stake in system (nanoTOS)
    pub total_stake: u64,

    /// Miner registration status
    pub is_registered_miner: bool,

    /// Block height when registered (if applicable)
    pub registered_at: Option<u64>,
}

/// Response for get_ai_mining_history RPC
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetAIMiningHistoryResult {
    /// List of AI Mining transactions for this address
    pub transactions: Vec<AIMiningHistoryEntry>,

    /// Total number of AI Mining transactions available (before pagination)
    pub total: usize,

    /// Summary statistics for this address
    pub summary: AIMiningUserSummary,
}

#[derive(Serialize, Deserialize)]
pub struct GetAccountAssetsParams<'a> {
    pub address: Cow<'a, Address>,
    pub skip: Option<usize>,
    pub maximum: Option<usize>,
}

#[derive(Serialize, Deserialize)]
pub struct GetAssetParams<'a> {
    pub asset: Cow<'a, Hash>,
}

#[derive(Serialize, Deserialize)]
pub struct GetAssetsParams {
    pub skip: Option<usize>,
    pub maximum: Option<usize>,
    pub minimum_topoheight: Option<TopoHeight>,
    pub maximum_topoheight: Option<TopoHeight>,
}

#[derive(Serialize, Deserialize)]
pub struct GetAccountsParams {
    pub skip: Option<usize>,
    pub maximum: Option<usize>,
    pub minimum_topoheight: Option<TopoHeight>,
    pub maximum_topoheight: Option<TopoHeight>,
}

#[derive(Serialize, Deserialize)]
pub struct IsAccountRegisteredParams<'a> {
    pub address: Cow<'a, Address>,
    // If it is registered in stable height (confirmed)
    pub in_stable_height: bool,
}

#[derive(Serialize, Deserialize)]
pub struct GetAccountRegistrationParams<'a> {
    pub address: Cow<'a, Address>,
}

#[derive(Serialize, Deserialize)]
pub struct IsTxExecutedInBlockParams<'a> {
    pub tx_hash: Cow<'a, Hash>,
    pub block_hash: Cow<'a, Hash>,
}

// Struct to define dev fee threshold
#[derive(serde::Serialize, serde::Deserialize)]
pub struct DevFeeThreshold {
    // block height to start dev fee
    pub height: u64,
    // percentage of dev fee, example 10 = 10%
    pub fee_percentage: u64,
}

/// Fork activation condition for TIP (TOS Improvement Proposal) activation
///
/// This enum defines different conditions under which a hard fork can be activated.
/// Each condition type serves different use cases:
///
/// - `Block`: Deterministic activation at a specific block height
/// - `Timestamp`: Time-based activation (useful for coordinated upgrades)
/// - `TCD`: Threshold Cumulative Difficulty activation (network hashrate dependent)
/// - `Never`: Disabled activation (useful for testnet-only features)
///
/// # Examples
///
/// ```ignore
/// // Activate at block height 1,000,000
/// ForkCondition::Block(1_000_000)
///
/// // Activate at Unix timestamp (2026-01-01 00:00:00 UTC)
/// ForkCondition::Timestamp(1767225600000)
///
/// // Activate when cumulative difficulty reaches threshold
/// ForkCondition::TCD(1_000_000_000)
///
/// // Never activate (disabled feature)
/// ForkCondition::Never
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ForkCondition {
    /// Activate at a specific block height
    /// This is the most common and deterministic activation method
    Block(u64),

    /// Activate at a specific Unix timestamp (in milliseconds)
    /// Useful for time-coordinated network upgrades
    /// Note: Timestamp-based activation depends on block timestamps which
    /// can have some variance due to mining time
    Timestamp(u64),

    /// Activate when the network's cumulative difficulty reaches a threshold
    /// This is hashrate-dependent and useful for security-related upgrades
    /// that should only activate after the network has sufficient hashpower
    /// The value represents the minimum cumulative difficulty threshold
    TCD(u64),

    /// Never activate this fork
    /// Used for features that are disabled or testnet-only
    Never,
}

impl ForkCondition {
    /// Check if this fork condition is satisfied given the current state
    ///
    /// # Arguments
    /// * `height` - Current block height
    /// * `timestamp` - Current block timestamp (in milliseconds)
    /// * `cumulative_difficulty` - Current cumulative difficulty of the chain
    ///
    /// # Returns
    /// `true` if the fork condition is satisfied, `false` otherwise
    pub fn is_satisfied(&self, height: u64, timestamp: u64, cumulative_difficulty: u64) -> bool {
        match self {
            ForkCondition::Block(activation_height) => height >= *activation_height,
            ForkCondition::Timestamp(activation_timestamp) => timestamp >= *activation_timestamp,
            ForkCondition::TCD(threshold) => cumulative_difficulty >= *threshold,
            ForkCondition::Never => false,
        }
    }

    /// Get the activation height if this is a Block condition
    pub fn activation_height(&self) -> Option<u64> {
        match self {
            ForkCondition::Block(height) => Some(*height),
            _ => None,
        }
    }

    /// Get the activation timestamp if this is a Timestamp condition
    pub fn activation_timestamp(&self) -> Option<u64> {
        match self {
            ForkCondition::Timestamp(ts) => Some(*ts),
            _ => None,
        }
    }

    /// Get the TCD threshold if this is a TCD condition
    pub fn tcd_threshold(&self) -> Option<u64> {
        match self {
            ForkCondition::TCD(threshold) => Some(*threshold),
            _ => None,
        }
    }

    /// Check if this condition is Never (disabled)
    pub fn is_never(&self) -> bool {
        matches!(self, ForkCondition::Never)
    }

    /// Check if this condition uses block height (deterministic)
    pub fn is_block_based(&self) -> bool {
        matches!(self, ForkCondition::Block(_))
    }
}

impl std::fmt::Display for ForkCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ForkCondition::Block(height) => write!(f, "Block({})", height),
            ForkCondition::Timestamp(ts) => write!(f, "Timestamp({})", ts),
            ForkCondition::TCD(threshold) => write!(f, "TCD({})", threshold),
            ForkCondition::Never => write!(f, "Never"),
        }
    }
}

/// Struct to define hard fork configuration
///
/// A hard fork represents a protocol upgrade that introduces incompatible changes.
/// Each hard fork is associated with a `BlockVersion` and activation condition.
///
/// # Activation Conditions
///
/// Hard forks can be activated based on different conditions (see `ForkCondition`):
/// - **Block**: Deterministic activation at a specific block height
/// - **Timestamp**: Time-based activation (Unix timestamp in milliseconds)
/// - **TCD**: Hashrate-dependent activation (Threshold Cumulative Difficulty)
/// - **Never**: Disabled features (testnet-only or deprecated)
///
/// # Design
///
/// This design follows Ethereum's approach (see reth/alloy_hardforks) where
/// ForkCondition is the single source of truth for activation logic.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct HardFork {
    /// Fork activation condition
    /// Defines when this hard fork becomes active
    pub condition: ForkCondition,

    /// Block version to use after this fork activates
    pub version: BlockVersion,

    /// Description of changes in this hard fork
    pub changelog: &'static str,

    /// Minimum software version requirement (e.g., ">=1.13.0")
    /// Used for P2P protocol compatibility checking
    pub version_requirement: Option<&'static str>,
}

impl HardFork {
    /// Get the fork activation condition
    #[inline]
    pub fn condition(&self) -> ForkCondition {
        self.condition
    }

    /// Check if this hard fork is activated given the current state
    ///
    /// # Arguments
    /// * `height` - Current block height
    /// * `timestamp` - Current block timestamp (in milliseconds)
    /// * `cumulative_difficulty` - Current cumulative difficulty
    pub fn is_activated(&self, height: u64, timestamp: u64, cumulative_difficulty: u64) -> bool {
        self.condition
            .is_satisfied(height, timestamp, cumulative_difficulty)
    }

    /// Check if this hard fork is activated at a specific height
    /// For Block conditions, checks height directly
    /// For Timestamp/TCD conditions, returns false (cannot determine by height alone)
    /// For Never condition, always returns false
    pub fn is_activated_at_height(&self, height: u64) -> bool {
        match self.condition {
            ForkCondition::Block(activation_height) => height >= activation_height,
            ForkCondition::Never => false,
            // Timestamp and TCD conditions cannot be checked by height alone
            ForkCondition::Timestamp(_) | ForkCondition::TCD(_) => false,
        }
    }

    /// Get the activation block height if this is a Block-based condition
    pub fn activation_height(&self) -> Option<u64> {
        self.condition.activation_height()
    }

    /// Get the activation timestamp if this is a Timestamp-based condition
    pub fn activation_timestamp(&self) -> Option<u64> {
        self.condition.activation_timestamp()
    }

    /// Get the TCD threshold if this is a TCD-based condition
    pub fn tcd_threshold(&self) -> Option<u64> {
        self.condition.tcd_threshold()
    }

    /// Check if this fork is disabled (Never condition)
    pub fn is_disabled(&self) -> bool {
        self.condition.is_never()
    }
}

// ============================================================================
// TosHardfork - Independent TIP Activation (like Ethereum's EthereumHardfork)
// ============================================================================

/// TOS Hard Fork / TIP enumeration
///
/// Each variant represents a specific protocol change that can be independently
/// activated. This follows Ethereum's EthereumHardfork enum design where each
/// feature can be queried and activated independently.
///
/// # Examples
///
/// ```ignore
/// // Check if a TIP is active at a specific height
/// if chain_tips.is_active(TosHardfork::SomeFutureTip, height, timestamp, tcd) {
///     // Feature is enabled
/// }
/// ```
///
/// # Adding New TIPs
///
/// To add a new TIP, add a variant here and configure its activation in
/// `daemon/src/config.rs` (MAINNET_TIPS, TESTNET_TIPS, DEVNET_TIPS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum TosHardfork {
    // === Future TIPs will be added here ===
    // Example:
    // /// TIP-100: Smart contract support
    // SmartContracts,
}

impl TosHardfork {
    /// Returns all known hard forks in activation order
    pub const fn all() -> &'static [Self] {
        &[
            // Future TIPs will be added here
        ]
    }

    /// Returns the TIP number for this hardfork
    pub const fn tip_number(&self) -> u16 {
        match *self {
            // Future TIPs will be added here
            // Example: Self::SmartContracts => 100,
        }
    }

    /// Returns a human-readable name for this hardfork
    pub const fn name(&self) -> &'static str {
        match *self {
            // Future TIPs will be added here
            // Example: Self::SmartContracts => "SmartContracts",
        }
    }
}

impl std::fmt::Display for TosHardfork {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            // Future TIPs will be added here
            // Example: _ => write!(f, "TIP-{}: {}", self.tip_number(), self.name()),
        }
    }
}

/// Collection of TIP activations with independent ForkCondition per TIP
///
/// This follows the Ethereum/reth design where each hard fork has its own
/// ForkCondition, enabling independent feature activation.
///
/// # Examples
///
/// ```ignore
/// // When TIPs are added, configure them like this:
/// let tips = ChainTips::new(vec![
///     (TosHardfork::SomeFutureTip, ForkCondition::Block(100000)),
/// ]);
///
/// if tips.is_active(TosHardfork::SomeFutureTip, height, timestamp, tcd) {
///     // Feature is active
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct ChainTips {
    /// Map of TIP to its activation condition
    tips: HashMap<TosHardfork, ForkCondition>,
}

impl ChainTips {
    /// Create a new ChainTips from a list of (TIP, ForkCondition) pairs
    pub fn new(tips: Vec<(TosHardfork, ForkCondition)>) -> Self {
        Self {
            tips: tips.into_iter().collect(),
        }
    }

    /// Get the ForkCondition for a specific TIP
    pub fn fork(&self, hardfork: TosHardfork) -> ForkCondition {
        self.tips
            .get(&hardfork)
            .copied()
            .unwrap_or(ForkCondition::Never)
    }

    /// Check if a TIP is active given current state
    pub fn is_active(
        &self,
        hardfork: TosHardfork,
        height: u64,
        timestamp: u64,
        cumulative_difficulty: u64,
    ) -> bool {
        self.fork(hardfork)
            .is_satisfied(height, timestamp, cumulative_difficulty)
    }

    /// Check if a TIP is active at a specific block height (Block conditions only)
    pub fn is_active_at_height(&self, hardfork: TosHardfork, height: u64) -> bool {
        match self.fork(hardfork) {
            ForkCondition::Block(activation_height) => height >= activation_height,
            _ => false,
        }
    }

    /// Check if a TIP is active at a specific timestamp (Timestamp conditions only)
    pub fn is_active_at_timestamp(&self, hardfork: TosHardfork, timestamp: u64) -> bool {
        match self.fork(hardfork) {
            ForkCondition::Timestamp(activation_ts) => timestamp >= activation_ts,
            _ => false,
        }
    }

    /// Get the activation height for a TIP (if Block-based)
    pub fn activation_height(&self, hardfork: TosHardfork) -> Option<u64> {
        self.fork(hardfork).activation_height()
    }

    /// Get the activation timestamp for a TIP (if Timestamp-based)
    pub fn activation_timestamp(&self, hardfork: TosHardfork) -> Option<u64> {
        self.fork(hardfork).activation_timestamp()
    }

    /// Insert or update a TIP's activation condition
    pub fn insert(&mut self, hardfork: TosHardfork, condition: ForkCondition) {
        self.tips.insert(hardfork, condition);
    }

    /// Get all TIPs that are active at the given state
    pub fn active_tips(
        &self,
        height: u64,
        timestamp: u64,
        cumulative_difficulty: u64,
    ) -> Vec<TosHardfork> {
        self.tips
            .iter()
            .filter(|(_, cond)| cond.is_satisfied(height, timestamp, cumulative_difficulty))
            .map(|(hf, _)| *hf)
            .collect()
    }

    /// Get all configured TIPs
    pub fn all_tips(&self) -> Vec<(TosHardfork, ForkCondition)> {
        self.tips.iter().map(|(hf, cond)| (*hf, *cond)).collect()
    }
}

// Struct to returns the size of the blockchain on disk
#[derive(Serialize, Deserialize)]
pub struct SizeOnDiskResult {
    pub size_bytes: u64,
    pub size_formatted: String,
}

#[derive(Serialize, Deserialize)]
pub struct GetMempoolCacheParams<'a> {
    pub address: Cow<'a, Address>,
}

#[derive(Serialize, Deserialize)]
pub struct GetMempoolCacheResult {
    // lowest nonce used
    min: Nonce,
    // highest nonce used
    max: Nonce,
    // all txs ordered by nonce
    txs: Vec<Hash>,
    // All "final" cached balances used
    balances: HashMap<Hash, u64>,
}

impl GetMempoolCacheResult {
    /// Get the lowest nonce used in pending transactions
    pub fn get_min_nonce(&self) -> Nonce {
        self.min
    }

    /// Get the highest nonce used in pending transactions
    pub fn get_max_nonce(&self) -> Nonce {
        self.max
    }

    /// Get all transaction hashes in the mempool cache (ordered by nonce)
    pub fn get_txs(&self) -> &[Hash] {
        &self.txs
    }

    /// Get the cached balances for all assets
    pub fn get_balances(&self) -> &HashMap<Hash, u64> {
        &self.balances
    }
}

// This struct is used to store the fee rate estimation for the following priority levels:
// 1. Low
// 2. Medium
// 3. High
// Each priority is in fee per KB.  It cannot be below `FEE_PER_KB` which is required by the network.
#[derive(Serialize, Deserialize)]
pub struct FeeRatesEstimated {
    pub low: u64,
    pub medium: u64,
    pub high: u64,
    // The minimum fee rate possible on the network
    pub default: u64,
}

#[derive(Serialize, Deserialize)]
pub struct GetDifficultyResult {
    pub difficulty: Difficulty,
    pub hashrate: Difficulty,
    pub hashrate_formatted: String,
}

#[derive(Serialize, Deserialize)]
pub struct ValidateAddressParams<'a> {
    pub address: Cow<'a, Address>,
    #[serde(default)]
    pub allow_integrated: bool,
    #[serde(default)]
    pub max_integrated_data_size: Option<usize>,
}

#[derive(Serialize, Deserialize)]
pub struct ValidateAddressResult {
    pub is_valid: bool,
    pub is_integrated: bool,
}

#[derive(Serialize, Deserialize)]
pub struct ExtractKeyFromAddressParams<'a> {
    pub address: Cow<'a, Address>,
    #[serde(default)]
    pub as_hex: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractKeyFromAddressResult {
    Bytes(Vec<u8>),
    Hex(String),
}

#[derive(Serialize, Deserialize)]
pub struct MakeIntegratedAddressParams<'a> {
    pub address: Cow<'a, Address>,
    pub integrated_data: Cow<'a, DataElement>,
}

#[derive(Serialize, Deserialize)]
pub struct DecryptExtraDataParams<'a> {
    pub shared_key: Cow<'a, SharedKey>,
    pub extra_data: Cow<'a, UnknownExtraDataFormat>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultisigState {
    // If the user has deleted its multisig at requested topoheight
    Deleted,
    // If the user has a multisig at requested topoheight
    Active {
        participants: Vec<Address>,
        threshold: u8,
    },
}

#[derive(Serialize, Deserialize)]
pub struct GetMultisigAtTopoHeightParams<'a> {
    pub address: Cow<'a, Address>,
    pub topoheight: TopoHeight,
}

#[derive(Serialize, Deserialize)]
pub struct GetMultisigAtTopoHeightResult {
    pub state: MultisigState,
}

#[derive(Serialize, Deserialize)]
pub struct GetMultisigParams<'a> {
    pub address: Cow<'a, Address>,
}

#[derive(Serialize, Deserialize)]
pub struct GetMultisigResult {
    // State at topoheight
    pub state: MultisigState,
    // Topoheight of the last change
    pub topoheight: TopoHeight,
}

#[derive(Serialize, Deserialize)]
pub struct HasMultisigParams<'a> {
    pub address: Cow<'a, Address>,
}

#[derive(Serialize, Deserialize)]
pub struct HasMultisigAtTopoHeightParams<'a> {
    pub address: Cow<'a, Address>,
    pub topoheight: TopoHeight,
}

#[derive(Serialize, Deserialize)]
pub struct GetContractOutputsParams<'a> {
    pub transaction: Cow<'a, Hash>,
}

#[derive(Serialize, Deserialize)]
pub struct GetContractModuleParams<'a> {
    pub contract: Cow<'a, Hash>,
}

#[derive(Serialize, Deserialize)]
pub struct GetContractDataParams<'a> {
    pub contract: Cow<'a, Hash>,
    pub key: Cow<'a, ValueCell>,
}

#[derive(Serialize, Deserialize)]
pub struct GetContractDataAtTopoHeightParams<'a> {
    pub contract: Cow<'a, Hash>,
    pub key: Cow<'a, ValueCell>,
    pub topoheight: TopoHeight,
}

#[derive(Serialize, Deserialize)]
pub struct GetContractBalanceParams<'a> {
    pub contract: Cow<'a, Hash>,
    pub asset: Cow<'a, Hash>,
}

#[derive(Serialize, Deserialize)]
pub struct GetContractBalanceAtTopoHeightParams<'a> {
    pub contract: Cow<'a, Hash>,
    pub asset: Cow<'a, Hash>,
    pub topoheight: TopoHeight,
}

#[derive(Serialize, Deserialize)]
pub struct GetContractBalancesParams<'a> {
    pub contract: Cow<'a, Hash>,
    pub skip: Option<usize>,
    pub maximum: Option<usize>,
}

/// Retrieves contract events (LOG0-LOG4 syscalls) with filtering options
#[derive(Serialize, Deserialize)]
pub struct GetContractEventsParams<'a> {
    /// Filter by contract address (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract: Option<Cow<'a, Hash>>,
    /// Filter by transaction hash (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<Cow<'a, Hash>>,
    /// Filter by topic0 (event signature hash, optional)
    /// This is the first topic in LOG1-LOG4 events, typically the event type identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic0: Option<String>,
    /// Minimum topoheight (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_topoheight: Option<TopoHeight>,
    /// Maximum topoheight (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_topoheight: Option<TopoHeight>,
    /// Maximum number of events to return (default 100, max 1000)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Response event for get_contract_events RPC method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RPCContractEvent {
    /// Contract address that emitted the event
    pub contract: Hash,
    /// Transaction hash that triggered the event
    pub tx_hash: Hash,
    /// Block hash where the event was emitted
    pub block_hash: Hash,
    /// Topoheight when the event was emitted
    pub topoheight: TopoHeight,
    /// Log index within the transaction
    pub log_index: u32,
    /// Event topics (0-4 topics from LOG0-LOG4)
    pub topics: Vec<String>,
    /// Event data (hex-encoded)
    pub data: String,
}

/// Computes the deterministic contract address from a DeployContract transaction
#[derive(Serialize, Deserialize)]
pub struct GetContractAddressFromTxParams<'a> {
    /// The transaction hash of a DeployContract transaction
    pub transaction: Cow<'a, Hash>,
}

/// Response for get_contract_address_from_tx RPC method
#[derive(Serialize, Deserialize)]
pub struct GetContractAddressFromTxResult {
    /// The computed contract address (deterministic from deployer + bytecode)
    pub contract_address: Hash,
    /// The deployer's address (for reference)
    pub deployer: String,
}

#[derive(Serialize, Deserialize)]
pub struct GetEnergyParams<'a> {
    pub address: Cow<'a, Address>,
}

#[derive(Serialize, Deserialize)]
pub struct GetEnergyResult {
    pub frozen_tos: u64,
    pub energy: u64,
    pub available_energy: u64,
    pub last_update: u64,
    pub freeze_records: Vec<FreezeRecordInfo>,
}

#[derive(Serialize, Deserialize)]
pub struct FreezeRecordInfo {
    pub amount: u64,
    pub duration: String,
    pub freeze_topoheight: u64,
    pub unlock_topoheight: u64,
    pub energy_gained: u64,
    pub can_unlock: bool,
    pub remaining_blocks: u64,
}

#[derive(Serialize, Deserialize)]
pub struct RPCVersioned<T> {
    pub topoheight: TopoHeight,
    #[serde(flatten)]
    pub version: T,
}

#[derive(Serialize, Deserialize)]
pub struct P2pBlockPropagationResult {
    // peer id => entry
    pub peers: HashMap<u64, TimedDirection>,
    // When was the first time we saw this block
    pub first_seen: Option<TimestampMillis>,
    // At which time we started to process it
    pub processing_at: Option<TimestampMillis>,
}

#[derive(Serialize, Deserialize)]
pub struct GetP2pBlockPropagation<'a> {
    pub hash: Cow<'a, Hash>,
    #[serde(default = "default_true_value")]
    pub outgoing: bool,
    #[serde(default = "default_true_value")]
    pub incoming: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyEvent {
    // When a new block is accepted by chain
    // it contains NewBlockEvent as value
    NewBlock,
    // When a block (already in chain or not) is ordered (new topoheight)
    // it contains BlockOrderedEvent as value
    BlockOrdered,
    // When a block that was ordered is not in the new DAG order
    // it contains BlockOrphanedEvent that got orphaned
    BlockOrphaned,
    // When stable height has changed (different than the previous one)
    // it contains StableHeightChangedEvent struct as value
    StableHeightChanged,
    // When stable topoheight has changed (different than the previous one)
    // it contains StableTopoHeightChangedEvent struct as value
    StableTopoHeightChanged,
    // When a transaction that was executed in a block is not reintroduced in mempool
    // It contains TransactionOrphanedEvent as value
    TransactionOrphaned,
    // When a new transaction is added in mempool
    // it contains TransactionAddedInMempoolEvent struct as value
    TransactionAddedInMempool,
    // When a transaction has been included in a valid block & executed on chain
    // it contains TransactionExecutedEvent struct as value
    TransactionExecuted,
    // When the contract has been invoked
    // This allows to track all the contract invocations
    InvokeContract {
        contract: Hash,
    },
    // When a contract has transfered any token
    // to the receiver address
    // It contains ContractTransferEvent struct as value
    ContractTransfer {
        address: Address,
    },
    // When a contract fire an event
    // It contains ContractEvent struct as value
    ContractEvent {
        // Contract hash to track
        contract: Hash,
        // ID of the event that is fired from the contract
        id: u64,
    },
    // When a new contract has been deployed
    DeployContract,
    // When a new asset has been registered
    // It contains NewAssetEvent struct as value
    NewAsset,
    // When a new peer has connected to us
    // It contains PeerConnectedEvent struct as value
    PeerConnected,
    // When a peer has disconnected from us
    // It contains PeerDisconnectedEvent struct as value
    PeerDisconnected,
    // Peer peerlist updated, its all its connected peers
    // It contains PeerPeerListUpdatedEvent as value
    PeerPeerListUpdated,
    // Peer has been updated through a ping packet
    // Contains PeerStateUpdatedEvent as value
    PeerStateUpdated,
    // When a peer of a peer has disconnected
    // and that he notified us
    // It contains PeerPeerDisconnectedEvent as value
    PeerPeerDisconnected,
    // A new block template has been created
    NewBlockTemplate,
    // When a payment is received to a watched address
    // It contains AddressPaymentEvent struct as value
    // This is used for QR code payment monitoring (TIP-QR-PAYMENT)
    WatchAddressPayments {
        address: Address,
    },
}

// Value of NotifyEvent::NewBlock
pub type NewBlockEvent = BlockResponse;

// Value of NotifyEvent::BlockOrdered
#[derive(Serialize, Deserialize)]
pub struct BlockOrderedEvent<'a> {
    // block hash in which this event was triggered
    pub block_hash: Cow<'a, Hash>,
    pub block_type: BlockType,
    // the new topoheight of the block
    pub topoheight: TopoHeight,
}

// Value of NotifyEvent::BlockOrphaned
#[derive(Serialize, Deserialize)]
pub struct BlockOrphanedEvent<'a> {
    pub block_hash: Cow<'a, Hash>,
    // Tpoheight of the block before being orphaned
    pub old_topoheight: TopoHeight,
}

// Value of NotifyEvent::StableHeightChanged
#[derive(Serialize, Deserialize)]
pub struct StableHeightChangedEvent {
    pub previous_stable_height: u64,
    pub new_stable_height: u64,
}

// Value of NotifyEvent::StableTopoHeightChanged
#[derive(Serialize, Deserialize)]
pub struct StableTopoHeightChangedEvent {
    pub previous_stable_topoheight: TopoHeight,
    pub new_stable_topoheight: TopoHeight,
}

// Value of NotifyEvent::TransactionAddedInMempool
pub type TransactionAddedInMempoolEvent = MempoolTransactionSummary<'static>;
// Value of NotifyEvent::TransactionOrphaned
pub type TransactionOrphanedEvent = TransactionResponse<'static>;

// Value of NotifyEvent::TransactionExecuted
#[derive(Serialize, Deserialize)]
pub struct TransactionExecutedEvent<'a> {
    pub block_hash: Cow<'a, Hash>,
    pub tx_hash: Cow<'a, Hash>,
    pub topoheight: TopoHeight,
}

// Value of NotifyEvent::NewAsset
#[derive(Serialize, Deserialize)]
pub struct NewAssetEvent<'a> {
    pub asset: Cow<'a, Hash>,
    pub block_hash: Cow<'a, Hash>,
    pub topoheight: TopoHeight,
}

// Value of NotifyEvent::ContractTransfer
#[derive(Serialize, Deserialize)]
pub struct ContractTransferEvent<'a> {
    pub asset: Cow<'a, Hash>,
    pub amount: u64,
    pub block_hash: Cow<'a, Hash>,
    pub topoheight: TopoHeight,
}

// Value of NotifyEvent::ContractEvent
#[derive(Serialize, Deserialize)]
pub struct ContractEvent<'a> {
    pub data: Cow<'a, ValueCell>,
}

// Value of NotifyEvent::PeerConnected
pub type PeerConnectedEvent = PeerEntry<'static>;

// Value of NotifyEvent::PeerDisconnected
pub type PeerDisconnectedEvent = PeerEntry<'static>;

// Value of NotifyEvent::PeerPeerListUpdated
#[derive(Serialize, Deserialize)]
pub struct PeerPeerListUpdatedEvent {
    // Peer ID of the peer that sent us the new peer list
    pub peer_id: u64,
    // Peerlist received from this peer
    pub peerlist: IndexSet<SocketAddr>,
}

// Value of NotifyEvent::PeerStateUpdated
pub type PeerStateUpdatedEvent = PeerEntry<'static>;

// Value of NotifyEvent::PeerPeerDisconnected
#[derive(Serialize, Deserialize)]
pub struct PeerPeerDisconnectedEvent {
    // Peer ID of the peer that sent us this notification
    pub peer_id: u64,
    // address of the peer that disconnected from him
    pub peer_addr: SocketAddr,
}

// Value of NotifyEvent::InvokeContract
#[derive(Serialize, Deserialize)]
pub struct InvokeContractEvent<'a> {
    pub block_hash: Cow<'a, Hash>,
    pub tx_hash: Cow<'a, Hash>,
    pub topoheight: TopoHeight,
    pub contract_outputs: Vec<RPCContractOutput<'a>>,
}

// Value of NotifyEvent::NewContract
#[derive(Serialize, Deserialize)]
pub struct NewContractEvent<'a> {
    pub contract: Cow<'a, Hash>,
    pub block_hash: Cow<'a, Hash>,
    pub topoheight: TopoHeight,
}

// Value of NotifyEvent::WatchAddressPayments
// Used for real-time payment monitoring (TIP-QR-PAYMENT)
#[derive(Serialize, Deserialize)]
pub struct AddressPaymentEvent<'a> {
    /// Recipient address that received the payment
    pub address: Address,
    /// Transaction hash containing the payment
    pub tx_hash: Cow<'a, Hash>,
    /// Amount received in atomic units
    pub amount: u64,
    /// Asset hash (native TOS if matches XELIS_ASSET)
    pub asset: Cow<'a, Hash>,
    /// Block hash containing the transaction
    pub block_hash: Cow<'a, Hash>,
    /// Block topoheight
    pub topoheight: TopoHeight,
    /// Payment ID if present in extra_data (TIP-QR-PAYMENT format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_id: Option<Cow<'a, str>>,
    /// Memo if present in extra_data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<Cow<'a, str>>,
    /// Number of confirmations (1 = just included in block)
    pub confirmations: u64,
}

// ============================================================================
// QR Code Payment Types
// ============================================================================

/// Request to get payments received by an address
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetAddressPaymentsParams {
    /// Address to check for incoming payments
    pub address: Address,
    /// Minimum topoheight to start search from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_topoheight: Option<TopoHeight>,
    /// Maximum number of payments to return
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}
/// Parameters for get_contract_scheduled_executions_at_topoheight RPC method
#[derive(Serialize, Deserialize)]
pub struct GetContractScheduledExecutionsAtTopoHeightParams {
    pub topoheight: TopoHeight,
    pub max: Option<usize>,
    pub skip: Option<usize>,
}

/// Parameters for get_contracts RPC method - lists all deployed contracts
#[derive(Serialize, Deserialize)]
pub struct GetContractsParams {
    /// Number of contracts to skip (for pagination)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip: Option<usize>,
    /// Maximum number of contracts to return
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<usize>,
    /// Minimum topoheight filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_topoheight: Option<TopoHeight>,
    /// Maximum topoheight filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_topoheight: Option<TopoHeight>,
}

/// Parameters for get_contract_data_entries RPC method - lists contract storage entries
#[derive(Serialize, Deserialize)]
pub struct GetContractDataEntriesParams {
    /// Contract address to query
    pub contract: Hash,
    /// Maximum topoheight for version lookup
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_topoheight: Option<TopoHeight>,
    /// Number of entries to skip (for pagination)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip: Option<usize>,
    /// Maximum number of entries to return
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<usize>,
}

/// A single contract data entry (key-value pair)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDataEntry {
    /// Storage key
    pub key: ValueCell,
    /// Storage value
    pub value: ValueCell,
}

/// Parameters for key_to_address RPC method - converts public key to address
#[derive(Serialize, Deserialize)]
pub struct KeyToAddressParams {
    /// Public key in hex format
    pub key: String,
}

/// Parameters for get_block_summary_at_topoheight RPC method - lightweight block info
#[derive(Serialize, Deserialize)]
pub struct GetBlockSummaryAtTopoHeightParams {
    pub topoheight: TopoHeight,
}

/// Parameters for get_block_summary_by_hash RPC method
#[derive(Serialize, Deserialize)]
pub struct GetBlockSummaryByHashParams {
    pub hash: Hash,
}

/// Lightweight block summary response (no full transaction data)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockSummary<'a> {
    /// Block hash
    pub hash: Cow<'a, Hash>,
    /// Topological height
    pub topoheight: Option<TopoHeight>,
    /// Block height
    pub height: u64,
    /// Block timestamp
    pub timestamp: TimestampMillis,
    /// Block nonce
    pub nonce: u64,
    /// Block type (Sync, Side, Orphaned, Normal)
    pub block_type: BlockType,
    /// Miner address
    pub miner: Cow<'a, Address>,
    /// Block difficulty
    pub difficulty: Cow<'a, Difficulty>,
    /// Cumulative difficulty
    pub cumulative_difficulty: Cow<'a, CumulativeDifficulty>,
    /// Number of transactions in block
    pub txs_count: usize,
    /// Total size of block in bytes
    pub total_size_in_bytes: usize,
    /// Block reward (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward: Option<u64>,
    /// Total transaction fees
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_fees: Option<u64>,
}

/// Parameters for get_balances_at_maximum_topoheight RPC method
/// Batch query multiple asset balances for an address
#[derive(Serialize, Deserialize)]
pub struct GetBalancesAtMaximumTopoHeightParams {
    /// Address to query balances for
    pub address: Address,
    /// List of asset hashes to query
    pub assets: Vec<Hash>,
    /// Maximum topoheight for version lookup
    pub maximum_topoheight: TopoHeight,
}

/// Parameters for get_block_difficulty_by_hash RPC method
#[derive(Serialize, Deserialize)]
pub struct GetBlockDifficultyByHashParams {
    /// Block hash to query difficulty for
    pub block_hash: Hash,
}

// Note: GetDifficultyResult is already defined above and reused for get_block_difficulty_by_hash

// Note: get_block_base_fee_by_hash is not implemented in TOS
// TOS uses a different fee model. For fee estimation, use get_estimated_fee_rates.

/// Parameters for get_asset_supply_at_topoheight RPC method
#[derive(Serialize, Deserialize)]
pub struct GetAssetSupplyAtTopoHeightParams {
    /// Asset hash to query supply for
    pub asset: Hash,
    /// Topoheight to query supply at
    pub topoheight: TopoHeight,
}

// Note: get_estimated_fee_per_kb is not implemented in TOS
// TOS uses get_estimated_fee_rates for fee estimation.

/// Registered contract execution info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredExecution {
    /// Hash of the caller for the registered execution
    pub execution_hash: Hash,
    /// Topoheight at which the execution is scheduled
    pub execution_topoheight: TopoHeight,
}

// ============================================================================
// Referral System RPC Types
// ============================================================================

/// Parameters for has_referrer RPC method
#[derive(Serialize, Deserialize)]
pub struct HasReferrerParams<'a> {
    pub address: Cow<'a, Address>,
}

/// Result of has_referrer RPC method
#[derive(Serialize, Deserialize)]
pub struct HasReferrerResult {
    pub has_referrer: bool,
}

/// Parameters for get_referrer RPC method
#[derive(Serialize, Deserialize)]
pub struct GetReferrerParams<'a> {
    pub address: Cow<'a, Address>,
}

/// Result of get_referrer RPC method
#[derive(Serialize, Deserialize)]
pub struct GetReferrerResult {
    /// The referrer's address (None if no referrer)
    pub referrer: Option<Address>,
}

/// Parameters for get_uplines RPC method
#[derive(Serialize, Deserialize)]
pub struct GetUplinesParams<'a> {
    pub address: Cow<'a, Address>,
    /// Number of upline levels to retrieve (max 20)
    #[serde(default = "default_upline_levels")]
    pub levels: u8,
}

fn default_upline_levels() -> u8 {
    10
}

/// Result of get_uplines RPC method
#[derive(Serialize, Deserialize)]
pub struct GetUplinesResult {
    /// List of upline addresses (ordered from immediate referrer to higher levels)
    pub uplines: Vec<Address>,
    /// Number of levels actually returned
    pub levels_returned: u8,
}

/// Parameters for get_direct_referrals RPC method
#[derive(Serialize, Deserialize)]
pub struct GetDirectReferralsParams<'a> {
    pub address: Cow<'a, Address>,
    /// Offset for pagination
    #[serde(default)]
    pub offset: u32,
    /// Maximum number of referrals to return (default 100, max 1000)
    #[serde(default = "default_direct_referrals_limit")]
    pub limit: u32,
}

fn default_direct_referrals_limit() -> u32 {
    100
}

/// Result of get_direct_referrals RPC method
#[derive(Serialize, Deserialize)]
pub struct GetDirectReferralsResult {
    /// List of direct referral addresses
    pub referrals: Vec<Address>,
    /// Total count of direct referrals
    pub total_count: u32,
    /// Current offset
    pub offset: u32,
    /// Whether there are more results
    pub has_more: bool,
}

/// Parameters for get_referral_record RPC method
#[derive(Serialize, Deserialize)]
pub struct GetReferralRecordParams<'a> {
    pub address: Cow<'a, Address>,
}

/// Result of get_referral_record RPC method
#[derive(Serialize, Deserialize)]
pub struct GetReferralRecordResult {
    /// User's address
    pub user: Address,
    /// Referrer's address (None if no referrer)
    pub referrer: Option<Address>,
    /// Block topoheight when referrer was bound
    pub bound_at_topoheight: TopoHeight,
    /// Transaction hash of the binding transaction
    pub bound_tx_hash: Hash,
    /// Timestamp when the binding occurred
    pub bound_timestamp: u64,
    /// Number of direct referrals
    pub direct_referrals_count: u32,
    /// Cached team size
    pub team_size: u64,
}

/// Parameters for get_team_size RPC method
#[derive(Serialize, Deserialize)]
pub struct GetTeamSizeParams<'a> {
    pub address: Cow<'a, Address>,
    /// Use cached value (faster) or calculate real-time (slower but accurate)
    #[serde(default = "default_true_value")]
    pub use_cache: bool,
}

/// Result of get_team_size RPC method
#[derive(Serialize, Deserialize)]
pub struct GetTeamSizeResult {
    /// Total team size (all descendants in the referral tree)
    pub team_size: u64,
    /// Whether the value is from cache
    pub from_cache: bool,
}

/// Parameters for get_referral_level RPC method
#[derive(Serialize, Deserialize)]
pub struct GetReferralLevelParams<'a> {
    pub address: Cow<'a, Address>,
}

/// Result of get_referral_level RPC method
#[derive(Serialize, Deserialize)]
pub struct GetReferralLevelResult {
    /// Level in the referral tree (0 = root, 1 = has referrer, etc.)
    pub level: u8,
}

// ============================================================================
// KYC System RPC Types
// ============================================================================

/// Parameters for has_kyc RPC method
#[derive(Serialize, Deserialize)]
pub struct HasKycParams<'a> {
    pub address: Cow<'a, Address>,
}

/// Result of has_kyc RPC method
#[derive(Serialize, Deserialize)]
pub struct HasKycResult {
    pub has_kyc: bool,
}

/// Parameters for get_kyc RPC method
#[derive(Serialize, Deserialize)]
pub struct GetKycParams<'a> {
    pub address: Cow<'a, Address>,
}

/// KYC data returned by RPC
#[derive(Serialize, Deserialize)]
pub struct KycRpcData {
    /// KYC level bitmask
    pub level: u16,
    /// KYC tier (0-8)
    pub tier: u8,
    /// KYC status (Active, Expired, Revoked, Suspended)
    pub status: String,
    /// Verification timestamp
    pub verified_at: u64,
    /// Expiration timestamp (0 = no expiration)
    pub expires_at: u64,
    /// Days until expiration (None if no expiration)
    pub days_until_expiry: Option<u64>,
    /// Whether the KYC is currently valid
    pub is_valid: bool,
}

/// Result of get_kyc RPC method
#[derive(Serialize, Deserialize)]
pub struct GetKycResult {
    /// KYC data (None if user has no KYC)
    pub kyc: Option<KycRpcData>,
}

/// Parameters for get_kyc_batch RPC method
#[derive(Serialize, Deserialize)]
pub struct GetKycBatchParams<'a> {
    /// List of addresses to query (max 100)
    pub addresses: Cow<'a, Vec<Address>>,
}

/// Single entry in batch KYC result
#[derive(Serialize, Deserialize)]
pub struct KycBatchEntry {
    /// Address queried
    pub address: Address,
    /// KYC data (None if user has no KYC)
    pub kyc: Option<KycRpcData>,
}

/// Result of get_kyc_batch RPC method
#[derive(Serialize, Deserialize)]
pub struct GetKycBatchResult {
    /// KYC data for each address
    pub entries: Vec<KycBatchEntry>,
    /// Number of addresses with valid KYC
    pub valid_count: usize,
    /// Number of addresses with any KYC (valid or expired)
    pub kyc_count: usize,
}

/// Parameters for get_kyc_tier RPC method
#[derive(Serialize, Deserialize)]
pub struct GetKycTierParams<'a> {
    pub address: Cow<'a, Address>,
}

/// Result of get_kyc_tier RPC method
#[derive(Serialize, Deserialize)]
pub struct GetKycTierResult {
    /// Effective KYC tier (0 if invalid/expired)
    pub tier: u8,
    /// Effective KYC level (0 if invalid/expired)
    pub level: u16,
    /// Whether the KYC is currently valid
    pub is_valid: bool,
}

/// Parameters for is_kyc_valid RPC method
#[derive(Serialize, Deserialize)]
pub struct IsKycValidParams<'a> {
    pub address: Cow<'a, Address>,
}

/// Result of is_kyc_valid RPC method
#[derive(Serialize, Deserialize)]
pub struct IsKycValidResult {
    pub is_valid: bool,
}

/// Parameters for meets_kyc_level RPC method
#[derive(Serialize, Deserialize)]
pub struct MeetsKycLevelParams<'a> {
    pub address: Cow<'a, Address>,
    pub required_level: u16,
}

/// Result of meets_kyc_level RPC method
#[derive(Serialize, Deserialize)]
pub struct MeetsKycLevelResult {
    pub meets_level: bool,
}

/// Parameters for get_committee RPC method
#[derive(Serialize, Deserialize)]
pub struct GetCommitteeParams<'a> {
    pub committee_id: Cow<'a, Hash>,
}

/// Committee member data for RPC
#[derive(Serialize, Deserialize)]
pub struct CommitteeMemberRpc {
    pub public_key: Address,
    pub name: Option<String>,
    pub role: String,
    pub status: String,
    pub joined_at: u64,
    pub last_active_at: u64,
}

/// Committee data for RPC
#[derive(Serialize, Deserialize)]
pub struct CommitteeRpc {
    pub id: Hash,
    pub region: String,
    pub name: String,
    pub members: Vec<CommitteeMemberRpc>,
    pub threshold: u8,
    pub kyc_threshold: u8,
    pub max_kyc_level: u16,
    pub max_kyc_tier: u8,
    pub status: String,
    pub parent_id: Option<Hash>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Result of get_committee RPC method
#[derive(Serialize, Deserialize)]
pub struct GetCommitteeResult {
    pub committee: Option<CommitteeRpc>,
}

/// Result of get_global_committee RPC method
#[derive(Serialize, Deserialize)]
pub struct GetGlobalCommitteeResult {
    pub committee: Option<CommitteeRpc>,
    pub is_bootstrapped: bool,
}

/// Parameters for list_committees RPC method
#[derive(Serialize, Deserialize)]
pub struct ListCommitteesParams {
    /// Optional region filter (e.g., "Global", "NorthAmerica", "Europe")
    #[serde(default)]
    pub region: Option<String>,
    /// Only include active committees
    #[serde(default)]
    pub active_only: bool,
}

/// Summary information about a committee (lightweight, for listing)
#[derive(Serialize, Deserialize)]
pub struct CommitteeSummary {
    /// Committee ID
    pub id: Hash,
    /// Committee name
    pub name: String,
    /// Region (e.g., "Global", "NorthAmerica")
    pub region: String,
    /// Number of members
    pub member_count: usize,
    /// Number of active members
    pub active_member_count: usize,
    /// Governance threshold
    pub threshold: u8,
    /// KYC operation threshold
    pub kyc_threshold: u8,
    /// Maximum KYC level this committee can grant
    pub max_kyc_level: u16,
    /// Status (Active, Suspended, Dissolved)
    pub status: String,
    /// Parent committee ID (None for Global)
    pub parent_id: Option<Hash>,
    /// Whether this is the global committee
    pub is_global: bool,
    /// Created timestamp
    pub created_at: u64,
}

/// Result of list_committees RPC method
#[derive(Serialize, Deserialize)]
pub struct ListCommitteesResult {
    /// List of committee summaries
    pub committees: Vec<CommitteeSummary>,
    /// Total number of committees
    pub total_count: usize,
    /// Number of active committees
    pub active_count: usize,
}

/// Parameters for get_committees_by_region RPC method
#[derive(Serialize, Deserialize)]
pub struct GetCommitteesByRegionParams {
    pub region: String,
}

/// Result of get_committees_by_region RPC method
#[derive(Serialize, Deserialize)]
pub struct GetCommitteesByRegionResult {
    pub committees: Vec<CommitteeRpc>,
}

/// Parameters for get_verifying_committee RPC method
#[derive(Serialize, Deserialize)]
pub struct GetVerifyingCommitteeParams<'a> {
    pub address: Cow<'a, Address>,
}

/// Result of get_verifying_committee RPC method
#[derive(Serialize, Deserialize)]
pub struct GetVerifyingCommitteeResult {
    pub committee_id: Option<Hash>,
}

// ============================================================================
// Admin RPC Types (require --enable-admin-rpc flag)
// ============================================================================

/// Parameters for prune_chain RPC method
#[derive(Serialize, Deserialize)]
pub struct PruneChainParams {
    /// Topoheight to prune the chain to
    pub topoheight: TopoHeight,
}

/// Result of prune_chain RPC method
#[derive(Serialize, Deserialize)]
pub struct PruneChainResult {
    /// New pruned topoheight
    pub pruned_topoheight: TopoHeight,
}

/// Parameters for rewind_chain RPC method
#[derive(Serialize, Deserialize)]
pub struct RewindChainParams {
    /// Number of blocks to rewind
    pub count: u64,
    /// Should it stop at stable height
    #[serde(default)]
    pub until_stable_height: bool,
}

/// Result of rewind_chain RPC method
#[derive(Serialize, Deserialize)]
pub struct RewindChainResult {
    /// New topoheight after rewind
    pub topoheight: TopoHeight,
    /// All transactions that were removed from the chain
    pub txs: Vec<Hash>,
}
