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
/// require network access.
pub async fn clone_state_from_network(config: &CloneConfig) -> Result<ClonedState> {
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
    }

    Ok(state)
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
}
