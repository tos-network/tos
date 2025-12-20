use crate::core::error::BlockchainError;
use async_trait::async_trait;

#[async_trait]
pub trait SnapshotProvider {
    // Check if we have a snapshot already set
    async fn has_snapshot(&self) -> Result<bool, BlockchainError>;

    // Start a snapshot
    // This is useful to do some operations before applying the batch
    async fn start_snapshot(&mut self) -> Result<(), BlockchainError>;

    // Apply the batch to the storage
    async fn end_snapshot(&mut self, apply: bool) -> Result<(), BlockchainError>;
}
