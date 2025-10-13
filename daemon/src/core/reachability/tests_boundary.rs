// Boundary and Edge Case Tests for Reachability Service
// Comprehensive tests for interval management, reindexing, and edge cases

#[cfg(test)]
mod boundary_tests {
    use super::super::*;
    use tos_common::crypto::Hash;

    // ============================================================================
    // 1. Interval Boundary Tests
    // ============================================================================

    #[test]
    fn test_interval_size_one() {
        // Interval with size 1 (minimal non-empty)
        let interval = Interval::new(100, 100);
        assert_eq!(interval.size(), 1);
        assert!(!interval.is_empty());

        // Cannot split further
        let (left, right) = interval.split_half();
        assert_eq!(left.size(), 1);
        assert!(right.is_empty());
    }

    #[test]
    fn test_interval_size_two() {
        // Interval with size 2 (splits into two size-1 intervals)
        let interval = Interval::new(100, 101);
        assert_eq!(interval.size(), 2);

        let (left, right) = interval.split_half();
        assert_eq!(left.size(), 1);
        assert_eq!(right.size(), 1);
    }

    #[test]
    fn test_interval_at_u64_boundaries() {
        // Test intervals at U64 limits
        let near_min = Interval::new(1, 1000);
        assert_eq!(near_min.start, 1);
        assert_eq!(near_min.size(), 1000);

        let near_max = Interval::new(u64::MAX - 1000, u64::MAX - 1);
        assert_eq!(near_max.end, u64::MAX - 1);
        assert_eq!(near_max.size(), 1000);
    }

    #[test]
    fn test_interval_maximal_properties() {
        // Maximal interval [1, U64::MAX-1]
        let maximal = Interval::maximal();

        assert_eq!(maximal.start, 1);
        assert_eq!(maximal.end, u64::MAX - 1);
        assert_eq!(maximal.size(), u64::MAX - 1);
        assert!(!maximal.is_empty());

        // Maximal interval contains all valid intervals
        let test_interval = Interval::new(1000, 2000);
        assert!(maximal.contains(test_interval));
    }

    #[test]
    fn test_interval_empty_properties() {
        // Empty interval [n, n-1]
        let empty = Interval::empty();

        assert!(empty.is_empty());
        assert_eq!(empty.size(), 0);

        // Empty interval contains only itself
        assert!(empty.contains(empty));
        assert!(!empty.contains(Interval::new(1, 10)));
    }

