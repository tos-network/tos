//! TOS AI Miner Library
//!
//! This library provides components for AI mining operations on the TOS network.

pub mod config;
pub mod daemon_client;
pub mod storage;
pub mod transaction_builder;

pub use config::ConfigValidationError;
pub use daemon_client::{DaemonClient, DaemonClientConfig, DaemonHealthStatus};
pub use storage::{StorageManager, TaskInfo, TaskState, TransactionRecord};
pub use transaction_builder::{AIMiningTransactionBuilder, AIMiningTransactionMetadata};
