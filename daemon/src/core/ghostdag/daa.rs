// TOS Difficulty Adjustment Algorithm (DAA)
// Based on Kaspa's DAA implementation
// Reference: rusty-kaspa/consensus/src/processes/difficulty.rs

use anyhow::Result;
use std::collections::{HashSet, VecDeque};
use tos_common::crypto::Hash;
use tos_common::difficulty::Difficulty;

use crate::core::error::BlockchainError;
use crate::core::storage::Storage;

/// DAA window size - number of blocks to consider for difficulty adjustment
/// This is based on Kaspa's implementation
pub const DAA_WINDOW_SIZE: u64 = 2016;

/// Target time per block in seconds
/// TOS uses 1 second per block (vs Kaspa's 1 second)
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

    // For now, parent's daa_score is the same as its blue_score
    // (we'll update this as we implement full DAA)
    let parent_daa_score = parent_data.blue_score;

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

    // Calculate DAA score: parent's score + blues in window
    // Note: We always count the selected_parent itself (it's implicitly in window)
    let daa_score = parent_daa_score + blues_in_window_count;

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

/// Calculate target difficulty based on DAA window
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
    let window_start_block = find_block_at_daa_score(storage, selected_parent, window_start_score).await?;
    let window_end_block = selected_parent;

    // Get timestamps
    let start_header = storage.get_block_header_by_hash(&window_start_block).await?;
    let end_header = storage.get_block_header_by_hash(window_end_block).await?;

    let start_timestamp = start_header.get_timestamp();
    let end_timestamp = end_header.get_timestamp();

    // Calculate actual time taken (in seconds)
    let actual_time = if end_timestamp > start_timestamp {
        end_timestamp - start_timestamp
    } else {
        // Timestamp went backwards (shouldn't happen with proper validation)
        // Use minimum time to avoid division by zero
        1
    };

    // Calculate expected time
    let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK;

    // Get current difficulty
    let current_difficulty = storage.get_difficulty_for_block_hash(selected_parent).await?;

    // Calculate adjustment ratio
    // If actual_time < expected_time: blocks are too fast -> increase difficulty
    // If actual_time > expected_time: blocks are too slow -> decrease difficulty
    let ratio = expected_time as f64 / actual_time as f64;

    // Clamp ratio to prevent extreme adjustments
    let clamped_ratio = ratio.max(MIN_DIFFICULTY_RATIO).min(MAX_DIFFICULTY_RATIO);

    // Apply adjustment
    // new_difficulty = current_difficulty * clamped_ratio
    let new_difficulty = apply_difficulty_adjustment(&current_difficulty, clamped_ratio)?;

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

        // For now, use blue_score as proxy for daa_score
        // TODO: Once we store daa_score separately, use that
        if current_data.blue_score == target_score {
            return Ok(current);
        }

        // If we've gone past the target, traverse to parents
        if current_data.blue_score > target_score {
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

/// Apply difficulty adjustment ratio to current difficulty
///
/// # Arguments
/// * `current_difficulty` - Current difficulty value
/// * `ratio` - Adjustment ratio (e.g., 1.5 to increase by 50%)
///
/// # Returns
/// New difficulty after applying the ratio
fn apply_difficulty_adjustment(current_difficulty: &Difficulty, ratio: f64) -> Result<Difficulty, BlockchainError> {
    // Convert difficulty to f64 for calculation
    // TOS difficulty is stored as U256, we need to handle this carefully

    // Get the U256 value (version 0.13.1 API)
    let diff_u256 = current_difficulty.as_ref();

    // Convert to f64 (this may lose precision for very large values)
    // For now, we'll use a simple conversion
    // TODO: Implement precise arbitrary-precision arithmetic if needed

    // Convert U256 to bytes
    // In v0.13.1, to_big_endian() returns [u8; 32] directly (no parameter)
    let bytes = diff_u256.to_big_endian();

    // Take the last 16 bytes as u128 (assuming difficulty won't exceed u128)
    let mut u128_bytes = [0u8; 16];
    u128_bytes.copy_from_slice(&bytes[16..32]);
    let diff_u128 = u128::from_be_bytes(u128_bytes);

    // Apply ratio
    let new_diff_f64 = diff_u128 as f64 * ratio;
    let new_diff_u128 = new_diff_f64 as u128;

    // Convert back to U256
    let mut new_bytes = [0u8; 32];
    new_bytes[16..32].copy_from_slice(&new_diff_u128.to_be_bytes());

    // Create new difficulty from bytes
    // We need to use tos_common's U256 (v0.13.1) not daemon's (v0.12)
    // because VarUint implements From<tos_common::U256>
    //
    // Use tos_common::difficulty directly (which re-exports the right U256 version)
    use tos_common::varuint::VarUint;

    // Create VarUint directly from the new difficulty value (as u128)
    let new_difficulty = VarUint::from(new_diff_u128);

    Ok(new_difficulty)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
