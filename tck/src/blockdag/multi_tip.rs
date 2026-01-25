// Tests for multi-parent block construction in the BlockDAG.
//
// Key concepts tested:
// - calculate_height_at_tips: height = max(parent_heights) + 1
// - TIPS_LIMIT = 3: maximum number of parent tips per block
// - verify_non_reachability: tips must not be ancestors of each other
//   (traverses up to 2 * STABLE_LIMIT levels to build reachability sets)
//
// The tests use MockDagProvider and DagBuilder to construct in-memory DAG
// topologies for verifying the multi-tip block construction rules.

#[cfg(test)]
mod tests {
    use super::super::{make_hash, DagBuilder};
    use indexmap::IndexSet;
    use tos_common::{block::BlockVersion, crypto::Hash};
    use tos_daemon::core::blockdag::{calculate_height_at_tips, verify_non_reachability};

    // =========================================================================
    // calculate_height_at_tips tests
    // =========================================================================

    /// A block with a single parent at height H should have height H + 1.
    #[tokio::test]
    async fn test_single_parent_block() {
        let parent_hash = make_hash(0x01);

        let provider = DagBuilder::new()
            .add_block(parent_hash.clone(), 10, 100, 100, 1000)
            .build();

        let tips = [parent_hash];
        let height = calculate_height_at_tips(&provider, tips.iter())
            .await
            .unwrap();

        assert_eq!(height, 11, "Height should be parent_height(10) + 1 = 11");
    }

    /// A block with two parents should have height = max(parent_heights) + 1.
    #[tokio::test]
    async fn test_two_parent_tips() {
        let parent_a = make_hash(0x01);
        let parent_b = make_hash(0x02);

        let provider = DagBuilder::new()
            .add_block(parent_a.clone(), 5, 100, 500, 1000)
            .add_block(parent_b.clone(), 8, 100, 800, 2000)
            .build();

        let tips = [parent_a, parent_b];
        let height = calculate_height_at_tips(&provider, tips.iter())
            .await
            .unwrap();

        assert_eq!(height, 9, "Height should be max(5, 8) + 1 = 9");
    }

    /// A block with three parents should have height = max(parent_heights) + 1.
    #[tokio::test]
    async fn test_three_parent_tips() {
        let parent_a = make_hash(0x01);
        let parent_b = make_hash(0x02);
        let parent_c = make_hash(0x03);

        let provider = DagBuilder::new()
            .add_block(parent_a.clone(), 3, 100, 300, 1000)
            .add_block(parent_b.clone(), 7, 100, 700, 2000)
            .add_block(parent_c.clone(), 5, 100, 500, 1500)
            .build();

        let tips = [parent_a, parent_b, parent_c];
        let height = calculate_height_at_tips(&provider, tips.iter())
            .await
            .unwrap();

        assert_eq!(height, 8, "Height should be max(3, 7, 5) + 1 = 8");
    }

    /// When all parents are at the same height, the result is that height + 1.
    #[tokio::test]
    async fn test_tips_at_same_height() {
        let parent_a = make_hash(0x01);
        let parent_b = make_hash(0x02);
        let parent_c = make_hash(0x03);

        let provider = DagBuilder::new()
            .add_block(parent_a.clone(), 10, 100, 1000, 1000)
            .add_block(parent_b.clone(), 10, 150, 1050, 1100)
            .add_block(parent_c.clone(), 10, 200, 1100, 1200)
            .build();

        let tips = [parent_a, parent_b, parent_c];
        let height = calculate_height_at_tips(&provider, tips.iter())
            .await
            .unwrap();

        assert_eq!(height, 11, "Height should be max(10, 10, 10) + 1 = 11");
    }

    /// When parents are at very different heights, the maximum is selected.
    #[tokio::test]
    async fn test_tips_at_different_heights() {
        let parent_a = make_hash(0x01);
        let parent_b = make_hash(0x02);

        let provider = DagBuilder::new()
            .add_block(parent_a.clone(), 1, 100, 100, 1000)
            .add_block(parent_b.clone(), 100, 100, 10000, 50000)
            .build();

        let tips = [parent_a, parent_b];
        let height = calculate_height_at_tips(&provider, tips.iter())
            .await
            .unwrap();

        assert_eq!(height, 101, "Height should be max(1, 100) + 1 = 101");
    }

    /// TIPS_LIMIT is 3. Verify this constant value matches the protocol rule.
    #[test]
    fn test_tips_limit_three() {
        use tos_common::config::TIPS_LIMIT;
        assert_eq!(TIPS_LIMIT, 3, "TIPS_LIMIT should be 3");
    }

