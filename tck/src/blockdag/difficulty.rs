// Tests for cumulative difficulty accumulation and VarUint arithmetic
// used in BlockDAG fork choice decisions.
//
// Key concepts tested:
// - VarUint (U256-backed) provides overflow-safe difficulty representation
// - Cumulative difficulty accumulates along a chain: parent_cd + block_difficulty = child_cd
// - Fork choice rule: the branch with highest cumulative difficulty wins
// - Chain length alone does not determine the best fork

#[cfg(test)]
mod tests {
    use tos_common::{
        difficulty::{CumulativeDifficulty, Difficulty},
        varuint::VarUint,
    };

    // =========================================================================
    // Basic VarUint / Difficulty construction tests
    // =========================================================================

    /// VarUint::zero() should equal a VarUint constructed from 0.
    #[test]
    fn test_difficulty_zero() {
        let zero = VarUint::zero();
        let also_zero = VarUint::from_u64(0);
        assert_eq!(zero, also_zero, "VarUint::zero() should equal from_u64(0)");

        // Verify it compares as expected
        let one = VarUint::from_u64(1);
        assert!(zero < one, "Zero should be less than one");
    }

    /// VarUint::from_u64 correctly stores and retrieves the value.
    #[test]
    fn test_difficulty_from_u64() {
        let diff = VarUint::from_u64(100);
        let also_100 = VarUint::from_u64(100);
        assert_eq!(diff, also_100, "from_u64(100) should produce equal values");

        // Verify it's not equal to a different value
        let diff_200 = VarUint::from_u64(200);
        assert_ne!(diff, diff_200);
    }

    // =========================================================================
    // Comparison tests
    // =========================================================================

    /// Smaller difficulty compares less than larger difficulty.
    #[test]
    fn test_difficulty_comparison_less() {
        let small: Difficulty = VarUint::from_u64(50);
        let large: Difficulty = VarUint::from_u64(200);

        assert!(small < large, "50 should be less than 200");
        assert!(large > small, "200 should be greater than 50");
        assert_ne!(small, large);
    }

    /// Equal difficulty values compare as equal.
    #[test]
    fn test_difficulty_comparison_equal() {
        let a: Difficulty = VarUint::from_u64(1000);
        let b: Difficulty = VarUint::from_u64(1000);

        assert_eq!(a, b, "Same difficulty values should be equal");
        assert!(a >= b, "Equal values: a should not be less than b");
        assert!(a <= b, "Equal values: a should not be greater than b");
    }

    // =========================================================================
    // Cumulative difficulty accumulation tests
    // =========================================================================

    /// A child block's cumulative difficulty equals parent_cd + block_difficulty.
    #[test]
    fn test_difficulty_addition() {
        let parent_cd: CumulativeDifficulty = VarUint::from_u64(1000);
        let block_difficulty: Difficulty = VarUint::from_u64(250);

        // Accumulate: child_cd = parent_cd + block_difficulty
        let child_cd = parent_cd + block_difficulty;
        let expected = VarUint::from_u64(1250);

        assert_eq!(
            child_cd, expected,
            "parent_cd(1000) + block_diff(250) should equal 1250"
        );
    }

    /// A chain of 5 blocks accumulates difficulty correctly.
    /// genesis -> B1 -> B2 -> B3 -> B4 -> B5
    #[test]
    fn test_difficulty_accumulation_chain() {
        let block_difficulties: [u64; 5] = [100, 150, 200, 175, 225];
        let mut cumulative: CumulativeDifficulty = VarUint::from_u64(0);

        let mut expected_total: u64 = 0;
        for &diff in &block_difficulties {
            let block_diff = VarUint::from_u64(diff);
            cumulative += block_diff;
            expected_total = expected_total.checked_add(diff).unwrap();
        }

        assert_eq!(
            cumulative,
            VarUint::from_u64(expected_total),
            "Chain of 5 blocks should accumulate to {}",
            expected_total
        );
        assert_eq!(expected_total, 850);
    }

