//! RocksDB implementation of PruningCheckpointProvider

use async_trait::async_trait;
use log::trace;

use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, RocksStorage},
        PruningCheckpoint, PruningCheckpointProvider,
    },
};

/// Key used for storing the pruning checkpoint
const PRUNING_CHECKPOINT_KEY: &[u8] = b"PRUNING_CHECKPOINT";

#[async_trait]
impl PruningCheckpointProvider for RocksStorage {
    async fn get_pruning_checkpoint(&self) -> Result<Option<PruningCheckpoint>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get pruning checkpoint");
        }

        self.load_optional_from_disk(Column::PruningCheckpoint, &PRUNING_CHECKPOINT_KEY.to_vec())
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

        // Use sync write to ensure checkpoint durability
        // This is critical for crash recovery
        self.insert_into_disk_sync(
            Column::PruningCheckpoint,
            PRUNING_CHECKPOINT_KEY.to_vec(),
            checkpoint,
        )
    }

    async fn clear_pruning_checkpoint(&mut self) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("clear pruning checkpoint");
        }

        self.remove_from_disk(Column::PruningCheckpoint, PRUNING_CHECKPOINT_KEY.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::storage::rocksdb::RocksStorage;
    use tempdir::TempDir;
    use tos_common::network::Network;

    fn create_test_storage() -> (RocksStorage, TempDir) {
        let tmp_dir = TempDir::new("rocks-pruning-checkpoint-test").expect("Failed to create temp dir");
        let config = crate::core::config::RocksDBConfig::default();
        let storage = RocksStorage::new(
            tmp_dir.path().to_str().expect("Invalid temp dir path"),
            Network::Testnet,
            None,
            &config,
        );
        (storage, tmp_dir)
    }

    #[tokio::test]
    async fn test_checkpoint_crud() {
        let (mut storage, _tmp_dir) = create_test_storage();

        // Initially no checkpoint
        let checkpoint = storage.get_pruning_checkpoint().await.expect("Failed to get checkpoint");
        assert!(checkpoint.is_none());

        // Set checkpoint
        let checkpoint = PruningCheckpoint::new(100, 500);
        storage.set_pruning_checkpoint(&checkpoint).await.expect("Failed to set checkpoint");

        // Read it back
        let loaded = storage.get_pruning_checkpoint().await.expect("Failed to get checkpoint");
        assert!(loaded.is_some());
        let loaded = loaded.expect("Expected checkpoint");
        assert_eq!(loaded.start_topoheight, 100);
        assert_eq!(loaded.target_topoheight, 500);

        // Clear it
        storage.clear_pruning_checkpoint().await.expect("Failed to clear checkpoint");

        // Should be gone
        let checkpoint = storage.get_pruning_checkpoint().await.expect("Failed to get checkpoint");
        assert!(checkpoint.is_none());
    }

    #[tokio::test]
    async fn test_has_incomplete_pruning() {
        let (mut storage, _tmp_dir) = create_test_storage();

        // No checkpoint = no incomplete pruning
        assert!(!storage.has_incomplete_pruning().await.expect("Failed to check"));

        // Set incomplete checkpoint
        let checkpoint = PruningCheckpoint::new(0, 100);
        storage.set_pruning_checkpoint(&checkpoint).await.expect("Failed to set");
        assert!(storage.has_incomplete_pruning().await.expect("Failed to check"));

        // Complete the checkpoint
        let mut checkpoint = storage.get_pruning_checkpoint().await.expect("Failed to get").expect("Expected checkpoint");
        checkpoint.phase = crate::core::storage::PruningPhase::Complete;
        storage.set_pruning_checkpoint(&checkpoint).await.expect("Failed to set");

        // Should not be incomplete anymore
        assert!(!storage.has_incomplete_pruning().await.expect("Failed to check"));
    }
}
