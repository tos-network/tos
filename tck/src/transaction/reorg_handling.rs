// Tests for transaction handling during DAG reorganizations.
//
// When a block is orphaned:
// - Its transactions return to mempool
// - Nonce sequences may be broken
// - Transactions must be re-verified against new state
//
// Key properties under test:
// - clean_up(full=true) forces reverification of all first-txs
// - clean_up(full=false) only removes txs below blockchain nonce
// - If first tx in a cache fails reverification, entire cache is deleted (cascade)
// - DAG reorg can change which blocks are in the main chain

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use tos_common::crypto::Hash;

    // Mock of the AccountCache, duplicated here since the one in mempool_ops
    // is inside a #[cfg(test)] block and cannot be imported.
    struct MockAccountCache {
        min_nonce: u64,
        max_nonce: u64,
        txs: Vec<Hash>,
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

    // Mock state for testing reorg scenarios.
    // Tracks valid transactions, blockchain nonces, and mempool caches.
    struct MockReorgState {
        // Track which txs are valid in current chain state
        valid_txs: HashSet<Hash>,
        // Track blockchain nonces per account (keyed by sender id)
        blockchain_nonces: HashMap<u8, u64>,
        // Mempool state per sender
        mempool_caches: HashMap<u8, MockAccountCache>,
    }

    impl MockReorgState {
        fn new() -> Self {
            Self {
                valid_txs: HashSet::new(),
                blockchain_nonces: HashMap::new(),
                mempool_caches: HashMap::new(),
            }
        }

        fn add_account(&mut self, id: u8, blockchain_nonce: u64) {
            self.blockchain_nonces.insert(id, blockchain_nonce);
            self.mempool_caches
                .insert(id, MockAccountCache::new(blockchain_nonce));
        }

        fn add_mempool_tx(&mut self, sender: u8, nonce: u64, hash: Hash) {
            // Mark as valid by default
            self.valid_txs.insert(hash.clone());
            if let Some(cache) = self.mempool_caches.get_mut(&sender) {
                cache.add_tx(nonce, hash).unwrap();
            }
        }

        fn mark_tx_invalid(&mut self, hash: &Hash) {
            self.valid_txs.remove(hash);
        }

        fn simulate_block_mined(&mut self, sender: u8, nonces_processed: u64) {
            if let Some(n) = self.blockchain_nonces.get_mut(&sender) {
                *n = n.saturating_add(nonces_processed);
            }
        }

        fn simulate_reorg(&mut self, sender: u8, revert_to_nonce: u64) {
            self.blockchain_nonces.insert(sender, revert_to_nonce);
        }

        fn cleanup(&mut self, full: bool) -> Vec<Hash> {
            let mut removed = Vec::new();
            let senders: Vec<u8> = self.mempool_caches.keys().copied().collect();

            for sender in senders {
                let blockchain_nonce = self.blockchain_nonces.get(&sender).copied().unwrap_or(0);
                let cache = self.mempool_caches.get_mut(&sender).unwrap();

                if cache.txs.is_empty() {
                    continue;
                }

                if !full && blockchain_nonce <= cache.min_nonce {
                    // No cleanup needed for this sender
                    continue;
                }

                // Remove transactions below blockchain nonce
                if blockchain_nonce > cache.min_nonce {
                    let count = cache.remove_below_nonce(blockchain_nonce);
                    // The removed txs are already mined, just count them
                    let _ = count;
                }

                // Full cleanup: reverify first tx
                if full && !cache.txs.is_empty() {
                    let first_hash = cache.txs[0].clone();
                    if !self.valid_txs.contains(&first_hash) {
                        // Cascade delete: entire cache is invalid
                        removed.append(&mut cache.txs);
                    }
                }
            }
            removed
        }

        fn get_cache_size(&self, sender: u8) -> usize {
            self.mempool_caches
                .get(&sender)
                .map(|c| c.txs.len())
                .unwrap_or(0)
        }

        fn get_cache_min_nonce(&self, sender: u8) -> u64 {
            self.mempool_caches
                .get(&sender)
                .map(|c| c.min_nonce)
                .unwrap_or(0)
        }

        fn get_cache_next_nonce(&self, sender: u8) -> u64 {
            self.mempool_caches
                .get(&sender)
                .map(|c| c.get_next_nonce())
                .unwrap_or(0)
        }
    }

    // Helper to create a unique Hash from a u8 value
    fn make_hash(val: u8) -> Hash {
        Hash::new([val; 32])
    }

    // =========================================================================
    // Normal Cleanup (full=false) Tests
    // =========================================================================

    #[test]
    fn test_cleanup_after_block() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));
        state.add_mempool_tx(0, 2, make_hash(3));

        // Simulate block mining nonces 0 and 1
        state.simulate_block_mined(0, 2);

        let removed = state.cleanup(false);
        // Normal cleanup does not cascade, so removed is empty
        // (the below-nonce txs are just removed from cache, not returned)
        assert!(removed.is_empty());
        assert_eq!(state.get_cache_size(0), 1);
    }

    #[test]
    fn test_cleanup_preserves_unprocessed() {
        let mut state = MockReorgState::new();
        state.add_account(0, 5);
        state.add_mempool_tx(0, 5, make_hash(10));
        state.add_mempool_tx(0, 6, make_hash(11));
        state.add_mempool_tx(0, 7, make_hash(12));

        // Only nonce 5 was mined
        state.simulate_block_mined(0, 1);

        state.cleanup(false);
        // Nonces 6 and 7 should remain
        assert_eq!(state.get_cache_size(0), 2);
    }

    #[test]
    fn test_cleanup_multiple_senders() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_account(1, 10);

        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));
        state.add_mempool_tx(1, 10, make_hash(3));
        state.add_mempool_tx(1, 11, make_hash(4));

        // Sender 0: mined 2, Sender 1: mined 1
        state.simulate_block_mined(0, 2);
        state.simulate_block_mined(1, 1);

        state.cleanup(false);
        assert_eq!(state.get_cache_size(0), 0);
        assert_eq!(state.get_cache_size(1), 1);
    }

    #[test]
    fn test_cleanup_no_change_needed() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));

        // No blocks mined, blockchain nonce still 0
        let removed = state.cleanup(false);
        assert!(removed.is_empty());
        assert_eq!(state.get_cache_size(0), 2);
    }

    // =========================================================================
    // DAG Reorg Cleanup (full=true) Tests
    // =========================================================================

    #[test]
    fn test_reorg_reverifies_first_tx() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));

        // First tx is still valid
        let removed = state.cleanup(true);
        assert!(removed.is_empty());
        assert_eq!(state.get_cache_size(0), 2);
    }

    #[test]
    fn test_reorg_cascade_delete() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));
        state.add_mempool_tx(0, 2, make_hash(3));

        // Mark the first tx as invalid (simulating reorg invalidation)
        state.mark_tx_invalid(&make_hash(1));

        let removed = state.cleanup(true);
        // All 3 txs should be cascade-deleted
        assert_eq!(removed.len(), 3);
        assert!(removed.contains(&make_hash(1)));
        assert!(removed.contains(&make_hash(2)));
        assert!(removed.contains(&make_hash(3)));
        assert_eq!(state.get_cache_size(0), 0);
    }

    #[test]
    fn test_reorg_valid_first_tx_preserved() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));
        state.add_mempool_tx(0, 2, make_hash(3));

        // All txs are valid (default)
        let removed = state.cleanup(true);
        assert!(removed.is_empty());
        assert_eq!(state.get_cache_size(0), 3);
    }

    #[test]
    fn test_reorg_revert_nonce() {
        let mut state = MockReorgState::new();
        state.add_account(0, 5);
        state.add_mempool_tx(0, 5, make_hash(1));
        state.add_mempool_tx(0, 6, make_hash(2));
        state.add_mempool_tx(0, 7, make_hash(3));

        // Simulate: block mined nonce 5, then reverted
        state.simulate_block_mined(0, 1); // blockchain_nonce becomes 6
        state.cleanup(false); // Remove nonce 5 from mempool
        assert_eq!(state.get_cache_size(0), 2);

        // Reorg: revert blockchain nonce back to 5
        state.simulate_reorg(0, 5);
        // After reorg, nonces 6 and 7 are still in mempool
        // They are now "pending" again (blockchain nonce is 5 again)
        assert_eq!(state.get_cache_size(0), 2);
        assert_eq!(state.get_cache_min_nonce(0), 6);
    }

    // =========================================================================
    // Reorg Scenario Tests
    // =========================================================================

    #[test]
    fn test_reorg_block_orphaned() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));
        state.add_mempool_tx(0, 2, make_hash(3));

        // Block mined with nonces 0,1,2
        state.simulate_block_mined(0, 3);
        state.cleanup(false);
        assert_eq!(state.get_cache_size(0), 0);

        // Block orphaned: revert to nonce 0
        state.simulate_reorg(0, 0);

        // Re-add txs to mempool (they return from orphaned block)
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));
        state.add_mempool_tx(0, 2, make_hash(3));

        // Full cleanup to reverify
        let removed = state.cleanup(true);
        // All txs are valid, so nothing removed
        assert!(removed.is_empty());
        assert_eq!(state.get_cache_size(0), 3);
    }

    #[test]
    fn test_reorg_nonce_collision() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);

        // Original mempool has nonces 0, 1, 2
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));
        state.add_mempool_tx(0, 2, make_hash(3));

        // Block mined nonces 0, 1
        state.simulate_block_mined(0, 2);
        state.cleanup(false);
        assert_eq!(state.get_cache_size(0), 1); // Only nonce 2 remains

        // Reorg: new chain has different tx at nonce 1
        // Revert to nonce 0
        state.simulate_reorg(0, 0);

        // The mempool still has nonce 2 (from before)
        // But now blockchain nonce is 0, so nonce 2 has a gap
        // In real system, nonce 2 alone would be invalid without 0 and 1
        // The full cleanup would catch this via reverification
        state.mark_tx_invalid(&make_hash(3)); // Nonce 2 tx is now invalid

        let removed = state.cleanup(true);
        assert_eq!(removed.len(), 1);
        assert!(removed.contains(&make_hash(3)));
        assert_eq!(state.get_cache_size(0), 0);
    }

    #[test]
    fn test_reorg_multiple_blocks() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));
        state.add_mempool_tx(0, 2, make_hash(3));
        state.add_mempool_tx(0, 3, make_hash(4));
        state.add_mempool_tx(0, 4, make_hash(5));

        // Three blocks mined (nonces 0-4)
        state.simulate_block_mined(0, 5);
        state.cleanup(false);
        assert_eq!(state.get_cache_size(0), 0);

        // Deep reorg: revert all three blocks
        state.simulate_reorg(0, 0);

        // Re-add all txs
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));
        state.add_mempool_tx(0, 2, make_hash(3));
        state.add_mempool_tx(0, 3, make_hash(4));
        state.add_mempool_tx(0, 4, make_hash(5));

        // Mark first tx as invalid in new chain state
        state.mark_tx_invalid(&make_hash(1));

        let removed = state.cleanup(true);
        // Cascade: all 5 txs removed because first is invalid
        assert_eq!(removed.len(), 5);
        assert_eq!(state.get_cache_size(0), 0);
    }

    #[test]
    fn test_reorg_deep() {
        let mut state = MockReorgState::new();

        // Multiple senders affected by deep reorg
        state.add_account(0, 0);
        state.add_account(1, 0);
        state.add_account(2, 0);

        // Each sender has txs
        for nonce in 0..5 {
            state.add_mempool_tx(0, nonce, make_hash(nonce as u8 + 10));
            state.add_mempool_tx(1, nonce, make_hash(nonce as u8 + 20));
            state.add_mempool_tx(2, nonce, make_hash(nonce as u8 + 30));
        }

        // All blocks mined
        state.simulate_block_mined(0, 5);
        state.simulate_block_mined(1, 5);
        state.simulate_block_mined(2, 5);
        state.cleanup(false);

        // Deep reorg reverts everyone
        state.simulate_reorg(0, 0);
        state.simulate_reorg(1, 0);
        state.simulate_reorg(2, 0);

        // Re-add txs for all senders
        for nonce in 0..5 {
            state.add_mempool_tx(0, nonce, make_hash(nonce as u8 + 10));
            state.add_mempool_tx(1, nonce, make_hash(nonce as u8 + 20));
            state.add_mempool_tx(2, nonce, make_hash(nonce as u8 + 30));
        }

        // Sender 0's first tx is invalid, sender 1 and 2 are valid
        state.mark_tx_invalid(&make_hash(10));

        let removed = state.cleanup(true);
        // Only sender 0's cache is cascade-deleted (5 txs)
        assert_eq!(removed.len(), 5);
        assert_eq!(state.get_cache_size(0), 0);
        assert_eq!(state.get_cache_size(1), 5);
        assert_eq!(state.get_cache_size(2), 5);
    }

    // =========================================================================
    // State Consistency Tests
    // =========================================================================

    #[test]
    fn test_post_reorg_nonce_continuity() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));
        state.add_mempool_tx(0, 2, make_hash(3));
        state.add_mempool_tx(0, 3, make_hash(4));

        // Mine first 2, then reorg back 1
        state.simulate_block_mined(0, 2);
        state.cleanup(false);
        assert_eq!(state.get_cache_size(0), 2); // nonces 2, 3 remain

        // After cleanup, remaining txs should have valid consecutive nonces
        let cache = state.mempool_caches.get(&0).unwrap();
        assert_eq!(cache.min_nonce, 2);
        assert_eq!(cache.max_nonce, 3);
        // Next nonce should be max + 1
        assert_eq!(cache.get_next_nonce(), 4);
    }

    #[test]
    fn test_post_cleanup_min_nonce_correct() {
        let mut state = MockReorgState::new();
        state.add_account(0, 10);
        state.add_mempool_tx(0, 10, make_hash(1));
        state.add_mempool_tx(0, 11, make_hash(2));
        state.add_mempool_tx(0, 12, make_hash(3));
        state.add_mempool_tx(0, 13, make_hash(4));

        // Blockchain advances to nonce 12
        state.simulate_block_mined(0, 2);
        state.cleanup(false);

        // min_nonce should now be 12 (the blockchain nonce)
        assert_eq!(state.get_cache_min_nonce(0), 12);
        assert_eq!(state.get_cache_size(0), 2);
    }

    #[test]
    fn test_reorg_then_normal_operation() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));

        // Mine, then reorg
        state.simulate_block_mined(0, 2);
        state.cleanup(false);
        state.simulate_reorg(0, 0);

        // Re-add txs after reorg
        state.add_mempool_tx(0, 0, make_hash(10));
        state.add_mempool_tx(0, 1, make_hash(11));

        // Full cleanup (valid)
        let removed = state.cleanup(true);
        assert!(removed.is_empty());

        // Continue adding txs normally
        state.add_mempool_tx(0, 2, make_hash(12));
        assert_eq!(state.get_cache_size(0), 3);
        assert_eq!(state.get_cache_next_nonce(0), 3);
    }

    // =========================================================================
    // Edge Case Tests
    // =========================================================================

    #[test]
    fn test_reorg_empty_mempool() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);

        // Reorg with no txs in mempool
        state.simulate_reorg(0, 0);
        let removed = state.cleanup(true);
        assert!(removed.is_empty());
        assert_eq!(state.get_cache_size(0), 0);
    }

    #[test]
    fn test_reorg_single_tx_cache() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_mempool_tx(0, 0, make_hash(1));

        // Single tx, marked invalid
        state.mark_tx_invalid(&make_hash(1));

        let removed = state.cleanup(true);
        assert_eq!(removed.len(), 1);
        assert!(removed.contains(&make_hash(1)));
        assert_eq!(state.get_cache_size(0), 0);
    }

    #[test]
    fn test_full_cleanup_no_cascade_if_valid() {
        let mut state = MockReorgState::new();
        state.add_account(0, 0);
        state.add_mempool_tx(0, 0, make_hash(1));
        state.add_mempool_tx(0, 1, make_hash(2));
        state.add_mempool_tx(0, 2, make_hash(3));

        // full=true but first tx is valid: no cascade
        let removed = state.cleanup(true);
        assert!(removed.is_empty());
        assert_eq!(state.get_cache_size(0), 3);

        // Verify all txs are still there
        let cache = state.mempool_caches.get(&0).unwrap();
        assert_eq!(cache.txs[0], make_hash(1));
        assert_eq!(cache.txs[1], make_hash(2));
        assert_eq!(cache.txs[2], make_hash(3));
    }
}