    /// VarUint uses U256 internally, so very large values should not panic.
    /// This tests that additions with large u64 values are safe.
    #[test]
    fn test_difficulty_overflow_safety() {
        // Start with u64::MAX
        let large: CumulativeDifficulty = VarUint::from_u64(u64::MAX);
        let addition: Difficulty = VarUint::from_u64(u64::MAX);

        // This should not panic because VarUint internally uses U256
        let result = large + addition;

        // The result should be greater than either operand
        assert!(result > large, "Result of addition should exceed u64::MAX");
        assert!(
            result > addition,
            "Result should be greater than the addend"
        );

        // Verify the result is approximately 2 * u64::MAX
        let expected = VarUint::from_u128(u64::MAX as u128 * 2);
        assert_eq!(result, expected);
    }

    // =========================================================================
    // Fork choice tests
    // =========================================================================

    /// The fork with higher cumulative difficulty is the preferred fork.
    #[test]
    fn test_difficulty_ordering_determines_fork_choice() {
        let fork_a_cd: CumulativeDifficulty = VarUint::from_u64(5000);
        let fork_b_cd: CumulativeDifficulty = VarUint::from_u64(4500);

        // Fork A has higher cumulative difficulty, so it should be preferred
        assert!(
            fork_a_cd > fork_b_cd,
            "Fork A (CD=5000) should be preferred over Fork B (CD=4500)"
        );

        // Determine best fork by comparison
        let best_cd = if fork_a_cd >= fork_b_cd {
            fork_a_cd
        } else {
            fork_b_cd
        };
        assert_eq!(best_cd, VarUint::from_u64(5000));
    }

    /// Accumulation with varying block difficulties across a chain.
    /// Tests that different difficulty values at each block are summed correctly.
    #[test]
    fn test_cumulative_difficulty_with_varying_block_difficulties() {
        // Genesis has CD = 0, first block adds its own difficulty
        let genesis_cd: CumulativeDifficulty = VarUint::zero();

        // Block 1: difficulty 100, CD = 100
        let b1_diff: Difficulty = VarUint::from_u64(100);
        let b1_cd = genesis_cd + b1_diff;
        assert_eq!(b1_cd, VarUint::from_u64(100));

        // Block 2: difficulty 500 (harder), CD = 600
        let b2_diff: Difficulty = VarUint::from_u64(500);
        let b2_cd = b1_cd + b2_diff;
        assert_eq!(b2_cd, VarUint::from_u64(600));

        // Block 3: difficulty 50 (easier), CD = 650
        let b3_diff: Difficulty = VarUint::from_u64(50);
        let b3_cd = b2_cd + b3_diff;
        assert_eq!(b3_cd, VarUint::from_u64(650));

        // Block 4: difficulty 1000 (much harder), CD = 1650
        let b4_diff: Difficulty = VarUint::from_u64(1000);
        let b4_cd = b3_cd + b4_diff;
        assert_eq!(b4_cd, VarUint::from_u64(1650));

        // Block 5: difficulty 200, CD = 1850
        let b5_diff: Difficulty = VarUint::from_u64(200);
        let b5_cd = b4_cd + b5_diff;
        assert_eq!(b5_cd, VarUint::from_u64(1850));
    }

    /// Compare two forks with different lengths and difficulties.
    /// Fork A: 3 blocks with high difficulty
    /// Fork B: 5 blocks with low difficulty
    #[test]
    fn test_difficulty_fork_comparison() {
        // Fork A: 3 blocks, each with difficulty 1000 -> CD = 3000
        let mut fork_a_cd: CumulativeDifficulty = VarUint::zero();
        for _ in 0..3 {
            fork_a_cd += VarUint::from_u64(1000);
        }
        assert_eq!(fork_a_cd, VarUint::from_u64(3000));

        // Fork B: 5 blocks, each with difficulty 500 -> CD = 2500
        let mut fork_b_cd: CumulativeDifficulty = VarUint::zero();
        for _ in 0..5 {
            fork_b_cd += VarUint::from_u64(500);
        }
        assert_eq!(fork_b_cd, VarUint::from_u64(2500));

        // Fork A wins despite being shorter
        assert!(
            fork_a_cd > fork_b_cd,
            "Fork A (3 blocks, CD=3000) should beat Fork B (5 blocks, CD=2500)"
        );
    }

