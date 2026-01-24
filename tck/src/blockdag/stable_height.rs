// Tests for stable height and finality concepts in the BlockDAG.
//
// Key concepts tested:
// - STABLE_LIMIT = 24: the number of heights after which a block is considered stable
// - A block is stable when: current_height - block_height >= STABLE_LIMIT
// - A sync block is: the block with highest cumulative difficulty at its height,
//   within the stable zone
//
// These tests verify the conceptual properties without requiring a full Blockchain
// instance, since `is_sync_block_at_height` is a method on `Blockchain<S>`.

#[cfg(test)]
mod tests {
    use tos_common::block::BlockVersion;
    use tos_daemon::config::{get_stable_limit, STABLE_LIMIT};

    // =========================================================================
    // Tests for STABLE_LIMIT constant value
    // =========================================================================

    /// Verify that the STABLE_LIMIT constant is exactly 24.
    #[test]
    fn test_stable_limit_value() {
        assert_eq!(STABLE_LIMIT, 24, "STABLE_LIMIT must be 24");
    }

    /// Verify that get_stable_limit returns 24 for the Nobunaga block version.
    #[test]
    fn test_get_stable_limit_nobunaga() {
        let limit = get_stable_limit(BlockVersion::Nobunaga);
        assert_eq!(limit, 24, "get_stable_limit(Nobunaga) must return 24");
    }

    // =========================================================================
    // Tests for block stability depth logic
    // =========================================================================

    /// A block at height H is stable when the current height equals H + STABLE_LIMIT.
    /// This is the exact boundary where stability is reached.
    #[test]
    fn test_block_stability_at_exact_depth() {
        let block_height: u64 = 100;
        let current_height = block_height + STABLE_LIMIT;

        // The stability condition: current - block >= STABLE_LIMIT
        let depth = current_height - block_height;
        assert_eq!(depth, STABLE_LIMIT);
        assert!(
            depth >= STABLE_LIMIT,
            "Block at depth {} should be stable (STABLE_LIMIT = {})",
            depth,
            STABLE_LIMIT
        );
    }

    /// A block at height H is NOT stable when current height is H + STABLE_LIMIT - 1.
    /// One block short of stability.
    #[test]
    fn test_block_not_stable_below_depth() {
        let block_height: u64 = 100;
        let current_height = block_height + STABLE_LIMIT - 1;

        let depth = current_height - block_height;
        assert_eq!(depth, STABLE_LIMIT - 1);
        assert!(
            depth < STABLE_LIMIT,
            "Block at depth {} should NOT be stable (STABLE_LIMIT = {})",
            depth,
            STABLE_LIMIT
        );
    }

    /// A block at height H is stable when current height is H + STABLE_LIMIT + 1.
    /// One block beyond the stability boundary.
    #[test]
    fn test_block_stable_above_depth() {
        let block_height: u64 = 100;
        let current_height = block_height + STABLE_LIMIT + 1;

        let depth = current_height - block_height;
        assert_eq!(depth, STABLE_LIMIT + 1);
        assert!(
            depth >= STABLE_LIMIT,
            "Block at depth {} should be stable (STABLE_LIMIT = {})",
            depth,
            STABLE_LIMIT
        );
    }

    /// The genesis block (height 0) becomes stable once the chain reaches height 24.
    #[test]
    fn test_genesis_stable_after_24_blocks() {
        let genesis_height: u64 = 0;

        // Not yet stable at height 23
        let current_23 = 23u64;
        assert!(
            current_23 - genesis_height < STABLE_LIMIT,
            "Genesis should NOT be stable at chain height 23"
        );

        // Stable at height 24
        let current_24 = 24u64;
        assert!(
            current_24 - genesis_height >= STABLE_LIMIT,
            "Genesis should be stable at chain height 24"
        );

        // Also stable at height 25
        let current_25 = 25u64;
        assert!(
            current_25 - genesis_height >= STABLE_LIMIT,
            "Genesis should remain stable at chain height 25"
        );
    }

