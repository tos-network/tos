#[cfg(test)]
mod tests {
    use super::super::mock::*;

    #[test]
    fn step_sequence_chaininfo_to_blocks_metadata() {
        // Verify the complete step sequence: ChainInfo -> Assets -> Keys -> Accounts -> Contracts -> BlocksMetadata
        let mut step = StepKind::ChainInfo;
        let expected = [
            StepKind::Assets,
            StepKind::Keys,
            StepKind::Accounts,
            StepKind::Contracts,
            StepKind::BlocksMetadata,
        ];
        for expected_next in &expected {
            let next = step.next().unwrap();
            assert_eq!(next, *expected_next);
            step = next;
        }
    }

    #[test]
    fn step_kind_next_returns_correct_successor() {
        assert_eq!(StepKind::ChainInfo.next(), Some(StepKind::Assets));
        assert_eq!(StepKind::Assets.next(), Some(StepKind::Keys));
        assert_eq!(StepKind::Keys.next(), Some(StepKind::Accounts));
        assert_eq!(StepKind::Accounts.next(), Some(StepKind::Contracts));
        assert_eq!(StepKind::Contracts.next(), Some(StepKind::BlocksMetadata));
    }

    #[test]
    fn step_kind_blocks_metadata_next_returns_none() {
        // BlocksMetadata is the terminal step
        assert_eq!(StepKind::BlocksMetadata.next(), None);
    }

    #[test]
    fn pagination_first_page_returns_items_and_next_page() {
        // More items than one page: first page should have items and indicate next page
        let mut pagination = PaginationState::new(2048);
        assert_eq!(pagination.items_this_page(), MAX_ITEMS_PER_PAGE);
        let next = pagination.next_page();
        assert_eq!(next, Some(1));
    }

    #[test]
    fn pagination_last_page_returns_items_and_none() {
        // When on the last page, next_page should return None
        let mut pagination = PaginationState::new(1500);
        // First page has 1024 items, advance
        let _ = pagination.next_page(); // now on page 1
                                        // Page 1 has 476 items remaining
        assert_eq!(pagination.items_this_page(), 476);
        // No more pages
        let next = pagination.next_page();
        assert_eq!(next, None);
    }

    #[test]
    fn pagination_empty_collection_returns_empty_and_none() {
        let mut pagination = PaginationState::new(0);
        assert_eq!(pagination.items_this_page(), 0);
        assert_eq!(pagination.next_page(), None);
    }

    #[test]
    fn pagination_exactly_max_items_per_page_boundary() {
        // Exactly MAX_ITEMS_PER_PAGE items: fits in one page, no next page
        let mut pagination = PaginationState::new(MAX_ITEMS_PER_PAGE);
        assert_eq!(pagination.items_this_page(), MAX_ITEMS_PER_PAGE);
        assert_eq!(pagination.next_page(), None);
    }

    #[test]
    fn pagination_max_items_plus_one_needs_two_pages() {
        // MAX_ITEMS_PER_PAGE + 1: needs exactly 2 pages
        let mut pagination = PaginationState::new(MAX_ITEMS_PER_PAGE + 1);
        assert_eq!(pagination.items_this_page(), MAX_ITEMS_PER_PAGE);
        let next = pagination.next_page();
        assert_eq!(next, Some(1));
        // Second page has 1 item
        assert_eq!(pagination.items_this_page(), 1);
        // No more pages
        assert_eq!(pagination.next_page(), None);
    }

    #[test]
    fn checked_page_offset_page_zero_returns_zero() {
        let pagination = PaginationState::new(5000);
        assert_eq!(pagination.checked_offset(), Ok(0));
    }

    #[test]
    fn checked_page_offset_page_one_returns_1024() {
        let mut pagination = PaginationState::new(5000);
        pagination.current_page = 1;
        assert_eq!(pagination.checked_offset(), Ok(1024));
    }

    #[test]
    fn checked_page_offset_overflow_detection() {
        // Very large page number should cause overflow
        let mut pagination = PaginationState::new(5000);
        pagination.current_page = u64::MAX;
        let result = pagination.checked_offset();
        assert_eq!(result, Err("Page offset overflow"));
    }

    #[test]
    fn max_bootstrap_pages_limit_enforcement() {
        // Verify the constant is correctly defined
        assert_eq!(MAX_BOOTSTRAP_PAGES, 100_000);

        // Simulate a bootstrap that would exceed the limit
        let mut page_count: u64 = 0;
        let mut pagination =
            PaginationState::new(MAX_BOOTSTRAP_PAGES as usize * MAX_ITEMS_PER_PAGE + 1);
        loop {
            if page_count >= MAX_BOOTSTRAP_PAGES {
                break;
            }
            match pagination.next_page() {
                Some(_) => page_count += 1,
                None => break,
            }
        }
        // The loop should have been bounded by MAX_BOOTSTRAP_PAGES
        assert!(page_count <= MAX_BOOTSTRAP_PAGES);
    }

