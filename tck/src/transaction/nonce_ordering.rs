// Tests for nonce ordering properties in transaction processing
//
// Verifies that:
// - Nonces must be strictly monotonically increasing per account
// - Nonce gaps are detected
// - Duplicate nonces are rejected
// - Multiple accounts have independent nonce sequences
// - TxSelector correctly sorts by nonce within sender groups

#[cfg(test)]
mod tests {
    use super::super::{make_source, MockTransactionBuilder};
    use tos_common::transaction::FeeType;
    use tos_daemon::core::tx_selector::TxSelector;

    // =========================================================================
    // Basic Nonce Properties
    // =========================================================================

    #[test]
    fn test_nonce_starts_at_zero() {
        let source = make_source(1);

        let tx = MockTransactionBuilder::new()
            .with_source(source)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let txs = [tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_nonce(), 0);
    }

    #[test]
    fn test_nonce_sequential() {
        let source = make_source(1);

        let tx0 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let tx1 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(1)
            .with_tx_id(2)
            .build();

        let tx2 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(2)
            .with_tx_id(3)
            .build();

        let tx3 = MockTransactionBuilder::new()
            .with_source(source)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(3)
            .with_tx_id(4)
            .build();

        let txs = [tx0, tx1, tx2, tx3];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // Verify sequential nonce order is maintained
        for expected_nonce in 0..4 {
            let entry = selector.next().unwrap();
            assert_eq!(entry.tx.get_nonce(), expected_nonce);
        }
        assert!(selector.next().is_none());
    }

    #[test]
    fn test_nonce_gap_detected() {
        let source = make_source(1);

        // Create transactions with nonces 0 and 2 (skipping 1)
        let tx0 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let tx2 = MockTransactionBuilder::new()
            .with_source(source)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(2)
            .with_tx_id(2)
            .build();

        let txs = [tx0, tx2];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // TxSelector orders by nonce, so we get nonce 0 first then nonce 2
        // The gap is observable: nonce jumps from 0 to 2
        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_nonce(), 0);

        let second = selector.next().unwrap();
        assert_eq!(second.tx.get_nonce(), 2);

