use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{BlockDifficulty, Column},
        DifficultyProvider, RocksStorage,
    },
};
use async_trait::async_trait;
use indexmap::IndexSet;
use log::trace;
use tos_common::{
    block::{BlockHeader, BlockVersion},
    crypto::Hash,
    difficulty::{CumulativeDifficulty, Difficulty},
    immutable::Immutable,
    time::TimestampMillis,
    varuint::VarUint,
};

#[async_trait]
impl DifficultyProvider for RocksStorage {
    // Get the block height using its hash
    async fn get_height_for_block_hash(&self, hash: &Hash) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get height for block hash {}", hash);
        }
        let header = self.get_block_header_by_hash(hash).await?;
        Ok(header.get_height())
    }

    // Get the block version using its hash
    async fn get_version_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<BlockVersion, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get version for block hash {}", hash);
        }
        let block = self.get_block_header_by_hash(hash).await?;
        Ok(block.get_version())
    }

    // Get the timestamp from the block using its hash
    async fn get_timestamp_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<TimestampMillis, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get timestamp for block hash {}", hash);
        }
        let header = self.get_block_header_by_hash(hash).await?;
        Ok(header.get_timestamp())
    }

    // Get the difficulty for a block hash
    async fn get_difficulty_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<Difficulty, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get difficulty for block hash {}", hash);
        }
        self.load_block_difficulty(hash)
            .map(|block_difficulty| block_difficulty.difficulty)
    }

    // Get the cumulative difficulty for a block hash
    async fn get_cumulative_difficulty_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<CumulativeDifficulty, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get cumulative difficulty for block hash {}", hash);
        }
        self.load_block_difficulty(hash)
            .map(|block_difficulty| block_difficulty.cumulative_difficulty)
    }

    // Get past blocks (block tips) for a specific block hash
    async fn get_past_blocks_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<Immutable<IndexSet<Hash>>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get past blocks for block hash {}", hash);
        }
        let header = self.get_block_header_by_hash(hash).await?;
        Ok(header.get_immutable_tips().clone())
    }

    // Get a block header using its hash
    async fn get_block_header_by_hash(
        &self,
        hash: &Hash,
    ) -> Result<Immutable<BlockHeader>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get block header by hash {}", hash);
        }
        self.load_from_disk(Column::Blocks, hash)
    }

    // Retrieve the estimated covariance (P) for a block hash
    async fn get_estimated_covariance_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<VarUint, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get estimated covariance for block hash {}", hash);
        }
        self.load_block_difficulty(hash)
            .map(|block_difficulty| block_difficulty.covariance)
    }
}

impl RocksStorage {
    fn load_block_difficulty(&self, hash: &Hash) -> Result<BlockDifficulty, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("load block difficulty {}", hash);
        }
        self.load_from_disk(Column::BlockDifficulty, hash)
    }
}