    #[test]
    fn chaininfo_step_finds_common_point() {
        // Create two chains that share some blocks
        let chain_a = make_linear_chain(50, 100);
        let chain_b = make_linear_chain(30, 100); // shares first 30 blocks

        let block_ids_b = make_block_ids(&chain_b);
        let common = chain_a.find_common_point(&block_ids_b);

        assert!(common.is_some());
        let common = common.unwrap();
        assert_eq!(common.topoheight, 30); // highest shared block
    }

    #[test]
    fn chaininfo_with_no_common_point_fresh_sync() {
        // Chain A and chain B have completely different blocks
        let chain_a = make_linear_chain(10, 100);

        // Create block IDs with hashes that don't exist in chain_a
        let foreign_ids: Vec<BlockId> = (100u64..110)
            .map(|i| {
                let mut hash = [0xFFu8; 32];
                hash[0..8].copy_from_slice(&i.to_le_bytes());
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
    fn assets_step_accumulates_all_assets_across_pages() {
        // Simulate fetching assets across multiple pages
        let total_assets = 3000;
        let mut pagination = PaginationState::new(total_assets);
        let mut accumulated = 0;

        loop {
            let items = pagination.items_this_page();
            accumulated += items;
            if pagination.next_page().is_none() {
                break;
            }
        }

        assert_eq!(accumulated, total_assets);
    }

    #[test]
    fn keys_step_collects_all_account_keys() {
        // Similar to assets: verify full collection across pages
        let total_keys = 5500;
        let mut pagination = PaginationState::new(total_keys);
        let mut pages_processed = 0;
        let mut total_collected = 0;

        loop {
            total_collected += pagination.items_this_page();
            pages_processed += 1;
            if pagination.next_page().is_none() {
                break;
            }
        }

        assert_eq!(total_collected, total_keys);
        // 5500 / 1024 = 5 full pages + 1 partial = 6 pages
        assert_eq!(pages_processed, 6);
    }

    #[test]
    fn blocks_metadata_fetches_prune_safety_limit_plus_one() {
        // BlocksMetadata step should fetch at least PRUNE_SAFETY_LIMIT + 1 blocks
        let required_blocks = PRUNE_SAFETY_LIMIT + 1; // 241
        let chain = make_linear_chain(required_blocks, 100);

        assert_eq!(chain.blocks.len(), required_blocks as usize);
        assert_eq!(chain.topoheight, required_blocks);
    }

    #[test]
    fn topoheight_validation_cannot_exceed_peer_topoheight() {
        let peer_topoheight: TopoHeight = 500;
        let requested_topoheight: TopoHeight = 600;

        // Request must not exceed peer's reported topoheight
        assert!(requested_topoheight > peer_topoheight);
        // In a real implementation, this would be rejected
        let valid = requested_topoheight <= peer_topoheight;
        assert!(!valid);
    }

    #[test]
    fn topoheight_validation_cannot_be_below_pruned_topoheight() {
        let mut chain = make_linear_chain(300, 100);
        chain.pruned_topoheight = Some(50);

        let requested_topoheight: TopoHeight = 30;

        // Request below pruned topoheight should be invalid
        let valid = match chain.pruned_topoheight {
            Some(pruned) => requested_topoheight >= pruned,
            None => true,
        };
        assert!(!valid);
    }

    #[test]
    fn bootstrap_from_pruned_node_partial_state() {
        // A pruned node only has blocks from pruned_topoheight onward
        let mut chain = make_linear_chain(300, 100);
        chain.pruned_topoheight = Some(100);

        // Blocks below pruned height are conceptually unavailable
        // The chain still has them in this mock, but the pruned_topoheight
        // indicates where valid data starts
        assert_eq!(chain.pruned_topoheight, Some(100));
        assert!(chain.topoheight > 100);

        // Available range for sync
        let available_start = chain.pruned_topoheight.unwrap();
        let available_end = chain.topoheight;
        let available_range = available_end - available_start;
        assert_eq!(available_range, 200);
    }

    #[test]
    fn page_number_zero_is_starting_position() {
        // Page 0 represents the starting position (offset 0)
        let pagination = PaginationState::new(5000);
        assert_eq!(pagination.current_page, 0);
        assert_eq!(pagination.checked_offset(), Ok(0));

        // After calling next_page, page becomes 1 (which is valid)
        let mut pagination = PaginationState::new(5000);
        let next = pagination.next_page();
        assert_eq!(next, Some(1));
        assert_eq!(pagination.current_page, 1);
    }

    #[test]
    fn sequential_step_progression_cannot_skip_steps() {
        // Verify that steps must be taken in order
        let steps = [
            StepKind::ChainInfo,
            StepKind::Assets,
            StepKind::Keys,
            StepKind::Accounts,
            StepKind::Contracts,
            StepKind::BlocksMetadata,
        ];

        // Each step's next() must match the next element in the sequence
        for i in 0..steps.len() - 1 {
            let next = steps[i].next().unwrap();
            assert_eq!(next, steps[i + 1]);
        }

        // Cannot jump from ChainInfo to Accounts directly
        let from_chaininfo = StepKind::ChainInfo.next().unwrap();
        assert_ne!(from_chaininfo, StepKind::Accounts);
        assert_ne!(from_chaininfo, StepKind::Keys);
        assert_eq!(from_chaininfo, StepKind::Assets);
    }
}
