// Tests for BlockDAG ordering functions:
// - sort_descending_by_cumulative_difficulty
// - sort_ascending_by_cumulative_difficulty
//
// Primary sort key: cumulative difficulty
// Tiebreaker: hash value (descending sorts higher hash first, ascending sorts lower hash first)

#[cfg(test)]
mod tests {
    use super::super::make_hash;
    use tos_common::{crypto::Hash, difficulty::CumulativeDifficulty, varuint::VarUint};
    use tos_daemon::core::blockdag::{
        sort_ascending_by_cumulative_difficulty, sort_descending_by_cumulative_difficulty,
    };

    // =========================================================================
    // Tests for sort_descending_by_cumulative_difficulty
    // =========================================================================

    #[test]
    fn test_descending_empty_vec() {
        let mut scores: Vec<(Hash, CumulativeDifficulty)> = Vec::new();
        sort_descending_by_cumulative_difficulty(&mut scores);
        assert!(scores.is_empty());
    }

    #[test]
    fn test_descending_single_element() {
        let hash = make_hash(0x01);
        let difficulty = VarUint::from(100u64);
        let mut scores = vec![(hash.clone(), difficulty)];
        sort_descending_by_cumulative_difficulty(&mut scores);
        assert_eq!(scores.len(), 1);
        assert_eq!(scores[0].0, hash);
        assert_eq!(scores[0].1, difficulty);
    }

    #[test]
    fn test_descending_two_elements_different_difficulty() {
        let hash_a = make_hash(0x01);
        let hash_b = make_hash(0x02);
        let low_diff = VarUint::from(50u64);
        let high_diff = VarUint::from(200u64);

        // Place lower difficulty first
        let mut scores = vec![(hash_a.clone(), low_diff), (hash_b.clone(), high_diff)];
        sort_descending_by_cumulative_difficulty(&mut scores);

        // Higher difficulty should come first
        assert_eq!(scores[0].0, hash_b);
        assert_eq!(scores[0].1, high_diff);
        assert_eq!(scores[1].0, hash_a);
        assert_eq!(scores[1].1, low_diff);
    }

    #[test]
    fn test_descending_two_elements_same_difficulty() {
        // When difficulties are equal, higher hash comes first in descending order
        let hash_low = make_hash(0x01);
        let hash_high = make_hash(0xFF);
        let same_diff = VarUint::from(100u64);

        // Place lower hash first
        let mut scores = vec![
            (hash_low.clone(), same_diff),
            (hash_high.clone(), same_diff),
        ];
        sort_descending_by_cumulative_difficulty(&mut scores);

        // Higher hash should come first (descending tiebreaker)
        assert_eq!(scores[0].0, hash_high);
        assert_eq!(scores[1].0, hash_low);
    }

    #[test]
    fn test_descending_three_elements_mixed() {
        let hash_a = make_hash(0x10);
        let hash_b = make_hash(0x20);
        let hash_c = make_hash(0x30);
        let diff_low = VarUint::from(10u64);
        let diff_mid = VarUint::from(50u64);
        let diff_high = VarUint::from(100u64);

        // Place in random order
        let mut scores = vec![
            (hash_b.clone(), diff_mid),
            (hash_a.clone(), diff_high),
            (hash_c.clone(), diff_low),
        ];
        sort_descending_by_cumulative_difficulty(&mut scores);

        // Should be ordered: highest difficulty first
        assert_eq!(scores[0].0, hash_a);
        assert_eq!(scores[0].1, diff_high);
        assert_eq!(scores[1].0, hash_b);
        assert_eq!(scores[1].1, diff_mid);
        assert_eq!(scores[2].0, hash_c);
        assert_eq!(scores[2].1, diff_low);
    }

    #[test]
    fn test_descending_already_sorted() {
        let hash_a = make_hash(0x01);
        let hash_b = make_hash(0x02);
        let hash_c = make_hash(0x03);
        let diff_high = VarUint::from(300u64);
        let diff_mid = VarUint::from(200u64);
        let diff_low = VarUint::from(100u64);

        // Already in descending order
        let mut scores = vec![
            (hash_a.clone(), diff_high),
            (hash_b.clone(), diff_mid),
            (hash_c.clone(), diff_low),
        ];
        sort_descending_by_cumulative_difficulty(&mut scores);

        // Should remain unchanged
        assert_eq!(scores[0].0, hash_a);
        assert_eq!(scores[0].1, diff_high);
        assert_eq!(scores[1].0, hash_b);
        assert_eq!(scores[1].1, diff_mid);
        assert_eq!(scores[2].0, hash_c);
        assert_eq!(scores[2].1, diff_low);
    }

