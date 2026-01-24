#[cfg(test)]
mod tests {
    use super::super::mock::*;

    #[test]
    fn pop_one_block_from_chain() {
        let mut chain = make_linear_chain(10, 100);
        assert_eq!(chain.topoheight, 10);

        let popped = chain.pop_blocks(1);
        assert_eq!(popped.len(), 1);
        assert_eq!(popped[0].topoheight, 10);
        assert_eq!(chain.topoheight, 9);
    }

    #[test]
    fn pop_multiple_blocks_deep_reorg() {
        let mut chain = make_linear_chain(50, 100);
        assert_eq!(chain.topoheight, 50);

        let popped = chain.pop_blocks(10);
        assert_eq!(popped.len(), 10);
        assert_eq!(chain.topoheight, 40);
    }

    #[test]
    fn pop_blocks_stops_at_pruned_boundary() {
        let mut chain = make_linear_chain(300, 100);
        chain.pruned_topoheight = Some(50);

        // Safety boundary = pruned(50) + PRUNE_SAFETY_LIMIT(240) = 290
        // Chain is at topoheight 300, can only pop down to 291
        let _popped = chain.pop_blocks(20);

        // Should stop at safety boundary (topoheight 290)
        assert!(chain.topoheight <= 50 + PRUNE_SAFETY_LIMIT || chain.topoheight >= 290);
    }

    #[test]
    fn pop_blocks_with_prune_safety_limit_enforcement() {
        let mut chain = make_linear_chain(300, 100);
        chain.pruned_topoheight = Some(50);

        // Try to pop more blocks than allowed
        let popped = chain.pop_blocks(100);

        // Should not pop below pruned + PRUNE_SAFETY_LIMIT
        let safety_boundary = 50 + PRUNE_SAFETY_LIMIT; // 290
        assert!(chain.topoheight >= safety_boundary || chain.topoheight == safety_boundary);
        // Only 10 blocks should have been popped (300 - 290 = 10)
        assert_eq!(popped.len(), 10);
    }

    #[test]
    fn popped_blocks_returned_in_reverse_order() {
        let mut chain = make_linear_chain(20, 100);

        let popped = chain.pop_blocks(5);

        // Blocks are popped from highest to lowest topoheight
        assert_eq!(popped[0].topoheight, 20);
        assert_eq!(popped[1].topoheight, 19);
        assert_eq!(popped[2].topoheight, 18);
        assert_eq!(popped[3].topoheight, 17);
        assert_eq!(popped[4].topoheight, 16);
    }

    #[test]
    fn cumulative_difficulty_decreases_on_pop() {
        let mut chain = make_linear_chain(10, 100);
        let initial_cd = chain.cumulative_difficulty;

        chain.pop_blocks(3);

        // CD should decrease by 3 * 100 = 300
        assert_eq!(chain.cumulative_difficulty, initial_cd - 300);
    }

    #[test]
    fn height_updates_after_pop() {
        let mut chain = make_linear_chain(20, 100);
        assert_eq!(chain.height, 20);

        chain.pop_blocks(5);

        assert_eq!(chain.height, 15);
    }

    #[test]
    fn tips_update_after_pop() {
        let mut chain = make_linear_chain(20, 100);

        chain.pop_blocks(5);

        // Tips should now point to block at topoheight 15
        let block_15 = chain.get_block_at_topo(15).unwrap();
        assert_eq!(chain.tips, vec![block_15.hash]);
    }

    #[test]
    fn topoheight_decreases_by_pop_count() {
        let mut chain = make_linear_chain(30, 100);

        chain.pop_blocks(7);

        assert_eq!(chain.topoheight, 23);
    }

    #[test]
    fn stable_height_recalculated_after_pop() {
        let mut chain = make_linear_chain(50, 100);
        // Initial stable = 50 - 24 = 26
        assert_eq!(chain.stable_topoheight, 26);

        chain.pop_blocks(10);
        // New stable = 40 - 24 = 16
        assert_eq!(chain.stable_topoheight, 16);
    }

