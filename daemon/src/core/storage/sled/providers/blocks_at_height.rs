use std::borrow::Cow;

use crate::core::{
    error::BlockchainError,
    storage::{BlocksAtHeightProvider, OrderedHashes, SledStorage},
};
use async_trait::async_trait;
use indexmap::IndexSet;
use log::trace;
use tos_common::{crypto::Hash, serializer::Serializer};

#[async_trait]
impl BlocksAtHeightProvider for SledStorage {
    async fn has_blocks_at_blue_score(&self, blue_score: u64) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has blocks at blue_score {}", blue_score);
        }
        self.contains_data(&self.blocks_at_height, &blue_score.to_be_bytes())
    }

    async fn get_blocks_at_blue_score(
        &self,
        blue_score: u64,
    ) -> Result<IndexSet<Hash>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get blocks at blue_score {}", blue_score);
        }
        let hashes = self
            .load_optional_from_disk::<OrderedHashes>(
                &self.blocks_at_height,
                &blue_score.to_be_bytes(),
            )?
            .unwrap_or_default();
        Ok(hashes.0.into_owned())
    }

    async fn set_blocks_at_blue_score(
        &mut self,
        tips: &IndexSet<Hash>,
        blue_score: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set {} blocks at blue_score {}", tips.len(), blue_score);
        }
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.blocks_at_height,
            &blue_score.to_be_bytes(),
            OrderedHashes(Cow::Borrowed(tips)).to_bytes(),
        )?;
        Ok(())
    }

    async fn add_block_hash_at_blue_score(
        &mut self,
        hash: &Hash,
        blue_score: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("add block {} at blue_score {}", hash, blue_score);
        }
        let mut tips = if self.has_blocks_at_blue_score(blue_score).await? {
            let hashes = self.get_blocks_at_blue_score(blue_score).await?;
            if log::log_enabled!(log::Level::Trace) {
                trace!("Found {} blocks at this blue_score", hashes.len());
            }
            hashes
        } else {
            trace!("No blocks found at this blue_score");
            IndexSet::new()
        };

        tips.insert(hash.clone());
        self.set_blocks_at_blue_score(&tips, blue_score).await
    }

    async fn remove_block_hash_at_blue_score(
        &mut self,
        hash: &Hash,
        blue_score: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("remove block {} at blue_score {}", hash, blue_score);
        }
        let mut tips = self.get_blocks_at_blue_score(blue_score).await?;
        tips.shift_remove(hash);

        // Delete the blue_score if there are no blocks present anymore
        if tips.is_empty() {
            Self::remove_from_disk_without_reading(
                self.snapshot.as_mut(),
                &self.blocks_at_height,
                &blue_score.to_be_bytes(),
            )?;
        } else {
            self.set_blocks_at_blue_score(&tips, blue_score).await?;
        }

        Ok(())
    }
}