    #[test]
    fn test_descending_reverse_sorted() {
        let hash_a = make_hash(0x01);
        let hash_b = make_hash(0x02);
        let hash_c = make_hash(0x03);
        let diff_low = VarUint::from(100u64);
        let diff_mid = VarUint::from(200u64);
        let diff_high = VarUint::from(300u64);

        // In ascending order (opposite of desired)
        let mut scores = vec![
            (hash_a.clone(), diff_low),
            (hash_b.clone(), diff_mid),
            (hash_c.clone(), diff_high),
        ];
        sort_descending_by_cumulative_difficulty(&mut scores);

        // Should be reversed to descending
        assert_eq!(scores[0].0, hash_c);
        assert_eq!(scores[0].1, diff_high);
        assert_eq!(scores[1].0, hash_b);
        assert_eq!(scores[1].1, diff_mid);
        assert_eq!(scores[2].0, hash_a);
        assert_eq!(scores[2].1, diff_low);
    }

    #[test]
    fn test_descending_all_same_difficulty() {
        // All have the same difficulty, so tiebreaker is hash descending
        let hash_low = make_hash(0x01);
        let hash_mid = make_hash(0x80);
        let hash_high = make_hash(0xFF);
        let same_diff = VarUint::from(500u64);

        let mut scores = vec![
            (hash_mid.clone(), same_diff),
            (hash_low.clone(), same_diff),
            (hash_high.clone(), same_diff),
        ];
        sort_descending_by_cumulative_difficulty(&mut scores);

        // Should be sorted by hash descending: 0xFF > 0x80 > 0x01
        assert_eq!(scores[0].0, hash_high);
        assert_eq!(scores[1].0, hash_mid);
        assert_eq!(scores[2].0, hash_low);
    }

    #[test]
    fn test_descending_large_values() {
        let hash_a = make_hash(0xAA);
        let hash_b = make_hash(0xBB);
        let large_diff = VarUint::from(u64::MAX);
        let near_max_diff = VarUint::from(u64::MAX - 1);

        let mut scores = vec![
            (hash_b.clone(), near_max_diff),
            (hash_a.clone(), large_diff),
        ];
        sort_descending_by_cumulative_difficulty(&mut scores);

        // u64::MAX > u64::MAX - 1
        assert_eq!(scores[0].0, hash_a);
        assert_eq!(scores[0].1, large_diff);
        assert_eq!(scores[1].0, hash_b);
        assert_eq!(scores[1].1, near_max_diff);
    }

    #[test]
    fn test_descending_many_elements() {
        // Create 12 elements with varying difficulties
        let difficulties: Vec<u64> = vec![5, 100, 42, 999, 1, 50, 300, 77, 88, 200, 150, 600];
        let mut scores: Vec<(Hash, CumulativeDifficulty)> = difficulties
            .iter()
            .enumerate()
            .map(|(i, &d)| {
                let byte = (i as u8).wrapping_add(1);
                (make_hash(byte), VarUint::from(d))
            })
            .collect();

        sort_descending_by_cumulative_difficulty(&mut scores);

        // Verify the entire list is in descending order
        for i in 0..scores.len() - 1 {
            let curr_diff = &scores[i].1;
            let next_diff = &scores[i + 1].1;
            assert!(
                curr_diff >= next_diff,
                "Element at index {} (difficulty {:?}) should be >= element at index {} (difficulty {:?})",
                i,
                curr_diff,
                i + 1,
                next_diff,
            );
            // If same difficulty, hash should be descending
            if curr_diff == next_diff {
                assert!(
                    scores[i].0 >= scores[i + 1].0,
                    "With same difficulty, hash at index {} should be >= hash at index {}",
                    i,
                    i + 1,
                );
            }
        }

        // Verify first element has the highest difficulty (999)
        assert_eq!(scores[0].1, VarUint::from(999u64));
        // Verify last element has the lowest difficulty (1)
        assert_eq!(scores[scores.len() - 1].1, VarUint::from(1u64));
    }

    // =========================================================================
    // Tests for sort_ascending_by_cumulative_difficulty
    // =========================================================================

    #[test]
    fn test_ascending_empty_vec() {
        let mut scores: Vec<(Hash, CumulativeDifficulty)> = Vec::new();
        sort_ascending_by_cumulative_difficulty(&mut scores);
        assert!(scores.is_empty());
    }

    #[test]
    fn test_ascending_single_element() {
        let hash = make_hash(0x42);
        let difficulty = VarUint::from(250u64);
        let mut scores = vec![(hash.clone(), difficulty)];
        sort_ascending_by_cumulative_difficulty(&mut scores);
        assert_eq!(scores.len(), 1);
        assert_eq!(scores[0].0, hash);
        assert_eq!(scores[0].1, difficulty);
    }

