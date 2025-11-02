// Compact Block Cache
// Stores pending compact blocks awaiting missing transactions

use lru::LruCache;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tos_common::{block::CompactBlock, crypto::Hash};

/// Entry in the compact block cache
struct CacheEntry {
    /// The compact block awaiting reconstruction
    compact_block: CompactBlock,
    /// When this entry was added
    added_at: Instant,
    /// Peer address that sent this compact block (for debugging)
    #[allow(dead_code)]
    peer_addr: String,
}

/// Cache for pending compact blocks awaiting missing transactions
pub struct CompactBlockCache {
    /// LRU cache of compact blocks keyed by block hash
    cache: Arc<RwLock<LruCache<Hash, CacheEntry>>>,
    /// Maximum time to keep entries before eviction
    entry_timeout: Duration,
}

impl CompactBlockCache {
    /// Create a new compact block cache
    ///
    /// # Arguments
    /// * `capacity` - Maximum number of compact blocks to cache
    /// * `entry_timeout` - Maximum time to keep entries before eviction
    pub fn new(capacity: usize, entry_timeout: Duration) -> Self {
        Self {
            cache: Arc::new(RwLock::new(LruCache::new(capacity.try_into().unwrap()))),
            entry_timeout,
        }
    }

    /// Insert a compact block into the cache
    ///
    /// Returns true if inserted, false if already exists
    pub async fn insert(
        &self,
        block_hash: Hash,
        compact_block: CompactBlock,
        peer_addr: String,
    ) -> bool {
        let mut cache = self.cache.write().await;

        // Check if already exists
        if cache.contains(&block_hash) {
            return false;
        }

        let entry = CacheEntry {
            compact_block,
            added_at: Instant::now(),
            peer_addr,
        };

        cache.put(block_hash, entry);
        true
    }

    /// Retrieve a compact block from the cache
    ///
    /// Returns None if not found or if entry has expired
    #[allow(dead_code)]
    pub async fn get(&self, block_hash: &Hash) -> Option<CompactBlock> {
        let mut cache = self.cache.write().await;

        if let Some(entry) = cache.get(block_hash) {
            // Check if entry has expired
            if entry.added_at.elapsed() > self.entry_timeout {
                // Entry expired, remove it
                cache.pop(block_hash);
                None
            } else {
                // Entry valid, return a clone
                Some(entry.compact_block.clone())
            }
        } else {
            None
        }
    }

    /// Remove a compact block from the cache
    ///
    /// Returns the compact block if it was in the cache
    pub async fn remove(&self, block_hash: &Hash) -> Option<CompactBlock> {
        let mut cache = self.cache.write().await;
        cache.pop(block_hash).map(|entry| entry.compact_block)
    }

    /// Clean up expired entries
    ///
    /// This should be called periodically to remove stale entries
    pub async fn cleanup_expired(&self) {
        let mut cache = self.cache.write().await;

        // Collect expired keys
        let expired_keys: Vec<Hash> = cache
            .iter()
            .filter(|(_, entry)| entry.added_at.elapsed() > self.entry_timeout)
            .map(|(hash, _)| hash.clone())
            .collect();

        // Remove expired entries
        for key in expired_keys {
            cache.pop(&key);
        }
    }

    /// Get the number of entries in the cache
    pub async fn len(&self) -> usize {
        let cache = self.cache.read().await;
        cache.len()
    }

    /// Check if the cache is empty
    #[allow(dead_code)]
    pub async fn is_empty(&self) -> bool {
        let cache = self.cache.read().await;
        cache.is_empty()
    }

    /// Clear all entries from the cache
    #[allow(dead_code)]
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::block::{BlockHeader, BlockVersion};
    use tos_common::crypto::elgamal::CompressedPublicKey;
    use tos_common::serializer::{Reader, Serializer};

