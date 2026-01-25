// Tests for BlockDAG tip selection functions.
//
// Covers:
// - `find_best_tip_by_cumulative_difficulty` (selects tip with highest cumulative difficulty)
// - `find_newest_tip_by_timestamp` (selects tip with newest timestamp)
// - `calculate_height_at_tips` (calculates height as max(tip heights) + 1)

#[cfg(test)]
mod tests {
    use super::super::{make_hash, DagBuilder, MockDagProvider};
    use tos_common::crypto::Hash;
    use tos_daemon::core::blockdag::{
        calculate_height_at_tips, find_best_tip_by_cumulative_difficulty,
        find_newest_tip_by_timestamp,
    };

    // =========================================================================
    // Tests for find_best_tip_by_cumulative_difficulty
    // =========================================================================

    #[tokio::test]
    async fn test_single_tip() {
        // Single tip should return that tip regardless of difficulty
        let hash_a = make_hash(1);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 0, 100, 100, 1000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone()];
        let result = find_best_tip_by_cumulative_difficulty(&provider, tips.iter()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), &hash_a);
    }

    #[tokio::test]
    async fn test_two_tips_different_difficulty() {
        // Higher cumulative difficulty wins
        let hash_a = make_hash(1);
        let hash_b = make_hash(2);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 1, 50, 150, 1000)
            .add_block(hash_b.clone(), 1, 100, 300, 2000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone()];
        let result = find_best_tip_by_cumulative_difficulty(&provider, tips.iter()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), &hash_b);
    }

    #[tokio::test]
    async fn test_three_tips_different_difficulty() {
        // Highest of three cumulative difficulties wins
        let hash_a = make_hash(1);
        let hash_b = make_hash(2);
        let hash_c = make_hash(3);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 1, 50, 100, 1000)
            .add_block(hash_b.clone(), 1, 80, 500, 2000)
            .add_block(hash_c.clone(), 1, 70, 300, 3000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone(), hash_c.clone()];
        let result = find_best_tip_by_cumulative_difficulty(&provider, tips.iter()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), &hash_b);
    }

    #[tokio::test]
    async fn test_two_tips_same_difficulty() {
        // When cumulative difficulty is tied, the function selects the first
        // tip encountered (since it only updates on strictly greater).
        // This means the result is deterministic based on iteration order.
        let hash_a = make_hash(1);
        let hash_b = make_hash(2);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 1, 100, 200, 1000)
            .add_block(hash_b.clone(), 1, 100, 200, 2000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone()];
        let result = find_best_tip_by_cumulative_difficulty(&provider, tips.iter()).await;
        assert!(result.is_ok());
        // First tip with strictly greater difficulty wins.
        // Since both are equal, the first one encountered (hash_a) is selected.
        assert_eq!(result.unwrap(), &hash_a);
    }

    #[tokio::test]
    async fn test_empty_tips_returns_error() {
        // Empty iterator should return ExpectedTips error
        let provider = MockDagProvider::new();
        let tips: Vec<Hash> = vec![];
        let result = find_best_tip_by_cumulative_difficulty(&provider, tips.iter()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_large_difficulty_values() {
        // Test with u64::MAX-scale difficulties to verify no overflow
        let hash_a = make_hash(1);
        let hash_b = make_hash(2);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 5, 1000, u64::MAX - 1, 50000)
            .add_block(hash_b.clone(), 5, 1000, u64::MAX, 60000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone()];
        let result = find_best_tip_by_cumulative_difficulty(&provider, tips.iter()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), &hash_b);
    }

    #[tokio::test]
    async fn test_zero_difficulty_non_zero() {
        // Zero vs non-zero cumulative difficulty: non-zero wins
        let hash_a = make_hash(1);
        let hash_b = make_hash(2);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 0, 0, 0, 1000)
            .add_block(hash_b.clone(), 0, 50, 50, 2000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone()];
        let result = find_best_tip_by_cumulative_difficulty(&provider, tips.iter()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), &hash_b);
    }

    #[tokio::test]
    async fn test_all_same_difficulty() {
        // All tips have the same cumulative difficulty.
        // The first tip is selected since none is strictly greater.
        let hash_a = make_hash(1);
        let hash_b = make_hash(2);
        let hash_c = make_hash(3);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 2, 100, 500, 1000)
            .add_block(hash_b.clone(), 2, 100, 500, 2000)
            .add_block(hash_c.clone(), 2, 100, 500, 3000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone(), hash_c.clone()];
        let result = find_best_tip_by_cumulative_difficulty(&provider, tips.iter()).await;
        assert!(result.is_ok());
        // All have the same difficulty. The first encountered is selected.
        assert_eq!(result.unwrap(), &hash_a);
    }

    // =========================================================================
    // Tests for find_newest_tip_by_timestamp
    // =========================================================================

    #[tokio::test]
    async fn test_newest_single_tip() {
        // Single tip returns that tip along with its timestamp
        let hash_a = make_hash(10);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 0, 100, 100, 5000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone()];
        let result = find_newest_tip_by_timestamp(&provider, tips.iter()).await;
        assert!(result.is_ok());
        let (tip, timestamp) = result.unwrap();
        assert_eq!(tip, &hash_a);
        assert_eq!(timestamp, 5000);
    }

    #[tokio::test]
    async fn test_newest_two_tips() {
        // Newer timestamp wins
        let hash_a = make_hash(10);
        let hash_b = make_hash(20);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 1, 100, 200, 1000)
            .add_block(hash_b.clone(), 1, 100, 200, 3000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone()];
        let result = find_newest_tip_by_timestamp(&provider, tips.iter()).await;
        assert!(result.is_ok());
        let (tip, timestamp) = result.unwrap();
        assert_eq!(tip, &hash_b);
        assert_eq!(timestamp, 3000);
    }

    #[tokio::test]
    async fn test_newest_three_tips() {
        // Newest of three timestamps wins
        let hash_a = make_hash(10);
        let hash_b = make_hash(20);
        let hash_c = make_hash(30);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 2, 100, 300, 1000)
            .add_block(hash_b.clone(), 2, 100, 300, 5000)
            .add_block(hash_c.clone(), 2, 100, 300, 3000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone(), hash_c.clone()];
        let result = find_newest_tip_by_timestamp(&provider, tips.iter()).await;
        assert!(result.is_ok());
        let (tip, timestamp) = result.unwrap();
        assert_eq!(tip, &hash_b);
        assert_eq!(timestamp, 5000);
    }

    #[tokio::test]
    async fn test_newest_same_timestamp() {
        // Same timestamp on all tips: first encountered wins (strictly greater check)
        let hash_a = make_hash(10);
        let hash_b = make_hash(20);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 1, 100, 200, 4000)
            .add_block(hash_b.clone(), 1, 100, 200, 4000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone()];
        let result = find_newest_tip_by_timestamp(&provider, tips.iter()).await;
        assert!(result.is_ok());
        let (tip, timestamp) = result.unwrap();
        // Both have timestamp 4000. The first encountered is selected.
        assert_eq!(tip, &hash_a);
        assert_eq!(timestamp, 4000);
    }

    #[tokio::test]
    async fn test_newest_empty_returns_error() {
        // Empty iterator should return ExpectedTips error
        let provider = MockDagProvider::new();
        let tips: Vec<Hash> = vec![];
        let result = find_newest_tip_by_timestamp(&provider, tips.iter()).await;
        assert!(result.is_err());
    }

    // =========================================================================
    // Tests for calculate_height_at_tips
    // =========================================================================

    #[tokio::test]
    async fn test_height_single_tip() {
        // Single tip at height 5: result should be 5 + 1 = 6
        let hash_a = make_hash(1);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 5, 100, 500, 1000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone()];
        let result = calculate_height_at_tips(&provider, tips.iter()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 6);
    }

    #[tokio::test]
    async fn test_height_two_tips_same_height() {
        // Two tips at the same height 3: result should be 3 + 1 = 4
        let hash_a = make_hash(1);
        let hash_b = make_hash(2);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 3, 100, 300, 1000)
            .add_block(hash_b.clone(), 3, 100, 300, 2000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone()];
        let result = calculate_height_at_tips(&provider, tips.iter()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 4);
    }

    #[tokio::test]
    async fn test_height_two_tips_different_height() {
        // Two tips at different heights (2 and 7): result should be max(2, 7) + 1 = 8
        let hash_a = make_hash(1);
        let hash_b = make_hash(2);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 2, 100, 200, 1000)
            .add_block(hash_b.clone(), 7, 100, 700, 2000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone()];
        let result = calculate_height_at_tips(&provider, tips.iter()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 8);
    }

    #[tokio::test]
    async fn test_height_three_tips_mixed() {
        // Three tips at heights 1, 10, 5: picks highest (10) + 1 = 11
        let hash_a = make_hash(1);
        let hash_b = make_hash(2);
        let hash_c = make_hash(3);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 1, 50, 50, 1000)
            .add_block(hash_b.clone(), 10, 100, 1000, 2000)
            .add_block(hash_c.clone(), 5, 80, 400, 3000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone(), hash_b.clone(), hash_c.clone()];
        let result = calculate_height_at_tips(&provider, tips.iter()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 11);
    }

    #[tokio::test]
    async fn test_height_empty_tips() {
        // Empty tips: returns 0 (no +1 since tips_len == 0)
        let provider = MockDagProvider::new();
        let tips: Vec<Hash> = vec![];
        let result = calculate_height_at_tips(&provider, tips.iter()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_height_at_zero() {
        // Tips at height 0: result should be 0 + 1 = 1
        let hash_a = make_hash(1);
        let provider = DagBuilder::new()
            .add_block(hash_a.clone(), 0, 100, 100, 1000)
            .build();

        let tips: Vec<Hash> = vec![hash_a.clone()];
        let result = calculate_height_at_tips(&provider, tips.iter()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
    }
}
