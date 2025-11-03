/// Contract execution trait for dependency injection
///
/// This trait enables the common package to execute contracts without depending
/// on specific VM implementations (TOS-VM, TAKO VM, etc.). The daemon package
/// implements this trait and injects the executor into the transaction processor.
///
/// # Architecture
///
/// ```text
/// Common Package (transaction logic)
///     ↓ defines trait
/// ContractExecutor trait
///     ↑ implements
/// Daemon Package (VM implementations)
/// ```
///
/// This follows Solana's pattern of dependency injection for VM execution.

use async_trait::async_trait;
use anyhow::Result;

use crate::{
    block::TopoHeight,
    contract::ContractProvider,
    crypto::Hash,
};

/// Result of contract execution
///
/// This is a simplified result type that bridges VM-specific execution results
/// to the transaction processing layer.
#[derive(Debug, Clone)]
pub struct ContractExecutionResult {
    /// Gas consumed by the contract execution
    pub gas_used: u64,

    /// Exit code from the contract
    /// - `Some(0)` = success
    /// - `Some(x)` where x != 0 = error with code x
    /// - `None` = execution failed without exit code
    pub exit_code: Option<u64>,

    /// Optional return data from the contract
    /// Used for inter-contract calls and debugging
    pub return_data: Option<Vec<u8>>,
}

/// Contract executor trait
///
/// Implementations of this trait handle the actual execution of contract bytecode.
/// Different implementations can support different VM types (TOS-VM, TAKO VM, etc.).
///
/// # Example
///
/// ```ignore
/// use tos_common::contract::{ContractExecutor, ContractExecutionResult};
///
/// struct MyExecutor;
///
/// #[async_trait]
/// impl ContractExecutor for MyExecutor {
///     async fn execute<P: ContractProvider>(
///         &self,
///         bytecode: &[u8],
///         provider: &mut P,
///         // ... other parameters
///     ) -> Result<ContractExecutionResult> {
///         // Execute bytecode and return result
///         Ok(ContractExecutionResult {
///             gas_used: 1000,
///             exit_code: Some(0),
///             return_data: None,
///         })
///     }
/// }
/// ```
#[async_trait]
pub trait ContractExecutor: Send + Sync {
    /// Execute contract bytecode
    ///
    /// # Arguments
    ///
    /// * `bytecode` - Raw contract bytecode (ELF, TOS-VM format, etc.)
    /// * `provider` - Contract provider for storage/account operations
    /// * `topoheight` - Current topoheight for versioned reads
    /// * `contract_hash` - Hash of the contract being executed
    /// * `block_hash` - Current block hash
    /// * `block_height` - Current block height
    /// * `tx_hash` - Transaction hash
    /// * `tx_sender` - Transaction sender's address (as Hash)
    /// * `max_gas` - Maximum gas allowed for this execution
    /// * `parameters` - Optional execution parameters (VM-specific)
    ///
    /// # Returns
    ///
    /// `ContractExecutionResult` containing gas used, exit code, and optional return data
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Bytecode is invalid or unsupported format
    /// - VM creation/initialization fails
    /// - Execution fails in unrecoverable way
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
    ) -> Result<ContractExecutionResult>;

    /// Check if this executor supports the given bytecode format
    ///
    /// This allows the transaction processor to select the appropriate
    /// executor based on the bytecode format.
    ///
    /// # Arguments
    ///
    /// * `bytecode` - Contract bytecode to check
    ///
    /// # Returns
    ///
    /// `true` if this executor can execute the bytecode, `false` otherwise
    fn supports_format(&self, bytecode: &[u8]) -> bool;

    /// Get a human-readable name for this executor
    ///
    /// Used for logging and debugging.
    fn name(&self) -> &'static str;
}

/// Default no-op executor for testing and fallback
///
/// This executor always returns an error when attempting to execute.
/// Useful as a placeholder when no real executor is available.
pub struct NoOpExecutor;

#[async_trait]
impl ContractExecutor for NoOpExecutor {
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
    ) -> Result<ContractExecutionResult> {
        anyhow::bail!("NoOpExecutor: No contract executor configured")
    }

    fn supports_format(&self, _bytecode: &[u8]) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "NoOpExecutor"
    }
}