    /// Edge cases for the stability boundary condition.
    /// Tests height 0, height 1, and a very large height near u64::MAX.
    #[test]
    fn test_stability_boundary_conditions() {
        // Case 1: Block at height 0
        let block_h0: u64 = 0;
        let current_for_h0 = block_h0 + STABLE_LIMIT;
        assert_eq!(current_for_h0, 24);
        assert!(current_for_h0 - block_h0 >= STABLE_LIMIT);

        // Case 2: Block at height 1
        let block_h1: u64 = 1;
        let current_for_h1 = block_h1 + STABLE_LIMIT;
        assert_eq!(current_for_h1, 25);
        assert!(current_for_h1 - block_h1 >= STABLE_LIMIT);

        // Case 3: Block at a very large height (u64::MAX - 24)
        // This ensures no overflow in the subtraction
        let block_large: u64 = u64::MAX - STABLE_LIMIT;
        let current_large = u64::MAX;
        let depth_large = current_large - block_large;
        assert_eq!(depth_large, STABLE_LIMIT);
        assert!(
            depth_large >= STABLE_LIMIT,
            "Large height block should be stable at u64::MAX"
        );

        // Case 4: Block at height u64::MAX - 23 (one less than STABLE_LIMIT depth)
        let block_not_stable: u64 = u64::MAX - (STABLE_LIMIT - 1);
        let depth_not_stable = u64::MAX - block_not_stable;
        assert_eq!(depth_not_stable, STABLE_LIMIT - 1);
        assert!(
            depth_not_stable < STABLE_LIMIT,
            "Block one short of STABLE_LIMIT depth should not be stable"
        );
    }

    // =========================================================================
    // Tests for sync block concept
    // =========================================================================

    /// When there is only a single ordered block at a given height, it is
    /// trivially the sync block (highest cumulative difficulty at that height).
    #[test]
    fn test_sync_block_concept_single_block_at_height() {
        // Simulate: one block at height 10, cumulative difficulty 500
        struct BlockInfo {
            _height: u64,
            cumulative_difficulty: u64,
            is_sync: bool,
        }

        let block = BlockInfo {
            _height: 10,
            cumulative_difficulty: 500,
            is_sync: true, // single block at this height is always the sync block
        };

        // With only one block at a height, it must be the sync block
        let blocks_at_height = [&block];
        let best = blocks_at_height
            .iter()
            .max_by_key(|b| b.cumulative_difficulty);
        assert!(best.is_some());
        assert!(best.unwrap().is_sync);
        assert_eq!(best.unwrap().cumulative_difficulty, 500);
    }

    /// When multiple blocks exist at the same height, the one with highest
    /// cumulative difficulty is the sync block (fork choice rule).
    #[test]
    fn test_sync_block_concept_multiple_blocks_highest_difficulty() {
        struct BlockInfo {
            id: u8,
            cumulative_difficulty: u64,
        }

        let blocks_at_height = vec![
            BlockInfo {
                id: 1,
                cumulative_difficulty: 300,
            },
            BlockInfo {
                id: 2,
                cumulative_difficulty: 500,
            },
            BlockInfo {
                id: 3,
                cumulative_difficulty: 400,
            },
        ];

        // The sync block is the one with highest cumulative difficulty
        let sync_block = blocks_at_height
            .iter()
            .max_by_key(|b| b.cumulative_difficulty);
        assert!(sync_block.is_some());
        let sync = sync_block.unwrap();
        assert_eq!(
            sync.id, 2,
            "Block 2 has the highest CD and should be the sync block"
        );
        assert_eq!(sync.cumulative_difficulty, 500);

        // Verify it has strictly higher CD than all others
        for block in &blocks_at_height {
            if block.id != sync.id {
                assert!(
                    block.cumulative_difficulty < sync.cumulative_difficulty,
                    "Non-sync block {} should have lower CD than sync block",
                    block.id
                );
            }
        }
    }

    /// The stable height can only increase monotonically over time.
    /// Once a block becomes stable, it stays stable.
    #[test]
    fn test_stable_height_never_decreases() {
        // Simulate a chain growing from height 0 to 100
        // Track the stable height at each step
        let mut previous_stable_height: u64 = 0;

        for current_height in 0..=100u64 {
            // Stable height is defined as: current_height saturating_sub STABLE_LIMIT
            let stable_height = current_height.saturating_sub(STABLE_LIMIT);

            // Verify monotonically non-decreasing
            assert!(
                stable_height >= previous_stable_height,
                "Stable height must never decrease: was {}, now {} at chain height {}",
                previous_stable_height,
                stable_height,
                current_height
            );

            previous_stable_height = stable_height;
        }

        // Final stable height at chain height 100 should be 76
        assert_eq!(previous_stable_height, 100 - STABLE_LIMIT);
    }

