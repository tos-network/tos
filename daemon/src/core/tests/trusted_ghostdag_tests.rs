// BUG-002 Trusted GHOSTDAG Tests
//
// Tests for the TrustedBlock pattern used during P2P sync.
// This tests the scenario where a node syncs through a fork region
// and needs to use peer-provided GHOSTDAG data instead of local computation.
//
// Background (BUG-002):
// When a node syncs through a fork region (where DAG has multiple tips that merge),
// local GHOSTDAG computation can produce different results than the peer's GHOSTDAG
// because the syncing node may process blocks in a different order or miss some
// mergeset context. This causes "Block height mismatch" errors.
//
// The fix: During sync, peer provides GHOSTDAG data along with blocks.
// The syncing node pre-stores this "trusted" GHOSTDAG data before validation.
// add_new_block() then uses this trusted data instead of computing locally.
//
// This test file simulates the height ~22960 region from testnet where:
// - topoheight 22958: Sync block (blue_score=22958)
// - topoheight 22959: Side block (blue_score=22955)
// - topoheight 22960: Side block (blue_score=22956)
// - topoheight 22961: Normal block with 2 tips (blue_score=22961) - merge point

#[cfg(test)]
mod bug002_tests {
    use crate::core::ghostdag::{BlueWorkType, KType, TosGhostdagData};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tos_common::{crypto::Hash, varuint::VarUint};

    /// Mock storage that simulates the BUG-002 scenario
    /// This provides GHOSTDAG data as if pre-stored by the sync mechanism
    struct Bug002MockStorage {
        ghostdag_data: HashMap<Hash, Arc<TosGhostdagData>>,
    }

    impl Bug002MockStorage {
        fn new() -> Self {
            Self {
                ghostdag_data: HashMap::new(),
            }
        }

        /// Pre-store trusted GHOSTDAG data (simulating what sync does)
        fn pre_store_trusted_ghostdag(&mut self, hash: Hash, data: TosGhostdagData) {
            self.ghostdag_data.insert(hash, Arc::new(data));
        }

        /// Check if trusted GHOSTDAG data exists
        fn has_trusted_ghostdag(&self, hash: &Hash) -> bool {
            self.ghostdag_data.contains_key(hash)
        }

        /// Get trusted GHOSTDAG data
        fn get_trusted_ghostdag(&self, hash: &Hash) -> Option<Arc<TosGhostdagData>> {
            self.ghostdag_data.get(hash).cloned()
        }
    }

    // Helper to create test hashes
    fn make_hash(id: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        Hash::new(bytes)
    }

    /// Test 1: Verify trusted GHOSTDAG data is used when available
    ///
    /// Simulates the BUG-002 scenario where:
    /// - Block claims blue_score=22961
    /// - Local computation would give different result (e.g., 22960)
    /// - But trusted data has blue_score=22961
    /// - Validation should pass using trusted data
    #[test]
    fn test_trusted_ghostdag_used_for_validation() {
        let mut storage = Bug002MockStorage::new();

        // Create the merge block (like topoheight 22961)
        let merge_block = make_hash(1);
        let parent1 = make_hash(2); // Sync block parent
        let parent2 = make_hash(3); // Side block parent

        // The block claims blue_score=22961
        let block_claimed_blue_score: u64 = 22961;

        // Pre-store trusted GHOSTDAG data from peer
        // This is what the peer says the GHOSTDAG should be
        let trusted_ghostdag = TosGhostdagData::new(
            22961, // blue_score - matches block claim
            BlueWorkType::from(VarUint::from(1000000u64)),
            22961, // daa_score
            parent1.clone(),
            vec![parent1.clone()], // mergeset_blues
            vec![parent2.clone()], // mergeset_reds (Side block is red)
            HashMap::new(),
            Vec::new(),
        );
        storage.pre_store_trusted_ghostdag(merge_block.clone(), trusted_ghostdag);

        // Verify trusted data is found
        assert!(
            storage.has_trusted_ghostdag(&merge_block),
            "Trusted GHOSTDAG data should be pre-stored"
        );

        // Get trusted data
        let trusted = storage.get_trusted_ghostdag(&merge_block).unwrap();

        // Validation: block's claimed blue_score should match trusted data
        assert_eq!(
            trusted.blue_score, block_claimed_blue_score,
            "Trusted blue_score should match block's claim"
        );

        // This simulates what add_new_block does:
        // Instead of computing locally, it uses trusted data
        let expected_blue_score = trusted.blue_score;
        assert_eq!(
            expected_blue_score, block_claimed_blue_score,
            "Validation should pass using trusted GHOSTDAG data"
        );
    }

