use super::{ContractCache, TransferOutput};
use anyhow::Result;
/// Contract execution trait for dependency injection
///
/// This trait enables the common package to execute contracts without depending
/// on specific VM implementations (legacy VM, TOS Kernel(TAKO), etc.). The daemon package
/// implements this trait and injects the executor into the transaction processor.
///
/// # Architecture
///
/// ```text
/// Common Package (transaction logic)
///     | defines trait
/// ContractExecutor trait
///     ^ implements
/// Daemon Package (VM implementations)
/// ```
///
/// This follows SVM pattern of dependency injection for VM execution.
use async_trait::async_trait;

use crate::{block::TopoHeight, contract::ContractProvider, crypto::Hash};

/// Contract event emitted during execution
///
/// This is a VM-agnostic representation of contract events that bridges
/// different VM implementations (TAKO, legacy) to a common format.
/// Events are Ethereum-compatible with indexed topics and arbitrary data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractEvent {
    /// Contract address that emitted the event (as 32-byte array)
    pub contract: [u8; 32],
    /// Indexed topics for efficient filtering (max 4, Ethereum-compatible)
    /// topic[0] is typically the event signature hash
    pub topics: Vec<[u8; 32]>,
    /// Non-indexed event data (ABI-encoded parameters)
    pub data: Vec<u8>,
}

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

    /// Transfers requested by the contract during execution
    pub transfers: Vec<TransferOutput>,

    /// Events emitted by the contract during execution (Ethereum-style)
    /// Contains indexed topics and data for off-chain indexing and monitoring
    pub events: Vec<ContractEvent>,

    /// Optional contract cache overlay produced by the VM during execution.
    ///
    /// Contains storage writes made via `storage_write` syscall.
    /// Only merged to persistent storage when execution succeeds (exit_code == Some(0)).
    /// On failure, this cache is discarded to ensure atomic rollback.
    pub cache: Option<ContractCache>,
}

/// Contract executor trait
///
/// Implementations of this trait handle the actual execution of contract bytecode.
/// Different implementations can support different VM types (legacy VM, TOS Kernel(TAKO), etc.).
///
/// # Example
///
/// ```rust
/// use tos_common::contract::{ContractExecutor, ContractExecutionResult};
/// use tos_common::crypto::Hash;
/// use tos_common::block::TopoHeight;
/// use async_trait::async_trait;
/// use anyhow::Result;
///
/// struct MyExecutor;
///
/// #[async_trait]
/// impl ContractExecutor for MyExecutor {
///     async fn execute(
///         &self,
///         _bytecode: &[u8],
///         _provider: &(dyn tos_common::contract::ContractProvider + Send),
///         _topoheight: TopoHeight,
///         _contract_hash: &Hash,
///         _block_hash: &Hash,
///         _block_height: u64,
///         _block_timestamp: u64,
///         _tx_hash: &Hash,
///         _tx_sender: &Hash,
///         _max_gas: u64,
///         _parameters: Option<Vec<u8>>,
///     ) -> Result<ContractExecutionResult> {
///         // Execute bytecode and return result
///         Ok(ContractExecutionResult {
///             gas_used: 1000,
///             exit_code: Some(0),
///             return_data: None,
///             transfers: vec![],
///             events: vec![],
///             cache: None,
///         })
///     }
///
///     fn supports_format(&self, _bytecode: &[u8]) -> bool {
///         true
///     }
///
///     fn name(&self) -> &'static str {
///         "MyExecutor"
///     }
/// }
///
/// // Verify the executor can be created
/// let executor = MyExecutor;
/// assert_eq!(executor.name(), "MyExecutor");
/// assert!(executor.supports_format(&[]));
/// ```
#[async_trait]
pub trait ContractExecutor: Send + Sync {
    /// Execute contract bytecode
    ///
    /// # Arguments
    ///
    /// * `bytecode` - Raw contract bytecode (ELF format for TOS Kernel(TAKO), etc.)
    /// * `provider` - Contract provider for storage/account operations
    /// * `topoheight` - Current topoheight for versioned reads
    /// * `contract_hash` - Hash of the contract being executed
    /// * `block_hash` - Current block hash
    /// * `block_height` - Current block height
    /// * `block_timestamp` - Current block timestamp (Unix timestamp in seconds)
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
    async fn execute(
        &self,
        bytecode: &[u8],
        provider: &(dyn ContractProvider + Send),
        topoheight: TopoHeight,
        contract_hash: &Hash,
        block_hash: &Hash,
        block_height: u64,
        block_timestamp: u64,
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
    async fn execute(
        &self,
        _bytecode: &[u8],
        _provider: &(dyn ContractProvider + Send),
        _topoheight: TopoHeight,
        _contract_hash: &Hash,
        _block_hash: &Hash,
        _block_height: u64,
        _block_timestamp: u64,
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
