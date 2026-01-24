// Tests for DAG structural invariants that must always hold.
//
// These invariants are fundamental properties of the BlockDAG that
// should never be violated regardless of the DAG topology:
//
// 1. Best tip always has the highest cumulative difficulty
// 2. DAG is acyclic (no block can be its own ancestor)
// 3. Height monotonically increases along any path
// 4. Cumulative difficulty monotonically increases along any path
// 5. Tips are non-reachable from each other
// 6. Sort ordering is deterministic and idempotent

#[cfg(test)]
mod tests {
    use super::super::{make_hash, DagBuilder};
    use indexmap::IndexSet;
    use tos_common::{block::BlockVersion, crypto::Hash, varuint::VarUint};
    use tos_daemon::core::blockdag::{
        build_reachability, find_best_tip_by_cumulative_difficulty,
        sort_descending_by_cumulative_difficulty, verify_non_reachability,
    };
    use tos_daemon::core::storage::DifficultyProvider;

    // =========================================================================
    // Invariant 1: Best tip always has highest cumulative difficulty
    // =========================================================================

    /// find_best_tip_by_cumulative_difficulty returns the tip with the highest CD.
    #[tokio::test]
    async fn test_invariant_best_tip_highest_cd() {
        let tip_a = make_hash(0x01);
        let tip_b = make_hash(0x02);
        let tip_c = make_hash(0x03);

        let provider = DagBuilder::new()
            .add_block(tip_a.clone(), 5, 100, 500, 1000)
            .add_block(tip_b.clone(), 5, 200, 900, 2000) // Highest CD
            .add_block(tip_c.clone(), 5, 150, 700, 1500)
            .build();

        let tips = [tip_a, tip_b.clone(), tip_c];
        let best = find_best_tip_by_cumulative_difficulty(&provider, tips.iter())
            .await
            .unwrap();

        assert_eq!(
            *best, tip_b,
            "Best tip should be the one with highest cumulative difficulty (900)"
        );
    }

    /// The best tip from find_best_tip should be consistent with the first element
    /// after sort_descending_by_cumulative_difficulty.
    #[tokio::test]
    async fn test_invariant_best_tip_consistent_with_sort() {
        let tip_a = make_hash(0x01);
        let tip_b = make_hash(0x02);
        let tip_c = make_hash(0x03);

        let provider = DagBuilder::new()
            .add_block(tip_a.clone(), 3, 100, 300, 1000)
            .add_block(tip_b.clone(), 4, 200, 800, 2000) // Highest CD
            .add_block(tip_c.clone(), 5, 150, 600, 1500)
            .build();

        // Get best tip
        let tips_vec = [tip_a.clone(), tip_b.clone(), tip_c.clone()];
        let best = find_best_tip_by_cumulative_difficulty(&provider, tips_vec.iter())
            .await
            .unwrap();

        // Sort descending and get first
        let mut scores = vec![
            (tip_a, VarUint::from_u64(300)),
            (tip_b, VarUint::from_u64(800)),
            (tip_c, VarUint::from_u64(600)),
        ];
        sort_descending_by_cumulative_difficulty(&mut scores);

        assert_eq!(
            *best, scores[0].0,
            "Best tip from find_best_tip should equal first element after descending sort"
        );
    }

    // =========================================================================
    // Invariant 2: DAG is acyclic (no block can be its own ancestor)
    // =========================================================================

