// Executor module - handles parallel transaction execution

pub mod parallel_executor_v3;

pub use parallel_executor_v3::{ParallelExecutor, get_optimal_parallelism};
