/// TAKO VM Contract Executor Adapter
///
/// This module implements the ContractExecutor trait for TAKO VM, enabling
/// the transaction processor to execute eBPF contracts via dependency injection.
///
/// # Architecture
///
/// ```text
/// Common Package (ContractExecutor trait)
///     ↑ implements
/// TakoContractExecutor (this file)
///     ↓ uses
/// TakoExecutor (executor.rs)
///     ↓ executes
/// eBPF Contract
/// ```
use async_trait::async_trait;
use log::{debug, trace};
use tos_common::{
    block::TopoHeight,
    contract::{ContractExecutionResult, ContractExecutor, ContractProvider},
    crypto::Hash,
};

use super::TakoExecutor;

/// TAKO VM implementation of ContractExecutor trait
///
/// This adapter bridges the generic ContractExecutor interface with
/// TAKO VM's specific execution engine.
///
/// # Example
///
/// ```ignore
/// use tos_daemon::tako_integration::TakoContractExecutor;
/// use tos_common::contract::ContractExecutor;
///
/// let executor = TakoContractExecutor::new();
///
/// // Inject into transaction state
/// let state = ParallelChainState::new(..., Arc::new(executor));
/// ```
pub struct TakoContractExecutor;

impl TakoContractExecutor {
    /// Create a new TAKO contract executor
    pub fn new() -> Self {
        Self
    }
}

impl Default for TakoContractExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContractExecutor for TakoContractExecutor {
    async fn execute(
        &self,
        bytecode: &[u8],
        provider: &mut (dyn ContractProvider + Send),
        topoheight: TopoHeight,
        contract_hash: &Hash,
        block_hash: &Hash,
        block_height: u64,
        tx_hash: &Hash,
        tx_sender: &Hash,
        max_gas: u64,
        parameters: Option<Vec<u8>>,
    ) -> anyhow::Result<ContractExecutionResult> {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "TAKO executor: Executing contract {} from TX {} with max_gas: {}",
                contract_hash, tx_hash, max_gas
            );
        }

        // Extract input data from parameters (if any)
        let input_data = parameters.unwrap_or_default();

        if log::log_enabled!(log::Level::Trace) {
            trace!("TAKO executor: Input data size: {} bytes", input_data.len());
        }

        // Execute via TAKO VM
        let result = TakoExecutor::execute(
            bytecode,
            provider,
            topoheight,
            contract_hash,
            block_hash,
            block_height,
            tx_hash,
            tx_sender,
            &input_data,
            Some(max_gas),
        )?;

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "TAKO executor: Execution complete - return_value: {}, instructions: {}, gas_used: {}",
                result.return_value,
                result.instructions_executed,
                result.compute_units_used
            );
        }

        // Convert TAKO result to ContractExecutionResult
        Ok(ContractExecutionResult {
            // TAKO uses compute units, which we map 1:1 to gas
            gas_used: result.compute_units_used,

            // TAKO return value: 0 = success, non-zero = error
            exit_code: Some(result.return_value),

            // Return data from TAKO execution
            return_data: result.return_data,
        })
    }

    fn supports_format(&self, bytecode: &[u8]) -> bool {
        // Check for ELF magic number: 0x7F 'E' 'L' 'F'
        tos_common::contract::is_elf_bytecode(bytecode)
    }

    fn name(&self) -> &'static str {
        "TakoVM (eBPF)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_elf_format() {
        let executor = TakoContractExecutor::new();

        // Valid ELF bytecode
        let elf_bytecode = b"\x7FELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        assert!(executor.supports_format(elf_bytecode));

        // Invalid bytecode
        let invalid_bytecode = b"not an ELF file";
        assert!(!executor.supports_format(invalid_bytecode));
    }

    #[test]
    fn test_executor_name() {
        let executor = TakoContractExecutor::new();
        assert_eq!(executor.name(), "TakoVM (eBPF)");
    }
}
