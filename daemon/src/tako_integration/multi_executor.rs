/// Multi-Executor: Format-based Contract Execution Dispatcher
///
/// This executor automatically selects the appropriate VM (TAKO or TOS-VM)
/// based on the bytecode format. This enables gradual migration from TOS-VM
/// to TAKO VM without breaking existing contracts.
///
/// # Architecture
///
/// ```text
/// MultiExecutor::execute()
///     ↓
/// Check bytecode format
///     ↓
/// ┌─────────────────┐
/// │  is_elf_bytecode? │
/// └─────────────────┘
///     ↙         ↘
/// YES           NO
///  ↓             ↓
/// TAKO VM    TOS-VM (legacy)
/// (eBPF)     (interpreter)
/// ```

use async_trait::async_trait;
use log::{debug, info};
use std::sync::Arc;
use tos_common::{
    block::TopoHeight,
    contract::{ContractExecutionResult, ContractExecutor, ContractProvider},
    crypto::Hash,
};

use super::{TakoContractExecutor, TosVmExecutor};

/// Multi-format contract executor
///
/// Automatically dispatches to the appropriate VM based on bytecode format:
/// - ELF format → TAKO VM (eBPF)
/// - Other formats → TOS-VM (legacy interpreter)
///
/// # Example
///
/// ```ignore
/// use tos_daemon::tako_integration::MultiExecutor;
///
/// let executor = Arc::new(MultiExecutor::new());
///
/// // Use in ParallelChainState
/// let state = ParallelChainState::new(..., executor);
/// ```
pub struct MultiExecutor {
    tako_executor: TakoContractExecutor,
    tosvm_executor: TosVmExecutor,
}

impl MultiExecutor {
    /// Create a new multi-executor with both TAKO and TOS-VM support
    pub fn new() -> Self {
        info!("MultiExecutor initialized: TAKO VM (eBPF) + TOS-VM (legacy)");
        Self {
            tako_executor: TakoContractExecutor::new(),
            tosvm_executor: TosVmExecutor::new(),
        }
    }

    /// Get statistics about which executor would be used for given bytecode
    pub fn executor_for_bytecode(&self, bytecode: &[u8]) -> &'static str {
        if self.tako_executor.supports_format(bytecode) {
            self.tako_executor.name()
        } else {
            self.tosvm_executor.name()
        }
    }
}

impl Default for MultiExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContractExecutor for MultiExecutor {
    async fn execute<P: ContractProvider + Send>(
        &self,
        bytecode: &[u8],
        provider: &mut P,
        topoheight: TopoHeight,
        contract_hash: &Hash,
        block_hash: &Hash,
        block_height: u64,
        tx_hash: &Hash,
        tx_sender: &Hash,
        max_gas: u64,
        parameters: Option<Vec<u8>>,
    ) -> anyhow::Result<ContractExecutionResult> {
        // Auto-detect format and dispatch to appropriate executor
        let executor_name;
        let result = if self.tako_executor.supports_format(bytecode) {
            executor_name = "TAKO";
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "MultiExecutor: Dispatching to TAKO VM for contract {} (ELF format detected)",
                    contract_hash
                );
            }

            self.tako_executor
                .execute(
                    bytecode,
                    provider,
                    topoheight,
                    contract_hash,
                    block_hash,
                    block_height,
                    tx_hash,
                    tx_sender,
                    max_gas,
                    parameters,
                )
                .await
        } else {
            executor_name = "TOS-VM";
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "MultiExecutor: Dispatching to TOS-VM for contract {} (legacy format)",
                    contract_hash
                );
            }

            self.tosvm_executor
                .execute(
                    bytecode,
                    provider,
                    topoheight,
                    contract_hash,
                    block_hash,
                    block_height,
                    tx_hash,
                    tx_sender,
                    max_gas,
                    parameters,
                )
                .await
        };

        // Log result for monitoring
        match &result {
            Ok(exec_result) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "MultiExecutor: {} execution complete - exit_code: {:?}, gas_used: {}",
                        executor_name, exec_result.exit_code, exec_result.gas_used
                    );
                }
            }
            Err(e) => {
                debug!("MultiExecutor: {} execution failed: {}", executor_name, e);
            }
        }

        result
    }

    fn supports_format(&self, bytecode: &[u8]) -> bool {
        // Supports all formats (delegates to appropriate executor)
        self.tako_executor.supports_format(bytecode)
            || self.tosvm_executor.supports_format(bytecode)
    }

    fn name(&self) -> &'static str {
        "MultiExecutor (TAKO + TOS-VM)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supports_all_formats() {
        let executor = MultiExecutor::new();

        // Should support ELF (TAKO)
        let elf_bytecode = b"\x7FELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        assert!(executor.supports_format(elf_bytecode));

        // Should support non-ELF (TOS-VM)
        let tosvm_bytecode = b"tos-vm bytecode";
        assert!(executor.supports_format(tosvm_bytecode));
    }

    #[test]
    fn test_executor_selection() {
        let executor = MultiExecutor::new();

        // ELF → TAKO
        let elf_bytecode = b"\x7FELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        assert_eq!(
            executor.executor_for_bytecode(elf_bytecode),
            "TakoVM (eBPF)"
        );

        // Non-ELF → TOS-VM
        let tosvm_bytecode = b"tos-vm bytecode";
        assert_eq!(
            executor.executor_for_bytecode(tosvm_bytecode),
            "TosVM (Legacy Interpreter)"
        );
    }

    #[test]
    fn test_executor_name() {
        let executor = MultiExecutor::new();
        assert_eq!(executor.name(), "MultiExecutor (TAKO + TOS-VM)");
    }
}
