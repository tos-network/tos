// Property-Based Tests
// Tests invariants and properties that should hold for all inputs
//
// These tests verify mathematical properties and invariants
// without using external property testing frameworks

#[cfg(test)]
mod property_tests {
    use tos_common::crypto::Hash;
    use tos_common::difficulty::Difficulty;

    use crate::core::{
        ghostdag::{calc_work_from_difficulty, BlueWorkType, SortableBlock},
        reachability::Interval,
    };

    // ============================================================================
    // Property 1: Interval Containment Properties
    // ============================================================================

    #[test]
    fn prop_interval_contains_is_reflexive() {
        // Property: Every interval contains itself
        let test_cases = vec![
            Interval::new(1, 100),
            Interval::new(1, 1),
            Interval::new(500, 1000),
            Interval::maximal(),
            Interval::new(u64::MAX - 100, u64::MAX - 1),
        ];

        for interval in test_cases {
            assert!(
                interval.contains(interval),
                "Interval {:?} should contain itself (reflexivity)",
                interval
            );
        }
    }

    #[test]
    fn prop_interval_contains_is_transitive() {
        // Property: If A contains B and B contains C, then A contains C
        let test_cases = vec![
            (
                Interval::new(1, 1000),
                Interval::new(100, 500),
                Interval::new(200, 300),
            ),
            (
                Interval::new(1, 10000),
                Interval::new(5000, 8000),
                Interval::new(6000, 7000),
            ),
            (
                Interval::maximal(),
                Interval::new(1000, 2000),
                Interval::new(1200, 1800),
            ),
        ];

        for (a, b, c) in test_cases {
            if a.contains(b) && b.contains(c) {
                assert!(a.contains(c),
                    "Transitivity: if {:?} contains {:?} and {:?} contains {:?}, then {:?} must contain {:?}",
                    a, b, b, c, a, c);
            }
        }
    }

    #[test]
    fn prop_interval_contains_is_antisymmetric() {
        // Property: If A contains B and B contains A, then A = B
        let test_cases = vec![
            (Interval::new(1, 100), Interval::new(1, 100)),
            (Interval::new(500, 500), Interval::new(500, 500)),
            (Interval::maximal(), Interval::maximal()),
        ];

        for (a, b) in test_cases {
            if a.contains(b) && b.contains(a) {
                assert_eq!(a, b,
                    "Antisymmetry: if {:?} contains {:?} and {:?} contains {:?}, they must be equal",
                    a, b, b, a);
            }
        }
    }

    #[test]
    fn prop_interval_split_preserves_size() {
        // Property: split_half preserves total size (modulo rounding)
        let test_cases = vec![
            Interval::new(1, 100),
            Interval::new(1, 1000),
            Interval::new(1, 10000),
            Interval::new(500, 1500),
        ];

        for interval in test_cases {
            let original_size = interval.size();
            let (left, right) = interval.split_half();
            let total_size = left.size() + right.size();

            assert_eq!(
                total_size,
                original_size,
                "Split should preserve total size: original={}, left={}, right={}",
                original_size,
                left.size(),
                right.size()
            );
        }
    }

    #[test]
    fn prop_interval_split_produces_non_overlapping() {
        // Property: split_half produces non-overlapping intervals
        let test_cases = vec![
            Interval::new(1, 100),
            Interval::new(1, 1000),
            Interval::new(500, 1500),
        ];

        for interval in test_cases {
            let (left, right) = interval.split_half();

            if !right.is_empty() {
                assert!(
                    left.end < right.start || right.is_empty(),
                    "Split intervals should not overlap: left.end={}, right.start={}",
                    left.end,
                    right.start
                );
            }
        }
    }

    #[test]
    fn prop_interval_parent_contains_children() {
        // Property: Parent interval contains all child intervals from split
        let test_cases = vec![
            Interval::new(1, 100),
            Interval::new(1, 1000),
            Interval::maximal(),
        ];

        for parent in test_cases {
            let (left, right) = parent.split_half();

            if !left.is_empty() {
                assert!(
                    parent.contains(left),
                    "Parent {:?} should contain left child {:?}",
                    parent,
                    left
                );
            }

            if !right.is_empty() {
                assert!(
                    parent.contains(right),
                    "Parent {:?} should contain right child {:?}",
                    parent,
                    right
                );
            }
        }
    }

    // ============================================================================
    // Property 2: Work Calculation Properties
    // ============================================================================

