use crate::core::{error::BlockchainError, storage::rocksdb::Snapshot};
use async_trait::async_trait;
use std::hash::Hash;

#[async_trait]
pub trait SnapshotProvider {
    type Column: Hash + Eq;

    // Check if we have a snapshot already set
    async fn has_snapshot(&self) -> Result<bool, BlockchainError>;

    // Start a snapshot
    // This is useful to do some operations before applying the batch
    async fn start_snapshot(&mut self) -> Result<(), BlockchainError>;

    // Apply the batch to the storage
    async fn end_snapshot(&mut self, apply: bool) -> Result<(), BlockchainError>;

    // Swap the current snapshot with another one
    // Returns the previous snapshot if one was set
    async fn swap_snapshot(&mut self, other: Snapshot)
        -> Result<Option<Snapshot>, BlockchainError>;
}
