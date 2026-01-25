// Tests for TxSelector fee-priority ordering
//
// Verifies that TxSelector correctly orders transactions by:
// - Fee type priority: TOS > Energy > UNO
// - Within TOS: higher fee amount wins
// - Within Energy: higher energy cost wins
// - Within UNO: higher transfer count wins
// - Same sender transactions are grouped and ordered by nonce

#[cfg(test)]
mod tests {
    use super::super::{make_source, MockTransactionBuilder};
    use tos_common::transaction::FeeType;
    use tos_daemon::core::tx_selector::TxSelector;

    // =========================================================================
    // Fee Type Priority Tests
    // =========================================================================

    #[test]
    fn test_tos_over_energy() {
        let source_a = make_source(1);
        let source_b = make_source(2);

        let tos_tx = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let energy_tx = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(500)
            .with_fee_type(FeeType::Energy)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let txs = [energy_tx, tos_tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_fee_type(), &FeeType::TOS);

        let second = selector.next().unwrap();
        assert_eq!(second.tx.get_fee_type(), &FeeType::Energy);
    }

    #[test]
    fn test_tos_over_uno() {
        let source_a = make_source(1);
        let source_b = make_source(2);

        let tos_tx = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let uno_tx = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(0)
            .with_fee_type(FeeType::UNO)
            .with_transfers(5)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let txs = [uno_tx, tos_tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_fee_type(), &FeeType::TOS);

        let second = selector.next().unwrap();
        assert_eq!(second.tx.get_fee_type(), &FeeType::UNO);
    }

    #[test]
    fn test_energy_over_uno() {
        let source_a = make_source(1);
        let source_b = make_source(2);

        let energy_tx = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(100)
            .with_fee_type(FeeType::Energy)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let uno_tx = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(0)
            .with_fee_type(FeeType::UNO)
            .with_transfers(10)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let txs = [uno_tx, energy_tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_fee_type(), &FeeType::Energy);

        let second = selector.next().unwrap();
        assert_eq!(second.tx.get_fee_type(), &FeeType::UNO);
    }

    #[test]
    fn test_all_three_types_ordering() {
        let source_a = make_source(1);
        let source_b = make_source(2);
        let source_c = make_source(3);

        let tos_tx = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let energy_tx = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(200)
            .with_fee_type(FeeType::Energy)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let uno_tx = MockTransactionBuilder::new()
            .with_source(source_c)
            .with_fee(0)
            .with_fee_type(FeeType::UNO)
            .with_transfers(3)
            .with_nonce(0)
            .with_tx_id(3)
            .build();

        // Insert in reverse priority order
        let txs = [uno_tx, energy_tx, tos_tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_fee_type(), &FeeType::TOS);

        let second = selector.next().unwrap();
        assert_eq!(second.tx.get_fee_type(), &FeeType::Energy);

        let third = selector.next().unwrap();
        assert_eq!(third.tx.get_fee_type(), &FeeType::UNO);

        assert!(selector.next().is_none());
    }

    // =========================================================================
    // Same-Type Comparisons
    // =========================================================================

    #[test]
    fn test_tos_higher_fee_wins() {
        let source_a = make_source(1);
        let source_b = make_source(2);

        let low_fee_tx = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let high_fee_tx = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(1000)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let txs = [low_fee_tx, high_fee_tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_fee(), 1000);

        let second = selector.next().unwrap();
        assert_eq!(second.tx.get_fee(), 100);
    }

    #[test]
    fn test_tos_equal_fee() {
        let source_a = make_source(1);
        let source_b = make_source(2);

        let tx_a = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(500)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let tx_b = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(500)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let txs = [tx_a, tx_b];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // Both should be returned (order is deterministic but implementation-defined for equal priority)
        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_fee(), 500);

        let second = selector.next().unwrap();
        assert_eq!(second.tx.get_fee(), 500);