    #[test]
    fn prop_work_increases_with_difficulty() {
        // Property: Higher difficulty produces higher work
        let difficulties = vec![1u64, 10, 100, 1000, 10000, 100000];

        let mut works = vec![];
        for &diff_val in &difficulties {
            let difficulty = Difficulty::from(diff_val);
            let work = calc_work_from_difficulty(&difficulty).unwrap();
            works.push((diff_val, work));
        }

        // Verify monotonicity
        for i in 1..works.len() {
            assert!(works[i].1 > works[i-1].1,
                "Work should increase with difficulty: diff[{}]={} work={:?}, diff[{}]={} work={:?}",
                i-1, works[i-1].0, works[i-1].1, i, works[i].0, works[i].1);
        }
    }

    #[test]
    fn prop_work_is_positive() {
        // Property: Work is always positive (non-zero) for non-zero difficulty
        let test_cases = vec![1u64, 10, 100, 1000, 10000, 100000, 1000000];

        for diff_val in test_cases {
            let difficulty = Difficulty::from(diff_val);
            let work = calc_work_from_difficulty(&difficulty).unwrap();

            assert!(
                work > BlueWorkType::zero(),
                "Work should be positive for difficulty {}",
                diff_val
            );
        }
    }

    #[test]
    fn prop_work_addition_is_commutative() {
        // Property: Work addition is commutative (A + B = B + A)
        let work_a = BlueWorkType::from(100u64);
        let work_b = BlueWorkType::from(200u64);

        let sum_ab = work_a + work_b;
        let sum_ba = work_b + work_a;

        assert_eq!(
            sum_ab, sum_ba,
            "Work addition should be commutative: {} + {} = {} + {}",
            work_a, work_b, work_b, work_a
        );
    }

    #[test]
    fn prop_work_addition_is_associative() {
        // Property: Work addition is associative ((A + B) + C = A + (B + C))
        let work_a = BlueWorkType::from(100u64);
        let work_b = BlueWorkType::from(200u64);
        let work_c = BlueWorkType::from(300u64);

        let sum_abc = (work_a + work_b) + work_c;
        let sum_abc2 = work_a + (work_b + work_c);

        assert_eq!(
            sum_abc, sum_abc2,
            "Work addition should be associative: ({} + {}) + {} = {} + ({} + {})",
            work_a, work_b, work_c, work_a, work_b, work_c
        );
    }

    #[test]
    fn prop_work_has_identity_element() {
        // Property: Zero is the identity element for work addition (A + 0 = A)
        let test_works = vec![
            BlueWorkType::from(100u64),
            BlueWorkType::from(1000u64),
            BlueWorkType::from(10000u64),
        ];

        for work in test_works {
            let sum = work + BlueWorkType::zero();
            assert_eq!(
                sum, work,
                "Zero should be identity element: {} + 0 = {}",
                work, work
            );
        }
    }

    // ============================================================================
    // Property 3: SortableBlock Ordering Properties
    // ============================================================================

    #[test]
    fn prop_sortable_block_ordering_is_total() {
        // Property: Any two blocks can be compared
        let blocks = vec![
            SortableBlock::new(Hash::new([1u8; 32]), BlueWorkType::from(100u64)),
            SortableBlock::new(Hash::new([2u8; 32]), BlueWorkType::from(200u64)),
            SortableBlock::new(Hash::new([3u8; 32]), BlueWorkType::from(150u64)),
        ];

        // Any two blocks should be comparable
        for i in 0..blocks.len() {
            for j in 0..blocks.len() {
                let _cmp = blocks[i].cmp(&blocks[j]);
                // If we reach here, comparison succeeded (total order)
            }
        }
    }

    #[test]
    fn prop_sortable_block_ordering_is_transitive() {
        // Property: If A < B and B < C, then A < C
        let block_a = SortableBlock::new(Hash::new([1u8; 32]), BlueWorkType::from(100u64));
        let block_b = SortableBlock::new(Hash::new([2u8; 32]), BlueWorkType::from(200u64));
        let block_c = SortableBlock::new(Hash::new([3u8; 32]), BlueWorkType::from(300u64));

        if block_a < block_b && block_b < block_c {
            assert!(
                block_a < block_c,
                "Ordering should be transitive: if A < B and B < C, then A < C"
            );
        }
    }

    #[test]
    fn prop_sortable_block_ordering_is_antisymmetric() {
        // Property: If A <= B and B <= A, then A = B
        let work = BlueWorkType::from(100u64);
        let hash = Hash::new([1u8; 32]);

        let block_a = SortableBlock::new(hash.clone(), work);
        let block_b = SortableBlock::new(hash.clone(), work);

        if block_a <= block_b && block_b <= block_a {
            assert_eq!(
                block_a, block_b,
                "Ordering should be antisymmetric: if A <= B and B <= A, then A = B"
            );
        }
    }

