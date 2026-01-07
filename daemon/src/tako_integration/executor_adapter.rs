/// TOS Kernel(TAKO) Contract Executor Adapter
///
/// This module implements the ContractExecutor trait for TOS Kernel(TAKO), enabling
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
use std::collections::HashMap;
use std::sync::RwLock;
use tos_common::{
    block::TopoHeight,
    contract::{ContractEvent, ContractExecutionResult, ContractExecutor, ContractProvider},
    crypto::Hash,
};

use super::{ExecutionResult, TakoExecutor};
use crate::vrf::VrfData;

/// TOS Kernel(TAKO) implementation of ContractExecutor trait
///
/// This adapter bridges the generic ContractExecutor interface with
/// TOS Kernel(TAKO)'s specific execution engine.
///
/// # VRF Support
///
/// The executor can optionally hold VRF data for verifiable randomness.
/// When VRF data is set, contract executions will have access to VRF syscalls:
/// - `tos_vrf_random` - Get VRF output + proof + derived random
/// - `tos_vrf_verify` - Verify VRF proof
/// - `tos_vrf_public_key` - Get block producer's VRF public key
///
/// # Example
///
/// ```no_run
/// use tos_daemon::tako_integration::TakoContractExecutor;
/// use tos_common::contract::ContractExecutor;
/// use std::sync::Arc;
///
/// # // Mock ParallelChainState for demonstration
/// # struct ParallelChainState;
/// # impl ParallelChainState {
/// #     fn new(_storage: u32, _executor: Arc<dyn ContractExecutor>) -> Self {
/// #         Self
/// #     }
/// # }
///
/// // Create TAKO contract executor
/// let executor = TakoContractExecutor::new();
///
/// // Inject into transaction state
/// let state = ParallelChainState::new(0, Arc::new(executor));
/// ```
pub struct TakoContractExecutor {
    /// VRF data keyed by block_hash for thread-safe concurrent execution
    /// Each block execution can have its own VRF data without race conditions
    vrf_data: RwLock<HashMap<[u8; 32], VrfData>>,
}

impl TakoContractExecutor {
    /// Create a new TAKO contract executor
    pub fn new() -> Self {
        Self {
            vrf_data: RwLock::new(HashMap::new()),
        }
    }

    /// Set VRF data for a specific block hash
    ///
    /// # Arguments
    ///
    /// * `block_hash` - The block hash to associate VRF data with
    /// * `vrf_data` - VRF data (public_key, output, proof) from block producer
    ///
    /// # Thread Safety
    ///
    /// This method is thread-safe. Multiple blocks can have their VRF data
    /// set concurrently without overwriting each other.
    pub fn set_vrf_data(&self, block_hash: &Hash, vrf_data: Option<VrfData>) {
        if let Ok(mut guard) = self.vrf_data.write() {
            if let Some(data) = vrf_data {
                guard.insert(*block_hash.as_bytes(), data);
            } else {
                guard.remove(block_hash.as_bytes());
            }
        }
    }

    /// Get VRF data for a specific block hash
    ///
    /// # Arguments
    ///
    /// * `block_hash` - The block hash to get VRF data for
    ///
    /// # Returns
    ///
    /// VRF data if set for this block, None otherwise
    pub fn get_vrf_data(&self, block_hash: &Hash) -> Option<VrfData> {
        self.vrf_data
            .read()
            .ok()
            .and_then(|guard| guard.get(block_hash.as_bytes()).cloned())
    }

    /// Clear VRF data for a specific block hash
    ///
    /// Should be called after block execution completes to prevent memory leaks.
    pub fn clear_vrf_data(&self, block_hash: &Hash) {
        if let Ok(mut guard) = self.vrf_data.write() {
            guard.remove(block_hash.as_bytes());
        }
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
        block_timestamp: u64,
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

        // Get VRF data for this specific block (thread-safe lookup)
        let vrf_data = self.get_vrf_data(block_hash);
        if log::log_enabled!(log::Level::Debug) && vrf_data.is_some() {
            debug!("TAKO executor: VRF data available for block {}", block_hash);
        }

        // Execute via TOS Kernel(TAKO) with optional VRF data
        let result = TakoExecutor::execute_with_vrf(
            bytecode,
            provider,
            topoheight,
            contract_hash,
            block_hash,
            block_height,
            block_timestamp,
            tx_hash,
            tx_sender,
            &input_data,
            Some(max_gas),
            vrf_data.as_ref(),
        )?;

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "TAKO executor: Execution complete - return_value: {}, instructions: {}, gas_used: {}",
                result.return_value,
                result.instructions_executed,
                result.compute_units_used
            );
        }

        let ExecutionResult {
            return_value,
            compute_units_used,
            return_data,
            transfers,
            events,
            cache,
            ..
        } = result;

        // Convert TAKO events to ContractEvent format
        let contract_events: Vec<ContractEvent> = events
            .into_iter()
            .map(|e| ContractEvent {
                contract: e.contract,
                topics: e.topics,
                data: e.data,
            })
            .collect();

        if log::log_enabled!(log::Level::Debug) {
            if !contract_events.is_empty() {
                debug!(
                    "TAKO executor: {} events emitted by contract {}",
                    contract_events.len(),
                    contract_hash
                );
            }
        }

        // Convert TAKO result to ContractExecutionResult
        Ok(ContractExecutionResult {
            // TAKO uses compute units, which we map 1:1 to gas
            gas_used: compute_units_used,

            // TAKO return value: 0 = success, non-zero = error
            exit_code: Some(return_value),

            // Return data from TAKO execution
            return_data,

            transfers,

            // Events emitted by the contract
            events: contract_events,

            // VM cache overlay containing storage writes
            // Will be merged on success via merge_overlay_storage_only()
            cache: Some(cache),
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
