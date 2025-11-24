//! Doc-test helper utilities for tos_common
//!
//! This module provides mock implementations and test fixtures for use in
//! documentation examples. It is only compiled when running tests or doc-tests.

#![cfg(any(test, doctest))]

use crate::contract::{ContractCache, ContractExecutionResult, ContractExecutor, ContractProvider, TransferOutput};
use crate::crypto::Hash;
use crate::block::TopoHeight;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

/// Mock contract provider for doc-tests
///
/// Provides in-memory storage for contracts and their state,
/// suitable for simple documentation examples.
pub struct MockContractProvider {
    contracts: HashMap<Hash, Vec<u8>>,
    storage: HashMap<(Hash, Vec<u8>), Vec<u8>>,
    modules: HashMap<Hash, Vec<u8>>,
}

impl MockContractProvider {
    /// Create a new empty mock provider
    pub fn new() -> Self {
        Self {
            contracts: HashMap::new(),
            storage: HashMap::new(),
            modules: HashMap::new(),
        }
    }

    /// Add a contract with the given hash and bytecode
    pub fn with_contract(mut self, hash: Hash, bytecode: Vec<u8>) -> Self {
        self.contracts.insert(hash, bytecode.clone());
        self.modules.insert(hash, bytecode);
        self
    }

    /// Set a storage value for a contract
    pub fn with_storage(mut self, contract: Hash, key: Vec<u8>, value: Vec<u8>) -> Self {
        self.storage.insert((contract, key), value);
        self
    }
}

impl Default for MockContractProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ContractProvider for MockContractProvider {
    fn load_contract(&self, hash: &Hash, _topoheight: TopoHeight) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.contracts.get(hash).cloned())
    }

    fn load_contract_module(&self, hash: &Hash, _topoheight: TopoHeight) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.modules.get(hash).cloned())
    }

    fn get_contract_storage(
        &self,
        contract: &Hash,
        key: &[u8],
        _topoheight: TopoHeight,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.storage.get(&(*contract, key.to_vec())).cloned())
    }

    fn set_contract_storage(
        &mut self,
        contract: &Hash,
        key: &[u8],
        value: &[u8],
        _topoheight: TopoHeight,
    ) -> anyhow::Result<()> {
        self.storage.insert((*contract, key.to_vec()), value.to_vec());
        Ok(())
    }

    fn delete_contract_storage(
        &mut self,
        contract: &Hash,
        key: &[u8],
        _topoheight: TopoHeight,
    ) -> anyhow::Result<()> {
        self.storage.remove(&(*contract, key.to_vec()));
        Ok(())
    }

    fn flush_contract_storage(&mut self, _topoheight: TopoHeight) -> anyhow::Result<()> {
        // No-op for in-memory storage
        Ok(())
    }

    fn flush_contracts(&mut self, _topoheight: TopoHeight) -> anyhow::Result<()> {
        // No-op for in-memory storage
        Ok(())
    }
}

/// Mock contract executor for doc-tests
///
/// Simulates contract execution without actually running bytecode.
pub struct MockContractExecutor {
    /// Default gas used for mock executions
    pub default_gas: u64,
    /// Default exit code (0 = success)
    pub default_exit_code: u64,
}

impl MockContractExecutor {
    /// Create a new mock executor with default values
    pub fn new() -> Self {
        Self {
            default_gas: 1000,
            default_exit_code: 0,
        }
    }

    /// Set the default gas consumption
    pub fn with_gas(mut self, gas: u64) -> Self {
        self.default_gas = gas;
        self
    }

    /// Set the default exit code
    pub fn with_exit_code(mut self, code: u64) -> Self {
        self.default_exit_code = code;
        self
    }
}

impl Default for MockContractExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContractExecutor for MockContractExecutor {
    async fn execute<P: ContractProvider + Send>(
        &self,
        _bytecode: &[u8],
        _provider: &mut P,
        _topoheight: TopoHeight,
        _contract_hash: &Hash,
        _block_hash: &Hash,
        _block_height: u64,
        _tx_hash: &Hash,
        _tx_sender: &Hash,
        _input_data: &[u8],
        _compute_budget: u64,
    ) -> anyhow::Result<ContractExecutionResult> {
        Ok(ContractExecutionResult {
            gas_used: self.default_gas,
            exit_code: Some(self.default_exit_code),
            return_data: None,
            transfers: vec![],
        })
    }
}

/// Create a mock contract cache for doc-tests
pub fn create_mock_cache() -> Arc<RwLock<ContractCache>> {
    Arc::new(RwLock::new(ContractCache::new(1000)))
}

/// Generate a test hash from a simple seed
pub fn test_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

/// Create minimal ELF bytecode for testing
///
/// Returns a minimal valid ELF header that can be used in doc-tests.
pub fn minimal_elf_bytecode() -> Vec<u8> {
    vec![0x7F, b'E', b'L', b'F']
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_provider() {
        let hash = test_hash(1);
        let bytecode = vec![1, 2, 3];

        let provider = MockContractProvider::new()
            .with_contract(hash, bytecode.clone());

        let loaded = provider.load_contract(&hash, 0).unwrap();
        assert_eq!(loaded, Some(bytecode));
    }

    #[tokio::test]
    async fn test_mock_executor() {
        let executor = MockContractExecutor::new()
            .with_gas(5000)
            .with_exit_code(0);

        let mut provider = MockContractProvider::new();
        let result = executor.execute(
            &[],
            &mut provider,
            0,
            &test_hash(1),
            &test_hash(2),
            100,
            &test_hash(3),
            &test_hash(4),
            &[],
            10000,
        ).await.unwrap();

        assert_eq!(result.gas_used, 5000);
        assert_eq!(result.exit_code, Some(0));
    }
}