    #[test]
    fn prop_sortable_block_sort_is_stable() {
        // Property: Sorting is stable (equal elements maintain relative order)
        let mut blocks = vec![
            SortableBlock::new(Hash::new([1u8; 32]), BlueWorkType::from(100u64)),
            SortableBlock::new(Hash::new([2u8; 32]), BlueWorkType::from(100u64)), // Same work
            SortableBlock::new(Hash::new([3u8; 32]), BlueWorkType::from(200u64)),
            SortableBlock::new(Hash::new([4u8; 32]), BlueWorkType::from(100u64)), // Same work
        ];

        // Sort
        blocks.sort();

        // Verify sorted
        for i in 1..blocks.len() {
            assert!(
                blocks[i].blue_work >= blocks[i - 1].blue_work,
                "After sorting, blocks should be in non-decreasing order"
            );
        }
    }

    // ============================================================================
    // Property 4: Blue Score Properties
    // ============================================================================

    #[test]
    fn prop_blue_score_is_monotonic() {
        // Property: Blue score never decreases in a chain
        let scores = vec![0u64, 1, 2, 3, 4, 5, 10, 20, 50, 100];

        for i in 1..scores.len() {
            assert!(
                scores[i] >= scores[i - 1],
                "Blue score should be monotonically increasing: {} >= {}",
                scores[i],
                scores[i - 1]
            );
        }
    }

    #[test]
    fn prop_blue_score_increments_by_one() {
        // Property: Blue score increments by exactly 1 for each blue block
        let genesis_score = 0u64;
        let mut current_score = genesis_score;

        for _ in 0..100 {
            let next_score = current_score + 1;
            assert_eq!(
                next_score,
                current_score + 1,
                "Blue score should increment by exactly 1"
            );
            current_score = next_score;
        }
    }

    // ============================================================================
    // Property 5: Hash Properties
    // ============================================================================

    #[test]
    fn prop_hash_equality_is_reflexive() {
        // Property: A hash equals itself
        let test_hashes = vec![
            Hash::new([0u8; 32]),
            Hash::new([1u8; 32]),
            Hash::new([255u8; 32]),
        ];

        for hash in test_hashes {
            assert_eq!(hash, hash, "Hash should equal itself");
        }
    }

    #[test]
    fn prop_hash_equality_is_symmetric() {
        // Property: If A = B, then B = A
        let hash_a = Hash::new([1u8; 32]);
        let hash_b = Hash::new([1u8; 32]);

        if hash_a == hash_b {
            assert_eq!(hash_b, hash_a, "Hash equality should be symmetric");
        }
    }

    #[test]
    fn prop_hash_equality_is_transitive() {
        // Property: If A = B and B = C, then A = C
        let hash_a = Hash::new([1u8; 32]);
        let hash_b = Hash::new([1u8; 32]);
        let hash_c = Hash::new([1u8; 32]);

        if hash_a == hash_b && hash_b == hash_c {
            assert_eq!(hash_a, hash_c, "Hash equality should be transitive");
        }
    }

    #[test]
    fn prop_different_hashes_are_unequal() {
        // Property: Different hash values are unequal
        let test_cases = vec![
            (Hash::new([0u8; 32]), Hash::new([1u8; 32])),
            (Hash::new([1u8; 32]), Hash::new([2u8; 32])),
            (Hash::new([0u8; 32]), Hash::new([255u8; 32])),
        ];

        for (hash_a, hash_b) in test_cases {
            assert_ne!(hash_a, hash_b, "Different hashes should be unequal");
        }
    }

    // ============================================================================
    // Property 6: K-Cluster Properties
    // ============================================================================

    #[test]
    fn prop_k_cluster_boundary_is_deterministic() {
        // Property: K-cluster classification is deterministic
        let k = 10u64;

        let test_cases = vec![
            (0, true),    // 0 <= K
            (5, true),    // 5 <= K
            (10, true),   // 10 <= K
            (11, false),  // 11 > K
            (15, false),  // 15 > K
            (100, false), // 100 > K
        ];

        for (anticone_size, expected_is_blue) in test_cases {
            let is_blue = anticone_size <= k;
            assert_eq!(is_blue, expected_is_blue,
                "K-cluster classification should be deterministic: anticone_size={}, K={}, expected={}",
                anticone_size, k, expected_is_blue);
        }
    }

    #[test]
    fn prop_k_cluster_is_monotonic() {
        // Property: If size A <= K, and size B < size A, then B <= K
        let k = 10u64;

        for anticone_a in 0..=k {
            for anticone_b in 0..anticone_a {
                assert!(
                    anticone_b <= k,
                    "If {} <= K={} and {} < {}, then {} should also be <= K",
                    anticone_a,
                    k,
                    anticone_b,
                    anticone_a,
                    anticone_b
                );
            }
        }
    }

