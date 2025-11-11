use tos_common::{
    block::TopoHeight,
    contract::{ContractCache, ContractProvider},
    crypto::Hash,
    versioned_type::VersionedState,
};
use tos_kernel::ValueCell;
/// Storage adapter: TOS ContractProvider → TAKO StorageProvider
///
/// This module bridges TOS's contract storage system with TOS Kernel(TAKO)'s storage syscalls.
/// It translates between TOS's versioned storage model and TAKO's simple key-value interface.
use tos_program_runtime::storage::StorageProvider;
use tos_tbpf::error::EbpfError;

/// Adapter that wraps TOS's ContractProvider to implement TAKO's StorageProvider
///
/// # Architecture
///
/// ```text
/// TAKO syscall tos_storage_read(key)
///     ↓
/// TosStorageAdapter::get()
///     ↓
/// Check cache first (in-memory)
///     ↓
/// If cache miss: TOS ContractProvider::load_data()
///     ↓
/// RocksDB ContractsData column family
/// ```
///
/// # Why This Design?
///
/// - **Cache-first**: Reads check the contract cache before hitting storage
/// - **Write-through cache**: Writes go to cache immediately, persisted later
/// - **Type conversion**: Translates between TOS types (Hash, ValueCell) and TAKO types ([u8; 32], &[u8])
/// - **Topoheight isolation**: All storage operations are versioned at a specific topoheight
///
/// # Example
///
/// ```rust,ignore
/// use tako_integration::TosStorageAdapter;
///
/// // Create adapter
/// let mut adapter = TosStorageAdapter::new(
///     &mut tos_provider,
///     &contract_hash,
///     &mut cache,
///     topoheight,
/// );
///
/// // TOS Kernel(TAKO) will call these methods via syscalls
/// adapter.get(&contract_hash.as_bytes(), b"balance")?;
/// adapter.set(&contract_hash.as_bytes(), b"balance", b"1000")?;
/// ```
pub struct TosStorageAdapter<'a> {
    /// TOS contract provider (backend storage)
    provider: &'a (dyn ContractProvider + Send),
    /// Current contract being executed
    contract_hash: &'a Hash,
    /// Contract cache for this execution
    cache: &'a mut ContractCache,
    /// Current topoheight (for versioned reads)
    topoheight: TopoHeight,
}

impl<'a> TosStorageAdapter<'a> {
    /// Create a new storage adapter
    ///
    /// # Arguments
    ///
    /// * `provider` - TOS storage provider
    /// * `contract_hash` - Hash of the contract being executed
    /// * `cache` - Contract cache for this execution
    /// * `topoheight` - Current topoheight for versioned reads
    pub fn new(
        provider: &'a (dyn ContractProvider + Send),
        contract_hash: &'a Hash,
        cache: &'a mut ContractCache,
        topoheight: TopoHeight,
    ) -> Self {
        Self {
            provider,
            contract_hash,
            cache,
            topoheight,
        }
    }

    /// Convert a byte slice to a TOS Kernel ValueCell (for cache lookups)
    ///
    /// TOS Kernel uses ValueCell for all contract data, which is a dynamically-typed
    /// wrapper around primitive types and complex structures. For TAKO integration,
    /// we serialize byte slices as ValueCell::String.
    fn bytes_to_value_cell(bytes: &[u8]) -> Result<ValueCell, EbpfError> {
        // The ValueCell::Bytes variant preserves a one-to-one mapping between
        // arbitrary byte slices and the cached key/value representation.
        // This avoids the hash collisions that occurred when using
        // ValueCell::default() as a placeholder.
        Ok(ValueCell::Bytes(bytes.to_vec()))
    }

    /// Convert a ValueCell to bytes
    fn value_cell_to_bytes(cell: &ValueCell) -> Result<Vec<u8>, EbpfError> {
        if let ValueCell::Bytes(bytes) = cell {
            Ok(bytes.clone())
        } else {
            Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Stored value is not a raw byte array",
            ))))
        }
    }
}

