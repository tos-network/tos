use crate::core::{
    error::BlockchainError,
    storage::{rocksdb::Snapshot, CacheProvider, CommitPointProvider, RocksStorage},
};
use anyhow::Context;
use async_trait::async_trait;
use log::{debug, trace};
use rocksdb::WriteBatch;

#[async_trait]
impl CommitPointProvider for RocksStorage {
    // Check if we have a commit point already set
    async fn has_commit_point(&self) -> Result<bool, BlockchainError> {
        Ok(self.snapshot.is_some())
    }

    async fn start_commit_point(&mut self) -> Result<(), BlockchainError> {
        trace!("starting commit point");
        if self.snapshot.is_some() {
            return Err(BlockchainError::CommitPointAlreadyStarted);
        }

        self.snapshot = Some(Snapshot::new());
        Ok(())
    }

    async fn end_commit_point(&mut self, apply: bool) -> Result<(), BlockchainError> {
        trace!("end commit point");
        let snapshot = self
            .snapshot
            .take()
            .ok_or(BlockchainError::CommitPointNotStarted)?;

        if apply {
            trace!("applying commit point with atomic WriteBatch");

            // Create a single WriteBatch for atomic commit
            let mut write_batch = WriteBatch::default();

            // Add all operations to the batch
            for (column, batch) in snapshot.columns {
                let cf = self
                    .db
                    .cf_handle(column.as_ref())
                    .with_context(|| format!("Column {:?} not found", column))?;
                for (key, value) in batch {
                    if let Some(value) = value {
                        write_batch.put_cf(&cf, key.as_ref(), value.as_ref());
                    } else {
                        write_batch.delete_cf(&cf, key.as_ref());
                    }
                }
            }

            // Atomically write the entire batch to disk
            // All operations succeed or all fail together
            self.db
                .write(write_batch)
                .context("Failed to commit atomic write batch")?;

            if log::log_enabled!(log::Level::Trace) {
                trace!("Successfully committed atomic write batch");
            }
        } else {
            debug!("Clearing caches due to invalidation of the commit point");
            self.clear_caches().await?;
        }

        Ok(())
    }
}
