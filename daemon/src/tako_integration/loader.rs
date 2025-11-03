/// Contract loader adapter: TOS contract storage → TAKO ContractLoader
///
/// This module enables Cross-Program Invocation (CPI) by allowing TAKO contracts
/// to load and invoke other contracts from TOS storage.

use tos_program_runtime::storage::ContractLoader;
use tos_common::{
    block::TopoHeight,
    crypto::Hash,
    serializer::Serializer,
};
use tos_tbpf::error::EbpfError;

/// Adapter that enables TAKO contracts to load other contracts from TOS storage
///
/// # Architecture
///
/// ```text
/// TAKO syscall tos_invoke(contract_hash, instruction)
///     ↓
/// TosContractLoaderAdapter::load_contract()
///     ↓
/// TOS Storage::load_contract()
///     ↓
/// RocksDB Contracts column family
///     ↓
/// Return ELF bytecode for TAKO VM
/// ```
///
/// # Contract Type Handling
///
/// - If the loaded contract is TAKO VM (ELF format): Returns bytecode directly
/// - If the loaded contract is TOS-VM (non-ELF): Returns error (cross-VM CPI not supported in Phase 1)
///
/// # Example
///
/// ```rust,ignore
/// use tako_integration::TosContractLoaderAdapter;
///
/// // Create loader
/// let loader = TosContractLoaderAdapter::new(storage, topoheight);
///
/// // Load contract by hash (used during CPI)
/// let bytecode = loader.load_contract(&contract_hash)?;
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
    pub fn new(storage: &'a (dyn tos_common::contract::ContractProvider + Send), topoheight: TopoHeight) -> Self {
        Self {
            storage,
            topoheight,
        }
    }
}

impl<'a> ContractLoader for TosContractLoaderAdapter<'a> {
    fn load_contract(&self, contract_hash: &[u8; 32]) -> Result<Vec<u8>, EbpfError> {
        // Convert hash
        let _hash = <Hash as Serializer>::from_bytes(contract_hash).map_err(|e| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Invalid contract hash: {}", e),
            )))
        })?;

        // TODO [Phase 1 Implementation]: Load contract from TOS storage
        //
        // This requires accessing the TOS storage trait for contracts.
        // The exact implementation depends on the Storage trait being used.
        //
        // Expected implementation:
        // ```
        // let contract = self.storage
        //     .load_contract(&hash, self.topoheight)
        //     .map_err(|e| EbpfError::SyscallError(...))?;
        //
        // // Get bytecode
        // let bytecode = contract.bytecode();
        //
        // // Verify it's a TAKO contract (ELF format)
        // if !bytecode.starts_with(b"\x7FELF") {
        //     return Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
        //         std::io::ErrorKind::InvalidData,
        //         "Cross-VM CPI not supported: target contract is TOS-VM, not TAKO VM",
        //     ))));
        // }
        //
        // Ok(bytecode.to_vec())
        // ```

        // Placeholder implementation for Phase 1
        Err(EbpfError::SyscallError(Box::new(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Contract loading not yet implemented - requires TOS storage trait integration",
        ))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock storage for testing
    struct MockStorage;

    #[test]
    fn test_contract_loader_placeholder() {
        let storage = MockStorage;
        let loader = TosContractLoaderAdapter::new(&storage, 100);

        let contract_hash = [0u8; 32];
        let result = loader.load_contract(&contract_hash);

        // Currently returns error until storage integration is complete
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not yet implemented"));
    }
}
