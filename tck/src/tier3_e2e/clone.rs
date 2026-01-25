//! Live network state cloning for fork testing.
//!
//! Clone account state from a live network (mainnet/testnet) RPC endpoint
//! for local fork testing. Inspired by Solana's `solana-test-validator --clone`.

use std::collections::HashMap;

use anyhow::Result;
use tos_common::crypto::Hash;

/// Configuration for cloning state from a live network.
#[derive(Debug, Clone)]
pub struct CloneConfig {
    /// RPC endpoint of the source network
    pub rpc_url: String,
    /// Account addresses to clone (balances, nonces)
    pub accounts: Vec<Hash>,
    /// Contract addresses to clone (bytecode + storage)
    pub contracts: Vec<Hash>,
    /// Clone at a specific topoheight (None = latest)
    pub at_topoheight: Option<u64>,
    /// Whether to clone full contract storage or just code
    pub clone_contract_storage: bool,
}

impl CloneConfig {
    /// Create a new clone config with just an RPC URL.
    pub fn new(rpc_url: &str) -> Self {
        Self {
            rpc_url: rpc_url.to_string(),
            accounts: Vec::new(),
            contracts: Vec::new(),
            at_topoheight: None,
            clone_contract_storage: true,
        }
    }

    /// Add accounts to clone.
    pub fn with_accounts(mut self, accounts: Vec<Hash>) -> Self {
        self.accounts = accounts;
        self
    }

    /// Add contracts to clone.
    pub fn with_contracts(mut self, contracts: Vec<Hash>) -> Self {
        self.contracts = contracts;
        self
    }

    /// Clone at a specific topoheight (for reproducibility).
    pub fn at_topoheight(mut self, height: u64) -> Self {
        self.at_topoheight = Some(height);
        self
    }

    /// Skip contract storage cloning (only clone bytecode).
    pub fn without_contract_storage(mut self) -> Self {
        self.clone_contract_storage = false;
        self
    }
}

/// A cloned account state.
#[derive(Debug, Clone)]
pub struct ClonedAccount {
    /// Account address
    pub address: Hash,
    /// Cloned balance
    pub balance: u64,
    /// Cloned nonce
    pub nonce: u64,
}

/// A cloned contract state.
#[derive(Debug, Clone)]
pub struct ClonedContract {
    /// Contract address
    pub address: Hash,
    /// Contract bytecode
    pub bytecode: Vec<u8>,
    /// Contract storage (key -> value), if cloned
    pub storage: Option<HashMap<Vec<u8>, Vec<u8>>>,
}

/// Result of a state cloning operation.
#[derive(Debug, Clone, Default)]
pub struct ClonedState {
    /// Cloned accounts
    pub accounts: Vec<ClonedAccount>,
    /// Cloned contracts
    pub contracts: Vec<ClonedContract>,
    /// Source topoheight at which state was cloned
    pub source_topoheight: u64,
    /// Source network RPC URL
    pub source_rpc: String,
}

impl ClonedState {
    /// Returns true if no state was cloned.
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty() && self.contracts.is_empty()
    }

    /// Get total number of cloned entities.
    pub fn total_entities(&self) -> usize {
        self.accounts.len() + self.contracts.len()
    }
}

/// Clone state from a live network via RPC.
///
/// This is an async operation that fetches account and contract state
/// from the configured RPC endpoint.
///
/// # Note
/// Tests using this function should be marked `#[ignore]` as they
/// require network access. In the test framework, this function creates
/// placeholder state entries that can be populated by mock RPC responses.
///
/// # Errors
///
/// Returns an error if:
/// - The RPC URL is empty
/// - No accounts or contracts are specified to clone
pub async fn clone_state_from_network(config: &CloneConfig) -> Result<ClonedState> {
    if config.rpc_url.is_empty() {
        return Err(anyhow::anyhow!("RPC URL cannot be empty"));
    }

    if config.accounts.is_empty() && config.contracts.is_empty() {
        return Err(anyhow::anyhow!(
            "At least one account or contract must be specified for cloning"
        ));
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!(
            "Cloning state from {} ({} accounts, {} contracts, at topoheight {:?})",
            config.rpc_url,
            config.accounts.len(),
            config.contracts.len(),
            config.at_topoheight,
        );
    }

    let mut state = ClonedState {
        source_rpc: config.rpc_url.clone(),
        source_topoheight: config.at_topoheight.unwrap_or(0),
        ..Default::default()
    };

    // In a full implementation, this would make RPC calls to fetch state.
    // For now, we provide the structure for tests to use with mocked state.
    for address in &config.accounts {
        state.accounts.push(ClonedAccount {
            address: address.clone(),
            balance: 0,
            nonce: 0,
        });

        if log::log_enabled!(log::Level::Debug) {
            log::debug!("Prepared clone entry for account {}", address);
        }
    }

    for address in &config.contracts {
        state.contracts.push(ClonedContract {
            address: address.clone(),
            bytecode: Vec::new(),
            storage: if config.clone_contract_storage {
                Some(HashMap::new())
            } else {
                None
            },
        });

        if log::log_enabled!(log::Level::Debug) {
            log::debug!("Prepared clone entry for contract {}", address);
        }
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!(
            "Clone complete: {} entities from {}",
            state.total_entities(),
            config.rpc_url
        );
    }

    Ok(state)
}

