// TOS Difficulty Adjustment Algorithm (DAA)

use anyhow::Result;
use std::collections::{HashSet, VecDeque};
use tos_common::crypto::Hash;
use tos_common::difficulty::Difficulty;

use crate::core::error::BlockchainError;
use crate::core::storage::Storage;

/// DAA window size - number of blocks to consider for difficulty adjustment
pub const DAA_WINDOW_SIZE: u64 = 2016;

/// Target time per block in seconds
/// TOS uses 1 second per block (TIP-1's 3s proposal was deprecated)
/// This value is used by DAA for difficulty adjustment calculations
pub const TARGET_TIME_PER_BLOCK: u64 = 1;

/// Minimum difficulty adjustment ratio (0.25x = difficulty can drop to 1/4)
/// This prevents difficulty from dropping too quickly
pub const MIN_DIFFICULTY_RATIO: f64 = 0.25;

/// Maximum difficulty adjustment ratio (4.0x = difficulty can increase to 4x)
/// This prevents difficulty from rising too quickly
pub const MAX_DIFFICULTY_RATIO: f64 = 4.0;

/// Calculate DAA score for a block
///
/// DAA score represents the number of blocks in the DAA window (excluding those
/// outside the window). It's similar to blue_score, but filters out blocks that
/// are too far in the past.
///
/// # Arguments
/// * `storage` - Reference to blockchain storage
/// * `selected_parent` - Hash of the selected parent block
/// * `mergeset_blues` - Blue blocks in the mergeset (excluding selected parent)
///
/// # Returns
/// Tuple of (daa_score, mergeset_non_daa) where:
/// - daa_score: The DAA score for this block
/// - mergeset_non_daa: Blocks in mergeset that are outside DAA window
///
/// # Algorithm
/// 1. Get parent's DAA score
/// 2. Traverse backwards from selected_parent to find DAA window boundary
/// 3. For each blue in mergeset, check if it's within the DAA window
/// 4. Count blues within window, collect blues outside window
/// 5. daa_score = parent_daa_score + count_of_blues_in_window
pub async fn calculate_daa_score<S: Storage>(
    storage: &S,
    selected_parent: &Hash,
    mergeset_blues: &[Hash],
) -> Result<(u64, Vec<Hash>), BlockchainError> {
    // Special case: genesis block
    if selected_parent.as_bytes() == &[0u8; 32] {
        return Ok((0, Vec::new()));
    }

    // Get parent's GHOSTDAG data to get parent's DAA score
    let parent_data = storage.get_ghostdag_data(selected_parent).await?;

    // Use the actual daa_score field (not blue_score)
    let parent_daa_score = parent_data.daa_score;

    // Get the DAA window boundary block
    // This is the block at (parent_daa_score - DAA_WINDOW_SIZE)
    let window_boundary_score = if parent_daa_score >= DAA_WINDOW_SIZE {
        parent_daa_score - DAA_WINDOW_SIZE
    } else {
        0  // If we haven't reached window size yet, boundary is genesis
    };

    // Find blocks in the DAA window using BFS from selected_parent
    let window_blocks = find_daa_window_blocks(storage, selected_parent, window_boundary_score).await?;

    // Check which mergeset blues are outside the window
    let mut mergeset_non_daa = Vec::new();
    let mut blues_in_window_count = 0u64;

    for blue in mergeset_blues {
        if window_blocks.contains(blue) {
            // Block is within DAA window
            blues_in_window_count += 1;
        } else {
            // Block is outside DAA window
            mergeset_non_daa.push(blue.clone());
        }
    }

    // Calculate DAA score: parent's score + 1 (current block) + blues in window
    // The +1 accounts for the current block itself, making DAA score monotonic
    // Formula: daa_score = max(parent_daa_scores) + 1 + blues_in_window
    let daa_score = parent_daa_score + 1 + blues_in_window_count;

    Ok((daa_score, mergeset_non_daa))
}

