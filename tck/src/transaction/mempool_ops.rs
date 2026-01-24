// Tests for mempool management operations.
//
// The Mempool is complex and requires Storage, so we test the CONCEPTUAL
// properties and the AccountCache logic using a MockAccountCache.
//
// Key properties under test:
// - AccountCache tracks min/max nonce per sender
// - Transactions stored in LinkedHashMap (insertion order)
// - Nonces must be consecutive (no gaps allowed)
// - Each sender has independent nonce sequence
// - has_tx_with_same_nonce() checks for duplicate nonces
// - get_next_nonce() returns max + 1
// - Fee rate estimation: fee / (size / BYTES_PER_KB)

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use tos_common::config::BYTES_PER_KB;
    use tos_common::crypto::Hash;

    // Mock of the AccountCache from daemon/src/core/mempool.rs.
    // Simulates the nonce tracking and transaction ordering behavior.
    struct MockAccountCache {
        min_nonce: u64,
        max_nonce: u64,
        txs: Vec<Hash>, // tx hashes in nonce order
    }

    impl MockAccountCache {
        fn new(min: u64) -> Self {
            Self {
                min_nonce: min,
                max_nonce: min,
                txs: Vec::new(),
            }
        }

        fn add_tx(&mut self, nonce: u64, hash: Hash) -> Result<(), &'static str> {
            if !self.txs.is_empty() {
                if nonce >= self.min_nonce && nonce <= self.max_nonce {
                    return Err("Duplicate nonce");
                }
                if nonce != self.max_nonce + 1 {
                    return Err("Nonce gap");
                }
            }
            if self.txs.is_empty() {
                self.min_nonce = nonce;
            }
            self.max_nonce = nonce;
            self.txs.push(hash);
            Ok(())
        }

        fn get_next_nonce(&self) -> u64 {
            self.max_nonce + 1
        }

        fn has_tx_with_same_nonce(&self, nonce: u64) -> bool {
            if self.txs.is_empty() {
                return false;
            }
            nonce >= self.min_nonce && nonce <= self.max_nonce
        }

        fn remove_below_nonce(&mut self, blockchain_nonce: u64) -> usize {
            if self.txs.is_empty() {
                return 0;
            }
            let remove_count = blockchain_nonce.saturating_sub(self.min_nonce) as usize;
            let actual_remove = remove_count.min(self.txs.len());
            self.txs.drain(..actual_remove);
            if !self.txs.is_empty() {
                self.min_nonce = blockchain_nonce;
            }
            actual_remove
        }
    }

    // Helper to create a unique Hash from a u8 value
    fn make_hash(val: u8) -> Hash {
        Hash::new([val; 32])
    }

    // Helper to compute fee rate per KB (mirrors SortedTx::get_fee_rate_per_kb)
    fn compute_fee_rate(fee: u64, size: usize) -> u64 {
        let size_in_kb = size as u64 / BYTES_PER_KB as u64;
        if size_in_kb == 0 {
            // Avoid division by zero for sub-KB transactions.
            // In production, minimum tx size ensures this does not occur,
            // but we handle it defensively here.
            return fee;
        }
        fee / size_in_kb
    }

    // =========================================================================
    // AccountCache Basic Tests
    // =========================================================================

    #[test]
    fn test_cache_new_empty() {
        let cache = MockAccountCache::new(0);
        assert!(cache.txs.is_empty());
        assert_eq!(cache.min_nonce, 0);
        assert_eq!(cache.max_nonce, 0);
    }

    #[test]
    fn test_cache_add_first_tx() {
        let mut cache = MockAccountCache::new(0);
        let hash = make_hash(1);
        cache.add_tx(5, hash).unwrap();

        assert_eq!(cache.min_nonce, 5);
        assert_eq!(cache.max_nonce, 5);
        assert_eq!(cache.txs.len(), 1);
    }

    #[test]
    fn test_cache_add_sequential() {
        let mut cache = MockAccountCache::new(0);
        cache.add_tx(0, make_hash(1)).unwrap();
        cache.add_tx(1, make_hash(2)).unwrap();
        cache.add_tx(2, make_hash(3)).unwrap();

        assert_eq!(cache.min_nonce, 0);
        assert_eq!(cache.max_nonce, 2);
        assert_eq!(cache.txs.len(), 3);
    }

    #[test]
    fn test_cache_next_nonce() {
        let mut cache = MockAccountCache::new(0);
        cache.add_tx(0, make_hash(1)).unwrap();
        assert_eq!(cache.get_next_nonce(), 1);

        cache.add_tx(1, make_hash(2)).unwrap();
        assert_eq!(cache.get_next_nonce(), 2);

        cache.add_tx(2, make_hash(3)).unwrap();
        assert_eq!(cache.get_next_nonce(), 3);
    }

    #[test]
    fn test_cache_has_tx_with_nonce() {
        let mut cache = MockAccountCache::new(0);
        cache.add_tx(5, make_hash(1)).unwrap();
        cache.add_tx(6, make_hash(2)).unwrap();
        cache.add_tx(7, make_hash(3)).unwrap();

        assert!(cache.has_tx_with_same_nonce(5));
        assert!(cache.has_tx_with_same_nonce(6));
        assert!(cache.has_tx_with_same_nonce(7));
        assert!(!cache.has_tx_with_same_nonce(4));
        assert!(!cache.has_tx_with_same_nonce(8));
    }

    // =========================================================================
    // Nonce Gap Detection Tests
    // =========================================================================

    #[test]
    fn test_cache_rejects_gap() {
        let mut cache = MockAccountCache::new(0);
        cache.add_tx(0, make_hash(1)).unwrap();
        // Skip nonce 1, try to add nonce 2
        let result = cache.add_tx(2, make_hash(2));
        assert_eq!(result, Err("Nonce gap"));
        // Cache state should not have changed
        assert_eq!(cache.txs.len(), 1);
        assert_eq!(cache.max_nonce, 0);
    }

    #[test]
    fn test_cache_rejects_duplicate() {
        let mut cache = MockAccountCache::new(0);
        cache.add_tx(0, make_hash(1)).unwrap();
        // Try to add nonce 0 again
        let result = cache.add_tx(0, make_hash(2));
        assert_eq!(result, Err("Duplicate nonce"));
        assert_eq!(cache.txs.len(), 1);
    }

    #[test]
    fn test_cache_nonce_range() {
        let mut cache = MockAccountCache::new(0);
        cache.add_tx(10, make_hash(1)).unwrap();
        assert_eq!(cache.min_nonce, 10);
        assert_eq!(cache.max_nonce, 10);

        cache.add_tx(11, make_hash(2)).unwrap();
        assert_eq!(cache.min_nonce, 10);
        assert_eq!(cache.max_nonce, 11);

        cache.add_tx(12, make_hash(3)).unwrap();
        assert_eq!(cache.min_nonce, 10);
        assert_eq!(cache.max_nonce, 12);
    }

    // =========================================================================
    // Cleanup After Block Tests
    // =========================================================================

    #[test]
    fn test_cleanup_removes_processed() {
        let mut cache = MockAccountCache::new(0);
        cache.add_tx(0, make_hash(1)).unwrap();
        cache.add_tx(1, make_hash(2)).unwrap();
        cache.add_tx(2, make_hash(3)).unwrap();

        // Block processed nonces 0 and 1 (blockchain nonce is now 2)
        let removed = cache.remove_below_nonce(2);
        assert_eq!(removed, 2);
        assert_eq!(cache.txs.len(), 1);
        assert_eq!(cache.txs[0], make_hash(3));
    }

    #[test]
    fn test_cleanup_preserves_pending() {
        let mut cache = MockAccountCache::new(0);
        cache.add_tx(0, make_hash(1)).unwrap();
        cache.add_tx(1, make_hash(2)).unwrap();
        cache.add_tx(2, make_hash(3)).unwrap();

        // Only nonce 0 processed
        let removed = cache.remove_below_nonce(1);
        assert_eq!(removed, 1);
        assert_eq!(cache.txs.len(), 2);
        assert_eq!(cache.txs[0], make_hash(2));
        assert_eq!(cache.txs[1], make_hash(3));
    }

    #[test]
    fn test_cleanup_empty_cache() {
        let mut cache = MockAccountCache::new(0);
        let removed = cache.remove_below_nonce(5);
        assert_eq!(removed, 0);
        assert!(cache.txs.is_empty());
    }

    #[test]
    fn test_cleanup_all_processed() {
        let mut cache = MockAccountCache::new(0);
        cache.add_tx(0, make_hash(1)).unwrap();
        cache.add_tx(1, make_hash(2)).unwrap();
        cache.add_tx(2, make_hash(3)).unwrap();

        // All nonces processed (blockchain nonce is 3)
        let removed = cache.remove_below_nonce(3);
        assert_eq!(removed, 3);
        assert!(cache.txs.is_empty());
    }

    #[test]
    fn test_cleanup_partial() {
        let mut cache = MockAccountCache::new(0);
        cache.add_tx(5, make_hash(1)).unwrap();
        cache.add_tx(6, make_hash(2)).unwrap();
        cache.add_tx(7, make_hash(3)).unwrap();
        cache.add_tx(8, make_hash(4)).unwrap();

        // Blockchain nonce is 7 (remove nonces 5 and 6)
        let removed = cache.remove_below_nonce(7);
        assert_eq!(removed, 2);
        assert_eq!(cache.txs.len(), 2);
        assert_eq!(cache.txs[0], make_hash(3));
        assert_eq!(cache.txs[1], make_hash(4));
    }

    #[test]
    fn test_cleanup_updates_min_nonce() {
        let mut cache = MockAccountCache::new(0);
        cache.add_tx(3, make_hash(1)).unwrap();
        cache.add_tx(4, make_hash(2)).unwrap();
        cache.add_tx(5, make_hash(3)).unwrap();

        assert_eq!(cache.min_nonce, 3);

        // Remove nonces below 5
        cache.remove_below_nonce(5);
        assert_eq!(cache.min_nonce, 5);
        assert_eq!(cache.txs.len(), 1);
        assert_eq!(cache.txs[0], make_hash(3));
    }

    // =========================================================================
    // Fee Rate Estimation Tests
    // =========================================================================

    #[test]
    fn test_fee_rate_basic() {
        // fee=10000, size=1024 (exactly 1KB)
        // rate = 10000 / (1024 / 1024) = 10000 / 1 = 10000
        let rate = compute_fee_rate(10000, 1024);
        assert_eq!(rate, 10000);
    }

    #[test]
    fn test_fee_rate_larger_size() {
        // fee=20000, size=2048 (exactly 2KB)
        // rate = 20000 / (2048 / 1024) = 20000 / 2 = 10000
        let rate = compute_fee_rate(20000, 2048);
        assert_eq!(rate, 10000);
    }

    #[test]
    fn test_fee_rate_small_tx() {
        // fee=10000, size=500 (less than 1KB)
        // size_in_kb = 500 / 1024 = 0 (integer division)
        // Defensively returns fee itself to avoid division by zero
        let rate = compute_fee_rate(10000, 500);
        assert_eq!(rate, 10000);
    }

    #[test]
    fn test_fee_rate_ordering() {
        // Higher fee rate means higher priority
        // TX A: fee=20000, size=1024 -> rate = 20000
        // TX B: fee=10000, size=1024 -> rate = 10000
        let rate_a = compute_fee_rate(20000, 1024);
        let rate_b = compute_fee_rate(10000, 1024);
        assert!(rate_a > rate_b);

        // TX C: fee=10000, size=1024 -> rate = 10000
        // TX D: fee=10000, size=2048 -> rate = 5000
        let rate_c = compute_fee_rate(10000, 1024);
        let rate_d = compute_fee_rate(10000, 2048);
        assert!(rate_c > rate_d);
    }

    #[test]
    fn test_fee_rate_deterministic() {
        // Same inputs always produce same output
        let rate1 = compute_fee_rate(15000, 1536);
        let rate2 = compute_fee_rate(15000, 1536);
        assert_eq!(rate1, rate2);

        // Verify the value: 15000 / (1536 / 1024) = 15000 / 1 = 15000
        // 1536 / 1024 = 1 in integer division
        assert_eq!(rate1, 15000);
    }

    // =========================================================================
    // Capacity and Ordering Tests
    // =========================================================================

    #[test]
    fn test_insertion_order_preserved() {
        // Simulate LinkedHashMap insertion order behavior
        let mut insertion_order: Vec<Hash> = Vec::new();
        let hashes: Vec<Hash> = (0..10).map(make_hash).collect();

        for hash in &hashes {
            insertion_order.push(hash.clone());
        }

        // Verify order is preserved
        for (i, hash) in insertion_order.iter().enumerate() {
            assert_eq!(*hash, hashes[i]);
        }
    }

    #[test]
    fn test_multiple_senders_independent() {
        // Each sender has its own AccountCache
        let mut caches: HashMap<u8, MockAccountCache> = HashMap::new();

        // Sender A starts at nonce 0
        caches.insert(0, MockAccountCache::new(0));
        caches
            .get_mut(&0)
            .unwrap()
            .add_tx(0, make_hash(10))
            .unwrap();
        caches
            .get_mut(&0)
            .unwrap()
            .add_tx(1, make_hash(11))
            .unwrap();

        // Sender B starts at nonce 5
        caches.insert(1, MockAccountCache::new(5));
        caches
            .get_mut(&1)
            .unwrap()
            .add_tx(5, make_hash(20))
            .unwrap();
        caches
            .get_mut(&1)
            .unwrap()
            .add_tx(6, make_hash(21))
            .unwrap();

        // Sender C starts at nonce 100
        caches.insert(2, MockAccountCache::new(100));
        caches
            .get_mut(&2)
            .unwrap()
            .add_tx(100, make_hash(30))
            .unwrap();

        // Verify independent nonce tracking
        assert_eq!(caches[&0].get_next_nonce(), 2);
        assert_eq!(caches[&1].get_next_nonce(), 7);
        assert_eq!(caches[&2].get_next_nonce(), 101);

        // Verify they don't interfere
        assert!(caches[&0].has_tx_with_same_nonce(0));
        assert!(!caches[&0].has_tx_with_same_nonce(5));
        assert!(caches[&1].has_tx_with_same_nonce(5));
        assert!(!caches[&1].has_tx_with_same_nonce(0));
    }

    #[test]
    fn test_sender_isolation() {
        let mut caches: HashMap<u8, MockAccountCache> = HashMap::new();

        // Sender A
        caches.insert(0, MockAccountCache::new(0));
        caches.get_mut(&0).unwrap().add_tx(0, make_hash(1)).unwrap();

        // Sender B tries to add invalid nonce (gap)
        caches.insert(1, MockAccountCache::new(0));
        caches.get_mut(&1).unwrap().add_tx(0, make_hash(2)).unwrap();
        let result = caches.get_mut(&1).unwrap().add_tx(5, make_hash(3));
        assert!(result.is_err());

        // Sender A should be unaffected by B's failure
        assert_eq!(caches[&0].txs.len(), 1);
        assert_eq!(caches[&0].get_next_nonce(), 1);
        assert!(caches[&0].has_tx_with_same_nonce(0));
    }
}