impl<'a> StorageProvider for TosStorageAdapter<'a> {
    fn get(&self, contract_hash: &[u8; 32], key: &[u8]) -> Result<Option<Vec<u8>>, EbpfError> {
        // Verify contract hash matches (for safety)
        if contract_hash != self.contract_hash.as_bytes() {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Contract hash mismatch: cannot read from other contracts",
            ))));
        }

        // Convert key to ValueCell for cache lookup
        let key_cell = Self::bytes_to_value_cell(key)?;

        // Check cache first
        if let Some((_, value_opt)) = self.cache.storage.get(&key_cell) {
            return match value_opt {
                Some(value) => Ok(Some(Self::value_cell_to_bytes(value)?)),
                None => Ok(None),
            };
        }

        // Cache miss - load from storage
        // Note: We don't cache on read because get() takes &self (immutable)
        // Caching happens during writes via set() which takes &mut self
        let value = self
            .provider
            .load_data(self.contract_hash, &key_cell, self.topoheight)
            .map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Storage load failed: {}", e),
                )))
            })?;

        match value {
            Some((_topoheight, value_opt)) => match value_opt {
                Some(value) => Ok(Some(Self::value_cell_to_bytes(&value)?)),
                None => Ok(None),
            },
            None => Ok(None),
        }
    }

    fn set(&mut self, contract_hash: &[u8; 32], key: &[u8], value: &[u8]) -> Result<(), EbpfError> {
        // Verify contract hash matches
        if contract_hash != self.contract_hash.as_bytes() {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Contract hash mismatch: cannot write to other contracts",
            ))));
        }

        // Convert key and value to ValueCells
        let key_cell = Self::bytes_to_value_cell(key)?;
        let value_cell = Self::bytes_to_value_cell(value)?;

        // Determine versioned state
        let data_state = match self.cache.storage.get(&key_cell) {
            Some((mut state, _)) => {
                state.mark_updated();
                state
            }
            None => {
                // Load latest topoheight for this key
                match self.provider.load_data_latest_topoheight(
                    self.contract_hash,
                    &key_cell,
                    self.topoheight,
                ) {
                    Ok(Some(topoheight)) => VersionedState::Updated(topoheight),
                    Ok(None) => VersionedState::New,
                    Err(e) => {
                        return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to load topoheight: {}", e),
                        ))))
                    }
                }
            }
        };

        // Insert into cache (writes are cached, persisted later)
        self.cache
            .storage
            .insert(key_cell, (data_state, Some(value_cell)));

        Ok(())
    }

    fn delete(&mut self, contract_hash: &[u8; 32], key: &[u8]) -> Result<bool, EbpfError> {
        // Verify contract hash matches
        if contract_hash != self.contract_hash.as_bytes() {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Contract hash mismatch: cannot delete from other contracts",
            ))));
        }

        // Convert key to ValueCell
        let key_cell = Self::bytes_to_value_cell(key)?;

        // Check if key exists
        let exists = match self.cache.storage.get(&key_cell) {
            Some((_, value_opt)) => value_opt.is_some(),
            None => self
                .provider
                .has_data(self.contract_hash, &key_cell, self.topoheight)
                .map_err(|e| {
                    EbpfError::SyscallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Storage has_data failed: {}", e),
                    )))
                })?,
        };

        if !exists {
            return Ok(false);
        }

        // Determine versioned state
        let data_state = match self.cache.storage.get(&key_cell) {
            Some((s, _)) => match s {
                VersionedState::New => {
                    // Key was just created in this transaction, remove from cache entirely
                    self.cache.storage.remove(&key_cell);
                    return Ok(true);
                }
                VersionedState::FetchedAt(topoheight) => VersionedState::Updated(*topoheight),
                VersionedState::Updated(topoheight) => VersionedState::Updated(*topoheight),
            },
            None => {
                // Load latest topoheight for this key
                match self.provider.load_data_latest_topoheight(
                    self.contract_hash,
                    &key_cell,
                    self.topoheight,
                ) {
                    Ok(Some(topoheight)) => VersionedState::Updated(topoheight),
                    Ok(None) => return Ok(false), // Key doesn't exist
                    Err(e) => {
                        return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to load topoheight: {}", e),
                        ))))
                    }
                }
            }
        };

        // Mark as deleted (insert None)
        self.cache.storage.insert(key_cell, (data_state, None));

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tos_common::{contract::ContractCache, crypto::Hash, serializer::Serializer};

    // Mock ContractProvider for testing
    struct MockProvider {
        data: HashMap<(Hash, ValueCell), (TopoHeight, Option<ValueCell>)>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                data: HashMap::new(),
            }
        }
    }

    impl ContractProvider for MockProvider {
        fn get_contract_balance_for_asset(
            &self,
            _contract: &Hash,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(None)
        }

        fn get_account_balance_for_asset(
            &self,
            _key: &tos_common::crypto::PublicKey,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(None)
        }

        fn asset_exists(
            &self,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(false)
        }

        fn load_asset_data(
            &self,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, tos_common::asset::AssetData)>, anyhow::Error> {
            Ok(None)
        }

        fn load_asset_supply(
            &self,
            _asset: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
            Ok(None)
        }

        fn account_exists(
            &self,
            _key: &tos_common::crypto::PublicKey,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(false)
        }

        fn load_contract_module(
            &self,
            _contract: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<Vec<u8>>, anyhow::Error> {
            Ok(None)
        }
    }

    impl tos_common::contract::ContractStorage for MockProvider {
        fn load_data(
            &self,
            contract: &Hash,
            key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, Option<ValueCell>)>, anyhow::Error> {
            Ok(self.data.get(&(contract.clone(), key.clone())).cloned())
        }

        fn load_data_latest_topoheight(
            &self,
            contract: &Hash,
            key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<TopoHeight>, anyhow::Error> {
            Ok(self
                .data
                .get(&(contract.clone(), key.clone()))
                .map(|(topo, _)| *topo))
        }

        fn has_data(
            &self,
            contract: &Hash,
            key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(self.data.contains_key(&(contract.clone(), key.clone())))
        }

        fn has_contract(
            &self,
            _contract: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(false)
        }
    }

    #[test]
    fn test_storage_adapter_cache_workflow() {
        let provider = MockProvider::new();
        let contract_hash = Hash::zero();
        let mut cache = ContractCache::default();
        let topoheight = 100;

        let mut adapter = TosStorageAdapter::new(&provider, &contract_hash, &mut cache, topoheight);

        // Test write
        let key = b"test_key";
        let value = b"test_value";
        assert!(adapter.set(contract_hash.as_bytes(), key, value).is_ok());

        // Test read from cache
        let result = adapter.get(contract_hash.as_bytes(), key).unwrap();
        assert_eq!(result, Some(value.to_vec()));

        // Ensure cache keeps the exact ValueCell::Bytes entries for key/value
        assert_eq!(cache.storage.len(), 1);
    }

    #[test]
    fn test_storage_adapter_contract_isolation() {
        let provider = MockProvider::new();
        let contract_hash = Hash::zero();
        let other_contract = <Hash as Serializer>::from_bytes(&[1u8; 32]).unwrap();
        let mut cache = ContractCache::default();
        let topoheight = 100;

        let adapter = TosStorageAdapter::new(&provider, &contract_hash, &mut cache, topoheight);

        // Attempt to access different contract's storage
        let result = adapter.get(other_contract.as_bytes(), b"key");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("mismatch"));
    }

    #[test]
    fn test_storage_adapter_distinct_keys_preserved() {
        let provider = MockProvider::new();
        let contract_hash = Hash::zero();
        let mut cache = ContractCache::default();
        let topoheight = 100;

        let mut adapter = TosStorageAdapter::new(&provider, &contract_hash, &mut cache, topoheight);

        adapter
            .set(contract_hash.as_bytes(), b"key_a", b"value_a")
            .unwrap();
        adapter
            .set(contract_hash.as_bytes(), b"key_b", b"value_b")
            .unwrap();

        let value_a = adapter
            .get(contract_hash.as_bytes(), b"key_a")
            .unwrap()
            .unwrap();
        let value_b = adapter
            .get(contract_hash.as_bytes(), b"key_b")
            .unwrap()
            .unwrap();

        assert_eq!(value_a, b"value_a");
        assert_eq!(value_b, b"value_b");

        // Drop adapter to release mutable borrow of cache
        drop(adapter);

        // Now we can check the cache
        assert_eq!(cache.storage.len(), 2);
    }
}