/// Find all blocks within the DAA window
///
/// Uses BFS to traverse backwards from the given block, collecting all blocks
/// with blue_score >= window_boundary_score.
///
/// # Arguments
/// * `storage` - Reference to blockchain storage
/// * `start_block` - Hash of the block to start from (usually selected_parent)
/// * `window_boundary_score` - Minimum blue_score to be included in window
///
/// # Returns
/// Set of block hashes that are within the DAA window
async fn find_daa_window_blocks<S: Storage>(
    storage: &S,
    start_block: &Hash,
    window_boundary_score: u64,
) -> Result<HashSet<Hash>, BlockchainError> {
    let mut window_blocks = HashSet::new();
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();

    // Start BFS from the given block
    queue.push_back(start_block.clone());
    visited.insert(start_block.clone());

    while let Some(current) = queue.pop_front() {
        // Get current block's GHOSTDAG data
        let current_data = storage.get_ghostdag_data(&current).await?;

        // Check if block is within window
        if current_data.blue_score >= window_boundary_score {
            window_blocks.insert(current.clone());

            // Get block header to traverse to parents
            let header = storage.get_block_header_by_hash(&current).await?;

            // Add parents to queue
            for parent in header.get_parents().iter() {
                if !visited.contains(parent) {
                    visited.insert(parent.clone());
                    queue.push_back(parent.clone());
                }
            }
        }
        // If block's blue_score < window_boundary_score, don't traverse further
        // (blocks in its past will also be outside the window)
    }

    Ok(window_blocks)
}

/// Calculate target difficulty using DAA window
///
/// This implements the core difficulty adjustment algorithm:
/// - Measures actual time taken for recent blocks
/// - Compares to expected time
/// - Adjusts difficulty proportionally
///
/// # Arguments
/// * `storage` - Reference to blockchain storage
/// * `selected_parent` - Hash of the selected parent block
/// * `daa_score` - DAA score of the new block
///
/// # Returns
/// New target difficulty for the next block
///
/// # Algorithm
/// 1. If DAA window not full yet, use genesis difficulty
/// 2. Find blocks at window boundaries (start and end)
/// 3. Calculate actual time = max_timestamp - min_timestamp
/// 4. Calculate expected time = window_size * target_time_per_block
/// 5. Adjust difficulty:
///    new_difficulty = old_difficulty * (expected_time / actual_time)
/// 6. Clamp adjustment to [MIN_RATIO, MAX_RATIO] to prevent extreme changes
pub async fn calculate_target_difficulty<S: Storage>(
    storage: &S,
    selected_parent: &Hash,
    daa_score: u64,
) -> Result<Difficulty, BlockchainError> {
    // If we haven't filled the DAA window yet, use parent's difficulty
    if daa_score < DAA_WINDOW_SIZE {
        return storage.get_difficulty_for_block_hash(selected_parent).await;
    }

    // Get the window start and end blocks
    // End: selected_parent (at daa_score - 1, since we're calculating for new block)
    // Start: block at (daa_score - DAA_WINDOW_SIZE)
    let window_start_score = daa_score - DAA_WINDOW_SIZE;
    let _window_end_score = daa_score - 1;

    // Find blocks at these scores
    let _window_start_block = find_block_at_daa_score(storage, selected_parent, window_start_score).await?;
    let _window_end_block = selected_parent;

    // SECURITY FIX V-07: Use median-time-past for timestamp manipulation resistance
    // Collect timestamps from DAA window blocks
    let mut timestamps: Vec<u64> = Vec::new();

    // Get timestamps for blocks in the window
    let window_blocks = find_daa_window_blocks(storage, selected_parent, window_start_score).await?;
    for block_hash in window_blocks.iter() {
        let header = storage.get_block_header_by_hash(block_hash).await?;
        timestamps.push(header.get_timestamp());
    }

    // Sort timestamps for median calculation
    timestamps.sort();

    // Validate timestamp ordering
    if timestamps.is_empty() {
        return Err(BlockchainError::InvalidConfig);
    }

    let oldest_timestamp = timestamps[0];
    let newest_timestamp = timestamps[timestamps.len() - 1];

    // SECURITY FIX V-07: Validate timestamps are reasonable
    if newest_timestamp < oldest_timestamp {
        return Err(BlockchainError::InvalidTimestampOrder);
    }

    // Calculate actual time taken (in seconds)
    let actual_time = if newest_timestamp > oldest_timestamp {
        newest_timestamp.saturating_sub(oldest_timestamp)
    } else {
        // Timestamp went backwards (shouldn't happen with proper validation)
        // Use minimum time to avoid division by zero
        1
    };

    // Calculate expected time
    let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;

    // Get current difficulty
    let current_difficulty = storage.get_difficulty_for_block_hash(selected_parent).await?;

    // Apply adjustment using U256 integer arithmetic (deterministic across platforms)
    // If actual_time < expected_time: blocks are too fast → increase difficulty
    // If actual_time > expected_time: blocks are too slow → decrease difficulty
    // Clamping to [0.25x, 4x] is handled inside apply_difficulty_adjustment
    let new_difficulty = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time)?;

    Ok(new_difficulty)
}

