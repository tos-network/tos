// Mock storage provider with HashMap backend for DeFi integration tests
//
// This module provides a MockProvider that implements actual storage
// using HashMap, enabling full stateful testing of smart contracts.

#![allow(clippy::disallowed_methods)]

use anyhow::Result;
use std::collections::HashMap;
use std::sync::RwLock;
use tos_common::{
    asset::AssetData,
    block::TopoHeight,
    contract::{ContractProvider, ContractStorage},
    crypto::{Hash, PublicKey},
};
use tos_kernel::ValueCell;

/// Mock provider with actual HashMap storage
pub struct MockProvider {
    /// In-memory storage: key → value
    storage: RwLock<HashMap<Vec<u8>, Vec<u8>>>,
}

impl MockProvider {
    /// Create a new MockProvider with empty storage
    pub fn new() -> Self {
        Self {
            storage: RwLock::new(HashMap::new()),
        }
    }

    /// Clear all storage (useful for test isolation)
    #[allow(dead_code)]
    pub fn clear(&self) {
        self.storage.write().unwrap().clear();
    }

    /// Get number of stored keys (useful for debugging)
    #[allow(dead_code)]
    pub fn storage_size(&self) -> usize {
        self.storage.read().unwrap().len()
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ContractProvider for MockProvider {
    fn get_contract_balance_for_asset(
        &self,
        _contract: &Hash,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        // Mock: all contracts have 1M balance for any asset
        Ok(Some((100, 1_000_000)))
    }

    fn get_account_balance_for_asset(
        &self,
        _key: &PublicKey,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        // Mock: all accounts have 1M balance for any asset
        Ok(Some((100, 1_000_000)))
    }

    fn asset_exists(&self, _asset: &Hash, _topoheight: TopoHeight) -> Result<bool> {
        // Mock: all assets exist
        Ok(true)
    }

    fn load_asset_data(
        &self,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, AssetData)>> {
        // Mock: no asset data available
        Ok(None)
    }

    fn load_asset_supply(
        &self,
        _asset: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>> {
        // Mock: no supply data available
        Ok(None)
    }

    fn account_exists(&self, _key: &PublicKey, _topoheight: TopoHeight) -> Result<bool> {
        // Mock: all accounts exist
        Ok(true)
    }

    fn load_contract_module(
        &self,
        _contract: &Hash,
        _topoheight: TopoHeight,
    ) -> Result<Option<Vec<u8>>> {
        // Mock: no contract modules available (not needed for these tests)
        Ok(None)
    }
}

impl ContractStorage for MockProvider {
    /// Load data from storage
    ///
    /// This is the critical method that enables stateful contracts.
    /// Returns Some((topoheight, Some(value))) if key exists,
    /// Some((topoheight, None)) if key was explicitly deleted,
    /// None if key never existed.
    fn load_data(
        &self,
        _contract: &Hash,
        key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, Option<ValueCell>)>> {
        let key_bytes = if let ValueCell::Bytes(bytes) = key {
            bytes
        } else {
            return Ok(None);
        };

        let storage = self.storage.read().unwrap();

        match storage.get(key_bytes.as_slice()) {
            Some(value) => {
                // Key exists with data
                let value_cell = ValueCell::Bytes(value.clone());
                Ok(Some((100, Some(value_cell))))
            }
            None => {
                // Key doesn't exist
                // Return None to indicate key has never been set
                // (contract will handle this as "not found")
                Ok(None)
            }
        }
    }

    /// Get the latest topoheight for a key
    ///
    /// For mock, we always return topoheight 100 if key exists
    fn load_data_latest_topoheight(
        &self,
        _contract: &Hash,
        key: &ValueCell,
        _topoheight: TopoHeight,
    ) -> Result<Option<TopoHeight>> {
        let key_bytes = if let ValueCell::Bytes(bytes) = key {
            bytes
        } else {
            return Ok(None);
        };

        let storage = self.storage.read().unwrap();

        if storage.contains_key(key_bytes.as_slice()) {
            Ok(Some(100))
        } else {
            Ok(None)
        }
    }

    /// Check if key has data
    fn has_data(&self, _contract: &Hash, key: &ValueCell, _topoheight: TopoHeight) -> Result<bool> {
        let key_bytes = if let ValueCell::Bytes(bytes) = key {
            bytes
        } else {
            return Ok(false);
        };

        let storage = self.storage.read().unwrap();
        Ok(storage.contains_key(key_bytes.as_slice()))
    }

    /// Check if contract exists (always true for mock)
    fn has_contract(&self, _contract: &Hash, _topoheight: TopoHeight) -> Result<bool> {
        Ok(true)
    }
}

/// Storage write operations
///
/// These methods are called by the TAKO VM through syscalls.
/// They need to be implemented on MockProvider but are accessed
/// through the VM's storage syscall interface.
impl MockProvider {
    /// Write data to storage
    ///
    /// This simulates the storage_write syscall behavior.
    /// Called internally by TAKO VM when contract calls storage_write.
    pub fn write_storage(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut storage = self.storage.write().unwrap();
        storage.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    /// Delete data from storage
    ///
    /// This simulates the storage_delete syscall behavior.
    pub fn delete_storage(&self, key: &[u8]) -> Result<()> {
        let mut storage = self.storage.write().unwrap();
        storage.remove(key);
        Ok(())
    }

    /// Read data from storage (alternative interface)
    ///
    /// Returns None if key doesn't exist, Some(value) if it does.
    pub fn read_storage(&self, key: &[u8]) -> Option<Vec<u8>> {
        let storage = self.storage.read().unwrap();
        storage.get(key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_storage_basic() {
        let provider = MockProvider::new();

        // Initially empty
        assert_eq!(provider.storage_size(), 0);

        // Write some data
        let key = b"test_key";
        let value = b"test_value";
        provider.write_storage(key, value).unwrap();

        // Verify written
        assert_eq!(provider.storage_size(), 1);
        let read_value = provider.read_storage(key).unwrap();
        assert_eq!(&read_value, value);

        // Delete data
        provider.delete_storage(key).unwrap();
        assert_eq!(provider.storage_size(), 0);
        assert!(provider.read_storage(key).is_none());
    }

    #[test]
    fn test_mock_storage_contract_storage_trait() {
        let provider = MockProvider::new();

        // Write via direct interface
        let key = b"my_key";
        let value = b"my_value";
        provider.write_storage(key, value).unwrap();

        // Read via ContractStorage trait
        let key_cell = ValueCell::Bytes(key.to_vec());
        let result = provider.load_data(&Hash::zero(), &key_cell, 100).unwrap();

        assert!(result.is_some());
        let (topoheight, value_opt) = result.unwrap();
        assert_eq!(topoheight, 100);
        assert!(value_opt.is_some());
        if let ValueCell::Bytes(bytes) = value_opt.unwrap() {
            assert_eq!(bytes.as_slice(), value);
        } else {
            panic!("Expected ValueCell::Bytes");
        }
    }

    #[test]
    fn test_mock_storage_has_data() {
        let provider = MockProvider::new();

        let key = b"test";
        let key_cell = ValueCell::Bytes(key.to_vec());

        // Initially doesn't exist
        assert!(!provider.has_data(&Hash::zero(), &key_cell, 100).unwrap());

        // Write data
        provider.write_storage(key, b"data").unwrap();

        // Now exists
        assert!(provider.has_data(&Hash::zero(), &key_cell, 100).unwrap());
    }
}
