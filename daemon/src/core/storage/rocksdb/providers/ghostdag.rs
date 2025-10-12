// TOS GHOSTDAG RocksDB Storage Implementation
// Implements GhostdagDataProvider trait for RocksDB backend

use async_trait::async_trait;
use std::sync::Arc;
use log::trace;
use tos_common::crypto::Hash;

use crate::core::{
    error::BlockchainError,
    ghostdag::{BlueWorkType, CompactGhostdagData, KType, TosGhostdagData},
    storage::{
        rocksdb::{Column, RocksStorage},
        GhostdagDataProvider,
    },
};

#[async_trait]
impl GhostdagDataProvider for RocksStorage {
    async fn get_ghostdag_blue_score(&self, hash: &Hash) -> Result<u64, BlockchainError> {
        trace!("get ghostdag blue score for {}", hash);
        let compact: CompactGhostdagData = self.load_from_disk(Column::GhostdagCompact, hash)?;
        Ok(compact.blue_score)
    }

    async fn get_ghostdag_blue_work(&self, hash: &Hash) -> Result<BlueWorkType, BlockchainError> {
        trace!("get ghostdag blue work for {}", hash);
        let compact: CompactGhostdagData = self.load_from_disk(Column::GhostdagCompact, hash)?;
        Ok(compact.blue_work)
    }

    async fn get_ghostdag_selected_parent(&self, hash: &Hash) -> Result<Hash, BlockchainError> {
        trace!("get ghostdag selected parent for {}", hash);
        let compact: CompactGhostdagData = self.load_from_disk(Column::GhostdagCompact, hash)?;
        Ok(compact.selected_parent)
    }

    async fn get_ghostdag_mergeset_blues(&self, hash: &Hash) -> Result<Arc<Vec<Hash>>, BlockchainError> {
        trace!("get ghostdag mergeset blues for {}", hash);
        let data: TosGhostdagData = self.load_from_disk(Column::GhostdagData, hash)?;
        Ok(data.mergeset_blues)
    }

    async fn get_ghostdag_mergeset_reds(&self, hash: &Hash) -> Result<Arc<Vec<Hash>>, BlockchainError> {
        trace!("get ghostdag mergeset reds for {}", hash);
        let data: TosGhostdagData = self.load_from_disk(Column::GhostdagData, hash)?;
        Ok(data.mergeset_reds)
    }

    async fn get_ghostdag_blues_anticone_sizes(
        &self,
        hash: &Hash,
    ) -> Result<Arc<std::collections::HashMap<Hash, KType>>, BlockchainError> {
        trace!("get ghostdag blues anticone sizes for {}", hash);
        let data: TosGhostdagData = self.load_from_disk(Column::GhostdagData, hash)?;
        Ok(data.blues_anticone_sizes)
    }

    async fn get_ghostdag_data(&self, hash: &Hash) -> Result<Arc<TosGhostdagData>, BlockchainError> {
        trace!("get ghostdag data for {}", hash);
        let data: TosGhostdagData = self.load_from_disk(Column::GhostdagData, hash)?;
        Ok(Arc::new(data))
    }

    async fn get_ghostdag_compact_data(&self, hash: &Hash) -> Result<CompactGhostdagData, BlockchainError> {
        trace!("get ghostdag compact data for {}", hash);
        self.load_from_disk(Column::GhostdagCompact, hash)
    }

    async fn has_ghostdag_data(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        trace!("has ghostdag data for {}", hash);
        self.contains_data(Column::GhostdagData, hash)
    }

    async fn insert_ghostdag_data(&mut self, hash: &Hash, data: Arc<TosGhostdagData>) -> Result<(), BlockchainError> {
        trace!("insert ghostdag data for {}", hash);

        // Store full data
        self.insert_into_disk(Column::GhostdagData, hash, &*data)?;

        // Store compact data for efficient queries
        let compact: CompactGhostdagData = data.as_ref().into();
        self.insert_into_disk(Column::GhostdagCompact, hash, &compact)?;

        Ok(())
    }

    async fn delete_ghostdag_data(&mut self, hash: &Hash) -> Result<(), BlockchainError> {
        trace!("delete ghostdag data for {}", hash);

        // Delete both full and compact data
        self.remove_from_disk(Column::GhostdagData, hash)?;
        self.remove_from_disk(Column::GhostdagCompact, hash)?;

        Ok(())
    }
}
