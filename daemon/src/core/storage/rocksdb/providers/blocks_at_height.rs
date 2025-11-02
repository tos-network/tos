use std::borrow::Cow;

use crate::core::{
    error::BlockchainError,
    storage::{rocksdb::Column, BlocksAtHeightProvider, RocksStorage},
};
use async_trait::async_trait;
use indexmap::IndexSet;
use log::trace;
use tos_common::crypto::Hash;

#[async_trait]
impl BlocksAtHeightProvider for RocksStorage {
    // Check if there are blocks at a specific blue_score (DAG depth position)
    async fn has_blocks_at_blue_score(&self, blue_score: u64) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has blocks at blue_score {}", blue_score);
        }
        self.contains_data(Column::BlocksAtHeight, &blue_score.to_be_bytes())
    }

    // Retrieve the blocks hashes at a specific blue_score (DAG depth position)
    async fn get_blocks_at_blue_score(
        &self,
        blue_score: u64,
    ) -> Result<IndexSet<Hash>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get blocks at blue_score {}", blue_score);
        }
        self.load_optional_from_disk(Column::BlocksAtHeight, &blue_score.to_be_bytes())
            .map(|v| v.unwrap_or_default())
    }

    // Store the blocks hashes at a specific blue_score (DAG depth position)
    async fn set_blocks_at_blue_score(
        &mut self,
        tips: &IndexSet<Hash>,
        blue_score: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set blocks at blue_score {}", blue_score);
        }
        self.insert_into_disk(Column::BlocksAtHeight, blue_score.to_be_bytes(), tips)
    }

    // Append a block hash at a specific blue_score (DAG depth position)
    async fn add_block_hash_at_blue_score(
        &mut self,
        hash: &Hash,
        blue_score: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("add block hash at blue_score {}", blue_score);
        }
        let mut blocks: IndexSet<Cow<'_, Hash>> = self
            .load_optional_from_disk(Column::BlocksAtHeight, &blue_score.to_be_bytes())?
            .unwrap_or_default();

        if blocks.insert(Cow::Borrowed(hash)) {
            if log::log_enabled!(log::Level::Trace) {
                trace!("inserted block hash at blue_score {}", blue_score);
            }
            self.insert_into_disk(Column::BlocksAtHeight, blue_score.to_be_bytes(), &blocks)?;
        }

        Ok(())
    }

    // Remove a block hash at a specific blue_score (DAG depth position)
    async fn remove_block_hash_at_blue_score(
        &mut self,
        hash: &Hash,
        blue_score: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("remove block hash at blue_score {}", blue_score);
        }
        let Some(mut blocks): Option<IndexSet<Cow<'_, Hash>>> =
            self.load_optional_from_disk(Column::BlocksAtHeight, &blue_score.to_be_bytes())?
        else {
            return Ok(());
        };

        if blocks.shift_remove(&Cow::Borrowed(hash)) {
            if log::log_enabled!(log::Level::Trace) {
                trace!("removed block hash at blue_score {}", blue_score);
            }
            self.insert_into_disk(Column::BlocksAtHeight, hash, &blocks)?;
        }

        Ok(())
    }
}
