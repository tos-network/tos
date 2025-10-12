// TOS GHOSTDAG Sled Storage Implementation
// Implements GhostdagDataProvider trait for Sled backend

use async_trait::async_trait;
use std::sync::Arc;
use log::trace;
use tos_common::{crypto::Hash, serializer::Serializer};

use crate::core::{
    error::{BlockchainError, DiskContext},
    ghostdag::{BlueWorkType, CompactGhostdagData, KType, TosGhostdagData},
    storage::{GhostdagDataProvider, SledStorage},
};

#[async_trait]
impl GhostdagDataProvider for SledStorage {
    async fn get_ghostdag_blue_score(&self, hash: &Hash) -> Result<u64, BlockchainError> {
        trace!("get ghostdag blue score for {}", hash);
        let compact: CompactGhostdagData = self.load_from_disk(&self.ghostdag_compact, hash.as_bytes(), DiskContext::GhostdagCompact)?;
        Ok(compact.blue_score)
    }

    async fn get_ghostdag_blue_work(&self, hash: &Hash) -> Result<BlueWorkType, BlockchainError> {
        trace!("get ghostdag blue work for {}", hash);
        let compact: CompactGhostdagData = self.load_from_disk(&self.ghostdag_compact, hash.as_bytes(), DiskContext::GhostdagCompact)?;
        Ok(compact.blue_work)
    }

    async fn get_ghostdag_selected_parent(&self, hash: &Hash) -> Result<Hash, BlockchainError> {
        trace!("get ghostdag selected parent for {}", hash);
        let compact: CompactGhostdagData = self.load_from_disk(&self.ghostdag_compact, hash.as_bytes(), DiskContext::GhostdagCompact)?;
        Ok(compact.selected_parent)
    }

    async fn get_ghostdag_mergeset_blues(&self, hash: &Hash) -> Result<Arc<Vec<Hash>>, BlockchainError> {
        trace!("get ghostdag mergeset blues for {}", hash);
        let data: TosGhostdagData = self.load_from_disk(&self.ghostdag_data, hash.as_bytes(), DiskContext::GhostdagData)?;
        Ok(data.mergeset_blues)
    }

    async fn get_ghostdag_mergeset_reds(&self, hash: &Hash) -> Result<Arc<Vec<Hash>>, BlockchainError> {
        trace!("get ghostdag mergeset reds for {}", hash);
        let data: TosGhostdagData = self.load_from_disk(&self.ghostdag_data, hash.as_bytes(), DiskContext::GhostdagData)?;
        Ok(data.mergeset_reds)
    }

    async fn get_ghostdag_blues_anticone_sizes(
        &self,
        hash: &Hash,
    ) -> Result<Arc<std::collections::HashMap<Hash, KType>>, BlockchainError> {
        trace!("get ghostdag blues anticone sizes for {}", hash);
        let data: TosGhostdagData = self.load_from_disk(&self.ghostdag_data, hash.as_bytes(), DiskContext::GhostdagData)?;
        Ok(data.blues_anticone_sizes)
    }

    async fn get_ghostdag_data(&self, hash: &Hash) -> Result<Arc<TosGhostdagData>, BlockchainError> {
        trace!("get ghostdag data for {}", hash);
        let data: TosGhostdagData = self.load_from_disk(&self.ghostdag_data, hash.as_bytes(), DiskContext::GhostdagData)?;
        Ok(Arc::new(data))
    }

    async fn get_ghostdag_compact_data(&self, hash: &Hash) -> Result<CompactGhostdagData, BlockchainError> {
        trace!("get ghostdag compact data for {}", hash);
        self.load_from_disk(&self.ghostdag_compact, hash.as_bytes(), DiskContext::GhostdagCompact)
    }

    async fn has_ghostdag_data(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        trace!("has ghostdag data for {}", hash);
        self.contains_data(&self.ghostdag_data, hash)
    }

    async fn insert_ghostdag_data(&mut self, hash: &Hash, data: Arc<TosGhostdagData>) -> Result<(), BlockchainError> {
        trace!("insert ghostdag data for {}", hash);

        // Serialize using Serializer trait
        let data_bytes = data.as_ref().to_bytes();
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.ghostdag_data,
            hash.as_bytes(),
            data_bytes,
        )?;

        // Store compact data for efficient queries
        let compact: CompactGhostdagData = data.as_ref().into();
        let compact_bytes = compact.to_bytes();
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.ghostdag_compact,
            hash.as_bytes(),
            compact_bytes,
        )?;

        Ok(())
    }

    async fn delete_ghostdag_data(&mut self, hash: &Hash) -> Result<(), BlockchainError> {
        trace!("delete ghostdag data for {}", hash);

        // Delete both full and compact data
        Self::remove_from_disk::<TosGhostdagData>(self.snapshot.as_mut(), &self.ghostdag_data, hash.as_bytes())?;
        Self::remove_from_disk::<CompactGhostdagData>(self.snapshot.as_mut(), &self.ghostdag_compact, hash.as_bytes())?;

        Ok(())
    }
}