    #[test]
    fn test_ascending_two_elements_different_difficulty() {
        let hash_a = make_hash(0x01);
        let hash_b = make_hash(0x02);
        let low_diff = VarUint::from(50u64);
        let high_diff = VarUint::from(200u64);

        // Place higher difficulty first
        let mut scores = vec![(hash_b.clone(), high_diff), (hash_a.clone(), low_diff)];
        sort_ascending_by_cumulative_difficulty(&mut scores);

        // Lower difficulty should come first
        assert_eq!(scores[0].0, hash_a);
        assert_eq!(scores[0].1, low_diff);
        assert_eq!(scores[1].0, hash_b);
        assert_eq!(scores[1].1, high_diff);
    }

    #[test]
    fn test_ascending_two_elements_same_difficulty() {
        // When difficulties are equal, lower hash comes first in ascending order
        let hash_low = make_hash(0x01);
        let hash_high = make_hash(0xFF);
        let same_diff = VarUint::from(100u64);

        // Place higher hash first
        let mut scores = vec![
            (hash_high.clone(), same_diff),
            (hash_low.clone(), same_diff),
        ];
        sort_ascending_by_cumulative_difficulty(&mut scores);

        // Lower hash should come first (ascending tiebreaker)
        assert_eq!(scores[0].0, hash_low);
        assert_eq!(scores[1].0, hash_high);
    }

    #[test]
    fn test_ascending_three_elements_mixed() {
        let hash_a = make_hash(0x10);
        let hash_b = make_hash(0x20);
        let hash_c = make_hash(0x30);
        let diff_low = VarUint::from(10u64);
        let diff_mid = VarUint::from(50u64);
        let diff_high = VarUint::from(100u64);

        // Place in random order
        let mut scores = vec![
            (hash_b.clone(), diff_mid),
            (hash_c.clone(), diff_high),
            (hash_a.clone(), diff_low),
        ];
        sort_ascending_by_cumulative_difficulty(&mut scores);

        // Should be ordered: lowest difficulty first
        assert_eq!(scores[0].0, hash_a);
        assert_eq!(scores[0].1, diff_low);
        assert_eq!(scores[1].0, hash_b);
        assert_eq!(scores[1].1, diff_mid);
        assert_eq!(scores[2].0, hash_c);
        assert_eq!(scores[2].1, diff_high);
    }

    #[test]
    fn test_ascending_all_same_difficulty() {
        // All have the same difficulty, so tiebreaker is hash ascending
        let hash_low = make_hash(0x01);
        let hash_mid = make_hash(0x80);
        let hash_high = make_hash(0xFF);
        let same_diff = VarUint::from(500u64);

        let mut scores = vec![
            (hash_high.clone(), same_diff),
            (hash_low.clone(), same_diff),
            (hash_mid.clone(), same_diff),
        ];
        sort_ascending_by_cumulative_difficulty(&mut scores);

        // Should be sorted by hash ascending: 0x01 < 0x80 < 0xFF
        assert_eq!(scores[0].0, hash_low);
        assert_eq!(scores[1].0, hash_mid);
        assert_eq!(scores[2].0, hash_high);
    }

    // =========================================================================
    // Ordering determinism tests
    // =========================================================================

    #[test]
    fn test_descending_deterministic() {
        // Running the sort multiple times on the same input must produce the same output
        let build_scores = || -> Vec<(Hash, CumulativeDifficulty)> {
            vec![
                (make_hash(0x05), VarUint::from(30u64)),
                (make_hash(0xAA), VarUint::from(100u64)),
                (make_hash(0x50), VarUint::from(30u64)),
                (make_hash(0x01), VarUint::from(200u64)),
                (make_hash(0xFF), VarUint::from(100u64)),
            ]
        };

        let mut first_run = build_scores();
        sort_descending_by_cumulative_difficulty(&mut first_run);

        // Run the sort 5 more times and compare each result
        for _ in 0..5 {
            let mut subsequent_run = build_scores();
            sort_descending_by_cumulative_difficulty(&mut subsequent_run);
            assert_eq!(
                first_run.len(),
                subsequent_run.len(),
                "Result lengths should match"
            );
            for (i, (first, second)) in first_run.iter().zip(subsequent_run.iter()).enumerate() {
                assert_eq!(
                    first.0, second.0,
                    "Hash mismatch at index {} across runs",
                    i
                );
                assert_eq!(
                    first.1, second.1,
                    "Difficulty mismatch at index {} across runs",
                    i
                );
            }
        }
    }