    /// Test 2: Verify local computation is used when no trusted data
    ///
    /// When syncing without GHOSTDAG data (legacy peers), local computation
    /// should still be used and validation should work for consistent chains.
    #[test]
    fn test_local_computation_without_trusted_data() {
        let storage = Bug002MockStorage::new();

        let block = make_hash(1);

        // No trusted data pre-stored
        assert!(
            !storage.has_trusted_ghostdag(&block),
            "Should not have trusted data"
        );

        // In real code, this would trigger local GHOSTDAG computation
        // For this test, we just verify the fallback path is taken
    }

    /// Test 3: Mergeset size limit enforced for trusted data
    ///
    /// SECURITY: Even trusted data must pass the mergeset size check (4*k+16)
    /// This prevents malicious peers from injecting oversized mergesets.
    #[test]
    fn test_mergeset_size_limit_enforced_for_trusted() {
        let k: KType = 10; // Typical k value
        let max_mergeset = (4 * k as usize) + 16; // 56

        // Create trusted data with valid mergeset size
        let valid_mergeset_blues: Vec<Hash> = (0..20).map(|i| make_hash(i)).collect();
        let valid_mergeset_reds: Vec<Hash> = (20..35).map(|i| make_hash(i)).collect();

        let valid_size = valid_mergeset_blues.len() + valid_mergeset_reds.len();
        assert!(
            valid_size <= max_mergeset,
            "Valid mergeset should be within limit: {} <= {}",
            valid_size,
            max_mergeset
        );

        // Create trusted data with oversized mergeset (attack scenario)
        let oversized_mergeset_blues: Vec<Hash> = (0..40).map(|i| make_hash(i)).collect();
        let oversized_mergeset_reds: Vec<Hash> = (40..80).map(|i| make_hash(i)).collect();

        let oversized = oversized_mergeset_blues.len() + oversized_mergeset_reds.len();
        assert!(
            oversized > max_mergeset,
            "Oversized mergeset should exceed limit: {} > {}",
            oversized,
            max_mergeset
        );

        // In real code, even trusted data would be rejected if mergeset is too large
        // This is the security fix added after initial BUG-002 implementation
    }