    /// A block cannot reference itself as a past block.
    /// The reachability set of a block should not make cycles possible.
    #[tokio::test]
    async fn test_invariant_no_self_reference() {
        let genesis = make_hash(0x00);
        let block_a = make_hash(0x01);

        // Block A references only genesis, not itself
        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(block_a.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .build();

        let reachable = build_reachability(&provider, block_a.clone(), BlockVersion::Nobunaga)
            .await
            .unwrap();

        // The block itself is in the reachability set (by design of the algorithm),
        // but it should not create a cycle in the DAG structure.
        // The key invariant is that the past_blocks of block_a do NOT contain block_a.
        let past_of_a = provider
            .get_past_blocks_for_block_hash(&block_a)
            .await
            .unwrap();
        assert!(
            !past_of_a.iter().any(|h| *h == block_a),
            "A block must not reference itself as a past block"
        );

        // Reachability includes ancestors (genesis) and the starting block itself
        assert!(reachable.contains(&genesis));
        assert!(reachable.contains(&block_a));
    }

    /// The reachability set should contain only ancestors and the block itself,
    /// never descendants. This prevents cycles.
    #[tokio::test]
    async fn test_invariant_reachability_excludes_cycles() {
        let genesis = make_hash(0x00);
        let block_a = make_hash(0x01);
        let block_b = make_hash(0x02);
        let block_c = make_hash(0x03);

        // Chain: genesis -> A -> B -> C
        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(block_a.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(block_b.clone(), 2, 100, 300, 2000, vec![block_a.clone()])
            .add_block_with_tips(block_c.clone(), 3, 100, 400, 3000, vec![block_b.clone()])
            .build();

        // Reachability of block_a should include genesis and A, but NOT B or C
        let reach_a = build_reachability(&provider, block_a.clone(), BlockVersion::Nobunaga)
            .await
            .unwrap();

        assert!(
            reach_a.contains(&genesis),
            "Reachability of A should include genesis"
        );
        assert!(
            reach_a.contains(&block_a),
            "Reachability of A should include A itself"
        );
        assert!(
            !reach_a.contains(&block_b),
            "Reachability of A should NOT include descendant B"
        );
        assert!(
            !reach_a.contains(&block_c),
            "Reachability of A should NOT include descendant C"
        );
    }

    // =========================================================================
    // Invariant 3: Height monotonically increases along any path
    // =========================================================================

    /// A child block must always have strictly greater height than its parent.
    #[tokio::test]
    async fn test_invariant_child_height_greater_than_parent() {
        let parent = make_hash(0x01);
        let child = make_hash(0x02);

        let parent_height = 10u64;
        let child_height = 11u64;

        let provider = DagBuilder::new()
            .add_block(parent.clone(), parent_height, 100, 1000, 1000)
            .add_block_with_tips(
                child.clone(),
                child_height,
                100,
                1100,
                2000,
                vec![parent.clone()],
            )
            .build();

        let p_h = provider.get_height_for_block_hash(&parent).await.unwrap();
        let c_h = provider.get_height_for_block_hash(&child).await.unwrap();

        assert!(
            c_h > p_h,
            "Child height ({}) must be strictly greater than parent height ({})",
            c_h,
            p_h
        );
    }

    /// In a linear chain, heights must be strictly increasing.
    #[tokio::test]
    async fn test_invariant_height_increases_in_chain() {
        let genesis = make_hash(0x00);
        let b1 = make_hash(0x01);
        let b2 = make_hash(0x02);
        let b3 = make_hash(0x03);
        let b4 = make_hash(0x04);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(b1.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(b2.clone(), 2, 100, 300, 2000, vec![b1.clone()])
            .add_block_with_tips(b3.clone(), 3, 100, 400, 3000, vec![b2.clone()])
            .add_block_with_tips(b4.clone(), 4, 100, 500, 4000, vec![b3.clone()])
            .build();

        let chain = [genesis, b1, b2, b3, b4];
        for i in 1..chain.len() {
            let prev_h = provider
                .get_height_for_block_hash(&chain[i - 1])
                .await
                .unwrap();
            let curr_h = provider.get_height_for_block_hash(&chain[i]).await.unwrap();
            assert!(
                curr_h > prev_h,
                "Height at index {} ({}) must be > height at index {} ({})",
                i,
                curr_h,
                i - 1,
                prev_h
            );
        }
    }

    // =========================================================================
    // Invariant 4: Cumulative difficulty monotonically increases along any path
    // =========================================================================

    /// A child's CD must be >= parent's CD + child's own difficulty.
    #[tokio::test]
    async fn test_invariant_child_cd_greater_than_parent() {
        let parent = make_hash(0x01);
        let child = make_hash(0x02);

        // Parent: CD = 1000, Child: difficulty = 200, CD = 1200
        let provider = DagBuilder::new()
            .add_block(parent.clone(), 5, 500, 1000, 1000)
            .add_block_with_tips(child.clone(), 6, 200, 1200, 2000, vec![parent.clone()])
            .build();

        let parent_cd = provider
            .get_cumulative_difficulty_for_block_hash(&parent)
            .await
            .unwrap();
        let child_cd = provider
            .get_cumulative_difficulty_for_block_hash(&child)
            .await
            .unwrap();
        let child_diff = provider
            .get_difficulty_for_block_hash(&child)
            .await
            .unwrap();

        assert!(
            child_cd >= parent_cd + child_diff,
            "Child CD ({:?}) must be >= parent CD ({:?}) + child difficulty ({:?})",
            child_cd,
            parent_cd,
            child_diff
        );
    }

    /// In a linear chain, cumulative difficulty must be strictly increasing
    /// (assuming non-zero block difficulties).
    #[tokio::test]
    async fn test_invariant_cd_increases_in_chain() {
        let genesis = make_hash(0x00);
        let b1 = make_hash(0x01);
        let b2 = make_hash(0x02);
        let b3 = make_hash(0x03);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(b1.clone(), 1, 150, 250, 1000, vec![genesis.clone()])
            .add_block_with_tips(b2.clone(), 2, 200, 450, 2000, vec![b1.clone()])
            .add_block_with_tips(b3.clone(), 3, 300, 750, 3000, vec![b2.clone()])
            .build();

        let chain = [genesis, b1, b2, b3];
        for i in 1..chain.len() {
            let prev_cd = provider
                .get_cumulative_difficulty_for_block_hash(&chain[i - 1])
                .await
                .unwrap();
            let curr_cd = provider
                .get_cumulative_difficulty_for_block_hash(&chain[i])
                .await
                .unwrap();
            assert!(
                curr_cd > prev_cd,
                "CD at index {} ({:?}) must be > CD at index {} ({:?})",
                i,
                curr_cd,
                i - 1,
                prev_cd
            );
        }
    }

    // =========================================================================
    // Invariant 5: Tips are non-reachable from each other
    // =========================================================================

    /// In a simple 2-branch DAG, the tips of independent branches should be
    /// non-reachable from each other.
    #[tokio::test]
    async fn test_invariant_tips_non_reachable_simple_dag() {
        let genesis = make_hash(0x00);
        let branch_a = make_hash(0x01);
        let branch_b = make_hash(0x02);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(branch_a.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(branch_b.clone(), 1, 150, 250, 1000, vec![genesis.clone()])
            .build();

        let mut tips = IndexSet::new();
        tips.insert(branch_a);
        tips.insert(branch_b);

        let result = verify_non_reachability(&provider, &tips, BlockVersion::Nobunaga)
            .await
            .unwrap();

        assert!(
            result,
            "Tips of independent branches must be non-reachable from each other"
        );
    }

    /// In a complex multi-branch DAG with deeper structure, tips at the ends
    /// of independent branches should still be non-reachable.
    ///
    /// DAG structure:
    ///   genesis -> A1 -> A2 -> A3 (branch A tip)
    ///   genesis -> B1 -> B2 (branch B tip)
    ///   genesis -> C1 -> C2 -> C3 -> C4 (branch C tip)
    #[tokio::test]
    async fn test_invariant_tips_non_reachable_complex_dag() {
        let genesis = make_hash(0x00);
        // Branch A
        let a1 = make_hash(0x11);
        let a2 = make_hash(0x12);
        let a3 = make_hash(0x13);
        // Branch B
        let b1 = make_hash(0x21);
        let b2 = make_hash(0x22);
        // Branch C
        let c1 = make_hash(0x31);
        let c2 = make_hash(0x32);
        let c3 = make_hash(0x33);
        let c4 = make_hash(0x34);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            // Branch A
            .add_block_with_tips(a1.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(a2.clone(), 2, 100, 300, 2000, vec![a1.clone()])
            .add_block_with_tips(a3.clone(), 3, 100, 400, 3000, vec![a2.clone()])
            // Branch B
            .add_block_with_tips(b1.clone(), 1, 200, 300, 1000, vec![genesis.clone()])
            .add_block_with_tips(b2.clone(), 2, 200, 500, 2000, vec![b1.clone()])
            // Branch C
            .add_block_with_tips(c1.clone(), 1, 50, 150, 1000, vec![genesis.clone()])
            .add_block_with_tips(c2.clone(), 2, 50, 200, 2000, vec![c1.clone()])
            .add_block_with_tips(c3.clone(), 3, 50, 250, 3000, vec![c2.clone()])
            .add_block_with_tips(c4.clone(), 4, 50, 300, 4000, vec![c3.clone()])
            .build();

        let mut tips = IndexSet::new();
        tips.insert(a3);
        tips.insert(b2);
        tips.insert(c4);

        let result = verify_non_reachability(&provider, &tips, BlockVersion::Nobunaga)
            .await
            .unwrap();

        assert!(
            result,
            "Tips of complex multi-branch DAG must be non-reachable from each other"
        );
    }

    // =========================================================================
    // Invariant 6: Sort ordering is deterministic
    // =========================================================================

    /// Sorting the same input 100 times should always produce the same output.
    #[test]
    fn test_invariant_sort_deterministic_repeated() {
        let hash_a = make_hash(0x01);
        let hash_b = make_hash(0x02);
        let hash_c = make_hash(0x03);

        let original = vec![
            (hash_a.clone(), VarUint::from_u64(300)),
            (hash_b.clone(), VarUint::from_u64(500)),
            (hash_c.clone(), VarUint::from_u64(100)),
        ];

        // Sort once to get the reference result
        let mut reference = original.clone();
        sort_descending_by_cumulative_difficulty(&mut reference);

        // Sort 100 times and verify each produces the same result
        for i in 0..100 {
            let mut attempt = original.clone();
            sort_descending_by_cumulative_difficulty(&mut attempt);
            assert_eq!(
                attempt, reference,
                "Sort attempt {} produced different result",
                i
            );
        }

        // Verify the expected order: 500, 300, 100 (descending)
        assert_eq!(reference[0].1, VarUint::from_u64(500));
        assert_eq!(reference[1].1, VarUint::from_u64(300));
        assert_eq!(reference[2].1, VarUint::from_u64(100));
    }

    /// Sorting already-sorted data should be idempotent (no change).
    #[test]
    fn test_invariant_sort_preserved_after_mutation() {
        let hash_a = make_hash(0x01);
        let hash_b = make_hash(0x02);
        let hash_c = make_hash(0x03);
        let hash_d = make_hash(0x04);

        let mut scores = vec![
            (hash_a, VarUint::from_u64(1000)),
            (hash_b, VarUint::from_u64(500)),
            (hash_c, VarUint::from_u64(750)),
            (hash_d, VarUint::from_u64(200)),
        ];

        // First sort
        sort_descending_by_cumulative_difficulty(&mut scores);
        let first_sort = scores.clone();

        // Second sort (re-sorting sorted data)
        sort_descending_by_cumulative_difficulty(&mut scores);

        assert_eq!(
            scores, first_sort,
            "Re-sorting already sorted data must produce identical result (idempotent)"
        );

        // Verify order is descending
        for i in 1..scores.len() {
            assert!(
                scores[i - 1].1 >= scores[i].1,
                "Element at {} ({:?}) should be >= element at {} ({:?})",
                i - 1,
                scores[i - 1].1,
                i,
                scores[i].1
            );
        }
    }

    // =========================================================================
    // Additional invariant tests
    // =========================================================================

    /// When all tips have equal cumulative difficulty, the sort should use
    /// hash as a tiebreaker (descending hash order for descending sort).
    #[test]
    fn test_invariant_sort_tiebreaker_by_hash() {
        let hash_low = make_hash(0x01);
        let hash_mid = make_hash(0x80);
        let hash_high = make_hash(0xFF);

        let mut scores = vec![
            (hash_low.clone(), VarUint::from_u64(1000)),
            (hash_mid.clone(), VarUint::from_u64(1000)),
            (hash_high.clone(), VarUint::from_u64(1000)),
        ];

        sort_descending_by_cumulative_difficulty(&mut scores);

        // With equal CD, hash is used as tiebreaker (descending)
        // Hash 0xFF > 0x80 > 0x01
        assert_eq!(
            scores[0].0, hash_high,
            "With equal CD, highest hash should come first"
        );
        assert_eq!(scores[1].0, hash_mid);
        assert_eq!(scores[2].0, hash_low);
    }

    /// Verify that find_best_tip returns error for empty tips.
    #[tokio::test]
    async fn test_invariant_best_tip_empty_tips_error() {
        let provider = DagBuilder::new().build();

        let tips: Vec<Hash> = vec![];
        let result = find_best_tip_by_cumulative_difficulty(&provider, tips.iter()).await;

        assert!(
            result.is_err(),
            "find_best_tip with empty tips should return an error"
        );
    }

    /// Single-tip case should trivially return that tip.
    #[tokio::test]
    async fn test_invariant_best_tip_single() {
        let single = make_hash(0x42);

        let provider = DagBuilder::new()
            .add_block(single.clone(), 10, 500, 5000, 1000)
            .build();

        let tips = [single.clone()];
        let best = find_best_tip_by_cumulative_difficulty(&provider, tips.iter())
            .await
            .unwrap();

        assert_eq!(*best, single, "Single tip should be returned as best tip");
    }

    /// Reachability from genesis should only contain genesis (no past blocks).
    #[tokio::test]
    async fn test_invariant_genesis_reachability() {
        let genesis = make_hash(0x00);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .build();

        let reachable = build_reachability(&provider, genesis.clone(), BlockVersion::Nobunaga)
            .await
            .unwrap();

        // Genesis has no parents, so reachability is just itself
        assert_eq!(reachable.len(), 1);
        assert!(reachable.contains(&genesis));
    }
}