        assert!(selector.next().is_none());
    }

    #[test]
    fn test_energy_higher_cost_wins() {
        let source_a = make_source(1);
        let source_b = make_source(2);

        // Energy cost is derived from transaction size and transfer count
        // More transfers = higher energy cost
        let low_cost_tx = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(100)
            .with_fee_type(FeeType::Energy)
            .with_transfers(1)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let high_cost_tx = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(100)
            .with_fee_type(FeeType::Energy)
            .with_transfers(5)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let txs = [low_cost_tx, high_cost_tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let first = selector.next().unwrap();
        // The tx with more transfers (higher energy cost) should come first
        assert!(
            first.tx.calculate_energy_cost() >= selector.next().unwrap().tx.calculate_energy_cost()
        );
    }

    #[test]
    fn test_uno_more_transfers_wins() {
        let source_a = make_source(1);
        let source_b = make_source(2);

        let few_transfers_tx = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(0)
            .with_fee_type(FeeType::UNO)
            .with_transfers(2)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let many_transfers_tx = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(0)
            .with_fee_type(FeeType::UNO)
            .with_transfers(8)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let txs = [few_transfers_tx, many_transfers_tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_outputs_count(), 8);

        let second = selector.next().unwrap();
        assert_eq!(second.tx.get_outputs_count(), 2);
    }

    // =========================================================================
    // Sender Grouping
    // =========================================================================

    #[test]
    fn test_same_sender_nonce_order() {
        let source = make_source(1);

        let tx_nonce_2 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(2)
            .with_tx_id(1)
            .build();

        let tx_nonce_0 = MockTransactionBuilder::new()
            .with_source(source.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let tx_nonce_1 = MockTransactionBuilder::new()
            .with_source(source)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(1)
            .with_tx_id(3)
            .build();

        // Insert out of nonce order
        let txs = [tx_nonce_2, tx_nonce_0, tx_nonce_1];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        assert_eq!(selector.next().unwrap().tx.get_nonce(), 0);
        assert_eq!(selector.next().unwrap().tx.get_nonce(), 1);
        assert_eq!(selector.next().unwrap().tx.get_nonce(), 2);
        assert!(selector.next().is_none());
    }

    #[test]
    fn test_different_senders_independent() {
        let source_a = make_source(1);
        let source_b = make_source(2);

        // Sender A: nonces 2, 0, 1
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

        // Sender B: nonces 1, 0
        let tx_b0 = MockTransactionBuilder::new()
            .with_source(source_b.clone())
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(4)
            .build();

        let tx_b1 = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(1)
            .with_tx_id(5)
            .build();

        let txs = [tx_a2, tx_b1, tx_a0, tx_b0, tx_a1];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // Collect all results and verify each sender's nonces are in order
        let mut results = Vec::new();
        while let Some(entry) = selector.next() {
            results.push((entry.tx.get_source().clone(), entry.tx.get_nonce()));
        }

        // Extract nonces per sender and verify ordering
        let source_a_key = make_source(1);
        let source_b_key = make_source(2);

        let a_nonces: Vec<u64> = results
            .iter()
            .filter(|(src, _)| *src == source_a_key)
            .map(|(_, n)| *n)
            .collect();
        let b_nonces: Vec<u64> = results
            .iter()
            .filter(|(src, _)| *src == source_b_key)
            .map(|(_, n)| *n)
            .collect();

        assert_eq!(a_nonces, vec![0, 1, 2]);
        assert_eq!(b_nonces, vec![0, 1]);
    }

    #[test]
    fn test_mixed_senders_priority() {
        let source_a = make_source(1);
        let source_b = make_source(2);
        let source_c = make_source(3);

        // Sender A: TOS fee type, high fee
        let tx_a = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(1000)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        // Sender B: Energy fee type
        let tx_b = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(500)
            .with_fee_type(FeeType::Energy)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        // Sender C: UNO fee type
        let tx_c = MockTransactionBuilder::new()
            .with_source(source_c)
            .with_fee(0)
            .with_fee_type(FeeType::UNO)
            .with_transfers(3)
            .with_nonce(0)
            .with_tx_id(3)
            .build();

        let txs = [tx_c, tx_b, tx_a];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // TOS first (highest priority type)
        assert_eq!(selector.next().unwrap().tx.get_fee_type(), &FeeType::TOS);
        // Energy second
        assert_eq!(selector.next().unwrap().tx.get_fee_type(), &FeeType::Energy);
        // UNO last
        assert_eq!(selector.next().unwrap().tx.get_fee_type(), &FeeType::UNO);
    }

    // =========================================================================
    // TxSelector::next() Behavior
    // =========================================================================

    #[test]
    fn test_next_returns_highest_priority() {
        let source_a = make_source(1);
        let source_b = make_source(2);
        let source_c = make_source(3);

        let low_tx = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(10)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let mid_tx = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(500)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let high_tx = MockTransactionBuilder::new()
            .with_source(source_c)
            .with_fee(9999)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(3)
            .build();

        let txs = [low_tx, mid_tx, high_tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_fee(), 9999);
    }

    #[test]
    fn test_next_exhausts_all_txs() {
        let source_a = make_source(1);
        let source_b = make_source(2);
        let source_c = make_source(3);

        let tx1 = MockTransactionBuilder::new()
            .with_source(source_a)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let tx2 = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(200)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let tx3 = MockTransactionBuilder::new()
            .with_source(source_c)
            .with_fee(300)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(3)
            .build();

        let txs = [tx1, tx2, tx3];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let mut count = 0;
        while selector.next().is_some() {
            count += 1;
        }
        assert_eq!(count, 3);
        assert!(selector.next().is_none());
    }

    #[test]
    fn test_next_same_sender_sequential() {
        // Sender A has nonces 0, 1, 2 with low fee
        // Sender B has nonce 0 with high fee
        // Expected: B's tx comes first, then A's nonces in order
        let source_a = make_source(1);
        let source_b = make_source(2);

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

        let tx_b0 = MockTransactionBuilder::new()
            .with_source(source_b)
            .with_fee(5000)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(4)
            .build();

        let txs = [tx_a2, tx_a0, tx_b0, tx_a1];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // B has higher fee, comes first
        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_fee(), 5000);

        // A's transactions come in nonce order
        let second = selector.next().unwrap();
        assert_eq!(second.tx.get_nonce(), 0);
        assert_eq!(second.tx.get_fee(), 100);

        let third = selector.next().unwrap();
        assert_eq!(third.tx.get_nonce(), 1);

        let fourth = selector.next().unwrap();
        assert_eq!(fourth.tx.get_nonce(), 2);

        assert!(selector.next().is_none());
    }

    #[test]
    fn test_empty_selector() {
        let txs: Vec<super::super::MockTransaction> = vec![];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        assert!(selector.next().is_none());
    }

    #[test]
    fn test_single_tx() {
        let source = make_source(1);

        let tx = MockTransactionBuilder::new()
            .with_source(source)
            .with_fee(500)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(1)
            .build();

        let txs = [tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        let first = selector.next().unwrap();
        assert_eq!(first.tx.get_fee(), 500);
        assert_eq!(first.tx.get_nonce(), 0);

        assert!(selector.next().is_none());
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_selector_with_many_senders() {
        // Create 12 different senders, each with a single transaction
        let txs: Vec<super::super::MockTransaction> = (0..12)
            .map(|i| {
                let source = make_source(i as u64 + 1);
                MockTransactionBuilder::new()
                    .with_source(source)
                    .with_fee(100 * (i as u64 + 1))
                    .with_fee_type(FeeType::TOS)
                    .with_nonce(0)
                    .with_tx_id(i as u64)
                    .build()
            })
            .collect();

        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);

        // Verify all 12 transactions are returned
        let mut count = 0;
        let mut prev_fee = u64::MAX;
        while let Some(entry) = selector.next() {
            // Fees should be in descending order (highest priority first)
            assert!(entry.tx.get_fee() <= prev_fee);
            prev_fee = entry.tx.get_fee();
            count += 1;
        }
        assert_eq!(count, 12);
    }

    #[test]
    fn test_selector_is_empty() {
        // Empty selector
        let txs: Vec<super::super::MockTransaction> = vec![];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let selector = TxSelector::new(iter);
        assert!(selector.is_empty());

        // Non-empty selector
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
        let selector = TxSelector::new(iter);
        assert!(!selector.is_empty());

        // After exhausting all transactions
        let source = make_source(2);
        let tx = MockTransactionBuilder::new()
            .with_source(source)
            .with_fee(100)
            .with_fee_type(FeeType::TOS)
            .with_nonce(0)
            .with_tx_id(2)
            .build();

        let txs = [tx];
        let iter = txs.iter().map(|m| (m.size, &m.hash, &m.tx));
        let mut selector = TxSelector::new(iter);
        selector.next();
        assert!(selector.is_empty());
    }
}
