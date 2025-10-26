// TOS Parallel Transaction Execution Engine
// Based on Solana's thread-aware account locking mechanism
//
// This module implements deterministic parallel transaction execution using:
// - Pre-declared account dependencies (V2 transactions)
// - Thread-aware read/write locks for conflict detection
// - Look-ahead scheduling for maximizing parallelism
//
// Reference: ~/tos-network/memo/TOS_PARALLEL_EXECUTION_DESIGN_V2.md

mod account_locks;
mod scheduler;

pub use account_locks::{
    ThreadSet,
    ThreadId,
    LockCount,
    ThreadAwareAccountLocks,
    TryLockError,
};

pub use scheduler::{
    ParallelScheduler,
    TransactionExecutionResult,
    ExecutionStats,
};
