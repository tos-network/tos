// Block assembly capacity limit tests
//
// Verifies that block assembly respects size and count limits defined by:
// - MAX_BLOCK_SIZE (1.25 MB = 1310720 bytes)
// - MAX_TXS_PER_BLOCK (10000)
// - MAX_TRANSACTION_SIZE (1 MB = 1048576 bytes)
//
// Since actual block template generation is complex (in blockchain.rs),
// we test the constants and the conceptual invariants that must hold.

#[cfg(test)]
mod tests {
    use super::super::{make_source, MockTransactionBuilder};
    use tos_common::config::{
        BYTES_PER_KB, MAX_BLOCK_SIZE, MAX_TRANSACTION_SIZE, MAX_TXS_PER_BLOCK,
    };
    use tos_common::transaction::FeeType;
    use tos_daemon::core::tx_selector::TxSelector;

    // =========================================================================
    // Constant Verification Tests
    // =========================================================================

    #[test]
    fn test_max_block_size_value() {
        assert_eq!(MAX_BLOCK_SIZE, 1_310_720);
    }

    #[test]
    fn test_max_txs_per_block_value() {
        assert_eq!(MAX_TXS_PER_BLOCK, 10_000);
    }

    #[test]
    fn test_max_transaction_size_value() {
        assert_eq!(MAX_TRANSACTION_SIZE, 1_048_576);
    }

    #[test]
    fn test_block_larger_than_single_tx() {
        let block_size = MAX_BLOCK_SIZE;
        let tx_size = MAX_TRANSACTION_SIZE;
        assert!(
            block_size > tx_size,
            "Block size ({}) must be larger than max transaction size ({})",
            block_size,
            tx_size
        );
    }

    #[test]
    fn test_block_size_is_1_25_mb() {
        let expected = 1024 * 1024 + 256 * 1024;
        assert_eq!(
            MAX_BLOCK_SIZE, expected,
            "MAX_BLOCK_SIZE should be exactly 1.25 MB (1MB + 256KB)"
        );
    }

    // =========================================================================
    // Block Filling Simulation Tests
    // =========================================================================

