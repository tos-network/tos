use tos_common::{block::TopoHeight, crypto::Hash, serializer::Serializer};
/// Contract loader adapter: TOS contract storage → TAKO ContractLoader
///
/// This module enables Cross-Program Invocation (CPI) by allowing TAKO contracts
/// to load and invoke other contracts from TOS storage.
use tos_program_runtime::storage::ContractLoader;
use tos_tbpf::error::EbpfError;

/// Adapter that enables TAKO contracts to load other contracts from TOS storage
///
/// # Architecture
///
/// ```text
/// TAKO syscall invoke(contract_hash, instruction)
///     ↓
/// TosContractLoaderAdapter::load_contract()
///     ↓
/// TOS Storage::load_contract_module() → VersionedContract → Module → bytecode
///     ↓
/// Verify ELF format (0x7F 'E' 'L' 'F')
///     ↓
/// Return bytecode for execution
/// ```
///
/// # Implementation Status
///
/// **Phase 2 (Complete)**: Full Module Loading
/// - ✅ Validates contract hash format
/// - ✅ Loads Module bytecode from storage via `ContractProvider::load_contract_module()`
/// - ✅ Extracts bytecode from `VersionedContract<'a>`
/// - ✅ Verifies ELF format (`b"\x7FELF"`)
/// - ✅ Returns actual bytecode for execution
/// - ✅ Prevents cross-VM CPI (TAKO can only call TAKO contracts)
///
/// # Example
///
/// ```no_run
/// use tos_daemon::tako_integration::TosContractLoaderAdapter;
/// use tos_program_runtime::storage::ContractLoader;
/// use tos_common::block::TopoHeight;
/// use tos_common::contract::ContractProvider;
///
/// # // Mock storage provider for demonstration
/// # use tos_common::contract::ContractStorage;
/// # use tos_kernel::ValueCell;
/// # struct MockStorage;
/// # impl ContractStorage for MockStorage {
/// #     fn load_data(&self, _: &tos_common::crypto::Hash, _: &ValueCell, _: TopoHeight) -> Result<Option<(TopoHeight, Option<ValueCell>)>, anyhow::Error> { Ok(None) }
/// #     fn load_data_latest_topoheight(&self, _: &tos_common::crypto::Hash, _: &ValueCell, _: TopoHeight) -> Result<Option<TopoHeight>, anyhow::Error> { Ok(None) }
/// #     fn has_data(&self, _: &tos_common::crypto::Hash, _: &ValueCell, _: TopoHeight) -> Result<bool, anyhow::Error> { Ok(false) }
/// #     fn has_contract(&self, _: &tos_common::crypto::Hash, _: TopoHeight) -> Result<bool, anyhow::Error> { Ok(false) }
/// # }
/// # impl ContractProvider for MockStorage {
/// #     fn get_contract_balance_for_asset(
/// #         &self, _: &tos_common::crypto::Hash, _: &tos_common::crypto::Hash, _: TopoHeight
/// #     ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> { Ok(None) }
/// #     fn get_account_balance_for_asset(
/// #         &self, _: &tos_common::crypto::PublicKey, _: &tos_common::crypto::Hash, _: TopoHeight
/// #     ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> { Ok(None) }
/// #     fn asset_exists(&self, _: &tos_common::crypto::Hash, _: TopoHeight) -> Result<bool, anyhow::Error> { Ok(false) }
/// #     fn load_asset_data(&self, _: &tos_common::crypto::Hash, _: TopoHeight) -> Result<Option<(TopoHeight, tos_common::asset::AssetData)>, anyhow::Error> { Ok(None) }
/// #     fn load_asset_supply(&self, _: &tos_common::crypto::Hash, _: TopoHeight) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> { Ok(None) }
/// #     fn account_exists(&self, _: &tos_common::crypto::PublicKey, _: TopoHeight) -> Result<bool, anyhow::Error> { Ok(false) }
/// #     fn load_contract_module(&self, _: &tos_common::crypto::Hash, _: TopoHeight) -> Result<Option<Vec<u8>>, anyhow::Error> { Ok(None) }
/// # }
///
/// // Create loader adapter
/// let storage = MockStorage;
/// let topoheight = 100;
/// let loader = TosContractLoaderAdapter::new(&storage, topoheight);
///
/// // Load a contract for CPI
/// let contract_hash = [0u8; 32];
/// let result = loader.load_contract(&contract_hash);
/// ```
pub struct TosContractLoaderAdapter<'a> {
    /// TOS storage backend
    storage: &'a (dyn tos_common::contract::ContractProvider + Send),
    /// Current topoheight (for versioned reads)
    topoheight: TopoHeight,
}

