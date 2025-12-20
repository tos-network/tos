use crate::core::{
    error::BlockchainError,
    storage::{sled::Snapshot, CacheProvider, SledStorage, SnapshotProvider},
};
use async_trait::async_trait;
use log::{debug, trace};

#[async_trait]
impl SnapshotProvider for SledStorage {
    // Check if we have a snapshot already set
    async fn has_snapshot(&self) -> Result<bool, BlockchainError> {
        Ok(self.snapshot.is_some())
    }

    async fn start_snapshot(&mut self) -> Result<(), BlockchainError> {
        trace!("Starting snapshot");
        if self.snapshot.is_some() {
            return Err(BlockchainError::SnapshotAlreadyStarted);
        }

        let snapshot = Snapshot::new(self.cache.clone());
        self.snapshot = Some(snapshot);
        Ok(())
    }

    async fn end_snapshot(&mut self, apply: bool) -> Result<(), BlockchainError> {
        trace!("end snapshot");
        let snapshot = self
            .snapshot
            .take()
            .ok_or(BlockchainError::SnapshotNotStarted)?;

        if apply {
            self.cache = snapshot.cache;

            for (tree, batch) in snapshot.trees {
                trace!("Applying batch to tree {:?}", tree);
                match batch {
                    Some(batch) => {
                        let tree = self.db.open_tree(tree)?;
                        for (key, value) in batch.into_iter() {
                            match value {
                                Some(value) => tree.insert(key, value)?,
                                None => tree.remove(key)?,
                            };
                        }
                    }
                    None => {
                        trace!("Dropping tree {:?}", tree);
                        self.db.drop_tree(tree)?;
                    }
                };
            }
        } else {
            debug!("Clearing caches due to invalidation of the snapshot");
            self.clear_caches().await?;
        }

        Ok(())
    }
}
