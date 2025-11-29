//! Sled implementation of PruningCheckpointProvider

use async_trait::async_trait;
use log::trace;
use tos_common::serializer::Serializer;

use crate::core::{
    error::BlockchainError,
    storage::{
        sled::PRUNING_CHECKPOINT, PruningCheckpoint, PruningCheckpointProvider, SledStorage,
    },
};

#[async_trait]
impl PruningCheckpointProvider for SledStorage {
    async fn get_pruning_checkpoint(&self) -> Result<Option<PruningCheckpoint>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get pruning checkpoint");
        }

        self.load_optional_from_disk(&self.extra, PRUNING_CHECKPOINT)
    }

    async fn set_pruning_checkpoint(
        &mut self,
        checkpoint: &PruningCheckpoint,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set pruning checkpoint: phase={:?}, position={}, target={}",
                checkpoint.phase,
                checkpoint.current_position,
                checkpoint.target_topoheight
            );
        }

        let bytes = checkpoint.to_bytes();
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.extra,
            PRUNING_CHECKPOINT,
            bytes,
        )?;

        // Flush to ensure durability for crash recovery
        self.extra.flush()?;

        Ok(())
    }

    async fn clear_pruning_checkpoint(&mut self) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("clear pruning checkpoint");
        }

        Self::remove_from_disk_without_reading(
            self.snapshot.as_mut(),
            &self.extra,
            PRUNING_CHECKPOINT,
        )?;

        // Flush to ensure durability
        self.extra.flush()?;

        Ok(())
    }
}