    /// A shorter chain with higher per-block difficulty can have higher
    /// cumulative difficulty than a longer chain with lower difficulty.
    #[test]
    fn test_difficulty_longer_chain_not_always_higher_cd() {
        // Short chain: 2 blocks with difficulty 5000 each -> CD = 10000
        let mut short_chain_cd: CumulativeDifficulty = VarUint::zero();
        short_chain_cd += VarUint::from_u64(5000);
        short_chain_cd += VarUint::from_u64(5000);
        assert_eq!(short_chain_cd, VarUint::from_u64(10000));

        // Long chain: 10 blocks with difficulty 900 each -> CD = 9000
        let mut long_chain_cd: CumulativeDifficulty = VarUint::zero();
        for _ in 0..10 {
            long_chain_cd += VarUint::from_u64(900);
        }
        assert_eq!(long_chain_cd, VarUint::from_u64(9000));

        // Short chain wins despite having fewer blocks
        assert!(
            short_chain_cd > long_chain_cd,
            "Shorter chain (2 blocks, CD=10000) beats longer chain (10 blocks, CD=9000)"
        );

        // This is a key property: chain length alone does not determine the best fork
        let short_len = 2u64;
        let long_len = 10u64;
        assert!(short_len < long_len, "Short chain has fewer blocks");
        assert!(
            short_chain_cd > long_chain_cd,
            "But short chain has higher CD"
        );
    }

    /// Adding a zero-difficulty block does not change the cumulative difficulty.
    #[test]
    fn test_zero_difficulty_block_adds_nothing() {
        let parent_cd: CumulativeDifficulty = VarUint::from_u64(5000);
        let zero_diff: Difficulty = VarUint::zero();

        let child_cd = parent_cd + zero_diff;
        assert_eq!(
            child_cd,
            VarUint::from_u64(5000),
            "Adding zero difficulty should not change CD"
        );

        // Multiple zero-difficulty blocks still don't change CD
        let mut cd = parent_cd;
        for _ in 0..10 {
            cd += VarUint::zero();
        }
        assert_eq!(
            cd,
            VarUint::from_u64(5000),
            "Multiple zero-difficulty blocks should not change CD"
        );
    }

    // =========================================================================
    // Additional arithmetic safety tests
    // =========================================================================

    /// VarUint subtraction works correctly for difficulty deltas.
    #[test]
    fn test_difficulty_subtraction() {
        let a: VarUint = VarUint::from_u64(1000);
        let b: VarUint = VarUint::from_u64(400);
        let result = a - b;
        assert_eq!(result, VarUint::from_u64(600));
    }

    /// VarUint ordering is total (any two values are comparable).
    #[test]
    fn test_difficulty_total_ordering() {
        let values: Vec<VarUint> = vec![
            VarUint::from_u64(100),
            VarUint::from_u64(0),
            VarUint::from_u64(u64::MAX),
            VarUint::from_u64(50),
            VarUint::from_u64(1000),
        ];

        // All pairs should be comparable (no NaN-like behavior)
        for i in 0..values.len() {
            for j in 0..values.len() {
                let cmp = values[i].partial_cmp(&values[j]);
                assert!(
                    cmp.is_some(),
                    "VarUint should have total ordering for all values"
                );
            }
        }

        // Verify specific ordering
        assert!(VarUint::from_u64(0) < VarUint::from_u64(1));
        assert!(VarUint::from_u64(1) < VarUint::from_u64(u64::MAX));
    }

    /// VarUint AddAssign accumulates correctly over many iterations.
    #[test]
    fn test_difficulty_addassign_accumulation() {
        let mut total: CumulativeDifficulty = VarUint::zero();
        let increment: Difficulty = VarUint::from_u64(1);

        // Add 1 a thousand times
        for _ in 0..1000 {
            total += increment;
        }

        assert_eq!(
            total,
            VarUint::from_u64(1000),
            "1000 additions of 1 should equal 1000"
        );
    }
}
