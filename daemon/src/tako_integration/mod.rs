/// TAKO VM integration module for TOS blockchain.
///
/// This module provides the adapter layer that bridges TOS blockchain's contract infrastructure
/// with TAKO VM's eBPF execution engine. It implements the dependency injection pattern to connect
/// TOS's storage, account, and contract systems to TAKO's syscall interfaces.
///
/// # Architecture
///
/// ```text
/// TOS Blockchain
///     ↓
/// ContractProvider trait (TOS interface)
///     ↓
/// Adapter Layer (this module)
///     ↓
/// TAKO VM syscalls
///     ↓
/// eBPF execution
/// ```
///
/// # Modules
///
/// - `storage`: Adapts TOS storage to TAKO's storage syscalls
/// - `accounts`: Adapts TOS account system to TAKO's balance/transfer syscalls
/// - `loader`: Adapts TOS contract loading to TAKO's cross-program invocation
/// - `executor`: Main TAKO execution engine with TOS integration
/// - `executor_adapter`: ContractExecutor trait implementation for TAKO VM
/// - `error`: Error types for TAKO VM execution

mod storage;
mod accounts;
mod loader;
mod executor;
mod executor_adapter;
mod error;

pub use storage::TosStorageAdapter;
pub use accounts::TosAccountAdapter;
pub use loader::TosContractLoaderAdapter;
pub use executor::{TakoExecutor, ExecutionResult};
pub use executor_adapter::TakoContractExecutor;
pub use error::TakoExecutionError;