    // =========================================================================
    // verify_non_reachability tests
    // =========================================================================

    /// Two independent branches (no ancestor relationship) should pass
    /// non-reachability verification.
    ///
    /// DAG structure:
    ///   genesis -> A1 (branch A)
    ///   genesis -> B1 (branch B)
    /// Tips: {A1, B1}
    #[tokio::test]
    async fn test_non_reachability_independent_tips() {
        let genesis = make_hash(0x00);
        let tip_a = make_hash(0x01);
        let tip_b = make_hash(0x02);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(tip_a.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(tip_b.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .build();

        let mut tips = IndexSet::new();
        tips.insert(tip_a);
        tips.insert(tip_b);

        let result = verify_non_reachability(&provider, &tips, BlockVersion::Nobunaga)
            .await
            .unwrap();

        assert!(
            result,
            "Independent tips should pass non-reachability check"
        );
    }

    /// If tip A is an ancestor of tip B, non-reachability should fail.
    ///
    /// DAG structure:
    ///   genesis -> A -> B
    /// Tips: {A, B} -- A is ancestor of B, so this should fail
    #[tokio::test]
    async fn test_non_reachability_ancestor_tip_fails() {
        let genesis = make_hash(0x00);
        let block_a = make_hash(0x01);
        let block_b = make_hash(0x02);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(block_a.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(block_b.clone(), 2, 100, 300, 2000, vec![block_a.clone()])
            .build();

        let mut tips = IndexSet::new();
        tips.insert(block_a);
        tips.insert(block_b);

        let result = verify_non_reachability(&provider, &tips, BlockVersion::Nobunaga)
            .await
            .unwrap();

        assert!(
            !result,
            "Tips where one is ancestor of another should FAIL non-reachability"
        );
    }

    /// A single tip always passes non-reachability (nothing to compare against).
    #[tokio::test]
    async fn test_non_reachability_single_tip_passes() {
        let genesis = make_hash(0x00);
        let single_tip = make_hash(0x01);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(single_tip.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .build();

        let mut tips = IndexSet::new();
        tips.insert(single_tip);

        let result = verify_non_reachability(&provider, &tips, BlockVersion::Nobunaga)
            .await
            .unwrap();

        assert!(result, "Single tip should always pass non-reachability");
    }

    /// Three independent branches should all pass non-reachability.
    ///
    /// DAG structure:
    ///   genesis -> A1 (branch A)
    ///   genesis -> B1 (branch B)
    ///   genesis -> C1 (branch C)
    /// Tips: {A1, B1, C1}
    #[tokio::test]
    async fn test_non_reachability_three_independent_tips() {
        let genesis = make_hash(0x00);
        let tip_a = make_hash(0x01);
        let tip_b = make_hash(0x02);
        let tip_c = make_hash(0x03);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(tip_a.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(tip_b.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(tip_c.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .build();

        let mut tips = IndexSet::new();
        tips.insert(tip_a);
        tips.insert(tip_b);
        tips.insert(tip_c);

        let result = verify_non_reachability(&provider, &tips, BlockVersion::Nobunaga)
            .await
            .unwrap();

        assert!(
            result,
            "Three independent tips should pass non-reachability"
        );
    }

    /// A deep DAG with independent branches that diverge from a common ancestor.
    /// Each branch extends multiple levels deep, but they remain independent.
    ///
    /// DAG structure:
    ///   genesis -> A1 -> A2 -> A3 (branch A)
    ///   genesis -> B1 -> B2 -> B3 (branch B)
    /// Tips: {A3, B3}
    #[tokio::test]
    async fn test_non_reachability_deep_dag() {
        let genesis = make_hash(0x00);
        let a1 = make_hash(0x11);
        let a2 = make_hash(0x12);
        let a3 = make_hash(0x13);
        let b1 = make_hash(0x21);
        let b2 = make_hash(0x22);
        let b3 = make_hash(0x23);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            // Branch A
            .add_block_with_tips(a1.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(a2.clone(), 2, 100, 300, 2000, vec![a1.clone()])
            .add_block_with_tips(a3.clone(), 3, 100, 400, 3000, vec![a2.clone()])
            // Branch B
            .add_block_with_tips(b1.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(b2.clone(), 2, 100, 300, 2000, vec![b1.clone()])
            .add_block_with_tips(b3.clone(), 3, 100, 400, 3000, vec![b2.clone()])
            .build();

        let mut tips = IndexSet::new();
        tips.insert(a3);
        tips.insert(b3);

        let result = verify_non_reachability(&provider, &tips, BlockVersion::Nobunaga)
            .await
            .unwrap();

        assert!(
            result,
            "Deep independent branches should pass non-reachability"
        );
    }

    /// Sibling blocks (same parent, same height) should pass non-reachability.
    /// Neither is an ancestor of the other.
    ///
    /// DAG structure:
    ///   genesis -> parent -> sibling_1
    ///   genesis -> parent -> sibling_2
    /// Tips: {sibling_1, sibling_2}
    #[tokio::test]
    async fn test_non_reachability_sibling_blocks() {
        let genesis = make_hash(0x00);
        let parent = make_hash(0x01);
        let sibling_1 = make_hash(0x02);
        let sibling_2 = make_hash(0x03);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(parent.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(sibling_1.clone(), 2, 100, 300, 2000, vec![parent.clone()])
            .add_block_with_tips(sibling_2.clone(), 2, 150, 350, 2000, vec![parent.clone()])
            .build();

        let mut tips = IndexSet::new();
        tips.insert(sibling_1);
        tips.insert(sibling_2);

        let result = verify_non_reachability(&provider, &tips, BlockVersion::Nobunaga)
            .await
            .unwrap();

        assert!(
            result,
            "Sibling blocks (same parent) should pass non-reachability"
        );
    }

    // =========================================================================
    // Additional edge case tests
    // =========================================================================

    /// Empty tips should yield height 0 (no parents means genesis-like).
    #[tokio::test]
    async fn test_empty_tips_height_zero() {
        let provider = DagBuilder::new().build();

        let tips: Vec<Hash> = vec![];
        let height = calculate_height_at_tips(&provider, tips.iter())
            .await
            .unwrap();

        assert_eq!(height, 0, "Empty tips should yield height 0");
    }

    /// A parent at height 0 (genesis) should produce child height 1.
    #[tokio::test]
    async fn test_parent_at_genesis_height() {
        let genesis = make_hash(0x00);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .build();

        let tips = [genesis];
        let height = calculate_height_at_tips(&provider, tips.iter())
            .await
            .unwrap();

        assert_eq!(
            height, 1,
            "Child of genesis (height 0) should be at height 1"
        );
    }

    /// Non-reachability with a grandparent-grandchild relationship should fail.
    ///
    /// DAG structure:
    ///   genesis -> A -> B -> C
    /// Tips: {A, C} -- A is a grandparent of C
    #[tokio::test]
    async fn test_non_reachability_grandparent_fails() {
        let genesis = make_hash(0x00);
        let block_a = make_hash(0x01);
        let block_b = make_hash(0x02);
        let block_c = make_hash(0x03);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(block_a.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(block_b.clone(), 2, 100, 300, 2000, vec![block_a.clone()])
            .add_block_with_tips(block_c.clone(), 3, 100, 400, 3000, vec![block_b.clone()])
            .build();

        let mut tips = IndexSet::new();
        tips.insert(block_a);
        tips.insert(block_c);

        let result = verify_non_reachability(&provider, &tips, BlockVersion::Nobunaga)
            .await
            .unwrap();

        assert!(
            !result,
            "Grandparent-grandchild relationship should FAIL non-reachability"
        );
    }

    /// Multi-parent block referencing two independent branches.
    /// Verify the resulting height is computed from the max parent height.
    ///
    /// DAG structure:
    ///   genesis -> A (height 1)
    ///   genesis -> B (height 1)
    ///   {A, B} -> merge_block (height 2)
    #[tokio::test]
    async fn test_merge_block_height() {
        let genesis = make_hash(0x00);
        let branch_a = make_hash(0x01);
        let branch_b = make_hash(0x02);

        let provider = DagBuilder::new()
            .add_block(genesis.clone(), 0, 100, 100, 0)
            .add_block_with_tips(branch_a.clone(), 1, 100, 200, 1000, vec![genesis.clone()])
            .add_block_with_tips(branch_b.clone(), 1, 150, 250, 1000, vec![genesis.clone()])
            .build();

        // The merge block references both branches as tips
        let tips = [branch_a, branch_b];
        let height = calculate_height_at_tips(&provider, tips.iter())
            .await
            .unwrap();

        assert_eq!(height, 2, "Merge block should be at max(1, 1) + 1 = 2");
    }
}