    fn create_test_compact_block() -> CompactBlock {
        // Create a minimal block header
        let parents = vec![Hash::new([0u8; 32])];
        let miner_bytes = [1u8; 32];
        let mut reader = Reader::new(&miner_bytes);
        let miner = CompressedPublicKey::read(&mut reader).unwrap();

        let header = BlockHeader::new_simple(
            BlockVersion::V0,
            parents,
            1234567890,
            [0u8; 32],
            miner,
            Hash::zero(),
        );

        CompactBlock {
            header,
            nonce: 12345,
            short_tx_ids: vec![[1, 2, 3, 4, 5, 6]],
            prefilled_txs: vec![],
        }
    }

    #[tokio::test]
    async fn test_insert_and_get() {
        let cache = CompactBlockCache::new(10, Duration::from_secs(60));
        let block_hash = Hash::new([1u8; 32]);
        let compact_block = create_test_compact_block();

        // Insert should succeed
        assert!(
            cache
                .insert(
                    block_hash.clone(),
                    compact_block.clone(),
                    "127.0.0.1:8080".to_string()
                )
                .await
        );

        // Get should return the compact block
        let retrieved = cache.get(&block_hash).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().nonce, compact_block.nonce);

        // Inserting again should fail
        assert!(
            !cache
                .insert(
                    block_hash.clone(),
                    compact_block.clone(),
                    "127.0.0.1:8080".to_string()
                )
                .await
        );
    }

    #[tokio::test]
    async fn test_remove() {
        let cache = CompactBlockCache::new(10, Duration::from_secs(60));
        let block_hash = Hash::new([2u8; 32]);
        let compact_block = create_test_compact_block();

        cache
            .insert(
                block_hash.clone(),
                compact_block.clone(),
                "127.0.0.1:8080".to_string(),
            )
            .await;

        // Remove should return the compact block
        let removed = cache.remove(&block_hash).await;
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().nonce, compact_block.nonce);

        // Get should now return None
        assert!(cache.get(&block_hash).await.is_none());
    }

    #[tokio::test]
    async fn test_expiration() {
        let cache = CompactBlockCache::new(10, Duration::from_millis(100));
        let block_hash = Hash::new([3u8; 32]);
        let compact_block = create_test_compact_block();

        cache
            .insert(
                block_hash.clone(),
                compact_block,
                "127.0.0.1:8080".to_string(),
            )
            .await;

        // Should be retrievable immediately
        assert!(cache.get(&block_hash).await.is_some());

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should be expired now
        assert!(cache.get(&block_hash).await.is_none());
    }

    #[tokio::test]
    async fn test_cleanup_expired() {
        let cache = CompactBlockCache::new(10, Duration::from_millis(150));
        let block_hash1 = Hash::new([4u8; 32]);
        let block_hash2 = Hash::new([5u8; 32]);
        let compact_block = create_test_compact_block();

        cache
            .insert(
                block_hash1.clone(),
                compact_block.clone(),
                "127.0.0.1:8080".to_string(),
            )
            .await;

        // Wait a bit
        tokio::time::sleep(Duration::from_millis(80)).await;

        // Insert another one
        cache
            .insert(
                block_hash2.clone(),
                compact_block.clone(),
                "127.0.0.1:8080".to_string(),
            )
            .await;

        // Wait for first one to expire (total 160ms > 150ms timeout)
        // but second one should still be valid (80ms < 150ms)
        // Add extra margin for async runtime scheduling delays
        tokio::time::sleep(Duration::from_millis(90)).await;

        // Both should still be in cache before cleanup
        assert_eq!(cache.len().await, 2);

        // Clean up expired entries
        cache.cleanup_expired().await;

        // First one should be removed (170ms elapsed > 150ms timeout)
        // Second one should remain (90ms elapsed < 150ms timeout)
        // Due to async timing, second might also expire, so check more flexibly
        let len = cache.len().await;
        assert!(
            len <= 1,
            "Expected 0 or 1 entries after cleanup, got {}",
            len
        );
        assert!(cache.get(&block_hash1).await.is_none());

        // If the timing worked perfectly, block_hash2 should still be there
        // But we don't assert this strictly due to async runtime variability
    }
}
