/// TAKO VM Executor for TOS Blockchain
///
/// This module provides the main execution engine for TAKO VM contracts within TOS blockchain.
/// It handles bytecode loading, VM creation, execution, and result processing.
///
/// # Architecture
///
/// ```text
/// TOS Transaction
///     ↓
/// TakoExecutor::execute()
///     ↓
/// 1. Validate ELF bytecode
/// 2. Create adapters (Storage, Accounts, Loader)
/// 3. Load executable with syscalls
/// 4. Create InvokeContext with TOS state
/// 5. Execute in TBPF VM
/// 6. Process execution results
/// ```

use std::sync::Arc;
use tos_program_runtime::{
    invoke_context::InvokeContext,
    storage::{AccountProvider, ContractLoader, StorageProvider},
};
use tos_tbpf::{
    aligned_memory::AlignedMemory,
    ebpf,
    elf::Executable,
    error::{EbpfError, ProgramResult},
    memory_region::{MemoryMapping, MemoryRegion},
    program::BuiltinProgram,
    vm::{Config, ContextObject, EbpfVm},
};
use tos_common::{
    block::TopoHeight,
    contract::{ContractCache, ContractProvider},
    crypto::Hash,
};

use super::{TosAccountAdapter, TosContractLoaderAdapter, TosStorageAdapter, TakoExecutionError};

/// Default compute budget for contract execution (200,000 compute units)
///
/// This matches Solana's default for simple transactions. Can be adjusted
/// based on TOS's requirements.
pub const DEFAULT_COMPUTE_BUDGET: u64 = 200_000;

/// Maximum compute budget allowed (10,000,000 compute units)
///
/// Prevents excessive computation. Can be increased for complex contracts.
pub const MAX_COMPUTE_BUDGET: u64 = 10_000_000;

/// Stack size for contract execution (16KB)
///
/// Matches TBPF's default stack size.
const STACK_SIZE: usize = 16 * 1024;

/// TAKO VM executor for TOS blockchain
///
/// This is the main entry point for executing TAKO VM contracts within TOS.
/// It manages the complete lifecycle of contract execution from bytecode loading
/// to result processing.
///
/// # Example
///
/// ```ignore
/// use tako_integration::TakoExecutor;
///
/// let result = TakoExecutor::execute(
///     contract_bytecode,
///     &mut provider,
///     topoheight,
///     &contract_hash,
///     &block_hash,
///     block_height,
///     &tx_hash,
///     &tx_sender,
///     input_data,
///     compute_budget,
/// )?;
/// ```
pub struct TakoExecutor;

/// Result of contract execution
#[derive(Debug)]
pub struct ExecutionResult {
    /// Program return value (0 = success, non-zero = error code)
    pub return_value: u64,
    /// Number of instructions executed
    pub instructions_executed: u64,
    /// Compute units consumed
    pub compute_units_used: u64,
    /// Return data set by the contract (if any)
    pub return_data: Option<Vec<u8>>,
}

