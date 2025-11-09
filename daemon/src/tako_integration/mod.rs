mod accounts;
mod error;
mod executor;
mod executor_adapter;
mod loader;
pub mod precompile_cost;
mod precompile_verifier;
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
/// - `precompile_verifier`: Transaction-level precompile verification (Ed25519, secp256k1, secp256r1)
mod storage;

pub use accounts::TosAccountAdapter;
pub use error::TakoExecutionError;
pub use executor::{ExecutionResult, TakoExecutor};
pub use executor_adapter::TakoContractExecutor;
pub use loader::TosContractLoaderAdapter;
pub use precompile_cost::{
    costs, estimate_single_precompile_cost, estimate_transaction_precompile_cost,
    TransactionCostEstimator,
};
pub use precompile_verifier::{
    estimate_precompile_cost, verify_all_precompiles, verify_precompile_instruction,
};
pub use storage::TosStorageAdapter;
