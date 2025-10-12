// TOS Mining Cache Module
// Caches GHOSTDAG calculations and block template data for improved performance

use std::sync::Arc;
use std::num::NonZeroUsize;
use lru::LruCache;
use tos_common::{
    crypto::Hash,
    tokio::sync::RwLock,
};
use crate::core::ghostdag::TosGhostdagData;

/// Cache for GHOSTDAG data to avoid repeated calculations during block template generation
#[derive(Clone)]
pub struct GhostdagCache {
    /// LRU cache for GHOSTDAG data
    cache: Arc<RwLock<LruCache<Hash, Arc<TosGhostdagData>>>>,
}

impl GhostdagCache {
    /// Create a new GHOSTDAG cache with specified capacity
    pub fn new(capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(1000).unwrap());
        Self {
            cache: Arc::new(RwLock::new(LruCache::new(capacity))),
        }
    }

    /// Get GHOSTDAG data from cache
    pub async fn get(&self, hash: &Hash) -> Option<Arc<TosGhostdagData>> {
        let mut cache = self.cache.write().await;
        cache.get(hash).cloned()
    }

    /// Put GHOSTDAG data into cache
    pub async fn put(&self, hash: Hash, data: Arc<TosGhostdagData>) {
        let mut cache = self.cache.write().await;
        cache.put(hash, data);
    }

    /// Check if cache contains data for a hash
    pub async fn contains(&self, hash: &Hash) -> bool {
        let cache = self.cache.read().await;
        cache.contains(hash)
    }

    /// Get cache size
    pub async fn len(&self) -> usize {
        let cache = self.cache.read().await;
        cache.len()
    }

    /// Check if cache is empty
    pub async fn is_empty(&self) -> bool {
        let cache = self.cache.read().await;
        cache.is_empty()
    }

    /// Clear the cache
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

/// Cache for block templates to avoid regenerating them
#[derive(Clone)]
pub struct BlockTemplateCache {
    /// Cached template data: (tips_hash, timestamp, cached_data)
    cache: Arc<RwLock<Option<CachedTemplate>>>,
}

#[derive(Clone)]
struct CachedTemplate {
    /// Combined hash of tips (for quick comparison)
    tips_hash: Hash,

    /// Timestamp when cached
    timestamp: u64,

    /// Time to live in milliseconds
    ttl_ms: u64,
}

impl BlockTemplateCache {
    /// Create a new block template cache
    pub fn new(ttl_ms: u64) -> Self {
        Self {
            cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Check if cached template is still valid for given tips
    pub async fn is_valid(&self, tips_hash: &Hash, current_time: u64) -> bool {
        let cache = self.cache.read().await;

        if let Some(cached) = cache.as_ref() {
            // Check if tips match and TTL hasn't expired
            if &cached.tips_hash == tips_hash {
                let elapsed = current_time.saturating_sub(cached.timestamp);
                return elapsed < cached.ttl_ms;
            }
        }

        false
    }

    /// Update the cache with new template data
    pub async fn update(&self, tips_hash: Hash, timestamp: u64, ttl_ms: u64) {
        let mut cache = self.cache.write().await;
        *cache = Some(CachedTemplate {
            tips_hash,
            timestamp,
            ttl_ms,
        });
    }

    /// Clear the cache
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        *cache = None;
    }
}

/// Cache for tip selection to avoid repeated validation
#[derive(Clone)]
pub struct TipSelectionCache {
    /// Cached tips: (tips_hash, validated_tips)
    cache: Arc<RwLock<LruCache<Hash, Arc<Vec<Hash>>>>>,
}

impl TipSelectionCache {
    /// Create a new tip selection cache
    pub fn new(capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());
        Self {
            cache: Arc::new(RwLock::new(LruCache::new(capacity))),
        }
    }

    /// Get validated tips from cache
    pub async fn get(&self, tips_hash: &Hash) -> Option<Arc<Vec<Hash>>> {
        let mut cache = self.cache.write().await;
        cache.get(tips_hash).cloned()
    }

    /// Put validated tips into cache
    pub async fn put(&self, tips_hash: Hash, tips: Arc<Vec<Hash>>) {
        let mut cache = self.cache.write().await;
        cache.put(tips_hash, tips);
    }

    /// Clear the cache
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::tokio;

    #[tokio::test]
    async fn test_ghostdag_cache() {
        let cache = GhostdagCache::new(10);

        let hash = Hash::new([1u8; 32]);
        let data = Arc::new(TosGhostdagData::new(
            1,
            Default::default(),
            Hash::new([0u8; 32]),
            vec![],
            vec![],
            std::collections::HashMap::new(),
            vec![],
        ));

        // Cache should be empty initially
        assert!(cache.is_empty().await);
        assert_eq!(cache.get(&hash).await, None);

        // Put data into cache
        cache.put(hash.clone(), data.clone()).await;

        // Should now be in cache
        assert!(!cache.is_empty().await);
        assert_eq!(cache.len().await, 1);
        assert!(cache.contains(&hash).await);

        // Get should return the data
        let cached = cache.get(&hash).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().blue_score, 1);

        // Clear should empty the cache
        cache.clear().await;
        assert!(cache.is_empty().await);
    }

    #[tokio::test]
    async fn test_block_template_cache() {
        let cache = BlockTemplateCache::new(1000); // 1 second TTL

        let tips_hash = Hash::new([1u8; 32]);
        let timestamp = 100000;

        // Initially not valid
        assert!(!cache.is_valid(&tips_hash, timestamp).await);

        // Update cache
        cache.update(tips_hash.clone(), timestamp, 1000).await;

        // Should be valid with same tips and within TTL
        assert!(cache.is_valid(&tips_hash, timestamp + 500).await);

        // Should be invalid after TTL expires
        assert!(!cache.is_valid(&tips_hash, timestamp + 1500).await);

        // Should be invalid with different tips
        let other_tips_hash = Hash::new([2u8; 32]);
        assert!(!cache.is_valid(&other_tips_hash, timestamp + 500).await);

        // Clear should invalidate
        cache.clear().await;
        assert!(!cache.is_valid(&tips_hash, timestamp + 500).await);
    }

    #[tokio::test]
    async fn test_tip_selection_cache() {
        let cache = TipSelectionCache::new(10);

        let tips_hash = Hash::new([1u8; 32]);
        let tips = Arc::new(vec![
            Hash::new([10u8; 32]),
            Hash::new([20u8; 32]),
        ]);

        // Initially not in cache
        assert_eq!(cache.get(&tips_hash).await, None);

        // Put into cache
        cache.put(tips_hash.clone(), tips.clone()).await;

        // Should now be in cache
        let cached = cache.get(&tips_hash).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 2);

        // Clear should empty
        cache.clear().await;
        assert_eq!(cache.get(&tips_hash).await, None);
    }
}
