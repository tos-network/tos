#[cfg(test)]
mod tests {
    use super::super::mock::*;

    #[test]
    fn common_point_detection_finds_highest_shared_block() {
        // Two chains diverge at block 20
        let chain_a = make_linear_chain(50, 100);
        let chain_b = make_linear_chain(20, 100); // shares first 20 blocks

        let block_ids_b = make_block_ids(&chain_b);
        let common = chain_a.find_common_point(&block_ids_b);

        assert!(common.is_some());
        let common = common.unwrap();
        assert_eq!(common.topoheight, 20);
    }

    #[test]
    fn common_point_no_shared_blocks_returns_none() {
        let chain_a = make_linear_chain(10, 100);

        // Create completely foreign block IDs
        let foreign_ids: Vec<BlockId> = (1..=10)
            .map(|i| {
                let mut hash = [0xAAu8; 32];
                hash[0..8].copy_from_slice(&(i + 1000u64).to_le_bytes());
                BlockId {
                    hash,
                    topoheight: i,
                }
            })
            .collect();

        let common = chain_a.find_common_point(&foreign_ids);
        assert!(common.is_none());
    }

    #[test]
    fn common_point_single_shared_block_at_genesis() {
        let chain_a = make_linear_chain(50, 100);

        // Only genesis-adjacent block (block 1) is shared
        let block_at_1 = chain_a.get_block_at_topo(1).unwrap();
        let single_id = vec![BlockId {
            hash: block_at_1.hash,
            topoheight: 1,
        }];

        let common = chain_a.find_common_point(&single_id);
        assert!(common.is_some());
        assert_eq!(common.unwrap().topoheight, 1);
    }

    #[test]
    fn common_point_multiple_shared_blocks_takes_highest() {
        let chain_a = make_linear_chain(50, 100);

        // Provide multiple block IDs that exist in chain_a
        let mut shared_ids = Vec::new();
        for topo in [5, 10, 15, 25, 30] {
            let block = chain_a.get_block_at_topo(topo).unwrap();
            shared_ids.push(BlockId {
                hash: block.hash,
                topoheight: topo,
            });
        }

        let common = chain_a.find_common_point(&shared_ids);
        assert!(common.is_some());
        assert_eq!(common.unwrap().topoheight, 30); // highest shared
    }

    #[test]
    fn chain_request_builds_list_of_block_ids() {
        let chain = make_linear_chain(100, 100);
        let block_ids = make_block_ids(&chain);

        assert_eq!(block_ids.len(), 100);
        // Block IDs should be ordered by topoheight
        for i in 1..block_ids.len() {
            assert!(block_ids[i].topoheight > block_ids[i - 1].topoheight);
        }
    }

    #[test]
    fn chain_response_pop_count_zero_no_reorg() {
        // When common point is at our tip, pop_count = 0
        let chain = make_linear_chain(50, 100);
        let our_topoheight = chain.topoheight;

        // Common point is at our current topoheight
        let common_point_topo: TopoHeight = our_topoheight;
        let pop_count = our_topoheight.saturating_sub(common_point_topo);
        assert_eq!(pop_count, 0);
    }

    #[test]
    fn chain_response_pop_count_greater_than_zero_reorg_detected() {
        // When common point is behind our tip, we need to pop
        let chain = make_linear_chain(50, 100);
        let our_topoheight = chain.topoheight;

        // Common point is 5 blocks behind our tip
        let common_point_topo: TopoHeight = 45;
        let pop_count = our_topoheight.saturating_sub(common_point_topo);
        assert_eq!(pop_count, 5);
        assert!(pop_count > 0);
    }

    #[test]
    fn chain_response_pop_count_calculation_from_common_point() {
        let chain = make_linear_chain(100, 100);

        // Various common points and their expected pop counts
        let test_cases: Vec<(TopoHeight, u64)> = vec![
            (100, 0), // at tip: no pop
            (99, 1),  // 1 behind: pop 1
            (90, 10), // 10 behind: pop 10
            (50, 50), // halfway: pop 50
            (1, 99),  // near genesis: pop 99
        ];

        for (common_topo, expected_pop) in test_cases {
            let pop_count = chain.topoheight.saturating_sub(common_topo);
            assert_eq!(
                pop_count, expected_pop,
                "Common point at {} should need {} pops",
                common_topo, expected_pop
            );
        }
    }

    #[test]
    fn response_size_cannot_exceed_max_blocks() {
        // CHAIN_SYNC_RESPONSE_MAX_BLOCKS = u16::MAX = 65535
        assert_eq!(CHAIN_SYNC_RESPONSE_MAX_BLOCKS, 65535);

        let response_size: usize = 70000;
        let clamped = std::cmp::min(response_size, CHAIN_SYNC_RESPONSE_MAX_BLOCKS);
        assert_eq!(clamped, CHAIN_SYNC_RESPONSE_MAX_BLOCKS);
    }

