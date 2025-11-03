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
///
/// # Phase 1 Strategy
///
/// This integration follows a two-phase approach:
///
/// **Phase 1** (Current): Parallel coexistence
/// - Both TOS-VM and TAKO VM are supported
/// - Contract type auto-detection based on ELF magic number
/// - Zero modifications to existing TOS-VM code
/// - Shared storage layer via `ContractProvider` trait
///
/// **Phase 2** (Future): Full migration
/// - TAKO becomes the only VM
/// - TOS-VM deprecated
/// - All contracts migrated to eBPF format

mod storage;
mod accounts;
mod loader;
mod executor;
mod executor_adapter;
mod tosvm_executor;
mod multi_executor;

pub use storage::TosStorageAdapter;
pub use accounts::TosAccountAdapter;
pub use loader::TosContractLoaderAdapter;
pub use executor::{TakoExecutor, ExecutionResult};
pub use executor_adapter::TakoContractExecutor;
pub use tosvm_executor::TosVmExecutor;
pub use multi_executor::MultiExecutor;