impl<'a> TosContractLoaderAdapter<'a> {
    /// Create a new contract loader adapter
    ///
    /// # Arguments
    ///
    /// * `storage` - TOS storage backend
    /// * `topoheight` - Current topoheight for versioned reads
    pub fn new(
        storage: &'a (dyn tos_common::contract::ContractProvider + Send),
        topoheight: TopoHeight,
    ) -> Self {
        Self {
            storage,
            topoheight,
        }
    }
}

impl<'a> ContractLoader for TosContractLoaderAdapter<'a> {
    fn load_contract(&self, contract_hash: &[u8; 32]) -> Result<Vec<u8>, EbpfError> {
        // Convert hash bytes to TOS Hash type
        let hash = <Hash as Serializer>::from_bytes(contract_hash).map_err(|e| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid contract hash: {}", e),
            )))
        })?;

        // Load contract Module bytecode from storage
        let bytecode_opt = self
            .storage
            .load_contract_module(&hash, self.topoheight)
            .map_err(|e| {
                EbpfError::SyscallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to load contract Module from storage: {}", e),
                )))
            })?;

        // Check if contract exists
        let Some(bytecode) = bytecode_opt else {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Contract {} not found at topoheight {} or earlier",
                    hash, self.topoheight
                ),
            ))));
        };

        // Verify ELF format (magic bytes: 0x7F 'E' 'L' 'F')
        if !bytecode.starts_with(b"\x7FELF") {
            return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Cross-VM CPI not supported: Contract {} is legacy format (not TOS Kernel(TAKO) ELF). \
                    TOS Kernel(TAKO) can only invoke other TOS Kernel(TAKO) contracts.",
                    hash
                ),
            ))));
        }

        // Log successful load
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "Loaded TAKO contract {} for CPI: {} bytes ELF bytecode",
                hash,
                bytecode.len()
            );
        }

        Ok(bytecode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::{
        asset::AssetData,
        contract::{ContractProvider, ContractStorage},
        crypto::PublicKey,
    };
    use tos_kernel::ValueCell;

    // Mock storage for testing
    struct MockStorage;

    // Implement ContractStorage trait
    impl ContractStorage for MockStorage {
        fn load_data(
            &self,
            _contract: &Hash,
            _key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, Option<ValueCell>)>, anyhow::Error> {
            Ok(None)
        }

        fn load_data_latest_topoheight(
            &self,
            _contract: &Hash,
            _key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<TopoHeight>, anyhow::Error> {
            Ok(None)
        }

        fn has_data(
            &self,
            _contract: &Hash,
            _key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(false)
        }

        fn has_contract(
            &self,
            _contract: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(false)
        }
    }

    // Implement ContractProvider trait
    impl ContractProvider for MockStorage {
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
            _key: &PublicKey,
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
        ) -> Result<Option<(TopoHeight, AssetData)>, anyhow::Error> {
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
            _key: &PublicKey,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            Ok(false)
        }

        fn load_contract_module(
            &self,
            _contract: &Hash,
            _topoheight: TopoHeight,
        ) -> Result<Option<Vec<u8>>, anyhow::Error> {
            // MockStorage returns None (no contracts exist)
            Ok(None)
        }
    }

    #[test]
    fn test_contract_loader_nonexistent_contract() {
        let storage = MockStorage;
        let loader = TosContractLoaderAdapter::new(&storage, 100);

        let contract_hash = [0u8; 32];
        let result = loader.load_contract(&contract_hash);

        // Should return "not found" error since MockStorage returns false for has_contract
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("not found"),
            "Expected 'not found' error, got: {}",
            error_msg
        );
    }

    #[test]
    fn test_contract_loader_validation() {
        let storage = MockStorage;
        let loader = TosContractLoaderAdapter::new(&storage, 100);

        // Valid hash format but non-existent contract
        let contract_hash = [0u8; 32];
        let result = loader.load_contract(&contract_hash);

        // Should fail at contract existence check
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("not found"));
    }
}