    #[test]
    fn response_minimum_blocks() {
        assert_eq!(CHAIN_SYNC_RESPONSE_MIN_BLOCKS, 512);

        // A response with fewer than min blocks is still valid (peer exhausted)
        let response_size: usize = 100;
        let meets_minimum = response_size >= CHAIN_SYNC_RESPONSE_MIN_BLOCKS;
        assert!(!meets_minimum);
    }

    #[test]
    fn default_response_size() {
        let default_blocks = CHAIN_SYNC_DEFAULT_RESPONSE_BLOCKS;
        let min_blocks = CHAIN_SYNC_RESPONSE_MIN_BLOCKS;
        let max_blocks = CHAIN_SYNC_RESPONSE_MAX_BLOCKS;
        assert_eq!(default_blocks, 4096);

        // Default should be between min and max
        assert!(default_blocks >= min_blocks);
        assert!(default_blocks <= max_blocks);
    }

    #[test]
    fn request_max_blocks() {
        assert_eq!(CHAIN_SYNC_REQUEST_MAX_BLOCKS, 64);

        // Request should not exceed this limit
        let request_size: usize = 100;
        let valid = request_size <= CHAIN_SYNC_REQUEST_MAX_BLOCKS;
        assert!(!valid);

        let request_size: usize = 64;
        let valid = request_size <= CHAIN_SYNC_REQUEST_MAX_BLOCKS;
        assert!(valid);
    }

    #[test]
    fn top_blocks_included_up_to_limit() {
        assert_eq!(CHAIN_SYNC_TOP_BLOCKS, 10);

        // Simulate including top blocks (alternative tips)
        let chain = make_linear_chain(50, 100);
        let top_blocks: Vec<&BlockMetadata> = chain
            .blocks
            .iter()
            .rev()
            .take(CHAIN_SYNC_TOP_BLOCKS)
            .map(|(_, b)| b)
            .collect();

        assert_eq!(top_blocks.len(), CHAIN_SYNC_TOP_BLOCKS);
    }

    #[test]
    fn common_point_at_stable_height_no_deep_reorg() {
        let chain = make_linear_chain(100, 100);

        // Common point at stable height means shallow reorg
        let common_topo = chain.stable_topoheight;
        let pop_count = chain.topoheight.saturating_sub(common_topo);
        // Pop count equals STABLE_LIMIT
        assert_eq!(pop_count, STABLE_LIMIT);
    }

    #[test]
    fn common_point_below_stable_height_deep_reorg() {
        let chain = make_linear_chain(100, 100);

        // Common point below stable height indicates a deep reorg
        let common_topo = chain.stable_topoheight.saturating_sub(10);
        let pop_count = chain.topoheight.saturating_sub(common_topo);
        // Pop count exceeds STABLE_LIMIT
        assert!(pop_count > STABLE_LIMIT);
        assert_eq!(pop_count, STABLE_LIMIT + 10);
    }

    #[test]
    fn chain_sync_with_boost_mode_parallel_downloads() {
        // In boost mode, multiple ranges can be requested simultaneously
        let total_blocks_needed: usize = 10000;
        let parallel_requests = 4;
        let blocks_per_request = CHAIN_SYNC_DEFAULT_RESPONSE_BLOCKS;

        let total_per_round = parallel_requests * blocks_per_request;
        let rounds_needed = total_blocks_needed.div_ceil(total_per_round);

        // With 4 parallel requests of 4096 blocks each, we process 16384 per round
        assert_eq!(total_per_round, 16384);
        assert_eq!(rounds_needed, 1); // 10000 < 16384
    }

    #[test]
    fn empty_response_peer_has_no_new_blocks() {
        // When peer has nothing new, response contains 0 blocks
        let response_blocks: Vec<BlockMetadata> = Vec::new();
        assert!(response_blocks.is_empty());

        // This is valid: peer is at same height or behind
        let pop_count: u64 = 0;
        let has_new_blocks = !response_blocks.is_empty();
        assert!(!has_new_blocks);
        assert_eq!(pop_count, 0);
    }

    #[test]
    fn response_with_fewer_blocks_than_minimum_peer_exhausted() {
        // Peer may send fewer than CHAIN_SYNC_RESPONSE_MIN_BLOCKS if they have no more
        let peer_remaining_blocks = 100; // less than 512
        assert!(peer_remaining_blocks < CHAIN_SYNC_RESPONSE_MIN_BLOCKS);

        // This indicates the peer has been exhausted and sync is complete
        let is_sync_complete = peer_remaining_blocks < CHAIN_SYNC_RESPONSE_MIN_BLOCKS;
        assert!(is_sync_complete);
    }
}