    #[test]
    fn test_block_fill_by_count() {
        // Create exactly MAX_TXS_PER_BLOCK transactions with small size
        let tx_size = 100usize;
        let max_count = MAX_TXS_PER_BLOCK as usize;

        let mut total_size = 0usize;
        let mut count = 0usize;

        // Simulate block filling with small transactions
        let txs: Vec<_> = (0..max_count + 100)
            .map(|i| {
                MockTransactionBuilder::new()
                    .with_tx_id(i as u64)
                    .with_source(make_source(i as u64))
                    .with_nonce(0)
                    .with_fee(1000)
                    .build()
            })
            .collect();

        let iter = txs.iter().map(|m| (tx_size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        while let Some(entry) = selector.next() {
            if total_size.checked_add(entry.size).unwrap() > MAX_BLOCK_SIZE {
                break;
            }
            if count >= MAX_TXS_PER_BLOCK as usize {
                break;
            }
            total_size = total_size.checked_add(entry.size).unwrap();
            count = count.checked_add(1).unwrap();
        }

        assert_eq!(
            count, max_count,
            "Should have filled block to exactly MAX_TXS_PER_BLOCK"
        );
    }

    #[test]
    fn test_block_fill_by_size() {
        // Use large transactions that will hit the size limit before count limit
        let tx_size = MAX_BLOCK_SIZE / 5; // Each tx is 20% of block
        let mut total_size = 0usize;
        let mut count = 0usize;

        let txs: Vec<_> = (0..100)
            .map(|i| {
                MockTransactionBuilder::new()
                    .with_tx_id(i as u64)
                    .with_source(make_source(i as u64))
                    .with_nonce(0)
                    .with_fee(1000)
                    .build()
            })
            .collect();

        let iter = txs.iter().map(|m| (tx_size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        while let Some(entry) = selector.next() {
            if total_size.checked_add(entry.size).unwrap() > MAX_BLOCK_SIZE {
                break;
            }
            if count >= MAX_TXS_PER_BLOCK as usize {
                break;
            }
            total_size = total_size.checked_add(entry.size).unwrap();
            count = count.checked_add(1).unwrap();
        }

        // With tx_size = MAX_BLOCK_SIZE/5, we should fit exactly 5
        assert_eq!(count, 5, "Should fit exactly 5 transactions at 20% each");
        assert!(
            total_size <= MAX_BLOCK_SIZE,
            "Total size ({}) must not exceed MAX_BLOCK_SIZE ({})",
            total_size,
            MAX_BLOCK_SIZE
        );
    }

    #[test]
    fn test_block_count_limit_before_size() {
        // Very small transactions: count limit will be reached before size limit
        let tx_size = 10usize; // Very small
        let max_txs = MAX_TXS_PER_BLOCK as usize;

        // Total size if we filled to count limit: 10 * 10000 = 100,000 bytes
        // This is far less than MAX_BLOCK_SIZE (1,310,720 bytes)
        let total_if_count_full = tx_size.checked_mul(max_txs).unwrap();
        assert!(
            total_if_count_full < MAX_BLOCK_SIZE,
            "With tx_size={}, filling to count limit ({}) gives {} bytes, \
             which should be less than MAX_BLOCK_SIZE ({})",
            tx_size,
            max_txs,
            total_if_count_full,
            MAX_BLOCK_SIZE
        );

        let mut total_size = 0usize;
        let mut count = 0usize;

        // Simulate filling
        while count < max_txs {
            let next_size = total_size.checked_add(tx_size).unwrap();
            if next_size > MAX_BLOCK_SIZE {
                break;
            }
            total_size = next_size;
            count = count.checked_add(1).unwrap();
        }

        assert_eq!(
            count, max_txs,
            "Count limit should be reached before size limit"
        );
        assert!(total_size < MAX_BLOCK_SIZE);
    }

    #[test]
    fn test_block_size_limit_before_count() {
        // Large transactions: size limit will be reached before count limit
        let tx_size = MAX_BLOCK_SIZE / 3; // Each tx is ~33% of block

        let mut total_size = 0usize;
        let mut count = 0usize;

        while count < MAX_TXS_PER_BLOCK as usize {
            let next_size = total_size.checked_add(tx_size).unwrap();
            if next_size > MAX_BLOCK_SIZE {
                break;
            }
            total_size = next_size;
            count = count.checked_add(1).unwrap();
        }

        assert!(
            count < MAX_TXS_PER_BLOCK as usize,
            "Size limit should be reached before count limit ({} txs fit)",
            count
        );
        assert_eq!(count, 3, "Should fit exactly 3 transactions at ~33% each");
    }

    #[test]
    fn test_single_max_size_tx_fits() {
        // A single transaction at MAX_TRANSACTION_SIZE should fit in a block
        let tx_size = MAX_TRANSACTION_SIZE;
        let mut total_size = 0usize;
        let mut count = 0usize;

        if total_size.checked_add(tx_size).unwrap() <= MAX_BLOCK_SIZE
            && count < MAX_TXS_PER_BLOCK as usize
        {
            total_size = total_size.checked_add(tx_size).unwrap();
            count = count.checked_add(1).unwrap();
        }

        assert_eq!(count, 1, "A max-size transaction should fit in the block");
        assert!(
            total_size <= MAX_BLOCK_SIZE,
            "A max-size tx ({}) should fit within block size ({})",
            total_size,
            MAX_BLOCK_SIZE
        );
    }

    // =========================================================================
    // TxSelector Exhaustion Tests
    // =========================================================================

    #[test]
    fn test_selector_provides_ordered_txs_for_block() {
        // Create transactions with different fees; selector should provide highest first
        let high_fee_tx = MockTransactionBuilder::new()
            .with_tx_id(1)
            .with_source(make_source(1))
            .with_fee(5000)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .build();

        let low_fee_tx = MockTransactionBuilder::new()
            .with_tx_id(2)
            .with_source(make_source(2))
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .build();

        let mid_fee_tx = MockTransactionBuilder::new()
            .with_tx_id(3)
            .with_source(make_source(3))
            .with_fee(2500)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .build();

        let txs = [&low_fee_tx, &mid_fee_tx, &high_fee_tx];
        let iter = txs.iter().map(|m| (256usize, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // First tx should have the highest fee
        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_fee(), 5000);

        // Second should have mid fee
        let second = selector.next().unwrap();
        assert_eq!(second.tx.get_fee(), 2500);

        // Third should have lowest fee
        let third = selector.next().unwrap();
        assert_eq!(third.tx.get_fee(), 100);

        // No more transactions
        assert!(selector.next().is_none());
    }

    #[test]
    fn test_selector_skips_oversized_tx() {
        // Conceptual: when a tx exceeds remaining block space, it should be skipped
        let small_tx = MockTransactionBuilder::new()
            .with_tx_id(1)
            .with_source(make_source(1))
            .with_fee(1000)
            .with_nonce(0)
            .build();

        let large_tx = MockTransactionBuilder::new()
            .with_tx_id(2)
            .with_source(make_source(2))
            .with_fee(2000) // Higher fee but too large
            .with_nonce(0)
            .build();

        let another_small = MockTransactionBuilder::new()
            .with_tx_id(3)
            .with_source(make_source(3))
            .with_fee(500)
            .with_nonce(0)
            .build();

        // Assign sizes: large_tx gets a size exceeding the block limit
        let small_size = 100usize;
        let large_size = MAX_BLOCK_SIZE + 1; // Exceeds MAX_BLOCK_SIZE, cannot fit

        // Simulate block assembly with size check
        let mut total_size = 0usize;
        let mut count = 0usize;
        let mut included_fees = Vec::new();

        // Manual iteration simulating the skip logic
        let entries = vec![
            (large_tx.tx.get_fee(), large_size),
            (small_tx.tx.get_fee(), small_size),
            (another_small.tx.get_fee(), small_size),
        ];

        for (fee, size) in &entries {
            if total_size.checked_add(*size).unwrap() > MAX_BLOCK_SIZE {
                // Skip this tx, it doesn't fit
                continue;
            }
            if count >= MAX_TXS_PER_BLOCK as usize {
                break;
            }
            total_size = total_size.checked_add(*size).unwrap();
            count = count.checked_add(1).unwrap();
            included_fees.push(*fee);
        }

        // The large tx should be skipped, but small txs should be included
        assert!(
            !included_fees.contains(&2000),
            "Oversized tx should be skipped"
        );
        assert!(included_fees.contains(&1000), "Small tx should be included");
        assert!(
            included_fees.contains(&500),
            "Another small tx should be included"
        );
    }

    #[test]
    fn test_empty_block_has_zero_txs() {
        // An empty selector produces no transactions
        let txs: Vec<super::super::MockTransaction> = vec![];
        let iter = txs.iter().map(|m| (256usize, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let mut count = 0usize;
        let mut total_size = 0usize;

        while let Some(entry) = selector.next() {
            if total_size.checked_add(entry.size).unwrap() > MAX_BLOCK_SIZE {
                break;
            }
            if count >= MAX_TXS_PER_BLOCK as usize {
                break;
            }
            total_size = total_size.checked_add(entry.size).unwrap();
            count = count.checked_add(1).unwrap();
        }

        assert_eq!(count, 0, "Empty selector should produce zero transactions");
        assert_eq!(total_size, 0, "Empty block should have zero size");
    }

    #[test]
    fn test_block_fee_priority_ordering() {
        // Higher fee transactions should be included first in block assembly
        let fees = [100u64, 5000, 250, 10000, 1];

        let txs: Vec<_> = fees
            .iter()
            .enumerate()
            .map(|(i, &fee)| {
                MockTransactionBuilder::new()
                    .with_tx_id(i as u64)
                    .with_source(make_source(i as u64))
                    .with_fee(fee)
                    .with_fee_type(FeeType::TOS)
                    .with_nonce(0)
                    .build()
            })
            .collect();

        let iter = txs.iter().map(|m| (256usize, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let mut collected_fees = Vec::new();
        while let Some(entry) = selector.next() {
            collected_fees.push(entry.tx.get_fee());
        }

        // Verify the fees are in descending order (highest priority first)
        for i in 1..collected_fees.len() {
            assert!(
                collected_fees[i - 1] >= collected_fees[i],
                "Fees should be in descending order: {} should be >= {}",
                collected_fees[i - 1],
                collected_fees[i]
            );
        }

        assert_eq!(
            collected_fees.len(),
            5,
            "All 5 transactions should be yielded"
        );
        assert_eq!(collected_fees[0], 10000, "Highest fee first");
        assert_eq!(
            collected_fees[collected_fees.len() - 1],
            1,
            "Lowest fee last"
        );
    }

    // =========================================================================
    // Capacity Invariant Tests
    // =========================================================================

    #[test]
    fn test_block_size_never_negative() {
        // Block remaining capacity must always be >= 0
        // Since we use usize (unsigned), this is guaranteed by the type system,
        // but we verify the logic never underflows
        let tx_sizes = [100usize, 200, 500, 1000, MAX_BLOCK_SIZE / 2];
        let mut total_size = 0usize;

        for &size in &tx_sizes {
            let remaining = MAX_BLOCK_SIZE.checked_sub(total_size).unwrap();
            assert!(
                remaining <= MAX_BLOCK_SIZE,
                "Remaining capacity should never exceed MAX_BLOCK_SIZE"
            );

            if size > remaining {
                break;
            }
            total_size = total_size.checked_add(size).unwrap();
        }

        assert!(
            total_size <= MAX_BLOCK_SIZE,
            "Total size must never exceed MAX_BLOCK_SIZE"
        );
    }

    #[test]
    fn test_total_tx_sizes_bounded() {
        // Sum of all included transaction sizes must be <= MAX_BLOCK_SIZE
        let tx_count = 50usize;
        let tx_size = MAX_BLOCK_SIZE / 100; // Each tx is 1% of block

        let txs: Vec<_> = (0..tx_count)
            .map(|i| {
                MockTransactionBuilder::new()
                    .with_tx_id(i as u64)
                    .with_source(make_source(i as u64))
                    .with_nonce(0)
                    .with_fee(1000)
                    .build()
            })
            .collect();

        let iter = txs.iter().map(|m| (tx_size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let mut total_size = 0usize;
        let mut count = 0usize;

        while let Some(entry) = selector.next() {
            if total_size.checked_add(entry.size).unwrap() > MAX_BLOCK_SIZE {
                break;
            }
            if count >= MAX_TXS_PER_BLOCK as usize {
                break;
            }
            total_size = total_size.checked_add(entry.size).unwrap();
            count = count.checked_add(1).unwrap();
        }

        assert!(
            total_size <= MAX_BLOCK_SIZE,
            "Total tx sizes ({}) must be bounded by MAX_BLOCK_SIZE ({})",
            total_size,
            MAX_BLOCK_SIZE
        );
    }

    #[test]
    fn test_tx_count_bounded() {
        // Number of transactions in block must be <= MAX_TXS_PER_BLOCK
        let tx_size = 10usize; // Very small so count is the limiting factor
        let num_txs = MAX_TXS_PER_BLOCK as usize + 500; // More than limit

        let txs: Vec<_> = (0..num_txs)
            .map(|i| {
                MockTransactionBuilder::new()
                    .with_tx_id(i as u64)
                    .with_source(make_source(i as u64))
                    .with_nonce(0)
                    .with_fee(1000)
                    .build()
            })
            .collect();

        let iter = txs.iter().map(|m| (tx_size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let mut total_size = 0usize;
        let mut count = 0usize;

        while let Some(entry) = selector.next() {
            if total_size.checked_add(entry.size).unwrap() > MAX_BLOCK_SIZE {
                break;
            }
            if count >= MAX_TXS_PER_BLOCK as usize {
                break;
            }
            total_size = total_size.checked_add(entry.size).unwrap();
            count = count.checked_add(1).unwrap();
        }

        assert!(
            count <= MAX_TXS_PER_BLOCK as usize,
            "Transaction count ({}) must be bounded by MAX_TXS_PER_BLOCK ({})",
            count,
            MAX_TXS_PER_BLOCK
        );
        assert_eq!(
            count, MAX_TXS_PER_BLOCK as usize,
            "Should fill exactly to MAX_TXS_PER_BLOCK"
        );
    }

    // =========================================================================
    // Additional invariant: BYTES_PER_KB consistency
    // =========================================================================

    #[test]
    fn test_bytes_per_kb_consistency() {
        assert_eq!(BYTES_PER_KB, 1024, "BYTES_PER_KB must be 1024");
        assert_eq!(
            MAX_TRANSACTION_SIZE,
            BYTES_PER_KB * BYTES_PER_KB,
            "MAX_TRANSACTION_SIZE must be BYTES_PER_KB^2 (1 MB)"
        );
        assert_eq!(
            MAX_BLOCK_SIZE,
            BYTES_PER_KB * BYTES_PER_KB + 256 * BYTES_PER_KB,
            "MAX_BLOCK_SIZE must be 1MB + 256KB"
        );
    }
}
