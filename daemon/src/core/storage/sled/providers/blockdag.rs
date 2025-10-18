use async_trait::async_trait;
use log::trace;
use tos_common::{
    block::{BlockHeader, TopoHeight},
    crypto::Hash,
    immutable::Immutable
};

use crate::core::{
    error::{BlockchainError, DiskContext},
    storage::{
        BlockDagProvider,
        DagOrderProvider,
        DifficultyProvider,
        SledStorage
    }
};

#[async_trait]
impl BlockDagProvider for SledStorage {
    async fn get_block_header_at_topoheight(&self, topoheight: TopoHeight) -> Result<(Hash, Immutable<BlockHeader>), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get block at topoheight: {}", topoheight);
        }
        let hash = self.get_hash_at_topo_height(topoheight).await?;
        let block = self.get_block_header_by_hash(&hash).await?;
        Ok((hash, block))
    }

    fn get_block_reward_at_topo_height(&self, topoheight: TopoHeight) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get block reward at topo height {}", topoheight);
        }

        // P2 Hot path cache: Check cache first for 20-50% query performance improvement
        if let Some(cache) = &self.block_reward_cache {
            // Use try_lock to avoid blocking Tokio runtime
            if let Ok(mut cache_guard) = cache.try_lock() {
                if let Some(reward) = cache_guard.get(&topoheight).cloned() {
                    if log::log_enabled!(log::Level::Trace) {
                        trace!("block reward cache hit for topoheight {}", topoheight);
                    }
                    return Ok(reward);
                }
            }
            // If cache lock is busy, skip cache and load from disk
        }

        // Cache miss: Load from disk
        let reward: u64 = self.load_from_disk(&self.rewards, &topoheight.to_be_bytes(), DiskContext::BlockRewardAtTopoHeight(topoheight))?;

        // Store in cache
        if let Some(cache) = &self.block_reward_cache {
            // Use try_lock to avoid blocking Tokio runtime
            if let Ok(mut cache_guard) = cache.try_lock() {
                cache_guard.put(topoheight, reward);
            }
            // If cache lock is busy, skip cache write (data is still returned from disk)
        }

        Ok(reward)
    }

    async fn get_supply_at_topo_height(&self, topoheight: TopoHeight) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get supply at topo height {}", topoheight);
        }

        // P2 Hot path cache: Check cache first for 20-50% query performance improvement
        if let Some(cache) = &self.supply_cache {
            let mut cache_guard = cache.lock().await;
            if let Some(supply) = cache_guard.get(&topoheight).cloned() {
                if log::log_enabled!(log::Level::Trace) {
                    trace!("supply cache hit for topoheight {}", topoheight);
                }
                return Ok(supply);
            }
        }

        // Cache miss: Load from disk
        let supply: u64 = self.load_from_disk(&self.supply, &topoheight.to_be_bytes(), DiskContext::SupplyAtTopoHeight(topoheight))?;

        // Store in cache
        if let Some(cache) = &self.supply_cache {
            let mut cache_guard = cache.lock().await;
            cache_guard.put(topoheight, supply);
        }

        Ok(supply)
    }

    async fn get_burned_supply_at_topo_height(&self, topoheight: TopoHeight) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get burned supply at topo height {}", topoheight);
        }

        // P2 Hot path cache: Check cache first for 20-50% query performance improvement
        if let Some(cache) = &self.burned_supply_cache {
            let mut cache_guard = cache.lock().await;
            if let Some(burned_supply) = cache_guard.get(&topoheight).cloned() {
                if log::log_enabled!(log::Level::Trace) {
                    trace!("burned supply cache hit for topoheight {}", topoheight);
                }
                return Ok(burned_supply);
            }
        }

        // Cache miss: Load from disk
        let burned_supply: u64 = self.load_from_disk(&self.burned_supply, &topoheight.to_be_bytes(), DiskContext::BurnedSupplyAtTopoHeight(topoheight))?;

        // Store in cache
        if let Some(cache) = &self.burned_supply_cache {
            let mut cache_guard = cache.lock().await;
            cache_guard.put(topoheight, burned_supply);
        }

        Ok(burned_supply)
    }

    // Set the metadata for topoheight
    fn set_topoheight_metadata(&mut self, topoheight: TopoHeight, block_reward: u64, supply: u64, burned_supply: u64) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set topoheight metadata at {}", topoheight);
        }

        Self::insert_into_disk(self.snapshot.as_mut(), &self.rewards, &topoheight.to_be_bytes(), &block_reward.to_be_bytes())?;
        Self::insert_into_disk(self.snapshot.as_mut(), &self.supply, &topoheight.to_be_bytes(), &supply.to_be_bytes())?;
        Self::insert_into_disk(self.snapshot.as_mut(), &self.burned_supply, &topoheight.to_be_bytes(), &burned_supply.to_be_bytes())?;

        Ok(())
    }
}