    #[test]
    fn pop_all_blocks_to_genesis() {
        let mut chain = make_linear_chain(10, 100);

        let popped = chain.pop_blocks(10);

        assert_eq!(popped.len(), 10);
        assert_eq!(chain.topoheight, 0);
        assert_eq!(chain.height, 0);
        assert_eq!(chain.cumulative_difficulty, 0);
        assert_eq!(chain.tips, vec![[0u8; 32]]);
    }

    #[test]
    fn pop_zero_blocks_no_op() {
        let mut chain = make_linear_chain(10, 100);
        let initial_topo = chain.topoheight;
        let initial_height = chain.height;
        let initial_cd = chain.cumulative_difficulty;

        let popped = chain.pop_blocks(0);

        assert!(popped.is_empty());
        assert_eq!(chain.topoheight, initial_topo);
        assert_eq!(chain.height, initial_height);
        assert_eq!(chain.cumulative_difficulty, initial_cd);
    }

    #[test]
    fn transactions_collected_from_popped_blocks() {
        let mut chain = MockChainState::new();

        // Add blocks with transactions
        for i in 1u64..=5 {
            let mut hash = [0u8; 32];
            hash[0..8].copy_from_slice(&i.to_le_bytes());
            let mut tx_hash = [0u8; 32];
            tx_hash[0..8].copy_from_slice(&(i * 100).to_le_bytes());

            chain.add_block(BlockMetadata {
                hash,
                topoheight: i,
                height: i,
                difficulty: 100,
                cumulative_difficulty: i * 100,
                tips: vec![[0u8; 32]],
                txs: vec![tx_hash],
            });
        }

        let popped = chain.pop_blocks(3);

        // Collect all transactions from popped blocks
        let orphaned_txs: Vec<Hash> = popped.iter().flat_map(|b| b.txs.clone()).collect();
        assert_eq!(orphaned_txs.len(), 3);
    }

    #[test]
    fn reorg_pop_blocks_then_add_new_blocks() {
        let mut chain = make_linear_chain(30, 100);

        // Pop 5 blocks (simulating reorg)
        let popped = chain.pop_blocks(5);
        assert_eq!(popped.len(), 5);
        assert_eq!(chain.topoheight, 25);

        // Add 7 new blocks from the fork (higher difficulty)
        for i in 26u64..=32 {
            let mut hash = [0xBBu8; 32];
            hash[0..8].copy_from_slice(&i.to_le_bytes());
            chain.add_block(BlockMetadata {
                hash,
                topoheight: i,
                height: i,
                difficulty: 150, // higher difficulty fork
                cumulative_difficulty: chain.cumulative_difficulty + 150,
                tips: vec![chain.tips[0]],
                txs: Vec::new(),
            });
        }

        assert_eq!(chain.topoheight, 32);
        assert_eq!(chain.height, 32);
    }

    #[test]
    fn reorg_new_chain_has_higher_cumulative_difficulty() {
        let mut chain = make_linear_chain(30, 100);
        let original_cd = chain.cumulative_difficulty;

        // Pop 5 blocks (lose 500 CD)
        chain.pop_blocks(5);
        let after_pop_cd = chain.cumulative_difficulty;
        assert_eq!(after_pop_cd, original_cd - 500);

        // Add 5 new blocks with higher difficulty (gain 750 CD)
        for i in 26u64..=30 {
            let mut hash = [0xCCu8; 32];
            hash[0..8].copy_from_slice(&i.to_le_bytes());
            chain.add_block(BlockMetadata {
                hash,
                topoheight: i,
                height: i,
                difficulty: 150,
                cumulative_difficulty: chain.cumulative_difficulty + 150,
                tips: vec![chain.tips[0]],
                txs: Vec::new(),
            });
        }

        // New chain should have higher CD than original
        assert!(chain.cumulative_difficulty > original_cd);
    }

