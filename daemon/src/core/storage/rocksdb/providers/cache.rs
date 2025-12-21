use crate::core::{
    error::BlockchainError,
    storage::{CacheProvider, ChainCache, RocksStorage},
};
use async_trait::async_trait;
use log::trace;

#[async_trait]
impl CacheProvider for RocksStorage {
    /// Clear all the internal LRU caches
    ///
    /// This clears both chain caches (tip_base, common_base, tip_work_score, full_order)
    /// and object caches (transactions, blocks, topoheight mappings) if enabled.
    /// Counter values and chain state (height, topoheight, etc.) are NOT affected.
    async fn clear_objects_cache(&mut self) -> Result<(), BlockchainError> {
        trace!("clear caches");
        self.cache_mut().clear_caches();

        trace!("reload caches from disk");
        // also load the atomic counters from disk
        self.load_cache_from_disk();

        Ok(())
    }

    async fn chain_cache_mut(&mut self) -> Result<&mut ChainCache, BlockchainError> {
        Ok(&mut self.cache_mut().chain)
    }

    async fn chain_cache(&self) -> &ChainCache {
        &self.cache().chain
    }
}