impl TakoExecutor {
    /// Execute a TAKO VM contract
    ///
    /// # Arguments
    ///
    /// * `bytecode` - ELF bytecode of the contract
    /// * `provider` - TOS contract provider (for storage, accounts, etc.)
    /// * `topoheight` - Current topoheight for versioned reads
    /// * `contract_hash` - Hash of the contract being executed
    /// * `block_hash` - Current block hash
    /// * `block_height` - Current block height
    /// * `tx_hash` - Transaction hash
    /// * `tx_sender` - Transaction sender's public key
    /// * `input_data` - Input data for the contract
    /// * `compute_budget` - Maximum compute units allowed
    ///
    /// # Returns
    ///
    /// `ExecutionResult` containing return value, compute usage, and return data
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Bytecode is not valid ELF format
    /// - VM creation fails
    /// - Execution fails (out of compute, invalid memory access, etc.)
    /// - Compute budget exceeds maximum
    #[allow(clippy::too_many_arguments)]
    pub fn execute<P: ContractProvider>(
        bytecode: &[u8],
        provider: &mut P,
        topoheight: TopoHeight,
        contract_hash: &Hash,
        block_hash: &Hash,
        block_height: u64,
        tx_hash: &Hash,
        tx_sender: &Hash, // Using Hash type for sender (32 bytes)
        input_data: &[u8],
        compute_budget: Option<u64>,
    ) -> Result<ExecutionResult, TakoExecutionError> {
        use log::{debug, info, warn, error};

        info!(
            "TAKO VM execution starting: contract={}, compute_budget={}, bytecode_size={}",
            contract_hash,
            compute_budget.unwrap_or(DEFAULT_COMPUTE_BUDGET),
            bytecode.len()
        );

        // 1. Validate compute budget
        let compute_budget = compute_budget.unwrap_or(DEFAULT_COMPUTE_BUDGET);
        if compute_budget > MAX_COMPUTE_BUDGET {
            warn!(
                "Compute budget validation failed: requested={}, maximum={}",
                compute_budget, MAX_COMPUTE_BUDGET
            );
            return Err(TakoExecutionError::ComputeBudgetExceeded {
                requested: compute_budget,
                maximum: MAX_COMPUTE_BUDGET,
            });
        }

        // 2. Validate ELF bytecode
        debug!("Validating ELF bytecode: size={} bytes", bytecode.len());
        tos_common::contract::validate_contract_bytecode(bytecode)
            .map_err(|e| {
                error!("Bytecode validation failed: {:?}", e);
                TakoExecutionError::invalid_bytecode("Invalid ELF format", Some(e))
            })?;

        // 3. Create TOS adapters
        let mut cache = ContractCache::default();
        let mut storage = TosStorageAdapter::new(provider, contract_hash, &mut cache, topoheight);
        let mut accounts = TosAccountAdapter::new(provider, topoheight);
        let loader_adapter = TosContractLoaderAdapter::new(provider, topoheight);

        // 4. Create TBPF loader with syscalls
        // Note: JIT compilation is enabled via the "jit" feature in Cargo.toml
        // This provides 10-50x performance improvement over interpreter-only execution
        let config = Config::default();
        let mut loader = BuiltinProgram::<InvokeContext>::new_loader(config.clone());
        tos_syscalls::register_syscalls(&mut loader)
            .map_err(|e| TakoExecutionError::SyscallRegistrationFailed {
                reason: "Syscall registration error".to_string(),
                error_details: format!("{:?}", e),
            })?;
        let loader = Arc::new(loader);

        // 5. Load executable
        let executable = Executable::load(bytecode, loader.clone())
            .map_err(|e| TakoExecutionError::ExecutableLoadFailed {
                reason: "ELF parsing failed".to_string(),
                bytecode_size: bytecode.len(),
                error_details: format!("{:?}", e),
            })?;

        // 6. Create InvokeContext with TOS blockchain state
        let mut invoke_context = InvokeContext::new_with_state(
            compute_budget,
            *contract_hash.as_bytes(),
            *block_hash.as_bytes(),
            block_height,
            *tx_hash.as_bytes(),
            *tx_sender.as_bytes(),
            &mut storage,
            &mut accounts,
            &loader_adapter,
            loader.clone(),
        );

        // Enable debug mode if TOS is in debug mode
        #[cfg(debug_assertions)]
        invoke_context.enable_debug();

        // 7. Create memory mapping
        let mut stack = AlignedMemory::<{ ebpf::HOST_ALIGN }>::zero_filled(STACK_SIZE);
        let stack_len = stack.len();
        let regions: Vec<MemoryRegion> = vec![
            executable.get_ro_region(),
            MemoryRegion::new_writable(stack.as_slice_mut(), ebpf::MM_STACK_START),
        ];
        let memory_mapping = MemoryMapping::new(regions, &config, executable.get_tbpf_version())
            .map_err(|e| TakoExecutionError::MemoryMappingFailed {
                reason: "Memory region setup failed".to_string(),
                stack_size: STACK_SIZE,
                error_details: format!("{:?}", e),
            })?;

        // 8. Create VM
        let mut vm = EbpfVm::new(
            executable.get_loader().clone(),
            executable.get_tbpf_version(),
            &mut invoke_context,
            memory_mapping,
            stack_len,
        );

        // 9. Execute contract
        debug!("Executing contract bytecode via TBPF VM");
        let (instruction_count, result) = vm.execute_program(&executable, true);

        // 10. Calculate compute units used
        let compute_units_used = compute_budget - invoke_context.get_remaining();
        debug!(
            "Execution complete: instructions={}, compute_units_used={}/{}",
            instruction_count, compute_units_used, compute_budget
        );

        // 11. Get return data (if any)
        let return_data = invoke_context
            .get_return_data()
            .map(|(_, data)| data.to_vec());

        // 12. Process result
        match result {
            ProgramResult::Ok(return_value) => {
                info!(
                    "TAKO VM execution succeeded: return_value={}, instructions={}, compute_units={}, return_data_size={}",
                    return_value,
                    instruction_count,
                    compute_units_used,
                    return_data.as_ref().map(|d| d.len()).unwrap_or(0)
                );
                Ok(ExecutionResult {
                    return_value,
                    instructions_executed: instruction_count,
                    compute_units_used,
                    return_data,
                })
            }
            ProgramResult::Err(err) => {
                let execution_error = TakoExecutionError::from_ebpf_error(err, instruction_count, compute_units_used);
                error!(
                    "TAKO VM execution failed: category={}, error={}",
                    execution_error.category(),
                    execution_error.user_message()
                );
                Err(execution_error)
            }
        }
    }