    // =========================================================================
    // Additional stability concept tests
    // =========================================================================

    /// Verify the stable height formula matches expectations for known values.
    #[test]
    fn test_stable_height_formula() {
        // stable_height = current_height - STABLE_LIMIT (when current >= STABLE_LIMIT)
        assert_eq!(24u64.saturating_sub(STABLE_LIMIT), 0);
        assert_eq!(25u64.saturating_sub(STABLE_LIMIT), 1);
        assert_eq!(48u64.saturating_sub(STABLE_LIMIT), 24);
        assert_eq!(100u64.saturating_sub(STABLE_LIMIT), 76);
        assert_eq!(1000u64.saturating_sub(STABLE_LIMIT), 976);
    }

    /// Below STABLE_LIMIT height, stable height should be 0 (saturating).
    #[test]
    fn test_stable_height_below_limit() {
        for h in 0..STABLE_LIMIT {
            let stable = h.saturating_sub(STABLE_LIMIT);
            assert_eq!(
                stable, 0,
                "For chain height {} (< STABLE_LIMIT), stable height should be 0",
                h
            );
        }
    }

    /// The stability depth doubles for reachability purposes (2 * STABLE_LIMIT = 48).
    /// This is used in build_reachability to traverse enough of the DAG.
    #[test]
    fn test_reachability_depth_is_double_stable_limit() {
        let reachability_depth = 2 * STABLE_LIMIT;
        assert_eq!(reachability_depth, 48);

        let reachability_depth_versioned = 2 * get_stable_limit(BlockVersion::Nobunaga);
        assert_eq!(reachability_depth_versioned, 48);
    }

    /// Verify that PRUNE_SAFETY_LIMIT is derived from STABLE_LIMIT.
    /// PRUNE_SAFETY_LIMIT = STABLE_LIMIT * 10 = 240
    #[test]
    fn test_prune_safety_limit_derived_from_stable_limit() {
        use tos_daemon::config::PRUNE_SAFETY_LIMIT;
        assert_eq!(PRUNE_SAFETY_LIMIT, STABLE_LIMIT * 10);
        assert_eq!(PRUNE_SAFETY_LIMIT, 240);
    }

    /// Verify that blocks propagation capacity is derived from STABLE_LIMIT.
    /// BLOCKS_PROPAGATION_CAPACITY = STABLE_LIMIT * TIPS_LIMIT = 24 * 3 = 72
    #[test]
    fn test_blocks_propagation_capacity_derived() {
        use tos_common::config::TIPS_LIMIT;
        use tos_daemon::config::BLOCKS_PROPAGATION_CAPACITY;
        assert_eq!(
            BLOCKS_PROPAGATION_CAPACITY,
            STABLE_LIMIT as usize * TIPS_LIMIT
        );
        assert_eq!(BLOCKS_PROPAGATION_CAPACITY, 72);
    }

    /// Multiple block versions should all return the same stable limit.
    /// Currently only Nobunaga exists, but this test documents the expectation
    /// that future versions may have different limits.
    #[test]
    fn test_stable_limit_consistency_across_versions() {
        let nobunaga_limit = get_stable_limit(BlockVersion::Nobunaga);
        assert_eq!(nobunaga_limit, STABLE_LIMIT);
        // When new versions are added, they can be tested here
    }

    /// Verify that a sequence of blocks shows correct stability transitions.
    /// Blocks transition from unstable to stable exactly at depth STABLE_LIMIT.
    #[test]
    fn test_stability_transition_sequence() {
        let block_height: u64 = 50;

        // Track the transition from not-stable to stable
        let mut became_stable_at: Option<u64> = None;

        for current in block_height..=(block_height + STABLE_LIMIT + 5) {
            let depth = current - block_height;
            let is_stable = depth >= STABLE_LIMIT;

            if is_stable && became_stable_at.is_none() {
                became_stable_at = Some(current);
            }
        }

        // The block becomes stable exactly at block_height + STABLE_LIMIT
        assert_eq!(
            became_stable_at,
            Some(block_height + STABLE_LIMIT),
            "Block should become stable exactly at height + STABLE_LIMIT"
        );
    }
}