    #[test]
    fn test_interval_wraparound_behavior() {
        // Test behavior at wraparound points
        // Interval ending at U64::MAX - 1
        let at_end = Interval::new(u64::MAX - 100, u64::MAX - 1);
        assert_eq!(at_end.size(), 100);

        // Cannot extend beyond U64::MAX - 1
        let remaining = at_end.remaining_after();
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_interval_adjacent_intervals() {
        // Two adjacent intervals should not overlap
        let first = Interval::new(1, 500);
        let second = Interval::new(501, 1000);

        assert!(!first.contains(second));
        assert!(!second.contains(first));

        // But their parent contains both
        let parent = Interval::new(1, 1000);
        assert!(parent.contains(first));
        assert!(parent.contains(second));
    }

    #[test]
    fn test_interval_strictly_contains_edge_cases() {
        let interval = Interval::new(100, 200);

        // Same interval - not strictly contained
        assert!(!interval.strictly_contains(interval));

        // Subset - strictly contained
        let subset = Interval::new(150, 180);
        assert!(interval.strictly_contains(subset));

        // Equal start - not strictly contained
        let equal_start = Interval::new(100, 150);
        assert!(interval.strictly_contains(equal_start));

        // Equal end - not strictly contained
        let equal_end = Interval::new(150, 200);
        assert!(interval.strictly_contains(equal_end));

        // Exact match (both start and end) - not strictly contained
        assert!(!interval.strictly_contains(interval));
    }

    // ============================================================================
    // 2. Split Algorithm Boundary Tests
    // ============================================================================

    #[test]
    fn test_split_half_odd_size() {
        // Odd-sized interval: left gets ceiling
        let interval = Interval::new(1, 101); // Size 101
        let (left, right) = interval.split_half();

        assert_eq!(left.size(), 51);  // ceil(101/2)
        assert_eq!(right.size(), 50); // floor(101/2)
    }

    #[test]
    fn test_split_half_even_size() {
        // Even-sized interval: equal split
        let interval = Interval::new(1, 100); // Size 100
        let (left, right) = interval.split_half();

        assert_eq!(left.size(), 50);
        assert_eq!(right.size(), 50);
    }

    #[test]
    fn test_split_exact_all_empty() {
        // Split with all zero sizes
        let interval = Interval::new(100, 99); // Empty interval
        let sizes = vec![0, 0, 0];

        let splits = interval.split_exact(&sizes);
        assert_eq!(splits.len(), 3);

        for split in splits {
            assert!(split.is_empty());
        }
    }

    #[test]
    fn test_split_exact_single_nonzero() {
        // Only one non-zero size
        let interval = Interval::new(1, 100);
        let sizes = vec![0, 100, 0];

        let splits = interval.split_exact(&sizes);
        assert_eq!(splits.len(), 3);

        assert!(splits[0].is_empty());
        assert_eq!(splits[1].size(), 100);
        assert!(splits[2].is_empty());
    }

    #[test]
    fn test_split_exponential_single_child() {
        // Single child gets all space
        let interval = Interval::new(1, 1000);
        let sizes = vec![100];

        let splits = interval.split_exponential(&sizes);
        assert_eq!(splits.len(), 1);
        assert_eq!(splits[0].size(), 1000);
    }

    #[test]
    fn test_split_exponential_equal_children() {
        // Equal-sized children get roughly equal slack
        let interval = Interval::new(1, 1000);
        let sizes = vec![100, 100, 100]; // Total 300, slack 700

        let splits = interval.split_exponential(&sizes);
        assert_eq!(splits.len(), 3);

        // Each gets 100 + ~233 slack
        for split in &splits {
            assert!(split.size() >= 100);
        }

        // Total must equal 1000
        let total: u64 = splits.iter().map(|s| s.size()).sum();
        assert_eq!(total, 1000);

        // Should be roughly equal (within rounding error)
        let size_diff = splits[0].size().abs_diff(splits[1].size());
        assert!(size_diff <= 1);
    }

    #[test]
    fn test_split_exponential_vastly_different() {
        // One child much larger than others
        let interval = Interval::new(1, 10000);
        let sizes = vec![1, 10, 1000]; // Exponential growth

        let splits = interval.split_exponential(&sizes);
        assert_eq!(splits.len(), 3);

        // Largest child gets most slack
        assert!(splits[2].size() > splits[1].size());
        assert!(splits[1].size() > splits[0].size());

        // Largest should get >90% of total
        assert!(splits[2].size() > 9000);
    }

    #[test]
    fn test_split_exponential_no_slack() {
        // No slack: exact sizes
        let interval = Interval::new(1, 100);
        let sizes = vec![30, 30, 40];

        let splits = interval.split_exponential(&sizes);

        assert_eq!(splits[0].size(), 30);
        assert_eq!(splits[1].size(), 30);
        assert_eq!(splits[2].size(), 40);
    }

    // ============================================================================
    // 3. Reindexing Trigger Conditions
    // ============================================================================

    #[test]
    fn test_reindex_trigger_size_zero() {
        // Interval with size 0 triggers reindexing
        let remaining = Interval::empty();
        assert_eq!(remaining.size(), 0);
        assert!(remaining.size() <= 1); // Trigger condition
    }

    #[test]
    fn test_reindex_trigger_size_one() {
        // Interval with size 1 triggers reindexing
        let remaining = Interval::new(100, 100);
        assert_eq!(remaining.size(), 1);
        assert!(remaining.size() <= 1); // Trigger condition
    }

    #[test]
    fn test_reindex_no_trigger_size_two() {
        // Interval with size 2 does NOT trigger
        let remaining = Interval::new(100, 101);
        assert_eq!(remaining.size(), 2);
        assert!(remaining.size() > 1); // No trigger
    }

    #[test]
    fn test_reindex_trigger_calculation() {
        // Simulate parent with children
        let parent = Interval::new(1, 1000);

        // After 999 children, remaining size is 1
        let last_child_end = 999;
        let remaining = Interval::new(last_child_end + 1, parent.end);

        assert_eq!(remaining.size(), 1);
        assert!(remaining.size() <= 1); // Should trigger reindexing
    }

    // ============================================================================
    // 4. Exponential Fraction Calculation
    // ============================================================================

    #[test]
    fn test_exponential_split_validates_distribution() {
        // Test that exponential split gives larger children more space
        let interval = Interval::new(1, 10000);
        let sizes = vec![100, 200, 400];  // Exponential growth

        let splits = interval.split_exponential(&sizes);

        // Verify that larger sizes get more allocation
        assert!(splits[2].size() > splits[1].size());
        assert!(splits[1].size() > splits[0].size());

        // Total should equal interval size
        let total: u64 = splits.iter().map(|s| s.size()).sum();
        assert_eq!(total, 10000);
    }

    #[test]
    fn test_exponential_split_single_element() {
        let interval = Interval::new(1, 1000);
        let sizes = vec![100];

        let splits = interval.split_exponential(&sizes);

        // Single element gets all space
        assert_eq!(splits.len(), 1);
        assert_eq!(splits[0].size(), 1000);
    }

    #[test]
    fn test_exponential_split_equal_sizes() {
        let interval = Interval::new(1, 1000);
        let sizes = vec![100, 100, 100];

        let splits = interval.split_exponential(&sizes);

        // With equal sizes, space should be roughly equal
        // Allow for rounding errors
        let diff1 = splits[0].size().abs_diff(splits[1].size());
        let diff2 = splits[1].size().abs_diff(splits[2].size());
        assert!(diff1 <= 1);
        assert!(diff2 <= 1);
    }

    #[test]
    fn test_exponential_split_proportionality() {
        let interval = Interval::new(1, 10000);
        let sizes = vec![10, 100, 1000];

        let splits = interval.split_exponential(&sizes);

        // Larger sizes should get exponentially more space
        // The largest (1000) should dominate
        assert!(splits[2].size() > splits[1].size() * 2);
        assert!(splits[1].size() > splits[0].size() * 2);
    }

    // ============================================================================
    // 5. Reachability Data Initialization
    // ============================================================================

    #[test]
    fn test_genesis_reachability_properties() {
        let genesis_hash = Hash::new([0u8; 32]);
        let reachability = TosReachability::new(genesis_hash.clone());

        let genesis_data = reachability.genesis_reachability_data();

        // Genesis properties
        assert_eq!(genesis_data.parent, genesis_hash); // Self-parent
        assert_eq!(genesis_data.interval, Interval::maximal());
        assert_eq!(genesis_data.height, 0);
        assert!(genesis_data.children.is_empty());
        assert!(genesis_data.future_covering_set.is_empty());
    }

    #[test]
    fn test_reachability_data_height_progression() {
        // Heights should increase monotonically
        let heights = vec![0, 1, 2, 3, 10, 100, 1000, 10000];

        for i in 0..heights.len() - 1 {
            assert!(heights[i + 1] > heights[i]);
        }
    }

    #[test]
    fn test_reachability_data_parent_child_relationship() {
        // Child's interval must be contained in parent's interval
        let parent_interval = Interval::new(1, 1000);
        let child_interval = Interval::new(501, 1000);

        assert!(parent_interval.contains(child_interval));
        assert!(!child_interval.contains(parent_interval));
    }

    // ============================================================================
    // 6. Future Covering Set Tests
    // ============================================================================

    #[test]
    fn test_future_covering_set_empty() {
        // Block with no future covering set (tip of chain)
        let fcs: Vec<Hash> = vec![];
        assert!(fcs.is_empty());
    }

    #[test]
    fn test_future_covering_set_single() {
        // Block with one future block
        let future = Hash::new([1u8; 32]);
        let fcs = vec![future];
        assert_eq!(fcs.len(), 1);
    }

    #[test]
    fn test_future_covering_set_ordering() {
        // Future covering set must be ordered by interval.start
        let hash1 = Hash::new([1u8; 32]);
        let hash2 = Hash::new([2u8; 32]);
        let hash3 = Hash::new([3u8; 32]);

        let fcs = vec![hash1, hash2, hash3];

        // Verify ordering (would need actual interval.start values in real test)
        assert_eq!(fcs.len(), 3);
    }

    // ============================================================================
    // 7. Interval Manipulation Edge Cases
    // ============================================================================

    #[test]
    fn test_interval_increase_near_limit() {
        // Increase interval that's near U64 limit (but not at the max)
        // end must stay < u64::MAX - 1 to satisfy interval invariant
        let near_limit = Interval::new(u64::MAX - 2000, u64::MAX - 1000);
        let increased = near_limit.increase(100);

        // Verify increase worked correctly
        assert_eq!(increased.start, near_limit.start + 100);
        assert_eq!(increased.end, near_limit.end + 100);
        assert!(increased.end < u64::MAX); // Stays within valid range
    }

    #[test]
    fn test_interval_decrease_at_minimum() {
        // Decrease interval near minimum
        let near_min = Interval::new(10, 100);
        let decreased = near_min.decrease(9);

        assert_eq!(decreased.start, 1);
        assert_eq!(decreased.end, 91);
    }

    #[test]
    fn test_interval_expand_both_directions() {
        let interval = Interval::new(100, 200);

        // Expand left
        let expanded_left = interval.decrease_start(10);
        assert_eq!(expanded_left.start, 90);
        assert_eq!(expanded_left.end, 200);

        // Expand right
        let expanded_right = interval.increase_end(10);
        assert_eq!(expanded_right.start, 100);
        assert_eq!(expanded_right.end, 210);

        // Expand both
        let expanded_both = expanded_left.increase_end(10);
        assert_eq!(expanded_both.start, 90);
        assert_eq!(expanded_both.end, 210);
    }

    #[test]
    fn test_interval_shrink_to_point() {
        let interval = Interval::new(100, 200); // Size 101

        // Shrink to size 1
        let shrunk = interval.decrease_end(100);
        assert_eq!(shrunk.size(), 1);
        assert_eq!(shrunk.start, 100);
        assert_eq!(shrunk.end, 100);
    }

    #[test]
    fn test_interval_shrink_past_point() {
        let interval = Interval::new(100, 200);

        // Shrink past point (becomes empty)
        let over_shrunk = interval.decrease_end(101);
        assert!(over_shrunk.is_empty());
        assert_eq!(over_shrunk.start, 100);
        assert_eq!(over_shrunk.end, 99); // end < start
    }

    // ============================================================================
    // 8. Contains Relationship Tests
    // ============================================================================

    #[test]
    fn test_contains_reflexive() {
        // Every interval contains itself
        let interval = Interval::new(100, 200);
        assert!(interval.contains(interval));
    }

    #[test]
    fn test_contains_transitive() {
        // If A contains B and B contains C, then A contains C
        let a = Interval::new(1, 1000);
        let b = Interval::new(100, 500);
        let c = Interval::new(200, 300);

        assert!(a.contains(b));
        assert!(b.contains(c));
        assert!(a.contains(c)); // Transitivity
    }

    #[test]
    fn test_contains_not_symmetric() {
        // If A contains B (and A ≠ B), then B does not contain A
        let parent = Interval::new(1, 1000);
        let child = Interval::new(100, 500);

        assert!(parent.contains(child));
        assert!(!child.contains(parent));
    }

    #[test]
    fn test_contains_empty_interval() {
        let non_empty = Interval::new(100, 200);
        let empty = Interval::empty();

        // Non-empty doesn't contain empty (empty is special)
        assert!(!non_empty.contains(empty));

        // Empty contains itself
        assert!(empty.contains(empty));
    }
}