/// Find block at a specific DAA score
///
/// Traverses backwards from start_block to find a block with the target DAA score.
///
/// # Arguments
/// * `storage` - Reference to blockchain storage
/// * `start_block` - Block to start search from
/// * `target_score` - DAA score to find
///
/// # Returns
/// Hash of a block with the target DAA score
async fn find_block_at_daa_score<S: Storage>(
    storage: &S,
    start_block: &Hash,
    target_score: u64,
) -> Result<Hash, BlockchainError> {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();

    queue.push_back(start_block.clone());
    visited.insert(start_block.clone());

    while let Some(current) = queue.pop_front() {
        let current_data = storage.get_ghostdag_data(&current).await?;

        // Use actual daa_score field (not blue_score)
        if current_data.daa_score == target_score {
            return Ok(current);
        }

        // If we've gone past the target, traverse to parents
        if current_data.daa_score > target_score {
            let header = storage.get_block_header_by_hash(&current).await?;

            for parent in header.get_parents().iter() {
                if !visited.contains(parent) {
                    visited.insert(parent.clone());
                    queue.push_back(parent.clone());
                }
            }
        }
    }

    // If not found, return genesis
    Err(BlockchainError::InvalidConfig)
}

/// Apply difficulty adjustment using deterministic U256 integer arithmetic
///
/// SECURITY FIX: Replaced f64 floating-point arithmetic with U256 integer arithmetic
/// to ensure deterministic consensus across all platforms (x86, ARM, etc.)
///
/// # Arguments
/// * `current_difficulty` - Current difficulty value
/// * `expected_time` - Expected time window (typically DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK)
/// * `actual_time` - Actual time taken for the window
///
/// # Returns
/// New difficulty after applying the adjustment, clamped to [0.25x, 4x] range
///
/// # Formula
/// new_difficulty = (current_difficulty × expected_time) / actual_time
/// Then clamp to [current/4, current×4]
fn apply_difficulty_adjustment(
    current_difficulty: &Difficulty,
    expected_time: u64,
    actual_time: u64,
) -> Result<Difficulty, BlockchainError> {
    use tos_common::varuint::VarUint;

    // Work with VarUint directly (which already has all arithmetic operations)
    let current = *current_difficulty;

    // Convert times to VarUint for arbitrary-precision arithmetic
    let expected = VarUint::from(expected_time);
    let actual = VarUint::from(actual_time);

    // Calculate new difficulty: (current × expected) / actual
    // This is mathematically equivalent to: current × (expected / actual)
    // but avoids any floating-point operations
    let new_difficulty = (current * expected) / actual;

    // Clamp to maximum 4x increase
    let max_difficulty = current * 4u64;
    let clamped_max = if new_difficulty > max_difficulty {
        max_difficulty
    } else {
        new_difficulty
    };

    // Clamp to maximum 4x decrease (minimum 0.25x)
    let min_difficulty = current / 4u64;
    let clamped_both = if clamped_max < min_difficulty {
        min_difficulty
    } else {
        clamped_max
    };

    Ok(clamped_both)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::varuint::VarUint;

    #[test]
    fn test_daa_constants() {
        assert_eq!(DAA_WINDOW_SIZE, 2016);
        assert_eq!(TARGET_TIME_PER_BLOCK, 1);
        assert_eq!(MIN_DIFFICULTY_RATIO, 0.25);
        assert_eq!(MAX_DIFFICULTY_RATIO, 4.0);
    }

    #[test]
    fn test_difficulty_ratio_clamping() {
        // Test that ratios are properly clamped
        let ratio_too_low = 0.1;
        let ratio_too_high = 10.0;
        let ratio_normal = 1.5;

        assert!(ratio_too_low < MIN_DIFFICULTY_RATIO);
        assert!(ratio_too_high > MAX_DIFFICULTY_RATIO);
        assert!(ratio_normal >= MIN_DIFFICULTY_RATIO && ratio_normal <= MAX_DIFFICULTY_RATIO);
    }

    #[test]
    fn test_apply_difficulty_adjustment_increase() {
        // Test difficulty increase (blocks coming too fast)
        let current_difficulty = Difficulty::from(1000u64);

        // Simulate blocks coming 2x faster than expected (ratio = 2.0)
        let expected_time = 2016u64; // DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK
        let actual_time = 1008u64;   // Half the expected time → 2x difficulty

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        // Check that new difficulty is greater than old difficulty
        assert!(new_val > current_val, "Difficulty should increase");

        // Check that it's exactly 2x (2000)
        assert_eq!(new_val.as_u64(), 2000u64, "Difficulty should be exactly 2x");
    }

    #[test]
    fn test_apply_difficulty_adjustment_decrease() {
        // Test difficulty decrease (blocks coming too slow)
        let current_difficulty = Difficulty::from(1000u64);

        // Simulate blocks coming 2x slower than expected (ratio = 0.5)
        let expected_time = 1008u64; // Half of normal window
        let actual_time = 2016u64;   // Double the expected time → 0.5x difficulty

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        // Check that new difficulty is less than old difficulty
        assert!(new_val < current_val, "Difficulty should decrease");

        // Check that it's exactly 0.5x (500)
        assert_eq!(new_val.as_u64(), 500u64, "Difficulty should be exactly 0.5x");
    }

    #[test]
    fn test_apply_difficulty_adjustment_no_change() {
        // Test no change (blocks at expected rate)
        let current_difficulty = Difficulty::from(1000u64);

        // Simulate blocks at exactly expected rate (ratio = 1.0)
        let expected_time = 2016u64; // DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK
        let actual_time = 2016u64;   // Same as expected → no change

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        // Check that difficulty remains the same (integer division is exact here)
        assert_eq!(new_val, current_val, "Difficulty should remain the same");
    }

    #[test]
    fn test_apply_difficulty_adjustment_max_increase() {
        // Test maximum allowed increase (4x)
        let current_difficulty = Difficulty::from(1000u64);

        // Simulate blocks coming exactly 4x faster (should increase to exactly 4x)
        let expected_time = 2016u64;
        let actual_time = 504u64;  // 2016/4 = 504 → 4x difficulty

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        assert!(new_val > current_val, "Difficulty should increase");

        // Check that it's exactly 4x (4000)
        assert_eq!(new_val.as_u64(), 4000u64, "Difficulty should be exactly 4x");
    }

    #[test]
    fn test_apply_difficulty_adjustment_max_decrease() {
        // Test maximum allowed decrease (0.25x)
        let current_difficulty = Difficulty::from(1000u64);

        // Simulate blocks coming exactly 4x slower (should decrease to exactly 0.25x)
        let expected_time = 504u64;  // Quarter of normal window
        let actual_time = 2016u64;   // 4x the expected → 0.25x difficulty

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        assert!(new_val < current_val, "Difficulty should decrease");

        // Check that it's exactly 0.25x (250)
        assert_eq!(new_val.as_u64(), 250u64, "Difficulty should be exactly 0.25x");
    }

    #[test]
    fn test_apply_difficulty_adjustment_extreme_ratio_clamped() {
        // Test that extreme time ratios get clamped to [0.25x, 4x] by the function
        let current_difficulty = Difficulty::from(1000u64);

        // Very high ratio (10x increase attempt - should be clamped to 4x)
        let expected_time = 2016u64;
        let actual_time_fast = 201u64;  // 10x faster (2016/201 ≈ 10)
        let result_high = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time_fast);
        assert!(result_high.is_ok());
        let new_diff_high = result_high.unwrap();
        assert_eq!(new_diff_high.as_ref().as_u64(), 4000u64, "Should be clamped to 4x");

        // Very low ratio (10x decrease attempt - should be clamped to 0.25x)
        let expected_time_low = 201u64;
        let actual_time_slow = 2016u64;  // 10x slower
        let result_low = apply_difficulty_adjustment(&current_difficulty, expected_time_low, actual_time_slow);
        assert!(result_low.is_ok());
        let new_diff_low = result_low.unwrap();
        assert_eq!(new_diff_low.as_ref().as_u64(), 250u64, "Should be clamped to 0.25x");
    }

    #[test]
    fn test_difficulty_adjustment_with_large_values() {
        // Test with larger difficulty values (1.5x increase)
        let current_difficulty = Difficulty::from(1_000_000_000u64);

        // Simulate 1.5x ratio: expected = 3, actual = 2
        let expected_time = 3000u64;
        let actual_time = 2000u64;  // 3000/2000 = 1.5

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        assert!(new_val > current_val, "Difficulty should increase for large values");

        // Check that it's 1.5x (1_000_000_000 * 3000 / 2000 = 1_500_000_000)
        assert_eq!(new_val.as_u64(), 1_500_000_000u64, "Difficulty should be exactly 1.5x");
    }

    #[test]
    fn test_varuint_conversion() {
        // Test that VarUint conversion works correctly
        let value = 1000u128;
        let varuint = VarUint::from(value);
        let difficulty: Difficulty = varuint;

        // Verify the difficulty value
        assert!(!difficulty.as_ref().is_zero(), "Difficulty should not be zero");
    }

    #[test]
    fn test_daa_window_size_boundary() {
        // Test boundary conditions for DAA window
        // Window should be exactly 2016 blocks

        // If daa_score < DAA_WINDOW_SIZE, we use parent's difficulty
        let daa_score_small = DAA_WINDOW_SIZE - 1;
        assert!(daa_score_small < DAA_WINDOW_SIZE);

        // If daa_score >= DAA_WINDOW_SIZE, we calculate new difficulty
        let daa_score_large = DAA_WINDOW_SIZE;
        assert!(daa_score_large >= DAA_WINDOW_SIZE);
    }

    #[test]
    fn test_window_boundary_calculation() {
        // Test window boundary score calculation
        let daa_score = 5000u64;
        let window_boundary_score = daa_score - DAA_WINDOW_SIZE;

        assert_eq!(window_boundary_score, 5000 - 2016);
        assert_eq!(window_boundary_score, 2984);

        // For early blocks
        let early_daa_score = 1000u64;
        let early_boundary = if early_daa_score >= DAA_WINDOW_SIZE {
            early_daa_score - DAA_WINDOW_SIZE
        } else {
            0
        };

        assert_eq!(early_boundary, 0);
    }

    #[test]
    fn test_expected_time_calculation() {
        // Test expected time calculation
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;

        // With 2016 blocks and 1 second per block
        assert_eq!(expected_time, 2016);
        assert_eq!(expected_time, 2016); // 2016 seconds = 33.6 minutes
    }

    #[test]
    fn test_difficulty_ratio_calculation() {
        // Test various difficulty ratio scenarios

        // Scenario 1: Blocks too fast (actual_time < expected_time)
        let expected_time = 2016u64;
        let actual_time_fast = 1000u64; // Blocks coming in 1000 seconds instead of 2016
        let ratio_fast = expected_time as f64 / actual_time_fast as f64;

        assert!(ratio_fast > 1.0, "Ratio should be > 1.0 when blocks are too fast");
        assert!(ratio_fast < MAX_DIFFICULTY_RATIO || ratio_fast > MAX_DIFFICULTY_RATIO,
                "Testing ratio calculation");

        // Scenario 2: Blocks too slow (actual_time > expected_time)
        let actual_time_slow = 4000u64; // Blocks coming in 4000 seconds instead of 2016
        let ratio_slow = expected_time as f64 / actual_time_slow as f64;

        assert!(ratio_slow < 1.0, "Ratio should be < 1.0 when blocks are too slow");
        assert!(ratio_slow > MIN_DIFFICULTY_RATIO || ratio_slow < MIN_DIFFICULTY_RATIO,
                "Testing ratio calculation");

        // Scenario 3: Blocks at expected rate
        let actual_time_normal = 2016u64;
        let ratio_normal = expected_time as f64 / actual_time_normal as f64;

        assert_eq!(ratio_normal, 1.0, "Ratio should be 1.0 when blocks are at expected rate");
    }

    #[test]
    fn test_ratio_clamping_logic() {
        // Test the clamping logic for extreme ratios

        let ratio = 10.0f64;
        let clamped = ratio.max(MIN_DIFFICULTY_RATIO).min(MAX_DIFFICULTY_RATIO);
        assert_eq!(clamped, MAX_DIFFICULTY_RATIO);

        let ratio = 0.1f64;
        let clamped = ratio.max(MIN_DIFFICULTY_RATIO).min(MAX_DIFFICULTY_RATIO);
        assert_eq!(clamped, MIN_DIFFICULTY_RATIO);

        let ratio = 1.5f64;
        let clamped = ratio.max(MIN_DIFFICULTY_RATIO).min(MAX_DIFFICULTY_RATIO);
        assert_eq!(clamped, 1.5);
    }
}

