#[cfg(test)]
mod tests {
    use super::super::mock::*;

    #[test]
    fn max_items_per_page_is_1024() {
        assert_eq!(MAX_ITEMS_PER_PAGE, 1024);
    }

    #[test]
    fn page_offset_page_zero_is_zero() {
        let pagination = PaginationState::new(5000);
        assert_eq!(pagination.current_page, 0);
        assert_eq!(pagination.checked_offset(), Ok(0));
    }

    #[test]
    fn page_offset_page_one_is_1024() {
        let mut pagination = PaginationState::new(5000);
        pagination.current_page = 1;
        assert_eq!(pagination.checked_offset(), Ok(1024));
    }

    #[test]
    fn page_offset_page_99_is_101376() {
        let mut pagination = PaginationState::new(200_000);
        pagination.current_page = 99;
        let expected = 99 * MAX_ITEMS_PER_PAGE; // 99 * 1024 = 101376
        assert_eq!(expected, 101_376);
        assert_eq!(pagination.checked_offset(), Ok(expected));
    }

    #[test]
    fn page_offset_overflow_u64_max_times_1024() {
        let mut pagination = PaginationState::new(5000);
        pagination.current_page = u64::MAX;

        // u64::MAX as usize (on 64-bit) * 1024 will overflow usize
        let result = pagination.checked_offset();
        assert_eq!(result, Err("Page offset overflow"));
    }

    #[test]
    fn items_this_page_full_page() {
        let pagination = PaginationState::new(5000);
        // First page: 5000 items total, page holds 1024
        assert_eq!(pagination.items_this_page(), 1024);
    }

    #[test]
    fn items_this_page_partial_last_page() {
        let mut pagination = PaginationState::new(1500);
        // Advance to page 1
        pagination.current_page = 1;
        // Page 1 offset = 1024, remaining = 1500 - 1024 = 476
        assert_eq!(pagination.items_this_page(), 476);
    }

    #[test]
    fn items_this_page_beyond_total_returns_zero() {
        let mut pagination = PaginationState::new(500);
        // Page 1 offset = 1024, which is beyond 500 total items
        pagination.current_page = 1;
        assert_eq!(pagination.items_this_page(), 0);
    }

    #[test]
    fn max_bootstrap_pages_is_100000() {
        assert_eq!(MAX_BOOTSTRAP_PAGES, 100_000);

        // This prevents infinite sync loops
        let mut pages_fetched: u64 = 0;
        let limit = MAX_BOOTSTRAP_PAGES;

        // Simulate a malicious peer sending endless pages
        for _ in 0..limit + 100 {
            if pages_fetched >= limit {
                break; // enforce limit
            }
            pages_fetched += 1;
        }

        assert_eq!(pages_fetched, MAX_BOOTSTRAP_PAGES);
    }

    #[test]
    fn chain_sync_request_max_blocks_is_64() {
        assert_eq!(CHAIN_SYNC_REQUEST_MAX_BLOCKS, 64);

        // A request with more than 64 block IDs is invalid
        let request_ids: Vec<BlockId> = (0..100)
            .map(|i| BlockId {
                hash: [i as u8; 32],
                topoheight: i as u64,
            })
            .collect();

        let is_valid = request_ids.len() <= CHAIN_SYNC_REQUEST_MAX_BLOCKS;
        assert!(!is_valid);

        let trimmed: Vec<BlockId> = request_ids
            .into_iter()
            .take(CHAIN_SYNC_REQUEST_MAX_BLOCKS)
            .collect();
        assert_eq!(trimmed.len(), 64);
    }

    #[test]
    fn chain_sync_response_max_blocks_is_u16_max() {
        assert_eq!(CHAIN_SYNC_RESPONSE_MAX_BLOCKS, 65535);
        assert_eq!(CHAIN_SYNC_RESPONSE_MAX_BLOCKS, u16::MAX as usize);
    }

    #[test]
    fn response_size_default_min_max_values() {
        assert_eq!(CHAIN_SYNC_RESPONSE_MIN_BLOCKS, 512);
        assert_eq!(CHAIN_SYNC_DEFAULT_RESPONSE_BLOCKS, 4096);
        assert_eq!(CHAIN_SYNC_RESPONSE_MAX_BLOCKS, 65535);
    }

    #[test]
    fn invariant_max_gte_default_gte_min() {
        let max = CHAIN_SYNC_RESPONSE_MAX_BLOCKS;
        let default = CHAIN_SYNC_DEFAULT_RESPONSE_BLOCKS;
        let min = CHAIN_SYNC_RESPONSE_MIN_BLOCKS;

        assert!(max >= default);
        assert!(default >= min);

        // Verify the ordering holds
        assert!(min < default);
        assert!(default < max);
    }

    #[test]
    fn max_accumulator_entries_is_10_million() {
        assert_eq!(MAX_ACCUMULATOR_ENTRIES, 10_000_000);

        // This limits memory usage during accumulation
        let entry_size_bytes: usize = 64; // typical hash + metadata
        let max_memory = MAX_ACCUMULATOR_ENTRIES
            .checked_mul(entry_size_bytes)
            .unwrap();
        // 10M * 64 = 640MB theoretical max
        assert_eq!(max_memory, 640_000_000);
    }
}
