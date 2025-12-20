use crate::core::{
    error::BlockchainError,
    storage::{rocksdb::Snapshot, CacheProvider, RocksStorage, SnapshotProvider},
};
use async_trait::async_trait;
use log::{debug, trace};

#[async_trait]
impl SnapshotProvider for RocksStorage {
    // Check if we have a snapshot already set
    async fn has_snapshot(&self) -> Result<bool, BlockchainError> {
        Ok(self.snapshot.is_some())
    }

    async fn start_snapshot(&mut self) -> Result<(), BlockchainError> {
        trace!("starting snapshot");
        if self.snapshot.is_some() {
            return Err(BlockchainError::SnapshotAlreadyStarted);
        }

        self.snapshot = Some(Snapshot::new());
        Ok(())
    }

    async fn end_snapshot(&mut self, apply: bool) -> Result<(), BlockchainError> {
        trace!("end snapshot");
        let snapshot = self
            .snapshot
            .take()
            .ok_or(BlockchainError::SnapshotNotStarted)?;

        if apply {
            trace!("applying snapshot");
            for (column, batch) in snapshot.columns {
                for (key, value) in batch {
                    if let Some(value) = value {
                        self.insert_into_disk(column, &key.as_ref(), &value.as_ref())?;
                    } else {
                        self.remove_from_disk(column, &key.as_ref())?;
                    }
                }
            }
        } else {
            debug!("Clearing caches due to invalidation of the snapshot");
            self.clear_caches().await?;
        }

        Ok(())
    }
}