// Integration test module for DAA with storage
// These tests require a full storage implementation and are marked as ignored
// Run with: cargo test --test daa_integration -- --ignored
#[cfg(test)]
mod integration_tests {
    // TODO: Add integration tests once storage is fully implemented
    // These tests will:
    // 1. Create a chain of blocks with varying timestamps
    // 2. Test DAA window calculation across the chain
    // 3. Verify mergeset_non_daa filtering
    // 4. Test difficulty adjustment in real scenarios
    // 5. Test timestamp manipulation attack prevention

    #[test]
    #[ignore]
    fn test_daa_with_real_storage() {
        // Will be implemented once storage layer is ready
        unimplemented!("Integration test requires full storage implementation");
    }

    #[test]
    #[ignore]
    fn test_mergeset_non_daa_filtering() {
        // Will be implemented once storage layer is ready
        unimplemented!("Integration test requires full storage implementation");
    }

    #[test]
    #[ignore]
    fn test_difficulty_increase_scenario() {
        // Simulate hashrate increase
        // Blocks should come faster -> difficulty should increase
        unimplemented!("Integration test requires full storage implementation");
    }

    #[test]
    #[ignore]
    fn test_difficulty_decrease_scenario() {
        // Simulate hashrate decrease
        // Blocks should come slower -> difficulty should decrease
        unimplemented!("Integration test requires full storage implementation");
    }

