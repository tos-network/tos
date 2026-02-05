use serde::Deserialize;
use std::collections::BTreeMap;
use tos_common::crypto::Hash;

/// Root structure for genesis state JSON file
#[derive(Debug, Deserialize)]
pub struct GenesisState {
    /// Format version - must be 1
    pub format_version: u32,

    /// Chain configuration
    pub config: GenesisConfig,

    /// Asset definitions (TOS and UNO required)
    pub assets: BTreeMap<String, AssetConfig>,

    /// Account allocations
    pub alloc: Vec<AllocEntry>,

    /// Computed values (optional, for verification)
    pub computed: Option<ComputedValues>,
}

/// Chain configuration section
#[derive(Debug, Deserialize)]
pub struct GenesisConfig {
    /// Chain ID (parsed to u64)
    pub chain_id: String,

    /// Network type: "mainnet", "testnet", or "devnet"
    pub network: String,

    /// Genesis timestamp in milliseconds (parsed to u64)
    pub genesis_timestamp_ms: String,

    /// Development public key (64 hex chars)
    pub dev_public_key: String,

    /// Fork activation heights
    #[serde(default)]
    pub forks: BTreeMap<String, String>,
}

/// Asset configuration
#[derive(Debug, Deserialize)]
pub struct AssetConfig {
    /// Number of decimal places
    pub decimals: u8,

    /// Full asset name
    pub name: String,

    /// Asset ticker symbol
    pub ticker: String,

    /// Maximum supply (optional, as string to parse to u64)
    pub max_supply: Option<String>,
}

/// Individual account allocation entry
#[derive(Debug, Deserialize)]
pub struct AllocEntry {
    /// Public key (64 hex chars) - authoritative source
    pub public_key: String,

    /// Optional bech32 address (for verification)
    pub address: Option<String>,

    /// Account nonce (parsed to u64)
    pub nonce: String,

    /// TOS balance (parsed to u64)
    pub balance: String,

    /// Energy configuration (default: { available: "0" })
    #[serde(default)]
    pub energy: Option<EnergyConfig>,
}

/// Energy configuration for an account
#[derive(Debug, Deserialize, Default)]
pub struct EnergyConfig {
    /// Available energy (parsed to u64)
    pub available: String,
}

/// Pre-computed values for verification
#[derive(Debug, Deserialize)]
pub struct ComputedValues {
    /// State hash for integrity verification
    pub state_hash: Option<Hash>,

    /// Total supply allocated
    pub total_supply: Option<String>,

    /// Number of accounts
    pub account_count: Option<u32>,
}

/// Parsed allocation entry with validated types
#[derive(Debug, Clone)]
pub struct ParsedAllocEntry {
    /// Validated public key
    pub public_key: tos_common::crypto::PublicKey,

    /// Validated nonce
    pub nonce: u64,

    /// Validated TOS balance (atomic units)
    pub balance: u64,

    /// Validated available energy
    pub energy_available: u64,
}
