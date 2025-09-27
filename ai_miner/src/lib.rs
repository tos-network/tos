//! TOS AI Miner Library
//!
//! This library provides components for AI mining operations on the TOS network.

pub mod daemon_client;
pub mod transaction_builder;
pub mod storage;
pub mod config;

pub use daemon_client::{DaemonClient, DaemonClientConfig, DaemonHealthStatus};
pub use transaction_builder::{AIMiningTransactionBuilder, AIMiningTransactionMetadata};
pub use storage::{StorageManager, TaskInfo, TaskState, TransactionRecord};
pub use config::ConfigValidationError;