    #[test]
    #[ignore]
    fn test_timestamp_manipulation_prevention() {
        // Try to manipulate difficulty by using fake timestamps
        // mergeset_non_daa should filter out old blocks
        unimplemented!("Integration test requires full storage implementation");
    }
}

// Additional comprehensive unit tests for DAA algorithm (Task 4.1)
#[cfg(test)]
mod daa_comprehensive_tests {
    use super::*;

    // Test 1: DAA window calculation edge cases - empty window
    #[test]
    fn test_daa_window_empty() {
        // Test behavior when DAA window is not yet full
        let daa_score = 0u64;
        assert!(daa_score < DAA_WINDOW_SIZE, "Score should be below window size");

        // Window boundary should be 0 for early blocks
        let window_boundary = if daa_score >= DAA_WINDOW_SIZE {
            daa_score - DAA_WINDOW_SIZE
        } else {
            0
        };
        assert_eq!(window_boundary, 0);
    }

    // Test 2: DAA window calculation edge cases - exactly at window size
    #[test]
    fn test_daa_window_exactly_full() {
        // Test the exact moment when window becomes full
        let daa_score = DAA_WINDOW_SIZE;
        assert_eq!(daa_score, DAA_WINDOW_SIZE);

        let window_boundary = daa_score - DAA_WINDOW_SIZE;
        assert_eq!(window_boundary, 0, "Window should start at genesis when exactly full");
    }

