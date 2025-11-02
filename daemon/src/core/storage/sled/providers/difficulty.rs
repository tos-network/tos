use crate::core::{
    error::{BlockchainError, DiskContext},
    storage::{DifficultyProvider, SledStorage},
};
use async_trait::async_trait;
use indexmap::IndexSet;
use log::trace;
use tos_common::{
    block::{BlockHeader, BlockVersion},
    crypto::Hash,
    difficulty::Difficulty,
    immutable::Immutable,
    time::TimestampMillis,
    varuint::VarUint,
};

#[async_trait]
impl DifficultyProvider for SledStorage {
    // Optimized: Reads 8 bytes instead of 500-800 byte header (62x-100x faster)
    async fn get_blue_score_for_block_hash(&self, hash: &Hash) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get blue_score for block hash {}", hash);
        }
        self.load_from_disk(
            &self.block_blue_score,
            hash.as_bytes(),
            DiskContext::BlueScoreForBlockHash,
        )
    }

    // Optimized: Reads 1-2 bytes instead of 500-800 byte header (250x-800x faster)
    async fn get_version_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<BlockVersion, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get version for block hash {}", hash);
        }
        self.load_from_disk(
            &self.block_version,
            hash.as_bytes(),
            DiskContext::VersionForBlockHash,
        )
    }

    // Optimized: Reads 8 bytes instead of 500-800 byte header (62x-100x faster)
    async fn get_timestamp_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<TimestampMillis, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get timestamp for hash {}", hash);
        }
        self.load_from_disk(
            &self.block_timestamp,
            hash.as_bytes(),
            DiskContext::TimestampForBlockHash,
        )
    }

    async fn get_difficulty_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<Difficulty, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get difficulty for hash {}", hash);
        }
        self.load_from_disk(
            &self.difficulty,
            hash.as_bytes(),
            DiskContext::DifficultyForBlockHash,
        )
    }

    // Phase 2: get_cumulative_difficulty_for_block_hash method removed
    // Use GhostdagDataProvider::get_ghostdag_blue_work() instead

    async fn get_past_blocks_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<Immutable<IndexSet<Hash>>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get past blocks of {}", hash);
        }
        let block = self.get_block_header_by_hash(hash).await?;
        let tips: IndexSet<Hash> = block.get_parents().iter().cloned().collect();
        Ok(Immutable::Owned(tips))
    }

    async fn get_block_header_by_hash(
        &self,
        hash: &Hash,
    ) -> Result<Immutable<BlockHeader>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get block by hash: {}", hash);
        }
        self.get_cacheable_arc_data(
            &self.blocks,
            &self.blocks_cache,
            hash,
            DiskContext::GetBlockHeaderByHash,
        )
        .await
    }

    async fn get_estimated_covariance_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<VarUint, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get p for hash {}", hash);
        }
        self.load_from_disk(
            &self.difficulty_covariance,
            hash.as_bytes(),
            DiskContext::EstimatedCovarianceForBlockHash,
        )
    }
}
