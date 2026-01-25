//! Configuration types for ChainClient.
//!
//! Defines how a ChainClient test environment is initialized, including
//! genesis accounts, deployed contracts, clock mode, feature gates, and
//! auto-mine behavior.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tos_common::crypto::Hash;

use crate::orchestrator::Clock;
use crate::tier1_5::features::FeatureSet;

/// Auto-mine configuration for background block production.
#[derive(Debug, Clone, Default)]
pub enum AutoMineConfig {
    /// No auto-mining: blocks are only created explicitly
    #[default]
    Disabled,
    /// Mine a block at regular intervals
    Interval(Duration),
    /// Mine a block whenever a transaction is submitted
    OnTransaction,
}

/// A pre-funded account for genesis state.
#[derive(Debug, Clone)]
pub struct GenesisAccount {
    /// Account address (hash of public key)
    pub address: Hash,
    /// Initial native TOS balance
    pub balance: u64,
    /// Initial nonce (usually 0)
    pub nonce: u64,
    /// Optional keypair bytes for signing transactions in tests
    pub keypair: Option<Vec<u8>>,
}

impl GenesisAccount {
    /// Create a genesis account with just an address and balance.
    pub fn new(address: Hash, balance: u64) -> Self {
        Self {
            address,
            balance,
            nonce: 0,
            keypair: None,
        }
    }

    /// Set the keypair bytes for transaction signing.
    pub fn with_keypair(mut self, keypair: Vec<u8>) -> Self {
        self.keypair = Some(keypair);
        self
    }

    /// Set the initial nonce.
    pub fn with_nonce(mut self, nonce: u64) -> Self {
        self.nonce = nonce;
        self
    }
}

/// A contract to deploy at genesis.
#[derive(Debug, Clone)]
pub struct GenesisContract {
    /// Contract hash (address)
    pub address: Hash,
    /// WASM/TAKO bytecode
    pub bytecode: Vec<u8>,
    /// Initial storage entries (key -> value)
    pub storage: Vec<(Vec<u8>, Vec<u8>)>,
    /// Owner account
    pub owner: Hash,
}

/// Storage backend for the ChainClient.
#[derive(Debug, Clone, Default)]
pub enum StorageBackend {
    /// In-memory storage (fastest, default for tests)
    #[default]
    Memory,
    /// Temporary directory-backed storage (realistic but ephemeral)
    TempDir,
    /// RocksDB storage at a specified path (most realistic, persistent)
    RocksDB(PathBuf),
}

/// VRF configuration for block production.
#[derive(Debug, Clone, Default)]
pub struct VrfConfig {
    /// Hex-encoded VRF secret key (64 hex chars = 32 bytes)
    /// If None, VRF is disabled and blocks have no VRF data
    pub secret_key_hex: Option<String>,
    /// Chain ID for VRF input domain separation
    pub chain_id: u64,
}

/// Fee configuration for the test environment.
#[derive(Debug, Clone)]
pub struct FeeConfig {
    /// Gas price per unit
    pub gas_price: u64,
    /// Burn percentage (0-100)
    pub burn_percent: u64,
    /// Whether to enforce fees (can disable for simpler tests)
    pub enforce_fees: bool,
}

impl Default for FeeConfig {
    fn default() -> Self {
        Self {
            gas_price: 1,
            burn_percent: 0,
            enforce_fees: true,
        }
    }
}

/// Configuration for initializing a ChainClient test environment.
///
/// # Example
/// ```ignore
/// let config = ChainClientConfig::default()
///     .with_account(GenesisAccount::new(alice_addr, 1_000_000))
///     .with_features(FeatureSet::mainnet().deactivate("fee_model_v2"));
/// let client = ChainClient::start(config).await?;
/// ```
#[derive(Clone)]
pub struct ChainClientConfig {
    /// Pre-funded genesis accounts
    pub genesis_accounts: Vec<GenesisAccount>,
    /// Pre-deployed contracts
    pub genesis_contracts: Vec<GenesisContract>,
    /// Feature gate configuration
    pub features: FeatureSet,
    /// Clock for time control (None = use PausedClock)
    pub clock: Option<Arc<dyn Clock>>,
    /// Auto-mine configuration
    pub auto_mine: AutoMineConfig,
    /// Maximum gas per block
    pub max_gas_per_block: u64,
    /// Maximum gas per transaction
    pub max_gas_per_tx: u64,
    /// Whether to record state diffs for simulation
    pub track_state_diffs: bool,
    /// Network type (mainnet vs testnet prefix for addresses)
    pub mainnet: bool,
    /// Storage backend configuration
    pub storage: StorageBackend,
    /// Fee model configuration
    pub fee_config: FeeConfig,
    /// Block time target in milliseconds (used for auto-mine interval timing)
    pub block_time_ms: u64,
    /// VRF configuration for block production
    pub vrf: VrfConfig,
    /// Miner address for reward tracking (scheduled execution payouts)
    pub miner_address: Option<Hash>,
}

impl Default for ChainClientConfig {
    fn default() -> Self {
        Self {
            genesis_accounts: Vec::new(),
            genesis_contracts: Vec::new(),
            features: FeatureSet::mainnet(),
            clock: None,
            auto_mine: AutoMineConfig::Disabled,
            max_gas_per_block: 10_000_000,
            max_gas_per_tx: 5_000_000,
            track_state_diffs: false,
            mainnet: false,
            storage: StorageBackend::default(),
            fee_config: FeeConfig::default(),
            block_time_ms: 500,
            vrf: VrfConfig::default(),
            miner_address: None,
        }
    }
}