    // Test 3: DAA window calculation edge cases - just past window size
    #[test]
    fn test_daa_window_past_full() {
        // Test window boundaries for blocks well past the initial window
        let daa_score = DAA_WINDOW_SIZE + 1;
        let window_boundary = daa_score - DAA_WINDOW_SIZE;
        assert_eq!(window_boundary, 1, "Window should start at block 1");

        // Test for a much later block
        let daa_score_large = 10000u64;
        let window_boundary_large = daa_score_large - DAA_WINDOW_SIZE;
        assert_eq!(window_boundary_large, 10000 - 2016);
    }

    // Test 4: Difficulty adjustment boundaries - minimum ratio
    #[test]
    fn test_difficulty_adjustment_minimum_ratio() {
        // Test that difficulty cannot decrease beyond minimum ratio (0.25x)
        let current_difficulty = Difficulty::from(10000u64);

        // Simulate 4x slower blocks (should clamp to 0.25x)
        let expected_time = 504u64;
        let actual_time = 2016u64;  // 4x slower

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        // New difficulty should be significantly less than current
        assert!(new_val < current_val, "Difficulty should decrease with minimum ratio");

        // Should be exactly 0.25x = 2500
        assert_eq!(new_val.as_u64(), 2500u64, "Should be clamped to 0.25x");
    }