    #[test]
    fn test_ascending_deterministic() {
        // Running the sort multiple times on the same input must produce the same output
        let build_scores = || -> Vec<(Hash, CumulativeDifficulty)> {
            vec![
                (make_hash(0x05), VarUint::from(30u64)),
                (make_hash(0xAA), VarUint::from(100u64)),
                (make_hash(0x50), VarUint::from(30u64)),
                (make_hash(0x01), VarUint::from(200u64)),
                (make_hash(0xFF), VarUint::from(100u64)),
            ]
        };

        let mut first_run = build_scores();
        sort_ascending_by_cumulative_difficulty(&mut first_run);

        // Run the sort 5 more times and compare each result
        for _ in 0..5 {
            let mut subsequent_run = build_scores();
            sort_ascending_by_cumulative_difficulty(&mut subsequent_run);
            assert_eq!(
                first_run.len(),
                subsequent_run.len(),
                "Result lengths should match"
            );
            for (i, (first, second)) in first_run.iter().zip(subsequent_run.iter()).enumerate() {
                assert_eq!(
                    first.0, second.0,
                    "Hash mismatch at index {} across runs",
                    i
                );
                assert_eq!(
                    first.1, second.1,
                    "Difficulty mismatch at index {} across runs",
                    i
                );
            }
        }
    }

    #[test]
    fn test_descending_ascending_inverse() {
        // When all difficulties are distinct, ascending order is the reverse of descending
        let mut desc_scores = vec![
            (make_hash(0x10), VarUint::from(500u64)),
            (make_hash(0x20), VarUint::from(100u64)),
            (make_hash(0x30), VarUint::from(300u64)),
            (make_hash(0x40), VarUint::from(50u64)),
            (make_hash(0x50), VarUint::from(700u64)),
        ];

        let mut asc_scores = desc_scores.clone();

        sort_descending_by_cumulative_difficulty(&mut desc_scores);
        sort_ascending_by_cumulative_difficulty(&mut asc_scores);

        // Ascending should be the exact reverse of descending (no ties to complicate things)
        let reversed_desc: Vec<_> = desc_scores.iter().rev().collect();
        let asc_refs: Vec<_> = asc_scores.iter().collect();
        assert_eq!(reversed_desc.len(), asc_refs.len());
        for (i, (rev_item, asc_item)) in reversed_desc.iter().zip(asc_refs.iter()).enumerate() {
            assert_eq!(
                rev_item.0, asc_item.0,
                "Hash mismatch at index {} between reversed descending and ascending",
                i
            );
            assert_eq!(
                rev_item.1, asc_item.1,
                "Difficulty mismatch at index {} between reversed descending and ascending",
                i
            );
        }
    }

    #[test]
    fn test_sort_stability_with_hash_tiebreaker() {
        // Verify that when difficulty is the same, the hash comparison direction
        // is correct for both ascending and descending sorts.
        let hash_01 = Hash::new([0x01; 32]);
        let hash_80 = Hash::new([0x80; 32]);
        let hash_ff = Hash::new([0xFF; 32]);
        let same_diff = VarUint::from(42u64);

        // Test descending: same difficulty -> hash descending (0xFF > 0x80 > 0x01)
        let mut desc_scores = vec![
            (hash_01.clone(), same_diff),
            (hash_ff.clone(), same_diff),
            (hash_80.clone(), same_diff),
        ];
        sort_descending_by_cumulative_difficulty(&mut desc_scores);

        assert_eq!(
            desc_scores[0].0, hash_ff,
            "Descending tiebreaker: 0xFF hash should be first"
        );
        assert_eq!(
            desc_scores[1].0, hash_80,
            "Descending tiebreaker: 0x80 hash should be second"
        );
        assert_eq!(
            desc_scores[2].0, hash_01,
            "Descending tiebreaker: 0x01 hash should be last"
        );

        // Test ascending: same difficulty -> hash ascending (0x01 < 0x80 < 0xFF)
        let mut asc_scores = vec![
            (hash_ff.clone(), same_diff),
            (hash_01.clone(), same_diff),
            (hash_80.clone(), same_diff),
        ];
        sort_ascending_by_cumulative_difficulty(&mut asc_scores);

        assert_eq!(
            asc_scores[0].0, hash_01,
            "Ascending tiebreaker: 0x01 hash should be first"
        );
        assert_eq!(
            asc_scores[1].0, hash_80,
            "Ascending tiebreaker: 0x80 hash should be second"
        );
        assert_eq!(
            asc_scores[2].0, hash_ff,
            "Ascending tiebreaker: 0xFF hash should be last"
        );
    }
}