impl ChainClientConfig {
    /// Create a minimal config with no accounts or contracts.
    pub fn minimal() -> Self {
        Self::default()
    }

    /// Add a genesis account.
    pub fn with_account(mut self, account: GenesisAccount) -> Self {
        self.genesis_accounts.push(account);
        self
    }

    /// Add multiple genesis accounts.
    pub fn with_accounts(mut self, accounts: Vec<GenesisAccount>) -> Self {
        self.genesis_accounts.extend(accounts);
        self
    }

    /// Add a genesis contract.
    pub fn with_contract(mut self, contract: GenesisContract) -> Self {
        self.genesis_contracts.push(contract);
        self
    }

    /// Set the feature gate configuration.
    pub fn with_features(mut self, features: FeatureSet) -> Self {
        self.features = features;
        self
    }

    /// Set the clock for time control.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Set the auto-mine configuration.
    pub fn with_auto_mine(mut self, config: AutoMineConfig) -> Self {
        self.auto_mine = config;
        self
    }

    /// Enable state diff tracking for simulations.
    pub fn with_state_diff_tracking(mut self) -> Self {
        self.track_state_diffs = true;
        self
    }

    /// Set maximum gas per block.
    pub fn with_max_gas_per_block(mut self, max: u64) -> Self {
        self.max_gas_per_block = max;
        self
    }

    /// Set maximum gas per transaction.
    pub fn with_max_gas_per_tx(mut self, max: u64) -> Self {
        self.max_gas_per_tx = max;
        self
    }

    /// Use mainnet address format.
    pub fn with_mainnet(mut self) -> Self {
        self.mainnet = true;
        self
    }

    /// Set the storage backend.
    pub fn with_storage(mut self, storage: StorageBackend) -> Self {
        self.storage = storage;
        self
    }

    /// Set the fee configuration.
    pub fn with_fee_config(mut self, fee_config: FeeConfig) -> Self {
        self.fee_config = fee_config;
        self
    }

    /// Disable fee enforcement for simpler tests.
    pub fn with_fees_disabled(mut self) -> Self {
        self.fee_config.enforce_fees = false;
        self
    }

    /// Set the block time target in milliseconds.
    pub fn with_block_time_ms(mut self, ms: u64) -> Self {
        self.block_time_ms = ms;
        self
    }

    /// Set the VRF configuration.
    pub fn with_vrf(mut self, vrf: VrfConfig) -> Self {
        self.vrf = vrf;
        self
    }

    /// Set the miner address for reward tracking.
    pub fn with_miner(mut self, address: Hash) -> Self {
        self.miner_address = Some(address);
        self
    }
}

impl std::fmt::Debug for ChainClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChainClientConfig")
            .field("genesis_accounts", &self.genesis_accounts.len())
            .field("genesis_contracts", &self.genesis_contracts.len())
            .field("features", &self.features)
            .field("auto_mine", &self.auto_mine)
            .field("max_gas_per_block", &self.max_gas_per_block)
            .field("max_gas_per_tx", &self.max_gas_per_tx)
            .field("track_state_diffs", &self.track_state_diffs)
            .field("mainnet", &self.mainnet)
            .field("storage", &self.storage)
            .field("fee_config", &self.fee_config)
            .field("block_time_ms", &self.block_time_ms)
            .field("vrf", &self.vrf)
            .field("miner_address", &self.miner_address)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hash(byte: u8) -> Hash {
        Hash::new([byte; 32])
    }

    #[test]
    fn test_default_config() {
        let config = ChainClientConfig::default();
        assert!(config.genesis_accounts.is_empty());
        assert!(config.genesis_contracts.is_empty());
        assert!(matches!(config.auto_mine, AutoMineConfig::Disabled));
        assert_eq!(config.max_gas_per_block, 10_000_000);
        assert_eq!(config.max_gas_per_tx, 5_000_000);
        assert!(!config.track_state_diffs);
        assert!(!config.mainnet);
    }

    #[test]
    fn test_builder_pattern() {
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(sample_hash(1), 1_000_000))
            .with_account(GenesisAccount::new(sample_hash(2), 500_000))
            .with_features(FeatureSet::empty())
            .with_auto_mine(AutoMineConfig::OnTransaction)
            .with_state_diff_tracking()
            .with_max_gas_per_block(20_000_000);

        assert_eq!(config.genesis_accounts.len(), 2);
        assert_eq!(config.genesis_accounts[0].balance, 1_000_000);
        assert!(matches!(config.auto_mine, AutoMineConfig::OnTransaction));
        assert!(config.track_state_diffs);
        assert_eq!(config.max_gas_per_block, 20_000_000);
    }

    #[test]
    fn test_genesis_account_with_nonce() {
        let account = GenesisAccount::new(sample_hash(1), 1000).with_nonce(5);
        assert_eq!(account.balance, 1000);
        assert_eq!(account.nonce, 5);
    }

    #[test]
    fn test_genesis_contract() {
        let contract = GenesisContract {
            address: sample_hash(10),
            bytecode: vec![0x00, 0x61, 0x73, 0x6d], // WASM magic
            storage: vec![(b"key1".to_vec(), b"value1".to_vec())],
            owner: sample_hash(1),
        };
        assert_eq!(contract.storage.len(), 1);
    }

    #[test]
    fn test_auto_mine_config() {
        assert!(matches!(
            AutoMineConfig::default(),
            AutoMineConfig::Disabled
        ));

        let interval = AutoMineConfig::Interval(Duration::from_secs(1));
        assert!(matches!(interval, AutoMineConfig::Interval(_)));
    }
}