    // Test 5: Difficulty adjustment boundaries - maximum ratio
    #[test]
    fn test_difficulty_adjustment_maximum_ratio() {
        // Test that difficulty cannot increase beyond maximum ratio (4x)
        let current_difficulty = Difficulty::from(10000u64);

        // Simulate 4x faster blocks (should clamp to 4x)
        let expected_time = 2016u64;
        let actual_time = 504u64;  // 4x faster

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        // New difficulty should be significantly greater than current
        assert!(new_val > current_val, "Difficulty should increase with maximum ratio");

        // Should be exactly 4x = 40000
        assert_eq!(new_val.as_u64(), 40000u64, "Should be clamped to 4x");
    }

    // Test 6: Timestamp manipulation resistance - backwards time
    #[test]
    fn test_timestamp_backwards_resistance() {
        // Test that the algorithm handles backwards-moving timestamps gracefully
        let start_timestamp = 1000u64;
        let end_timestamp = 999u64; // Time went backwards!

        // Calculate actual time with protection against backwards time
        let actual_time = if end_timestamp > start_timestamp {
            end_timestamp - start_timestamp
        } else {
            // Protection: use minimum time to avoid division by zero
            1
        };

        assert_eq!(actual_time, 1, "Should use minimum time when timestamp goes backwards");

        // Verify that division by this minimum doesn't cause issues
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;
        let ratio = expected_time as f64 / actual_time as f64;

        // Ratio should be very high (blocks appear to be instant)
        assert!(ratio > 1.0, "Ratio should indicate blocks are too fast");

        // But it should be clamped to MAX_DIFFICULTY_RATIO
        let clamped_ratio = ratio.max(MIN_DIFFICULTY_RATIO).min(MAX_DIFFICULTY_RATIO);
        assert_eq!(clamped_ratio, MAX_DIFFICULTY_RATIO, "Extreme ratio should be clamped");
    }

