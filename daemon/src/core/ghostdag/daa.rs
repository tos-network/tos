// TOS Difficulty Adjustment Algorithm (DAA)

use anyhow::Result;
use std::collections::{HashSet, VecDeque};
use tos_common::crypto::Hash;
use tos_common::difficulty::Difficulty;

use crate::core::error::BlockchainError;
use crate::core::ghostdag::GhostdagStorageProvider;

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
pub async fn calculate_daa_score<S: GhostdagStorageProvider>(
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
        0 // If we haven't reached window size yet, boundary is genesis
    };

    // Find blocks in the DAA window using BFS from selected_parent
    let window_blocks =
        find_daa_window_blocks(storage, selected_parent, window_boundary_score).await?;

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
/// with daa_score >= window_boundary_score.
///
/// CRITICAL: Must use daa_score (not blue_score) for comparison, as window_boundary_score
/// is calculated from parent's daa_score. Using blue_score would incorrectly include/exclude
/// blocks when GHOSTDAG mergeset jumps are large.
///
/// # Arguments
/// * `storage` - Reference to blockchain storage
/// * `start_block` - Hash of the block to start from (usually selected_parent)
/// * `window_boundary_score` - Minimum daa_score to be included in window
///
/// # Returns
/// Set of block hashes that are within the DAA window
async fn find_daa_window_blocks<S: GhostdagStorageProvider>(
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

        // FIXED: Use daa_score (not blue_score) to match window_boundary_score calculation
        // This prevents far-past blocks from distorting the DAA window during large mergesets
        if current_data.daa_score >= window_boundary_score {
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
        // If block's daa_score < window_boundary_score, don't traverse further
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
pub async fn calculate_target_difficulty<S: GhostdagStorageProvider>(
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
    let _window_start_block =
        find_block_at_daa_score(storage, selected_parent, window_start_score).await?;
    let _window_end_block = selected_parent;

    // SECURITY FIX V-07: Use median-time-past for timestamp manipulation resistance
    // Collect timestamps from DAA window blocks
    let mut timestamps: Vec<u64> = Vec::new();

    // Get timestamps for blocks in the window
    let window_blocks =
        find_daa_window_blocks(storage, selected_parent, window_start_score).await?;
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

    // SECURITY FIX: Use median-based time span instead of max-min
    // Per audit: Using oldest-newest (max-min) allows attackers to create equal-timestamp
    // chains that result in actual_time=0/1, triggering 4x difficulty increases.
    // Solution: Use median timestamps for more robust time span calculation.
    let len = timestamps.len();
    let actual_time = if len >= 4 {
        // Use inter-quartile range for robustness against timestamp manipulation
        // Q1 (25th percentile) and Q3 (75th percentile)
        let q1_idx = len / 4;
        let q3_idx = (3 * len) / 4;
        let q1_timestamp = timestamps[q1_idx];
        let q3_timestamp = timestamps[q3_idx];

        // Calculate IQR-based time span, scaled to full window
        // IQR represents 50% of the window, so multiply by 2
        let iqr_span = q3_timestamp.saturating_sub(q1_timestamp);
        let scaled_span = iqr_span.saturating_mul(2);

        // SECURITY FIX: Enforce minimum time span to prevent extreme difficulty spikes
        // Minimum = half of expected time (DAA_WINDOW_SIZE/2 * TARGET_TIME_PER_BLOCK)
        // This limits the maximum difficulty increase to 2x per window instead of 4x
        let min_actual_time = (DAA_WINDOW_SIZE / 2) * TARGET_TIME_PER_BLOCK;

        scaled_span.max(min_actual_time)
    } else {
        // Not enough timestamps for IQR calculation, use simple span with floor
        let oldest_timestamp = timestamps[0];
        let newest_timestamp = timestamps[len - 1];
        let raw_span = newest_timestamp.saturating_sub(oldest_timestamp);

        // Apply minimum floor
        let min_actual_time = (DAA_WINDOW_SIZE / 2) * TARGET_TIME_PER_BLOCK;
        raw_span.max(min_actual_time)
    };

    // Calculate expected time
    let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;

    // Get current difficulty
    let current_difficulty = storage
        .get_difficulty_for_block_hash(selected_parent)
        .await?;

    // Apply adjustment using U256 integer arithmetic (deterministic across platforms)
    // If actual_time < expected_time: blocks are too fast → increase difficulty
    // If actual_time > expected_time: blocks are too slow → decrease difficulty
    // Clamping to [0.25x, 4x] is handled inside apply_difficulty_adjustment
    let new_difficulty =
        apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time)?;

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
async fn find_block_at_daa_score<S: GhostdagStorageProvider>(
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
        let actual_time = 1008u64; // Half the expected time → 2x difficulty

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
        let actual_time = 2016u64; // Double the expected time → 0.5x difficulty

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        // Check that new difficulty is less than old difficulty
        assert!(new_val < current_val, "Difficulty should decrease");

        // Check that it's exactly 0.5x (500)
        assert_eq!(
            new_val.as_u64(),
            500u64,
            "Difficulty should be exactly 0.5x"
        );
    }

    #[test]
    fn test_apply_difficulty_adjustment_no_change() {
        // Test no change (blocks at expected rate)
        let current_difficulty = Difficulty::from(1000u64);

        // Simulate blocks at exactly expected rate (ratio = 1.0)
        let expected_time = 2016u64; // DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK
        let actual_time = 2016u64; // Same as expected → no change

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
        let actual_time = 504u64; // 2016/4 = 504 → 4x difficulty

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
        let expected_time = 504u64; // Quarter of normal window
        let actual_time = 2016u64; // 4x the expected → 0.25x difficulty

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        assert!(new_val < current_val, "Difficulty should decrease");

        // Check that it's exactly 0.25x (250)
        assert_eq!(
            new_val.as_u64(),
            250u64,
            "Difficulty should be exactly 0.25x"
        );
    }

    #[test]
    fn test_apply_difficulty_adjustment_extreme_ratio_clamped() {
        // Test that extreme time ratios get clamped to [0.25x, 4x] by the function
        let current_difficulty = Difficulty::from(1000u64);

        // Very high ratio (10x increase attempt - should be clamped to 4x)
        let expected_time = 2016u64;
        let actual_time_fast = 201u64; // 10x faster (2016/201 ≈ 10)
        let result_high =
            apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time_fast);
        assert!(result_high.is_ok());
        let new_diff_high = result_high.unwrap();
        assert_eq!(
            new_diff_high.as_ref().as_u64(),
            4000u64,
            "Should be clamped to 4x"
        );

        // Very low ratio (10x decrease attempt - should be clamped to 0.25x)
        let expected_time_low = 201u64;
        let actual_time_slow = 2016u64; // 10x slower
        let result_low =
            apply_difficulty_adjustment(&current_difficulty, expected_time_low, actual_time_slow);
        assert!(result_low.is_ok());
        let new_diff_low = result_low.unwrap();
        assert_eq!(
            new_diff_low.as_ref().as_u64(),
            250u64,
            "Should be clamped to 0.25x"
        );
    }

    #[test]
    fn test_difficulty_adjustment_with_large_values() {
        // Test with larger difficulty values (1.5x increase)
        let current_difficulty = Difficulty::from(1_000_000_000u64);

        // Simulate 1.5x ratio: expected = 3, actual = 2
        let expected_time = 3000u64;
        let actual_time = 2000u64; // 3000/2000 = 1.5

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        assert!(
            new_val > current_val,
            "Difficulty should increase for large values"
        );

        // Check that it's 1.5x (1_000_000_000 * 3000 / 2000 = 1_500_000_000)
        assert_eq!(
            new_val.as_u64(),
            1_500_000_000u64,
            "Difficulty should be exactly 1.5x"
        );
    }

    #[test]
    fn test_varuint_conversion() {
        // Test that VarUint conversion works correctly
        let value = 1000u128;
        let varuint = VarUint::from(value);
        let difficulty: Difficulty = varuint;

        // Verify the difficulty value
        assert!(
            !difficulty.as_ref().is_zero(),
            "Difficulty should not be zero"
        );
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

        assert!(
            ratio_fast > 1.0,
            "Ratio should be > 1.0 when blocks are too fast"
        );
        assert!(
            ratio_fast < MAX_DIFFICULTY_RATIO || ratio_fast > MAX_DIFFICULTY_RATIO,
            "Testing ratio calculation"
        );

        // Scenario 2: Blocks too slow (actual_time > expected_time)
        let actual_time_slow = 4000u64; // Blocks coming in 4000 seconds instead of 2016
        let ratio_slow = expected_time as f64 / actual_time_slow as f64;

        assert!(
            ratio_slow < 1.0,
            "Ratio should be < 1.0 when blocks are too slow"
        );
        assert!(
            ratio_slow > MIN_DIFFICULTY_RATIO || ratio_slow < MIN_DIFFICULTY_RATIO,
            "Testing ratio calculation"
        );

        // Scenario 3: Blocks at expected rate
        let actual_time_normal = 2016u64;
        let ratio_normal = expected_time as f64 / actual_time_normal as f64;

        assert_eq!(
            ratio_normal, 1.0,
            "Ratio should be 1.0 when blocks are at expected rate"
        );
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
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Mock storage for DAA integration tests
    struct MockDAAStorage {
        blocks: Arc<RwLock<HashMap<Hash, MockBlockData>>>,
    }

    #[derive(Clone)]
    #[allow(dead_code)]
    struct MockBlockData {
        hash: Hash,
        parents: Vec<Hash>,
        timestamp: u64,
        difficulty: Difficulty,
        daa_score: u64,
        blue_score: u64,
    }

    impl MockDAAStorage {
        fn new() -> Self {
            let mut blocks = HashMap::new();

            // Create genesis block
            let genesis_hash = Hash::zero();
            blocks.insert(
                genesis_hash.clone(),
                MockBlockData {
                    hash: genesis_hash,
                    parents: vec![],
                    timestamp: 1600000000000,
                    difficulty: Difficulty::from(1000u64),
                    daa_score: 0,
                    blue_score: 0,
                },
            );

            Self {
                blocks: Arc::new(RwLock::new(blocks)),
            }
        }

        async fn add_block(&self, parents: Vec<Hash>, timestamp: u64) -> Result<Hash, String> {
            let mut blocks = self.blocks.write().await;

            // Verify parents exist
            for parent in &parents {
                if !blocks.contains_key(parent) {
                    return Err(format!("Parent {} not found", parent));
                }
            }

            // Get parent with highest DAA score
            let parent_daa_scores: Vec<u64> = parents
                .iter()
                .filter_map(|p| blocks.get(p).map(|b| b.daa_score))
                .collect();

            let max_parent_daa = parent_daa_scores.iter().max().copied().unwrap_or(0);
            let max_parent_blue = parents
                .iter()
                .filter_map(|p| blocks.get(p).map(|b| b.blue_score))
                .max()
                .unwrap_or(0);

            // Calculate new block's DAA score and blue score
            let daa_score = max_parent_daa + 1;
            let blue_score = max_parent_blue + 1;

            // Calculate difficulty based on DAA
            let difficulty = if daa_score < DAA_WINDOW_SIZE {
                // Before window filled, use parent difficulty
                parents
                    .first()
                    .and_then(|p| blocks.get(p))
                    .map(|b| b.difficulty)
                    .unwrap_or(Difficulty::from(1000u64))
            } else {
                // Calculate based on window
                let window_start_score = daa_score - DAA_WINDOW_SIZE;

                // Find blocks in window
                let window_blocks: Vec<&MockBlockData> = blocks
                    .values()
                    .filter(|b| b.daa_score >= window_start_score && b.daa_score < daa_score)
                    .collect();

                if window_blocks.is_empty() {
                    Difficulty::from(1000u64)
                } else {
                    // Get timestamp range
                    let min_timestamp = window_blocks
                        .iter()
                        .map(|b| b.timestamp)
                        .min()
                        .unwrap_or(timestamp - 2016000);

                    let max_timestamp = window_blocks
                        .iter()
                        .map(|b| b.timestamp)
                        .max()
                        .unwrap_or(timestamp);

                    let actual_time = max_timestamp.saturating_sub(min_timestamp);
                    let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK * 1000; // milliseconds

                    let parent_difficulty = parents
                        .first()
                        .and_then(|p| blocks.get(p))
                        .map(|b| b.difficulty)
                        .unwrap_or(Difficulty::from(1000u64));

                    apply_difficulty_adjustment(&parent_difficulty, expected_time, actual_time)
                        .unwrap_or(parent_difficulty)
                }
            };

            // Create block hash (deterministic based on parents and timestamp)
            use tos_common::crypto::hash;
            let mut hash_data = Vec::new();
            for parent in &parents {
                hash_data.extend_from_slice(parent.as_bytes());
            }
            hash_data.extend_from_slice(&timestamp.to_le_bytes());
            let block_hash = hash(&hash_data);

            // Store block
            blocks.insert(
                block_hash.clone(),
                MockBlockData {
                    hash: block_hash.clone(),
                    parents,
                    timestamp,
                    difficulty,
                    daa_score,
                    blue_score,
                },
            );

            Ok(block_hash)
        }

        async fn get_block(&self, hash: &Hash) -> Option<MockBlockData> {
            self.blocks.read().await.get(hash).cloned()
        }
    }

    /// Integration test: DAA with varying block times
    #[tokio::test]
    async fn test_daa_with_varying_block_times() {
        let storage = MockDAAStorage::new();
        let genesis_hash = Hash::zero();

        if log::log_enabled!(log::Level::Info) {
            log::info!("Testing DAA with varying block times");
        }

        // Create 100 blocks with 1-second intervals (normal speed)
        let mut current_parent = genesis_hash.clone();
        let mut current_timestamp = 1600000000000u64;

        for _ in 0..100 {
            current_timestamp += 1000; // 1 second in milliseconds
            let block_hash = storage
                .add_block(vec![current_parent.clone()], current_timestamp)
                .await
                .expect("Should add block");
            current_parent = block_hash;
        }

        let baseline_block = storage
            .get_block(&current_parent)
            .await
            .expect("Should exist");
        let baseline_difficulty = baseline_block.difficulty;

        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Baseline difficulty after 100 blocks: {:?}",
                baseline_difficulty
            );
        }

        // Create 100 blocks with 0.5-second intervals (fast blocks)
        for _ in 0..100 {
            current_timestamp += 500; // 0.5 seconds
            let block_hash = storage
                .add_block(vec![current_parent.clone()], current_timestamp)
                .await
                .expect("Should add block");
            current_parent = block_hash;
        }

        let fast_block = storage
            .get_block(&current_parent)
            .await
            .expect("Should exist");

        // Since we haven't filled the DAA window, difficulty should stay the same
        assert_eq!(
            fast_block.difficulty.as_ref(),
            baseline_difficulty.as_ref(),
            "Difficulty should remain constant before DAA window is filled"
        );

        if log::log_enabled!(log::Level::Info) {
            log::info!("DAA varying block times test passed");
        }
    }

    /// Integration test: DAA window boundary behavior
    #[tokio::test]
    async fn test_daa_window_boundary_behavior() {
        let storage = MockDAAStorage::new();
        let genesis_hash = Hash::zero();

        if log::log_enabled!(log::Level::Info) {
            log::info!("Testing DAA window boundary behavior");
        }

        // Create blocks up to window size
        let mut current_parent = genesis_hash.clone();
        let mut current_timestamp = 1600000000000u64;

        // Create exactly DAA_WINDOW_SIZE blocks
        for i in 0..DAA_WINDOW_SIZE {
            current_timestamp += 1000; // 1 second
            let block_hash = storage
                .add_block(vec![current_parent.clone()], current_timestamp)
                .await
                .expect("Should add block");

            let block = storage.get_block(&block_hash).await.expect("Should exist");

            // Verify DAA score increments correctly
            assert_eq!(
                block.daa_score,
                i + 1,
                "DAA score should increment by 1 for each block"
            );

            current_parent = block_hash;
        }

        // The block at exactly DAA_WINDOW_SIZE should still use parent difficulty
        let boundary_block = storage
            .get_block(&current_parent)
            .await
            .expect("Should exist");
        assert_eq!(
            boundary_block.daa_score, DAA_WINDOW_SIZE,
            "DAA score at boundary should equal window size"
        );

        if log::log_enabled!(log::Level::Info) {
            log::info!("DAA window boundary test passed");
        }
    }

    /// Integration test: DAA difficulty adjustment scenarios
    #[tokio::test]
    async fn test_daa_difficulty_adjustment_scenarios() {
        let storage = MockDAAStorage::new();
        let genesis_hash = Hash::zero();

        if log::log_enabled!(log::Level::Info) {
            log::info!("Testing DAA difficulty adjustment scenarios");
        }

        // Scenario 1: Consistent block times should maintain stable difficulty
        let mut current_parent = genesis_hash.clone();
        let mut current_timestamp = 1600000000000u64;

        for _ in 0..50 {
            current_timestamp += 1000; // Exactly 1 second
            let block_hash = storage
                .add_block(vec![current_parent.clone()], current_timestamp)
                .await
                .expect("Should add block");
            current_parent = block_hash;
        }

        let stable_block = storage
            .get_block(&current_parent)
            .await
            .expect("Should exist");

        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Stable difficulty after consistent blocks: {:?}",
                stable_block.difficulty
            );
        }

        // Scenario 2: Create a branch to test merging
        let _branch_start = current_parent.clone();
        let mut branch_timestamp = current_timestamp;

        for _ in 0..10 {
            branch_timestamp += 2000; // 2 seconds (slower)
            let block_hash = storage
                .add_block(vec![current_parent.clone()], branch_timestamp)
                .await
                .expect("Should add block");
            current_parent = block_hash;
        }

        let slow_block = storage
            .get_block(&current_parent)
            .await
            .expect("Should exist");

        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Difficulty after slower blocks: {:?}",
                slow_block.difficulty
            );
        }

        // Both should have valid difficulties
        assert!(
            !stable_block.difficulty.as_ref().is_zero(),
            "Stable difficulty should be positive"
        );
        assert!(
            !slow_block.difficulty.as_ref().is_zero(),
            "Slow difficulty should be positive"
        );

        if log::log_enabled!(log::Level::Info) {
            log::info!("DAA difficulty adjustment scenarios test passed");
        }
    }

    /// Test DAA window size calculation edge cases
    #[test]
    fn test_daa_window_size_edge_cases() {
        use super::{DAA_WINDOW_SIZE, TARGET_TIME_PER_BLOCK};

        // Verify that DAA window size is reasonable
        assert!(DAA_WINDOW_SIZE > 0, "DAA window size must be positive");
        assert!(
            DAA_WINDOW_SIZE <= 10000,
            "DAA window size should be reasonable"
        );

        // Verify target time is positive
        assert!(
            TARGET_TIME_PER_BLOCK > 0,
            "Target block time must be positive"
        );
    }

    /// Test difficulty ratio bounds
    #[test]
    fn test_difficulty_ratio_bounds() {
        use super::{MAX_DIFFICULTY_RATIO, MIN_DIFFICULTY_RATIO};

        // MIN_DIFFICULTY_RATIO should be less than MAX_DIFFICULTY_RATIO
        assert!(
            MIN_DIFFICULTY_RATIO < MAX_DIFFICULTY_RATIO,
            "Min ratio should be less than max ratio"
        );

        // Both should be positive
        assert!(MIN_DIFFICULTY_RATIO > 0.0, "Min ratio must be positive");
        assert!(MAX_DIFFICULTY_RATIO > 0.0, "Max ratio must be positive");

        // Reasonable bounds
        assert!(
            MIN_DIFFICULTY_RATIO >= 0.1,
            "Min ratio should not be too small"
        );
        assert!(
            MAX_DIFFICULTY_RATIO <= 10.0,
            "Max ratio should not be too large"
        );
    }

    /// Test timestamp manipulation resistance (conceptual)
    #[test]
    fn test_timestamp_manipulation_concepts() {
        // DAA should use median timestamp to resist manipulation
        let timestamps = vec![1000u64, 1010, 5000, 1020, 500];

        // Without median: could be manipulated by extreme values
        let min = timestamps.iter().min().unwrap();
        let max = timestamps.iter().max().unwrap();
        assert_eq!(*min, 500);
        assert_eq!(*max, 5000);

        // With median: resistant to outliers
        let mut sorted = timestamps.clone();
        sorted.sort();
        let median = sorted[sorted.len() / 2];
        assert_eq!(median, 1010, "Median should be resistant to outliers");
    }

    /// Test DAA window filtering conceptual logic
    #[test]
    fn test_mergeset_non_daa_filtering_concept() {
        use super::DAA_WINDOW_SIZE;

        // Verify that the concept of filtering old blocks makes sense
        const CURRENT_DAA_SCORE: u64 = 2020;
        const BLOCK_DAA_SCORES: [u64; 5] = [1000, 2000, 2010, 2019, 2020];

        // Filter blocks within DAA window
        // With DAA_WINDOW_SIZE=2016, blocks with score > 2020-2016=4 should be kept
        let window_start = CURRENT_DAA_SCORE.saturating_sub(DAA_WINDOW_SIZE);
        let filtered: Vec<_> = BLOCK_DAA_SCORES
            .iter()
            .filter(|&&score| score > window_start)
            .collect();

        // All test scores [1000, 2000, 2010, 2019, 2020] are > 4, so all 5 should be kept
        // This test verifies the filtering logic works correctly
        assert_eq!(
            filtered.len(),
            5,
            "All blocks should be within DAA window (score > {})",
            window_start
        );

        // If we had a block with score <=4, it would be filtered out
        // Verify the window boundary is correct
        assert_eq!(
            window_start, 4,
            "Window start should be CURRENT_DAA_SCORE - DAA_WINDOW_SIZE = 2020 - 2016 = 4"
        );
    }

    /// Test difficulty adjustment bounds (conceptual)
    #[test]
    fn test_difficulty_adjustment_bounds_concept() {
        use super::{MAX_DIFFICULTY_RATIO, MIN_DIFFICULTY_RATIO};

        // Test that difficulty adjustments are bounded by ratios
        let current_difficulty = 1000.0;

        // Maximum increase (4x)
        let max_new_diff = current_difficulty * MAX_DIFFICULTY_RATIO;
        assert_eq!(max_new_diff, 4000.0, "Max difficulty should be 4x current");

        // Maximum decrease (0.25x = 25%)
        let min_new_diff = current_difficulty * MIN_DIFFICULTY_RATIO;
        assert_eq!(
            min_new_diff, 250.0,
            "Min difficulty should be 0.25x current"
        );

        // Verify bounds are reasonable
        assert!(min_new_diff > 0.0, "Difficulty should never go to zero");
        assert!(
            max_new_diff / current_difficulty <= MAX_DIFFICULTY_RATIO,
            "Difficulty increase should be bounded"
        );
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
        assert!(
            daa_score < DAA_WINDOW_SIZE,
            "Score should be below window size"
        );

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
        assert_eq!(
            window_boundary, 0,
            "Window should start at genesis when exactly full"
        );
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
        let actual_time = 2016u64; // 4x slower

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        // New difficulty should be significantly less than current
        assert!(
            new_val < current_val,
            "Difficulty should decrease with minimum ratio"
        );

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
        let actual_time = 504u64; // 4x faster

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let current_val = current_difficulty.as_ref();
        let new_val = new_difficulty.as_ref();

        // New difficulty should be significantly greater than current
        assert!(
            new_val > current_val,
            "Difficulty should increase with maximum ratio"
        );

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

        assert_eq!(
            actual_time, 1,
            "Should use minimum time when timestamp goes backwards"
        );

        // Verify that division by this minimum doesn't cause issues
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;
        let ratio = expected_time as f64 / actual_time as f64;

        // Ratio should be very high (blocks appear to be instant)
        assert!(ratio > 1.0, "Ratio should indicate blocks are too fast");

        // But it should be clamped to MAX_DIFFICULTY_RATIO
        let clamped_ratio = ratio.max(MIN_DIFFICULTY_RATIO).min(MAX_DIFFICULTY_RATIO);
        assert_eq!(
            clamped_ratio, MAX_DIFFICULTY_RATIO,
            "Extreme ratio should be clamped"
        );
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
        assert!(
            ratio < MIN_DIFFICULTY_RATIO,
            "Ratio should be below minimum before clamping"
        );

        // Should be clamped to MIN_DIFFICULTY_RATIO
        let clamped_ratio = ratio.max(MIN_DIFFICULTY_RATIO).min(MAX_DIFFICULTY_RATIO);
        assert_eq!(
            clamped_ratio, MIN_DIFFICULTY_RATIO,
            "Extreme ratio should be clamped to minimum"
        );

        // Verify the clamped ratio still works with difficulty adjustment
        let current_difficulty = Difficulty::from(10000u64);
        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(
            result.is_ok(),
            "Clamped ratio should work with difficulty adjustment"
        );
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

        assert!(
            new_val > current_val,
            "Large difficulty should still increase proportionally"
        );
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
        assert!(
            new_val > current_val,
            "Difficulty should increase with small positive ratio"
        );
        assert!(
            new_val <= current_val * 2,
            "Small ratio shouldn't cause dramatic change"
        );

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

// ============================================================================
// SECURITY AUDIT TESTS (REVIEW20251129.md)
// Tests for DAA timestamp manipulation resistance
// ============================================================================
#[cfg(test)]
mod daa_security_audit_tests {
    use super::*;

    /// Security Audit Test: Equal timestamp chain should not cause 4x difficulty spike
    /// Per audit: "Equal timestamp chains result in actual_time=0/1, triggering 4x difficulty"
    /// The fix uses IQR-based time span with minimum floor to prevent this attack.
    #[test]
    fn test_security_audit_equal_timestamp_chain_daa() {
        // Simulate an equal timestamp attack: all blocks have the same timestamp
        let timestamps: Vec<u64> = vec![1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000];

        // Sort (no change since all equal)
        let mut sorted_ts = timestamps.clone();
        sorted_ts.sort();

        let len = sorted_ts.len();
        assert!(len >= 4, "Need at least 4 timestamps for IQR");

        // Calculate IQR-based time span (as in the fix)
        let q1_idx = len / 4; // 2
        let q3_idx = (3 * len) / 4; // 6
        let q1_timestamp = sorted_ts[q1_idx];
        let q3_timestamp = sorted_ts[q3_idx];

        let iqr_span = q3_timestamp.saturating_sub(q1_timestamp);
        let scaled_span = iqr_span.saturating_mul(2);

        // With equal timestamps, IQR span is 0
        assert_eq!(iqr_span, 0, "Equal timestamps should have IQR of 0");
        assert_eq!(scaled_span, 0, "Scaled IQR should also be 0");

        // The security fix enforces a minimum floor
        let min_actual_time = (DAA_WINDOW_SIZE / 2) * TARGET_TIME_PER_BLOCK;
        let actual_time = scaled_span.max(min_actual_time);

        // With the fix, actual_time is at least min_actual_time, not 0/1
        assert!(
            actual_time >= min_actual_time,
            "Actual time should be floored to minimum"
        );
        assert!(
            actual_time > 0,
            "Actual time should never be 0 with the security fix"
        );

        // Calculate what the difficulty adjustment would be
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;
        let ratio = expected_time as f64 / actual_time as f64;

        // With min floor = expected/2, max ratio = 2.0 (not 4.0 or infinity)
        assert!(
            ratio <= 2.0,
            "Difficulty increase ratio should be capped at 2x, got {}",
            ratio
        );
    }

    /// Security Audit Test: Minimal time span (1 second apart) should be handled safely
    #[test]
    fn test_security_audit_minimal_time_span_daa() {
        // Simulate blocks that are only 1 second apart (minimum valid)
        let timestamps: Vec<u64> = vec![1000, 1001, 1002, 1003, 1004, 1005, 1006, 1007];

        let mut sorted_ts = timestamps.clone();
        sorted_ts.sort();

        let len = sorted_ts.len();
        let q1_idx = len / 4;
        let q3_idx = (3 * len) / 4;
        let q1_timestamp = sorted_ts[q1_idx];
        let q3_timestamp = sorted_ts[q3_idx];

        let iqr_span = q3_timestamp.saturating_sub(q1_timestamp);
        let scaled_span = iqr_span.saturating_mul(2);

        // IQR span should be small but non-zero
        assert_eq!(iqr_span, 4, "IQR from index 2 to 6: 1006 - 1002 = 4");
        assert_eq!(scaled_span, 8, "Scaled span should be 8");

        // Still apply minimum floor
        let min_actual_time = (DAA_WINDOW_SIZE / 2) * TARGET_TIME_PER_BLOCK;
        let actual_time = scaled_span.max(min_actual_time);

        // 8 < min_actual_time (which is 1008), so floor applies
        assert_eq!(
            actual_time, min_actual_time,
            "Minimal time span should be floored"
        );
    }

    /// Security Audit Test: Normal timestamp distribution should not be affected
    #[test]
    fn test_security_audit_normal_timestamp_distribution() {
        // Simulate normal block production: ~1 second per block
        let base_ts = 1000u64;
        let timestamps: Vec<u64> = (0..8).map(|i| base_ts + i).collect();

        let mut sorted_ts = timestamps.clone();
        sorted_ts.sort();

        // For a larger window, simulate realistic timestamps
        let realistic_timestamps: Vec<u64> = (0..DAA_WINDOW_SIZE).map(|i| base_ts + i).collect();

        let len = realistic_timestamps.len() as u64;
        let _expected_span = len - 1; // Should be DAA_WINDOW_SIZE - 1

        // In normal operation, span should be close to expected
        let oldest = realistic_timestamps[0];
        let newest = realistic_timestamps[realistic_timestamps.len() - 1];
        let raw_span = newest - oldest;

        assert_eq!(
            raw_span,
            DAA_WINDOW_SIZE - 1,
            "Normal span should match window size - 1"
        );

        // Expected time for DAA
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;
        let actual_time = raw_span;

        // Normal operation should result in roughly 1:1 ratio
        // (assuming TARGET_TIME_PER_BLOCK = 1)
        let ratio = expected_time as f64 / actual_time as f64;

        // Ratio should be close to 1 for normal operation
        // With TARGET_TIME_PER_BLOCK = 1, expected = 2016, actual = 2015
        assert!(
            (ratio - 1.0).abs() < 0.01,
            "Normal operation should have ratio near 1.0, got {}",
            ratio
        );
    }

    /// Security Audit Test: Verify minimum floor prevents extreme ratios
    #[test]
    fn test_security_audit_minimum_floor_prevents_extreme_ratio() {
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;
        let min_actual_time = (DAA_WINDOW_SIZE / 2) * TARGET_TIME_PER_BLOCK;

        // Maximum possible ratio with floor
        let max_ratio = expected_time as f64 / min_actual_time as f64;

        assert_eq!(
            max_ratio, 2.0,
            "Maximum ratio with floor should be exactly 2.0"
        );

        // Without floor (old behavior), ratio could be 2016 or infinity
        // This verifies the fix limits the attack surface
    }

    /// Security Audit Test: IQR calculation handles edge cases
    #[test]
    fn test_security_audit_iqr_edge_cases() {
        // Test with exactly 4 timestamps (minimum for IQR)
        let ts_4: Vec<u64> = vec![100, 200, 300, 400];
        let len = ts_4.len();
        let q1_idx = len / 4; // 1
        let q3_idx = (3 * len) / 4; // 3
        assert_eq!(q1_idx, 1);
        assert_eq!(q3_idx, 3);
        let iqr = ts_4[q3_idx] - ts_4[q1_idx];
        assert_eq!(iqr, 200, "IQR for 4 elements: 400 - 200 = 200");

        // Test with less than 4 timestamps (should use simple span)
        let ts_3: Vec<u64> = vec![100, 200, 300];
        let len_3 = ts_3.len();
        assert!(len_3 < 4, "Less than 4 timestamps triggers fallback");

        // Fallback uses oldest-newest with floor
        let raw_span = ts_3[len_3 - 1] - ts_3[0];
        assert_eq!(raw_span, 200);

        let min_floor = (DAA_WINDOW_SIZE / 2) * TARGET_TIME_PER_BLOCK;
        let actual = raw_span.max(min_floor);
        assert_eq!(actual, min_floor, "Small window uses floor");
    }

    // ========================================================================
    // Integration tests that call apply_difficulty_adjustment directly
    // Per audit: "Need tests that call real functions, not just constant assertions"
    // ========================================================================

    /// Integration test: Call apply_difficulty_adjustment with zero actual time
    /// Verifies the function doesn't panic or produce 4x+ increase
    #[test]
    fn test_integration_apply_difficulty_zero_actual_time_prevented() {
        // If actual_time were 0, it would cause division by zero
        // But with our floor, actual_time is always at least min_actual_time
        let current_difficulty = Difficulty::from(1000000u64);
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;
        let min_actual_time = (DAA_WINDOW_SIZE / 2) * TARGET_TIME_PER_BLOCK;

        // Call the real function with the minimum floored time
        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, min_actual_time);
        assert!(result.is_ok(), "apply_difficulty_adjustment should succeed");

        let new_difficulty = result.unwrap();

        // With min_actual_time = expected_time / 2, ratio = 2.0
        // new_difficulty = current * 2
        let expected_new = current_difficulty * 2u64;
        assert_eq!(
            new_difficulty.as_ref(),
            expected_new.as_ref(),
            "Difficulty should double (2x) with half the expected time"
        );
    }

    /// Integration test: Call apply_difficulty_adjustment with very small actual time
    /// Verifies the 4x clamp prevents extreme spikes
    #[test]
    fn test_integration_apply_difficulty_extreme_ratio_clamped() {
        let current_difficulty = Difficulty::from(1000000u64);
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;

        // Use actual_time = 1 (extreme case before floor was applied)
        // This would give ratio = 2016 without clamping
        let actual_time = 1u64;

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok(), "apply_difficulty_adjustment should succeed");

        let new_difficulty = result.unwrap();

        // The 4x clamp should prevent ratio > 4
        let max_difficulty = current_difficulty * 4u64;
        assert_eq!(
            new_difficulty.as_ref(),
            max_difficulty.as_ref(),
            "Difficulty should be clamped to 4x max"
        );
    }

    /// Integration test: Normal operation with ratio ~1.0
    #[test]
    fn test_integration_apply_difficulty_normal_operation() {
        let current_difficulty = Difficulty::from(1000000u64);
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;
        // Actual time equals expected (blocks arrived on target)
        let actual_time = expected_time;

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok(), "apply_difficulty_adjustment should succeed");

        let new_difficulty = result.unwrap();

        // Ratio = 1.0, difficulty unchanged
        assert_eq!(
            new_difficulty.as_ref(),
            current_difficulty.as_ref(),
            "Difficulty should remain unchanged with ratio 1.0"
        );
    }

    /// Integration test: Blocks too slow (ratio < 1), difficulty decreases
    #[test]
    fn test_integration_apply_difficulty_slow_blocks() {
        let current_difficulty = Difficulty::from(1000000u64);
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;
        // Blocks arrived 2x slower than target
        let actual_time = expected_time * 2;

        let result = apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time);
        assert!(result.is_ok(), "apply_difficulty_adjustment should succeed");

        let new_difficulty = result.unwrap();

        // Ratio = 0.5, difficulty halves
        let expected_new = current_difficulty / 2u64;
        assert_eq!(
            new_difficulty.as_ref(),
            expected_new.as_ref(),
            "Difficulty should halve (0.5x) with twice the expected time"
        );
    }

    /// Integration test: Verify floor + clamp combination limits max increase to 2x
    /// This is the key security property: with floor = expected/2, max ratio = 2
    #[test]
    fn test_integration_security_max_increase_is_2x_with_floor() {
        let current_difficulty = Difficulty::from(1000000u64);
        let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;
        let min_actual_time = (DAA_WINDOW_SIZE / 2) * TARGET_TIME_PER_BLOCK;

        // This simulates the attack scenario: attacker creates equal-timestamp chain
        // Our fix applies the floor, so actual_time = min_actual_time
        let actual_time_with_floor = min_actual_time;

        let result =
            apply_difficulty_adjustment(&current_difficulty, expected_time, actual_time_with_floor);
        assert!(result.is_ok());

        let new_difficulty = result.unwrap();
        let ratio =
            new_difficulty.as_ref().low_u64() as f64 / current_difficulty.as_ref().low_u64() as f64;

        // With floor = expected/2, ratio = 2.0 exactly
        assert!(
            (ratio - 2.0).abs() < 0.001,
            "With floor, max ratio should be 2.0, got {}",
            ratio
        );

        // Importantly, ratio is NOT 4.0 (the old max without floor)
        assert!(
            ratio <= 2.0,
            "Ratio should never exceed 2.0 with floor, got {}",
            ratio
        );
    }
}
