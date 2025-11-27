mod accounts;
mod error;
mod executor;
mod executor_adapter;
mod feature_set;
mod loader;
pub mod precompile_cost;
mod precompile_verifier;
/// TOS Kernel(TAKO) integration module for TOS blockchain.
///
/// This module provides the adapter layer that bridges TOS blockchain's contract infrastructure
/// with TOS Kernel(TAKO)'s eBPF execution engine. It implements the dependency injection pattern to connect
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
/// TOS Kernel(TAKO) syscalls
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
/// - `executor_adapter`: ContractExecutor trait implementation for TOS Kernel(TAKO)
/// - `error`: Error types for TOS Kernel(TAKO) execution
/// - `precompile_verifier`: Transaction-level precompile verification (Ed25519, secp256k1, secp256r1)
mod storage;
pub mod transaction_cost;

pub use accounts::TosAccountAdapter;
pub use error::TakoExecutionError;
pub use executor::{ExecutionResult, TakoExecutor};
pub use executor_adapter::TakoContractExecutor;
pub use feature_set::SVMFeatureSet;
pub use loader::TosContractLoaderAdapter;
pub use precompile_cost::{
    costs, estimate_single_precompile_cost, estimate_transaction_precompile_cost,
    TransactionCostEstimator,
};
pub use precompile_verifier::{
    estimate_precompile_cost, verify_all_precompiles, verify_precompile_instruction,
};
pub use storage::TosStorageAdapter;
pub use transaction_cost::{estimate_transaction_cost, validate_transaction_cost, TransactionCost};