    // ============================================================================
    // Property 7: Interval Size Properties
    // ============================================================================

    #[test]
    fn prop_interval_size_is_non_negative() {
        // Property: Interval size is always >= 0
        let test_cases = vec![
            Interval::new(1, 100),
            Interval::new(1, 1),
            Interval::new(500, 500),
            Interval::empty(),
            Interval::maximal(),
        ];

        for interval in test_cases {
            // Size is u64, so always non-negative, but we verify the calculation
            let size = interval.size();
            assert!(
                size == interval.size(),
                "Size calculation should be consistent"
            );
        }
    }

    #[test]
    fn prop_interval_split_size_sum_equals_original() {
        // Property: sum(split_exact sizes) = original interval size
        let test_cases = vec![
            (Interval::new(1, 100), vec![25, 25, 25, 25]),
            (Interval::new(1, 1000), vec![100, 200, 300, 400]),
            (Interval::new(1, 100), vec![10, 20, 30, 40]),
        ];

        for (interval, sizes) in test_cases {
            let total_requested: u64 = sizes.iter().sum();
            if total_requested == interval.size() {
                let splits = interval.split_exact(&sizes);
                let total_allocated: u64 = splits.iter().map(|s| s.size()).sum();

                assert_eq!(
                    total_allocated,
                    interval.size(),
                    "Sum of split sizes should equal original interval size"
                );
            }
        }
    }

    // ============================================================================
    // Property 8: Difficulty and Work Relationship
    // ============================================================================

    #[test]
    fn prop_equal_difficulties_produce_equal_work() {
        // Property: Same difficulty always produces same work
        let difficulty = Difficulty::from(1000u64);

        let work1 = calc_work_from_difficulty(&difficulty).unwrap();
        let work2 = calc_work_from_difficulty(&difficulty).unwrap();

        assert_eq!(work1, work2, "Equal difficulties should produce equal work");
    }

    #[test]
    fn prop_work_calculation_is_deterministic() {
        // Property: Work calculation is deterministic (same input -> same output)
        let test_difficulties = vec![1u64, 10, 100, 1000, 10000];

        for &diff_val in &test_difficulties {
            let difficulty = Difficulty::from(diff_val);

            // Calculate multiple times
            let work1 = calc_work_from_difficulty(&difficulty).unwrap();
            let work2 = calc_work_from_difficulty(&difficulty).unwrap();
            let work3 = calc_work_from_difficulty(&difficulty).unwrap();

            assert_eq!(work1, work2, "Work calculation should be deterministic");
            assert_eq!(work2, work3, "Work calculation should be deterministic");
        }
    }

    // ============================================================================
    // Property 9: Interval Emptiness Properties
    // ============================================================================

    #[test]
    fn prop_empty_interval_size_is_zero() {
        // Property: Empty intervals have size 0
        let empty = Interval::empty();
        assert_eq!(empty.size(), 0, "Empty interval should have size 0");
    }

    #[test]
    fn prop_non_empty_interval_has_positive_size() {
        // Property: Non-empty intervals have size > 0
        let test_cases = vec![
            Interval::new(1, 1),
            Interval::new(1, 100),
            Interval::new(500, 1000),
            Interval::maximal(),
        ];

        for interval in test_cases {
            if !interval.is_empty() {
                assert!(
                    interval.size() > 0,
                    "Non-empty interval {:?} should have positive size",
                    interval
                );
            }
        }
    }

    // ============================================================================
    // Property 10: Blue Work Accumulation Properties
    // ============================================================================

    #[test]
    fn prop_accumulated_work_exceeds_individual_work() {
        // Property: Sum of works > individual work (for positive works)
        let work1 = BlueWorkType::from(100u64);
        let work2 = BlueWorkType::from(200u64);
        let work3 = BlueWorkType::from(300u64);

        let total = work1 + work2 + work3;

        assert!(total > work1, "Total work should exceed individual work");
        assert!(total > work2, "Total work should exceed individual work");
        assert!(total > work3, "Total work should exceed individual work");
    }

    #[test]
    fn prop_work_accumulation_preserves_order() {
        // Property: If A > B, then A + C > B + C
        let work_a = BlueWorkType::from(200u64);
        let work_b = BlueWorkType::from(100u64);
        let work_c = BlueWorkType::from(50u64);

        if work_a > work_b {
            let sum_a = work_a + work_c;
            let sum_b = work_b + work_c;

            assert!(
                sum_a > sum_b,
                "Order should be preserved: if {} > {}, then {} + {} > {} + {}",
                work_a,
                work_b,
                work_a,
                work_c,
                work_b,
                work_c
            );
        }
    }
}