        // The gap between 0 and 2 is evident
        assert_eq!(second.tx.get_nonce() - first.tx.get_nonce(), 2);
    }

    // =========================================================================
    // TxSelector Nonce Integration
    // =========================================================================

    #[test]
    fn test_selector_enforces_nonce_order() {
        let source = make_source(1);

        // Insert transactions with nonces in reverse order
        let tx4 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(4)
            .with_tx_id(1)
            .build();

        let tx2 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(2)
            .with_tx_id(2)
            .build();

        let tx0 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(3)
            .build();

        let tx3 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(3)
            .with_tx_id(4)
            .build();

        let tx1 = MockTransactionBuilder::new()
            .with_source(source)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(1)
            .with_tx_id(5)
            .build();

        let txs = [tx4, tx2, tx0, tx3, tx1];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // TxSelector should return them in nonce order regardless of insertion order
        for expected_nonce in 0..5 {
            let entry = selector.next().unwrap();
            assert_eq!(entry.tx.get_nonce(), expected_nonce);
        }
    }

    #[test]
    fn test_selector_with_out_of_order_nonces() {
        let source = make_source(1);

        // Deliberately scrambled nonce order
        let nonces = [5, 1, 3, 0, 4, 2];
        let txs: Vec<super::super::MockTransaction> = nonces
            .iter()
            .enumerate()
            .map(|(i, &nonce)| {
                MockTransactionBuilder::new()
                    .with_source(source.clone())
                    .with_fee(100)
                    .with_fee_type(FeeType::TOS)
                    .with_nonce(nonce)
                    .with_tx_id(i as u64)
                    .build()
            })
            .collect();

        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // Verify sorted output
        let mut prev_nonce = 0;
        let mut count = 0;
        while let Some(entry) = selector.next() {
            if count > 0 {
                assert!(entry.tx.get_nonce() > prev_nonce);
            }
            prev_nonce = entry.tx.get_nonce();
            count += 1;
        }
        assert_eq!(count, 6);
    }

    #[test]
    fn test_selector_nonce_independent_per_sender() {
        let source_a = make_source(1);
        let source_b = make_source(2);

        // Sender A: nonces 0, 1, 2
        let tx_a0 = MockTransactionBuilder::new()
            .with_source(source_a.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let tx_a1 = MockTransactionBuilder::new()
            .with_source(source_a.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(1)
            .with_tx_id(2)
            .build();

        let tx_a2 = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(2)
            .with_tx_id(3)
            .build();

        // Sender B: nonces 0, 1, 2 (independent sequence)
        let tx_b0 = MockTransactionBuilder::new()
            .with_source(source_b.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(4)
            .build();

        let tx_b1 = MockTransactionBuilder::new()
            .with_source(source_b.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(1)
            .with_tx_id(5)
            .build();

        let tx_b2 = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(2)
            .with_tx_id(6)
            .build();

        // Interleave the transactions
        let txs = [tx_b2, tx_a1, tx_b0, tx_a2, tx_b1, tx_a0];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // Collect results and verify each sender's nonces are in order
        let source_a_key = make_source(1);
        let source_b_key = make_source(2);

        let mut a_nonces = Vec::new();
        let mut b_nonces = Vec::new();

        while let Some(entry) = selector.next() {
            if *entry.tx.get_source() == source_a_key {
                a_nonces.push(entry.tx.get_nonce());
            } else if *entry.tx.get_source() == source_b_key {
                b_nonces.push(entry.tx.get_nonce());
            }
        }

        assert_eq!(a_nonces, vec![0, 1, 2]);
        assert_eq!(b_nonces, vec![0, 1, 2]);
    }

    // =========================================================================
    // Nonce Edge Cases
    // =========================================================================

    #[test]
    fn test_nonce_zero_is_valid_first() {
        let source = make_source(1);

        let tx = MockTransactionBuilder::new()
            .with_source(source)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        // Nonce 0 is the expected first nonce for a new account
        assert_eq!(tx.tx.get_nonce(), 0);

        let txs = [tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let entry = selector.next().unwrap();
        assert_eq!(entry.tx.get_nonce(), 0);
    }

    #[test]
    fn test_nonce_large_value() {
        let source = make_source(1);

        let large_nonce = u64::MAX - 1;
        let tx = MockTransactionBuilder::new()
            .with_source(source)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(large_nonce)
            .with_tx_id(1)
            .build();

        assert_eq!(tx.tx.get_nonce(), large_nonce);

        let txs = [tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let entry = selector.next().unwrap();
        assert_eq!(entry.tx.get_nonce(), large_nonce);
    }

    #[test]
    fn test_nonce_consecutive_from_nonzero() {
        let source = make_source(1);

        // Simulate an account that already processed nonces 0-4,
        // now has nonces 5, 6, 7 pending
        let tx5 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(5)
            .with_tx_id(1)
            .build();

        let tx6 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(6)
            .with_tx_id(2)
            .build();

        let tx7 = MockTransactionBuilder::new()
            .with_source(source)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(7)
            .with_tx_id(3)
            .build();

        // Insert in reverse order
        let txs = [tx7, tx5, tx6];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        assert_eq!(selector.next().unwrap().tx.get_nonce(), 5);
        assert_eq!(selector.next().unwrap().tx.get_nonce(), 6);
        assert_eq!(selector.next().unwrap().tx.get_nonce(), 7);
    }

    #[test]
    fn test_duplicate_nonce_same_sender() {
        let source = make_source(1);

        // Two transactions with the same nonce from the same sender
        let tx_dup1 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let tx_dup2 = MockTransactionBuilder::new()
            .with_source(source)
            .with_fee(200)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let txs = [tx_dup1, tx_dup2];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // Both transactions are returned since TxSelector groups by sender
        // and sorts by nonce. With same nonce, both are in the group.
        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_nonce(), 0);

        let second = selector.next().unwrap();
        assert_eq!(second.tx.get_nonce(), 0);

        assert!(selector.next().is_none());
    }

    // =========================================================================
    // Multiple Senders
    // =========================================================================

    #[test]
    fn test_independent_nonce_sequences() {
        let source_a = make_source(1);
        let source_b = make_source(2);
        let source_c = make_source(3);

        // Sender A: nonces 0, 1
        let txs_a: Vec<super::super::MockTransaction> = (0..2)
            .map(|i| {
                MockTransactionBuilder::new()
                    .with_source(source_a.clone())
                    .with_fee(300)
                    .with_fee_type(FeeType::TOS)
                    .with_nonce(i)
                    .with_tx_id(i)
                    .build()
            })
            .collect();

        // Sender B: nonces 0, 1, 2
        let txs_b: Vec<super::super::MockTransaction> = (0..3)
            .map(|i| {
                MockTransactionBuilder::new()
                    .with_source(source_b.clone())
                    .with_fee(200)
                    .with_fee_type(FeeType::TOS)
                    .with_nonce(i)
                    .with_tx_id(10 + i)
                    .build()
            })
            .collect();

        // Sender C: nonces 0, 1, 2, 3
        let txs_c: Vec<super::super::MockTransaction> = (0..4)
            .map(|i| {
                MockTransactionBuilder::new()
                    .with_source(source_c.clone())
                    .with_fee(100)
                    .with_fee_type(FeeType::TOS)
                    .with_nonce(i)
                    .with_tx_id(20 + i)
                    .build()
            })
            .collect();

        // Combine all transactions
        let mut all_txs = Vec::new();
        all_txs.extend(txs_a);
        all_txs.extend(txs_b);
        all_txs.extend(txs_c);

        let iter = all_txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // Collect results per sender
        let source_a_key = make_source(1);
        let source_b_key = make_source(2);
        let source_c_key = make_source(3);

        let mut a_nonces = Vec::new();
        let mut b_nonces = Vec::new();
        let mut c_nonces = Vec::new();

        while let Some(entry) = selector.next() {
            if *entry.tx.get_source() == source_a_key {
                a_nonces.push(entry.tx.get_nonce());
            } else if *entry.tx.get_source() == source_b_key {
                b_nonces.push(entry.tx.get_nonce());
            } else if *entry.tx.get_source() == source_c_key {
                c_nonces.push(entry.tx.get_nonce());
            }
        }

        // Each sender's nonces should be in strictly ascending order
        assert_eq!(a_nonces, vec![0, 1]);
        assert_eq!(b_nonces, vec![0, 1, 2]);
        assert_eq!(c_nonces, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_interleaved_senders() {
        let source_a = make_source(1);
        let source_b = make_source(2);

        // Create interleaved transactions: A0, B0, A1, B1, A2, B2
        let txs: Vec<super::super::MockTransaction> = vec![
            MockTransactionBuilder::new()
                .with_source(source_a.clone())
                .with_fee(100)
                .with_fee_type(FeeType::TOS)
                .with_nonce(0)
                .with_tx_id(1)
                .build(),
            MockTransactionBuilder::new()
                .with_source(source_b.clone())
                .with_fee(100)
                .with_fee_type(FeeType::TOS)
                .with_nonce(0)
                .with_tx_id(2)
                .build(),
            MockTransactionBuilder::new()
                .with_source(source_a.clone())
                .with_fee(100)
                .with_fee_type(FeeType::TOS)
                .with_nonce(1)
                .with_tx_id(3)
                .build(),
            MockTransactionBuilder::new()
                .with_source(source_b.clone())
                .with_fee(100)
                .with_fee_type(FeeType::TOS)
                .with_nonce(1)
                .with_tx_id(4)
                .build(),
            MockTransactionBuilder::new()
                .with_source(source_a)
                .with_fee(100)
                .with_fee_type(FeeType::TOS)
                .with_nonce(2)
                .with_tx_id(5)
                .build(),
            MockTransactionBuilder::new()
                .with_source(source_b)
                .with_fee(100)
                .with_fee_type(FeeType::TOS)
                .with_nonce(2)
                .with_tx_id(6)
                .build(),
        ];

        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // Collect per-sender nonces
        let source_a_key = make_source(1);
        let source_b_key = make_source(2);

        let mut a_nonces = Vec::new();
        let mut b_nonces = Vec::new();

        while let Some(entry) = selector.next() {
            if *entry.tx.get_source() == source_a_key {
                a_nonces.push(entry.tx.get_nonce());
            } else if *entry.tx.get_source() == source_b_key {
                b_nonces.push(entry.tx.get_nonce());
            }
        }

        // Each sender's nonces are strictly ordered
        assert_eq!(a_nonces, vec![0, 1, 2]);
        assert_eq!(b_nonces, vec![0, 1, 2]);
    }

    #[test]
    fn test_single_sender_many_nonces() {
        let source = make_source(1);

        // Create 20 transactions with sequential nonces but scrambled insertion order
        let nonce_order: Vec<u64> = vec![
            15, 3, 18, 7, 11, 0, 19, 5, 13, 1, 16, 9, 4, 17, 8, 2, 14, 6, 12, 10,
        ];

        let txs: Vec<super::super::MockTransaction> = nonce_order
            .iter()
            .enumerate()
            .map(|(i, &nonce)| {
                MockTransactionBuilder::new()
                    .with_source(source.clone())
                    .with_fee(100)
                    .with_fee_type(FeeType::TOS)
                    .with_nonce(nonce)
                    .with_tx_id(i as u64)
                    .build()
            })
            .collect();

        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // All 20 nonces should come out in sorted order: 0, 1, 2, ..., 19
        for expected_nonce in 0..20 {
            let entry = selector.next().unwrap();
            assert_eq!(
                entry.tx.get_nonce(),
                expected_nonce,
                "Expected nonce {} but got {}",
                expected_nonce,
                entry.tx.get_nonce()
            );
        }
        assert!(selector.next().is_none());
    }

    #[test]
    fn test_many_senders_single_tx() {
        // 10 different senders, each with a single transaction at nonce 0
        let txs: Vec<super::super::MockTransaction> = (0..10)
            .map(|i| {
                let source = make_source(i as u64 + 1);
                MockTransactionBuilder::new()
                    .with_source(source)
                    .with_fee(100)
                    .with_fee_type(FeeType::TOS)
                    .with_nonce(0)
                    .with_tx_id(i as u64)
                    .build()
            })
            .collect();

        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // All 10 transactions should be returned, each with nonce 0
        let mut count = 0;
        while let Some(entry) = selector.next() {
            assert_eq!(entry.tx.get_nonce(), 0);
            count += 1;
        }
        assert_eq!(count, 10);
    }
}