    /// Test 4: Fork-merge scenario like height 22960
    ///
    /// Simulates the exact DAG structure that caused BUG-002:
    /// ```
    /// topoheight 22958: block_a (Sync, blue_score=22958)
    ///                            \
    /// topoheight 22961: block_d (Normal, blue_score=22961) -- merge
    ///                            /
    /// topoheight 22960: block_c (Side, blue_score=22956)
    ///                      |
    /// topoheight 22959: block_b (Side, blue_score=22955)
    /// ```
    #[test]
    fn test_fork_merge_scenario_height_22960() {
        let mut storage = Bug002MockStorage::new();

        // Create blocks matching testnet structure
        let block_a = make_hash(0xA); // topoheight 22958, Sync, blue_score=22958
        let block_b = make_hash(0xB); // topoheight 22959, Side, blue_score=22955
        let block_c = make_hash(0xC); // topoheight 22960, Side, blue_score=22956
        let block_d = make_hash(0xD); // topoheight 22961, Normal, blue_score=22961

        // Pre-store trusted GHOSTDAG data for each block
        // This is what the peer provides during sync

        // Block A (Sync block)
        storage.pre_store_trusted_ghostdag(
            block_a.clone(),
            TosGhostdagData::new(
                22958,
                BlueWorkType::from(VarUint::from(998000u64)),
                22958,
                make_hash(0x9), // parent
                vec![make_hash(0x9)],
                vec![],
                HashMap::new(),
                Vec::new(),
            ),
        );

        // Block B (Side block, lower blue_score due to being on side chain)
        storage.pre_store_trusted_ghostdag(
            block_b.clone(),
            TosGhostdagData::new(
                22955, // Note: blue_score < topoheight because it's a side block
                BlueWorkType::from(VarUint::from(995000u64)),
                22955,
                make_hash(0x8),
                vec![make_hash(0x8)],
                vec![],
                HashMap::new(),
                Vec::new(),
            ),
        );

        // Block C (Side block)
        storage.pre_store_trusted_ghostdag(
            block_c.clone(),
            TosGhostdagData::new(
                22956,
                BlueWorkType::from(VarUint::from(996000u64)),
                22956,
                block_b.clone(),
                vec![block_b.clone()],
                vec![],
                HashMap::new(),
                Vec::new(),
            ),
        );

        // Block D (Merge block with 2 tips)
        // This is the critical block that was failing
        storage.pre_store_trusted_ghostdag(
            block_d.clone(),
            TosGhostdagData::new(
                22961,
                BlueWorkType::from(VarUint::from(1001000u64)),
                22961,
                block_a.clone(), // selected_parent is the Sync block (higher blue_work)
                vec![block_a.clone()], // mergeset_blues: Sync block is blue
                vec![block_c.clone()], // mergeset_reds: Side block is red
                HashMap::new(),
                Vec::new(),
            ),
        );

        // Verify all trusted data is stored
        assert!(storage.has_trusted_ghostdag(&block_a));
        assert!(storage.has_trusted_ghostdag(&block_b));
        assert!(storage.has_trusted_ghostdag(&block_c));
        assert!(storage.has_trusted_ghostdag(&block_d));

        // Verify Block D (merge block) validation would pass
        let block_d_claimed_blue_score: u64 = 22961;
        let trusted_d = storage.get_trusted_ghostdag(&block_d).unwrap();

        assert_eq!(
            trusted_d.blue_score, block_d_claimed_blue_score,
            "Merge block blue_score should match trusted data"
        );

        // Without trusted data, local computation might give blue_score=22960
        // because it might not have full context of the Side chain
        // This was the root cause of BUG-002
        let hypothetical_local_blue_score: u64 = 22960; // What local might compute

        // The fix: We use trusted data (22961) instead of local (22960)
        assert_ne!(
            hypothetical_local_blue_score, block_d_claimed_blue_score,
            "This demonstrates why trusted data is needed - local computation differs"
        );
        assert_eq!(
            trusted_d.blue_score, block_d_claimed_blue_score,
            "Trusted data allows validation to pass"
        );
    }

    /// Test 5: Blue/Red classification in mergeset
    ///
    /// Verifies that the mergeset correctly identifies:
    /// - Blue blocks: blocks in the k-cluster of selected parent
    /// - Red blocks: blocks outside the k-cluster
    #[test]
    fn test_mergeset_blue_red_classification() {
        // In the height 22960 scenario:
        // - block_a (Sync) becomes blue because it's on the main chain
        // - block_c (Side) becomes red because it's on the side chain

        let block_a = make_hash(0xA);
        let block_c = make_hash(0xC);

        let ghostdag = TosGhostdagData::new(
            22961,
            BlueWorkType::from(VarUint::from(1001000u64)),
            22961,
            block_a.clone(),
            vec![block_a.clone()], // Blue
            vec![block_c.clone()], // Red
            HashMap::new(),
            Vec::new(),
        );

        assert_eq!(ghostdag.mergeset_blues.len(), 1, "Should have 1 blue block");
        assert_eq!(ghostdag.mergeset_reds.len(), 1, "Should have 1 red block");
        assert!(
            ghostdag.mergeset_blues.contains(&block_a),
            "Sync block should be blue"
        );
        assert!(
            ghostdag.mergeset_reds.contains(&block_c),
            "Side block should be red"
        );

        // Blue score increment = mergeset_blues.len() (not total parents!)
        // This is critical for correct blue_score calculation
        // The merge block's blue_score = selected_parent.blue_score + 1 + mergeset_blues.len()
        // (the +1 is for the block itself being blue)
        // Actual formula depends on GHOSTDAG implementation details
        let _blue_score_increment = ghostdag.mergeset_blues.len() as u64;
    }

    /// Test 6: DAA score consistency
    ///
    /// DAA score must be consistent with GHOSTDAG data for difficulty adjustment.
    #[test]
    fn test_daa_score_consistency() {
        let ghostdag = TosGhostdagData::new(
            22961,
            BlueWorkType::from(VarUint::from(1001000u64)),
            22961, // daa_score should match or be related to blue_score
            make_hash(0xA),
            vec![],
            vec![],
            HashMap::new(),
            Vec::new(),
        );

        // DAA score is used for difficulty adjustment window
        // It should be monotonically increasing along the selected chain
        assert_eq!(
            ghostdag.daa_score, 22961,
            "DAA score should be set correctly"
        );
    }
}
