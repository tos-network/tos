use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, TopoHeightMetadata},
        BlockDagProvider, DagOrderProvider, DifficultyProvider, RocksStorage,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    block::{BlockHeader, TopoHeight},
    crypto::Hash,
    immutable::Immutable,
};

#[async_trait]
impl BlockDagProvider for RocksStorage {
    // Get a block header & hash from its topoheight
    async fn get_block_header_at_topoheight(
        &self,
        topoheight: TopoHeight,
    ) -> Result<(Hash, Immutable<BlockHeader>), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get block header at topoheight {}", topoheight);
        }
        let hash = self.get_hash_at_topo_height(topoheight).await?;
        let header = self.get_block_header_by_hash(&hash).await?;
        Ok((hash, header))
    }

    // Get the block reward from using topoheight
    fn get_block_reward_at_topo_height(
        &self,
        topoheight: TopoHeight,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get block reward at topoheight {}", topoheight);
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

        // Cache miss: Load from disk via metadata
        let reward = self.get_metadata_at_topoheight(topoheight)?.rewards;

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

    // Get the supply from topoheight
    async fn get_supply_at_topo_height(
        &self,
        topoheight: TopoHeight,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get supply at topoheight {}", topoheight);
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

        // Cache miss: Load from disk via metadata
        let supply = self.get_metadata_at_topoheight(topoheight)?.emitted_supply;

        // Store in cache
        if let Some(cache) = &self.supply_cache {
            let mut cache_guard = cache.lock().await;
            cache_guard.put(topoheight, supply);
        }

        Ok(supply)
    }

    // Get the burned supply from topoheight
    async fn get_burned_supply_at_topo_height(
        &self,
        topoheight: TopoHeight,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get burned supply at topoheight {}", topoheight);
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

        // Cache miss: Load from disk via metadata
        let burned_supply = self.get_metadata_at_topoheight(topoheight)?.burned_supply;

        // Store in cache
        if let Some(cache) = &self.burned_supply_cache {
            let mut cache_guard = cache.lock().await;
            cache_guard.put(topoheight, burned_supply);
        }

        Ok(burned_supply)
    }

    // Set the metadata for topoheight
    fn set_topoheight_metadata(
        &mut self,
        topoheight: TopoHeight,
        rewards: u64,
        emitted_supply: u64,
        burned_supply: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set topoheight metadata {}", topoheight);
        }
        let metadata = TopoHeightMetadata {
            rewards,
            emitted_supply,
            burned_supply,
        };

        self.insert_into_disk(
            Column::TopoHeightMetadata,
            &topoheight.to_be_bytes(),
            &metadata,
        )
    }
}

impl RocksStorage {
    pub fn get_metadata_at_topoheight(
        &self,
        topoheight: TopoHeight,
    ) -> Result<TopoHeightMetadata, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get metadata at topoheight {}", topoheight);
        }
        // TODO: cache
        self.load_from_disk(Column::TopoHeightMetadata, &topoheight.to_be_bytes())
    }
}
