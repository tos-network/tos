use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, Snapshot},
        CacheProvider, RocksStorage, SnapshotProvider,
    },
};
use async_trait::async_trait;
use log::{debug, trace};

#[async_trait]
impl SnapshotProvider for RocksStorage {
    type Column = Column;

    // Check if we have a snapshot already set
    async fn has_snapshot(&self) -> Result<bool, BlockchainError> {
        trace!("has snapshot");
        Ok(self.snapshot.is_some())
    }

    async fn start_snapshot(&mut self) -> Result<(), BlockchainError> {
        trace!("start snapshot");
        if self.snapshot.is_some() {
            return Err(BlockchainError::SnapshotAlreadyStarted);
        }

        // Deep clone the current cache state for the snapshot
        // This allows independent modification during the snapshot
        self.snapshot = Some(Snapshot::new(self.cache.clone_mut()));
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

            // Get ownership of both trees and cache from the snapshot
            let (trees, cache) = snapshot.into_parts();

            // Apply disk changes from the snapshot
            for (column, changes) in trees {
                for (key, value) in changes {
                    if let Some(value) = value {
                        self.insert_raw_into_disk(column, key.as_ref(), value.as_ref())?;
                    } else {
                        self.remove_from_disk(column, key.as_ref())?;
                    }
                }
            }

            // Transfer the snapshot's cache state to the main storage
            // This ensures cache state is atomically updated with disk changes
            self.cache = cache;
        } else {
            debug!("Clearing caches due to invalidation of the snapshot");
            // On rollback, the snapshot's cache is simply discarded
            // The original storage cache remains unchanged
            self.clear_objects_cache().await?;
        }

        Ok(())
    }

    async fn swap_snapshot(
        &mut self,
        other: Snapshot,
    ) -> Result<Option<Snapshot>, BlockchainError> {
        trace!("swap snapshot");
        Ok(self.snapshot.replace(other))
    }
}