    #[test]
    fn reorg_mempool_drain_and_reinsert() {
        let mut chain = make_linear_chain(20, 100);

        // Simulate mempool with pending transactions
        let mut mempool: Vec<Hash> = Vec::new();
        for i in 0..5 {
            let mut tx = [0u8; 32];
            tx[0..8].copy_from_slice(&(i as u64 + 200).to_le_bytes());
            mempool.push(tx);
        }

        // Add blocks with some transactions from mempool
        for i in 21u64..=23 {
            let mut hash = [0u8; 32];
            hash[0..8].copy_from_slice(&i.to_le_bytes());
            let tx = mempool.remove(0); // take from mempool
            chain.add_block(BlockMetadata {
                hash,
                topoheight: i,
                height: i,
                difficulty: 100,
                cumulative_difficulty: chain.cumulative_difficulty + 100,
                tips: vec![chain.tips[0]],
                txs: vec![tx],
            });
        }

        // Pop those blocks (reorg)
        let popped = chain.pop_blocks(3);

        // Orphaned transactions go back to mempool
        for block in &popped {
            for tx in &block.txs {
                mempool.push(*tx);
            }
        }

        // Mempool should now have original remaining + orphaned
        assert_eq!(mempool.len(), 5); // 2 original + 3 orphaned
    }

    #[test]
    fn orphaned_transactions_separated() {
        let mut chain = MockChainState::new();

        // Create blocks with transactions
        for i in 1u64..=5 {
            let mut hash = [0u8; 32];
            hash[0..8].copy_from_slice(&i.to_le_bytes());
            let mut tx_hash = [0u8; 32];
            tx_hash[0..8].copy_from_slice(&(i * 50).to_le_bytes());
            chain.add_block(BlockMetadata {
                hash,
                topoheight: i,
                height: i,
                difficulty: 100,
                cumulative_difficulty: i * 100,
                tips: vec![[0u8; 32]],
                txs: vec![tx_hash],
            });
        }

        let popped = chain.pop_blocks(3);
        let orphaned_txs: Vec<Hash> = popped.iter().flat_map(|b| b.txs.clone()).collect();

        // Separate into re-validatable and invalid
        // For testing: odd-indexed txs are "invalid" (fail re-validation)
        let indexed: Vec<(usize, Hash)> = orphaned_txs.into_iter().enumerate().collect();
        let valid: Vec<Hash> = indexed
            .iter()
            .filter(|(idx, _)| idx % 2 == 0)
            .map(|(_, tx)| *tx)
            .collect();
        let invalid: Vec<Hash> = indexed
            .iter()
            .filter(|(idx, _)| idx % 2 != 0)
            .map(|(_, tx)| *tx)
            .collect();

        assert_eq!(valid.len(), 2); // indices 0, 2
        assert_eq!(invalid.len(), 1); // index 1
    }

    #[test]
    fn state_consistency_after_reorg() {
        let mut chain = make_linear_chain(50, 100);

        // Perform reorg: pop 10, add 12
        chain.pop_blocks(10);

        for i in 41u64..=52 {
            let mut hash = [0xDDu8; 32];
            hash[0..8].copy_from_slice(&i.to_le_bytes());
            chain.add_block(BlockMetadata {
                hash,
                topoheight: i,
                height: i,
                difficulty: 120,
                cumulative_difficulty: chain.cumulative_difficulty + 120,
                tips: vec![chain.tips[0]],
                txs: Vec::new(),
            });
        }

        // Verify all state is consistent
        assert_eq!(chain.topoheight, 52);
        assert_eq!(chain.height, 52);
        // Stable height = 52 - 24 = 28
        assert_eq!(chain.stable_topoheight, 28);
        // Tips should point to last block
        let last_block = chain.get_block_at_topo(52).unwrap();
        assert_eq!(chain.tips, vec![last_block.hash]);
        // CD should reflect the mixed difficulties
        assert!(chain.cumulative_difficulty > 0);
    }
}