    /// Execute a contract with minimal parameters (uses defaults for blockchain state)
    ///
    /// This is a convenience method for testing. Production code should use
    /// the full `execute()` method with proper blockchain state.
    pub fn execute_simple<P: ContractProvider>(
        bytecode: &[u8],
        provider: &mut P,
        topoheight: TopoHeight,
        contract_hash: &Hash,
    ) -> Result<ExecutionResult, TakoExecutionError> {
        Self::execute(
            bytecode,
            provider,
            topoheight,
            contract_hash,
            &Hash::zero(),   // block_hash
            0,               // block_height
            &Hash::zero(),   // tx_hash
            &Hash::zero(),   // tx_sender
            &[],             // input_data
            None,            // compute_budget (use default)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tos_common::{
        asset::AssetData,
        crypto::{Hash, PublicKey},
        serializer::Serializer,
    };
    use tos_program_runtime::storage::{InMemoryStorage, NoOpAccounts, NoOpContractLoader};
    use tos_vm::ValueCell;

    // Mock provider for testing
    struct MockProvider {
        storage: InMemoryStorage,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                storage: InMemoryStorage::new(),
            }
        }
    }

    impl tos_common::contract::ContractProvider for MockProvider {
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
            Ok(Some((100, 1000000)))
        }

        fn asset_exists(&self, _asset: &Hash, _topoheight: TopoHeight) -> Result<bool, anyhow::Error> {
            Ok(true)
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
            Ok(true)
        }
    }

    impl tos_common::contract::ContractStorage for MockProvider {
        fn load_data(
            &self,
            contract: &Hash,
            key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<(TopoHeight, Option<ValueCell>)>, anyhow::Error> {
            // Use InMemoryStorage through StorageProvider trait
            let key_bytes = bincode::serialize(key)?;
            match self.storage.get(contract.as_bytes(), &key_bytes) {
                Ok(Some(data)) => {
                    let value: ValueCell = bincode::deserialize(&data)?;
                    Ok(Some((100, Some(value))))
                }
                Ok(None) => Ok(None),
                Err(_) => Ok(None),
            }
        }

        fn load_data_latest_topoheight(
            &self,
            _contract: &Hash,
            _key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<Option<TopoHeight>, anyhow::Error> {
            Ok(Some(100))
        }

        fn has_data(
            &self,
            contract: &Hash,
            key: &ValueCell,
            _topoheight: TopoHeight,
        ) -> Result<bool, anyhow::Error> {
            let key_bytes = bincode::serialize(key)?;
            match self.storage.get(contract.as_bytes(), &key_bytes) {
                Ok(result) => Ok(result.is_some()),
                Err(_) => Ok(false),
            }
        }

        fn has_contract(&self, _contract: &Hash, _topoheight: TopoHeight) -> Result<bool, anyhow::Error> {
            Ok(true)
        }
    }

    #[test]
    fn test_executor_validate_compute_budget() {
        let mut provider = MockProvider::new();
        let bytecode = b"\x7FELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00"; // Minimal ELF header

        // Exceeds maximum
        let result = TakoExecutor::execute(
            bytecode,
            &mut provider,
            100,
            &Hash::zero(),
            &Hash::zero(),
            0,
            &Hash::zero(),
            &Hash::zero(),
            &[],
            Some(MAX_COMPUTE_BUDGET + 1),
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
    }

    #[test]
    fn test_executor_validate_elf() {
        let mut provider = MockProvider::new();
        let invalid_bytecode = b"not an ELF file";

        let result = TakoExecutor::execute_simple(
            invalid_bytecode,
            &mut provider,
            100,
            &Hash::zero(),
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = err.to_string();
        // The actual error from tos-tbpf ELF validation is "Invalid contract bytecode: Invalid ELF format"
        assert!(err_str.contains("Invalid") && err_str.contains("ELF"));
    }

    // Note: Full integration test with actual contract execution requires
    // a compiled TAKO contract (.so file). See integration tests for that.
}
