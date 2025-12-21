use crate::core::{error::BlockchainError, storage::ChainCache};
use async_trait::async_trait;

#[async_trait]
pub trait CacheProvider {
    // Clear all the internal caches if any
    async fn clear_objects_cache(&mut self) -> Result<(), BlockchainError>;

    // Get mutable reference to the chain cache
    async fn chain_cache_mut(&mut self) -> Result<&mut ChainCache, BlockchainError>;

    // Get immutable reference to the chain cache
    async fn chain_cache(&self) -> &ChainCache;
}
