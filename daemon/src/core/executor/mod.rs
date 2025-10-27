// Executor module - handles parallel transaction execution

pub mod parallel_executor;

pub use parallel_executor::{ParallelExecutor, get_optimal_parallelism};
