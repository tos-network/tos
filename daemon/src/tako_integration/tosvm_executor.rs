/// TOS-VM Legacy Executor Adapter
///
/// This module wraps the legacy TOS-VM interpreter to implement the ContractExecutor trait.
/// This allows gradual migration from TOS-VM to TAKO VM by supporting both execution engines.
///
/// **Note**: This is temporary and will be removed once all contracts are migrated to TAKO VM (eBPF format).

use async_trait::async_trait;
use log::debug;
use tos_common::{
    block::TopoHeight,
    contract::{ContractExecutionResult, ContractExecutor, ContractProvider},
    crypto::Hash,
};

/// Legacy TOS-VM implementation of ContractExecutor trait
///
/// This adapter is a placeholder that delegates to the existing TOS-VM execution code
/// in the transaction verification layer. It will be removed once full TAKO migration is complete.
pub struct TosVmExecutor;

impl TosVmExecutor {
    /// Create a new TOS-VM executor
    pub fn new() -> Self {
        Self
    }
}

impl Default for TosVmExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContractExecutor for TosVmExecutor {
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
        _max_gas: u64,
        _parameters: Option<Vec<u8>>,
    ) -> anyhow::Result<ContractExecutionResult> {
        // NOTE: This method should never be called directly.
        // The transaction verification layer still handles TOS-VM execution internally
        // for non-ELF contracts. This is just a marker implementation.
        //
        // Once we refactor to use the executor trait everywhere, this will contain
        // the actual TOS-VM execution logic moved from contract.rs

        if log::log_enabled!(log::Level::Debug) {
            debug!("TosVmExecutor: Legacy VM execution requested (should be handled in transaction layer)");
        }

        anyhow::bail!(
            "TosVmExecutor: Legacy execution not yet refactored to use trait pattern. \
             TOS-VM contracts are still executed directly in transaction verification layer."
        )
    }

    fn supports_format(&self, bytecode: &[u8]) -> bool {
        // TOS-VM format is anything that's NOT ELF
        // In the future, we might add proper TOS-VM format detection
        !tos_common::contract::is_elf_bytecode(bytecode)
    }

    fn name(&self) -> &'static str {
        "TosVM (Legacy Interpreter)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_non_elf_format() {
        let executor = TosVmExecutor::new();

        // Should support non-ELF bytecode
        let non_elf_bytecode = b"tos-vm bytecode here";
        assert!(executor.supports_format(non_elf_bytecode));

        // Should NOT support ELF
        let elf_bytecode = b"\x7FELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        assert!(!executor.supports_format(elf_bytecode));
    }

    #[test]
    fn test_executor_name() {
        let executor = TosVmExecutor::new();
        assert_eq!(executor.name(), "TosVM (Legacy Interpreter)");
    }
}