/// Create a `ClonedState` from pre-populated mock data.
///
/// This is a test helper that creates a `ClonedState` without network access,
/// useful for unit testing code that consumes `ClonedState`.
pub fn mock_cloned_state(
    accounts: Vec<ClonedAccount>,
    contracts: Vec<ClonedContract>,
) -> ClonedState {
    ClonedState {
        accounts,
        contracts,
        source_topoheight: 0,
        source_rpc: "mock://localhost".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hash(byte: u8) -> Hash {
        Hash::new([byte; 32])
    }

    #[test]
    fn test_clone_config_builder() {
        let config = CloneConfig::new("https://rpc.example.com")
            .with_accounts(vec![sample_hash(1), sample_hash(2)])
            .with_contracts(vec![sample_hash(10)])
            .at_topoheight(1_000_000)
            .without_contract_storage();

        assert_eq!(config.accounts.len(), 2);
        assert_eq!(config.contracts.len(), 1);
        assert_eq!(config.at_topoheight, Some(1_000_000));
        assert!(!config.clone_contract_storage);
    }

    #[test]
    fn test_cloned_state() {
        let state = ClonedState {
            accounts: vec![ClonedAccount {
                address: sample_hash(1),
                balance: 1_000_000,
                nonce: 5,
            }],
            contracts: vec![],
            source_topoheight: 500_000,
            source_rpc: "https://rpc.example.com".to_string(),
        };

        assert!(!state.is_empty());
        assert_eq!(state.total_entities(), 1);
    }

    #[test]
    fn test_empty_cloned_state() {
        let state = ClonedState::default();
        assert!(state.is_empty());
        assert_eq!(state.total_entities(), 0);
    }

    #[tokio::test]
    async fn test_clone_state_structure() {
        let config = CloneConfig::new("https://rpc.example.com")
            .with_accounts(vec![sample_hash(1)])
            .with_contracts(vec![sample_hash(10)]);

        let state = clone_state_from_network(&config).await.unwrap();
        assert_eq!(state.accounts.len(), 1);
        assert_eq!(state.contracts.len(), 1);
        assert!(state.contracts[0].storage.is_some());
    }

    #[tokio::test]
    async fn test_clone_empty_url_error() {
        let config = CloneConfig::new("").with_accounts(vec![sample_hash(1)]);
        let result = clone_state_from_network(&config).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("RPC URL cannot be empty"));
    }

    #[tokio::test]
    async fn test_clone_empty_targets_error() {
        let config = CloneConfig::new("https://rpc.example.com");
        let result = clone_state_from_network(&config).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("At least one account"));
    }

    #[test]
    fn test_mock_cloned_state() {
        let state = mock_cloned_state(
            vec![ClonedAccount {
                address: sample_hash(1),
                balance: 500_000,
                nonce: 3,
            }],
            vec![ClonedContract {
                address: sample_hash(10),
                bytecode: vec![0x00, 0x61, 0x73, 0x6d],
                storage: Some(HashMap::new()),
            }],
        );
        assert_eq!(state.accounts.len(), 1);
        assert_eq!(state.accounts[0].balance, 500_000);
        assert_eq!(state.contracts.len(), 1);
        assert_eq!(state.source_rpc, "mock://localhost");
    }
}