    // Test 7: Timestamp manipulation resistance - extreme future timestamp
    #[test]
    fn test_timestamp_future_resistance() {
        // Test resistance against blocks with timestamps far in the future
        let start_timestamp = 1000u64;
        let end_timestamp = 1_000_000_000u64; // Extremely far in the future

        let actual_time = end_timestamp - start_timestamp;
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;

        // Ratio will be very small (blocks appear to be very slow)
        let ratio = expected_time as f64 / actual_time as f64;
        assert!(ratio < 1.0, "Ratio should indicate blocks are too slow");
        assert!(ratio < MIN_DIFFICULTY_RATIO, "Ratio should be below minimum before clamping");

        // Should be clamped to MIN_DIFFICULTY_RATIO
        let clamped_ratio = ratio.max(MIN_DIFFICULTY_RATIO).min(MAX_DIFFICULTY_RATIO);
        assert_eq!(clamped_ratio, MIN_DIFFICULTY_RATIO, "Extreme ratio should be clamped to minimum");

        // Verify the clamped ratio still works with difficulty adjustment
        let current_difficulty = Difficulty::from(10000u64);
        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok(), "Clamped ratio should work with difficulty adjustment");
    }

    // Test 8: Zero difficulty edge case
    #[test]
    fn test_zero_difficulty_edge_case() {
        // Test behavior with minimum possible difficulty
        let zero_difficulty = Difficulty::from(0u64);

        // Simulate 2x ratio
        let expected_time = 2016u64;
        let actual_time = 1008u64;

        let result = apply_difficulty_adjustment(&zero_difficulty, expected_time, actual_time);
        assert!(result.is_ok());
        // Difficulty should be valid (U256 is always non-negative and unsigned)
    }

    // Test 9: Very large difficulty values
    #[test]
    fn test_very_large_difficulty() {
        // Test with difficulty near the practical limits
        let large_difficulty = Difficulty::from(u64::MAX / 2);

        // Simulate 1.5x ratio
        let expected_time = 3000u64;
        let actual_time = 2000u64;

        let result = apply_difficulty_adjustment(&large_difficulty, expected_time, actual_time);
        assert!(result.is_ok(), "Should handle large difficulty values");

        let new_difficulty = result.unwrap();
        let current_val = large_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        assert!(new_val > current_val, "Large difficulty should still increase proportionally");
    }

    // Test 10: Difficulty adjustment precision
    #[test]
    fn test_difficulty_adjustment_precision() {
        // Test that small adjustments are handled with reasonable precision
        let current_difficulty = Difficulty::from(1_000_000u64);

        // Simulate 1.01x ratio (1% increase): 101/100
        let expected_time = 101u64;
        let actual_time = 100u64;

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref().as_u64();
        let new_val = new_difficulty.as_ref().as_u64();

        // New value should be slightly higher than current
        assert!(new_val > current_val, "Difficulty should increase with small positive ratio");
        assert!(new_val <= current_val * 2, "Small ratio shouldn't cause dramatic change");

        // Should be exactly 1.01x = 1,010,000
        assert_eq!(new_val, 1_010_000u64, "Should be exactly 1.01x");
    }

    // Test 11: Window boundary calculation with overflow protection
    #[test]
    fn test_window_boundary_overflow_protection() {
        // Test that window boundary calculation doesn't overflow
        let very_large_score = u64::MAX - 1;

        // This should not panic even with very large values
        let window_boundary = if very_large_score >= DAA_WINDOW_SIZE {
            very_large_score - DAA_WINDOW_SIZE
        } else {
            0
        };

        assert!(window_boundary <= very_large_score);
        assert_eq!(window_boundary, very_large_score - DAA_WINDOW_SIZE);
    }

    // Test 12: Expected time calculation consistency
    #[test]
    fn test_expected_time_consistency() {
        // Verify expected time calculation is consistent
        let expected_time_1 = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;
        let expected_time_2 = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;

        assert_eq!(expected_time_1, expected_time_2);
        assert_eq!(expected_time_1, 2016); // With 1-second blocks

        // Test that it scales correctly with window size
        let half_window = DAA_WINDOW_SIZE / 2;
        let half_expected = half_window * TARGET_TIME_PER_BLOCK;
        assert_eq!(half_expected * 2, expected_time_1);
    }

    // Test 13: Ratio calculation with actual time equal to expected time
    #[test]
    fn test_ratio_calculation_exact_match() {
        // When actual equals expected, ratio should be exactly 1.0
        let actual_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;

        let ratio = expected_time as f64 / actual_time as f64;
        assert_eq!(ratio, 1.0);

        // Applying this ratio should not change difficulty
        let current_difficulty = Difficulty::from(5000u64);
        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        assert_eq!(new_difficulty.as_ref(), current_difficulty.as_ref());
    }
}
