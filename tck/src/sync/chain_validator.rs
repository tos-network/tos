#[cfg(test)]
mod tests {
    use super::super::mock::*;

    fn make_test_block(index: u64, difficulty: Difficulty, tips_count: usize) -> BlockMetadata {
        let mut hash = [0u8; 32];
        hash[0..8].copy_from_slice(&index.to_le_bytes());

        let tips: Vec<Hash> = (0..tips_count)
            .map(|t| {
                let mut tip_hash = [0u8; 32];
                let tip_val = index.saturating_sub(1).saturating_add(t as u64);
                tip_hash[0..8].copy_from_slice(&tip_val.to_le_bytes());
                tip_hash
            })
            .collect();

        BlockMetadata {
            hash,
            topoheight: index,
            height: index,
            difficulty,
            cumulative_difficulty: index * difficulty,
            tips,
            txs: Vec::new(),
        }
    }

    #[test]
    fn empty_validator_has_higher_cd_returns_error() {
        let validator = MockChainValidator::new();
        let result = validator.has_higher_cumulative_difficulty(100);
        assert_eq!(result, Err("No blocks in validator"));
    }

    #[test]
    fn single_block_cumulative_difficulty_equals_block_difficulty() {
        let mut validator = MockChainValidator::new();
        let block = make_test_block(1, 500, 1);
        validator.insert_block(block).unwrap();

        assert_eq!(validator.cumulative_difficulty, 500);
    }

    #[test]
    fn multiple_blocks_cumulative_difficulty_sums_correctly() {
        let mut validator = MockChainValidator::new();

        validator.insert_block(make_test_block(1, 100, 1)).unwrap();
        validator.insert_block(make_test_block(2, 200, 1)).unwrap();
        validator.insert_block(make_test_block(3, 300, 1)).unwrap();

        assert_eq!(validator.cumulative_difficulty, 600); // 100 + 200 + 300
    }

    #[test]
    fn higher_cd_than_current_chain_returns_true() {
        let mut validator = MockChainValidator::new();
        validator.insert_block(make_test_block(1, 1000, 1)).unwrap();

        let current_chain_cd: CumulativeDifficulty = 500;
        let result = validator.has_higher_cumulative_difficulty(current_chain_cd);
        assert_eq!(result, Ok(true));
    }

    #[test]
    fn lower_cd_than_current_chain_returns_false() {
        let mut validator = MockChainValidator::new();
        validator.insert_block(make_test_block(1, 100, 1)).unwrap();

        let current_chain_cd: CumulativeDifficulty = 500;
        let result = validator.has_higher_cumulative_difficulty(current_chain_cd);
        assert_eq!(result, Ok(false));
    }

    #[test]
    fn equal_cd_returns_false() {
        let mut validator = MockChainValidator::new();
        validator.insert_block(make_test_block(1, 500, 1)).unwrap();

        let current_chain_cd: CumulativeDifficulty = 500;
        let result = validator.has_higher_cumulative_difficulty(current_chain_cd);
        // Equal means NOT higher, so returns false (keep current chain)
        assert_eq!(result, Ok(false));
    }

    #[test]
    fn duplicate_block_insertion_returns_error() {
        let mut validator = MockChainValidator::new();
        let block = make_test_block(1, 100, 1);
        validator.insert_block(block.clone()).unwrap();

        let result = validator.insert_block(block);
        assert_eq!(result, Err("Block already in chain"));
    }

    #[test]
    fn block_with_empty_tips_rejected() {
        let mut validator = MockChainValidator::new();
        let block = BlockMetadata {
            hash: [1u8; 32],
            topoheight: 1,
            height: 1,
            difficulty: 100,
            cumulative_difficulty: 100,
            tips: Vec::new(), // 0 tips - invalid
            txs: Vec::new(),
        };

        let result = validator.insert_block(block);
        assert_eq!(result, Err("Invalid tips count"));
    }

    #[test]
    fn block_with_too_many_tips_rejected() {
        let mut validator = MockChainValidator::new();
        let block = BlockMetadata {
            hash: [1u8; 32],
            topoheight: 1,
            height: 1,
            difficulty: 100,
            cumulative_difficulty: 100,
            tips: vec![[2u8; 32], [3u8; 32], [4u8; 32], [5u8; 32]], // 4 tips > TIPS_LIMIT(3)
            txs: Vec::new(),
        };

        let result = validator.insert_block(block);
        assert_eq!(result, Err("Invalid tips count"));
    }

    #[test]
    fn block_with_one_tip_accepted() {
        let mut validator = MockChainValidator::new();
        let block = make_test_block(1, 100, 1);
        let result = validator.insert_block(block);
        assert!(result.is_ok());
    }

    #[test]
    fn block_with_exactly_tips_limit_accepted() {
        let mut validator = MockChainValidator::new();
        // TIPS_LIMIT = 3
        let block = make_test_block(1, 100, TIPS_LIMIT);
        assert_eq!(block.tips.len(), 3);
        let result = validator.insert_block(block);
        assert!(result.is_ok());
    }

    #[test]
    fn validator_accumulates_blocks_in_order() {
        let mut validator = MockChainValidator::new();

        for i in 1..=10 {
            validator.insert_block(make_test_block(i, 100, 1)).unwrap();
        }

        assert_eq!(validator.blocks.len(), 10);
        // Blocks should be in insertion order
        for (idx, block) in validator.blocks.iter().enumerate() {
            assert_eq!(block.topoheight, (idx as u64) + 1);
        }
    }

    #[test]
    fn height_validation_from_tips() {
        let mut validator = MockChainValidator::new();

        // Block at height 5 should reference tips from lower heights
        let block = BlockMetadata {
            hash: [10u8; 32],
            topoheight: 5,
            height: 5,
            difficulty: 100,
            cumulative_difficulty: 500,
            tips: vec![[4u8; 32]], // tip referencing "block 4"
            txs: Vec::new(),
        };

        let result = validator.insert_block(block);
        assert!(result.is_ok());
        assert_eq!(validator.blocks[0].height, 5);
    }

    #[test]
    fn tips_existence_verification() {
        let mut validator = MockChainValidator::new();

        // Insert a block with specific tip hashes
        let tip_hash = [42u8; 32];
        let block = BlockMetadata {
            hash: [1u8; 32],
            topoheight: 1,
            height: 1,
            difficulty: 100,
            cumulative_difficulty: 100,
            tips: vec![tip_hash],
            txs: Vec::new(),
        };

        validator.insert_block(block).unwrap();

        // Verify the tips are preserved
        assert_eq!(validator.blocks[0].tips.len(), 1);
        assert_eq!(validator.blocks[0].tips[0], tip_hash);
    }

    #[test]
    fn cumulative_difficulty_accumulation_across_100_blocks() {
        let mut validator = MockChainValidator::new();
        let difficulty_per_block: Difficulty = 150;

        for i in 1..=100 {
            validator
                .insert_block(make_test_block(i, difficulty_per_block, 1))
                .unwrap();
        }

        assert_eq!(validator.cumulative_difficulty, 100 * difficulty_per_block);
        assert_eq!(validator.cumulative_difficulty, 15000);
    }

    #[test]
    fn validator_block_count_matches_insertions() {
        let mut validator = MockChainValidator::new();

        for i in 1..=37 {
            validator.insert_block(make_test_block(i, 100, 1)).unwrap();
        }

        assert_eq!(validator.blocks.len(), 37);
    }
}
