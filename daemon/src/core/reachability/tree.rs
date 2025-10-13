// TOS Reachability Tree Management
// Based on Kaspa's tree.rs
// Reference: rusty-kaspa/consensus/src/processes/reachability/tree.rs
//
// This module manages the reachability tree structure and triggers reindexing
// when interval space is exhausted.

use super::{Interval, ReachabilityData};
use crate::core::error::BlockchainError;
use crate::core::storage::Storage;
use tos_common::crypto::Hash;

use super::reindex::ReindexContext;

/// Reindex configuration constants (from Kaspa)
pub const DEFAULT_REINDEX_DEPTH: u64 = 100;   // Reindex root stays ~100 blocks behind tip
pub const DEFAULT_REINDEX_SLACK: u64 = 1 << 14; // 16384 blocks - reorg protection threshold

/// Add a new block to the reachability tree with automatic reindexing
///
/// This is the main entry point for adding blocks to the reachability tree.
/// It handles both normal interval allocation and automatic reindexing when
/// interval space is exhausted.
///
/// # Algorithm
/// 1. Calculate remaining interval capacity after parent
/// 2. If capacity exhausted (size <= 1):
///    a. Initialize new block with empty interval
///    b. Add as child of parent
///    c. TRIGGER REINDEXING to redistribute intervals
/// 3. If capacity sufficient:
///    a. Allocate half of remaining space to new block (split-half)
///    b. Add as child of parent
///
/// # Arguments
/// * `storage` - Mutable storage reference
/// * `new_block` - Hash of the new block to add
/// * `parent` - Hash of the selected parent block
///
/// # Returns
/// Ok(()) if successful, Err on failure
pub async fn add_tree_block<S: Storage>(
    storage: &mut S,
    new_block: Hash,
    parent: Hash,
) -> Result<(), BlockchainError> {
    // Get parent's reachability data
    let mut parent_data = storage.get_reachability_data(&parent).await?;

    // Calculate remaining interval capacity after the last child
    let remaining = if let Some(last_child) = parent_data.children.last() {
        let last_child_data = storage.get_reachability_data(last_child).await?;
        Interval::new(last_child_data.interval.end + 1, parent_data.interval.end)
    } else {
        // No children yet - full parent interval available (minus 1 for strict containment)
        parent_data.interval.decrease_end(1)
    };

    // Check if we need reindexing
    if remaining.size() <= 1 {
        // CRITICAL PATH: Interval exhaustion detected - trigger reindexing
        log::warn!(
            "Interval exhaustion detected for parent {} (remaining: {}). Triggering reindexing...",
            parent,
            remaining.size()
        );

        // Step 1: Initialize new block with an empty interval temporarily
        // The reindexing process will assign a proper interval
        let empty_interval = if remaining.is_empty() {
            // Completely exhausted - use parent's end as both start and end
            Interval::new(parent_data.interval.end, parent_data.interval.end - 1)
        } else {
            // One slot remaining - use it
            remaining
        };

        let new_block_data = ReachabilityData {
            parent: parent.clone(),
            interval: empty_interval,
            height: parent_data.height + 1,
            children: Vec::new(),
            future_covering_set: Vec::new(),
        };

        storage.set_reachability_data(&new_block, &new_block_data).await?;

        // Step 2: Add new block as child of parent
        parent_data.children.push(new_block.clone());
        storage.set_reachability_data(&parent, &parent_data).await?;

        // Step 3: PERFORM REINDEXING
        let reindex_root = get_reindex_root(storage).await?;

        let mut ctx = ReindexContext::new(DEFAULT_REINDEX_DEPTH, DEFAULT_REINDEX_SLACK);
        ctx.reindex_intervals(storage, new_block.clone(), reindex_root).await?;

        log::info!("Reindexing completed successfully for block {}", new_block);
    } else {
        // Normal case: sufficient space - use split-half allocation
        let (allocated, _right) = remaining.split_half();

        let new_block_data = ReachabilityData {
            parent: parent.clone(),
            interval: allocated,
            height: parent_data.height + 1,
            children: Vec::new(),
            future_covering_set: Vec::new(),
        };

        storage.set_reachability_data(&new_block, &new_block_data).await?;

        // Add new block as child of parent
        parent_data.children.push(new_block);
        storage.set_reachability_data(&parent, &parent_data).await?;
    }

    Ok(())
}

/// Get the current reindex root from storage
///
/// The reindex root is a stable point in the chain that stays approximately
/// DEFAULT_REINDEX_DEPTH blocks behind the current tip. It provides a stable
/// reference point for reindexing operations.
///
/// # Arguments
/// * `storage` - Storage reference
///
/// # Returns
/// Hash of the current reindex root
async fn get_reindex_root<S: Storage>(storage: &S) -> Result<Hash, BlockchainError> {
    // For now, use a simple implementation: return genesis
    // TODO: Implement proper reindex root tracking and advancement

    // Try to get stored reindex root
    // If not set, fall back to genesis (initial state)
    match storage.get_reindex_root().await {
        Ok(root) => Ok(root),
        Err(_) => {
            // Fall back to genesis
            // In a real implementation, we would need to know the genesis hash
            // For now, return an error indicating reindex root not initialized
            log::warn!("Reindex root not initialized in storage, using genesis as fallback");
            Err(BlockchainError::InvalidReachability)
        }
    }
}

/// Try advancing the reindex root towards the tip
///
/// Called periodically (e.g., when virtual selected parent changes) to move
/// the reindex root forward as the chain grows. The reindex root should stay
/// approximately DEFAULT_REINDEX_DEPTH blocks behind the tip.
///
/// # Algorithm
/// 1. Get current reindex root
/// 2. Check if we can advance (new tip is far enough ahead)
/// 3. Find appropriate ancestor of new tip at depth DEFAULT_REINDEX_DEPTH
/// 4. Update reindex root if conditions met
///
/// # Arguments
/// * `storage` - Mutable storage reference
/// * `hint` - Hash of the new tip (hint for where to advance)
///
/// # Returns
/// Ok(()) if successful (whether or not advancement occurred)
pub async fn try_advancing_reindex_root<S: Storage>(
    storage: &mut S,
    hint: Hash,
) -> Result<(), BlockchainError> {
    let current_root = match storage.get_reindex_root().await {
        Ok(root) => root,
        Err(_) => {
            // Reindex root not initialized - initialize with the hint
            log::info!("Initializing reindex root to {}", hint);
            storage.set_reindex_root(hint).await?;
            return Ok(());
        }
    };

    let current_root_data = storage.get_reachability_data(&current_root).await?;
    let hint_data = storage.get_reachability_data(&hint).await?;

    // Check if hint is far enough ahead to warrant advancement
    if hint_data.height <= current_root_data.height + DEFAULT_REINDEX_DEPTH {
        // Not far enough ahead - no advancement needed
        return Ok(());
    }

    // Find ancestor of hint at depth DEFAULT_REINDEX_DEPTH from hint
    let new_root = find_ancestor_at_depth(storage, hint, DEFAULT_REINDEX_DEPTH).await?;

    // Check if new root is actually ahead of current root
    let new_root_data = storage.get_reachability_data(&new_root).await?;
    if new_root_data.height > current_root_data.height {
        log::info!(
            "Advancing reindex root from {} (height {}) to {} (height {})",
            current_root,
            current_root_data.height,
            new_root,
            new_root_data.height
        );
        storage.set_reindex_root(new_root).await?;
    }

    Ok(())
}

/// Find ancestor at a specific depth from a given block
///
/// Traverses up the selected parent chain by the specified depth.
///
/// # Arguments
/// * `storage` - Storage reference
/// * `block` - Starting block
/// * `depth` - Number of blocks to go back
///
/// # Returns
/// Hash of the ancestor at the specified depth
async fn find_ancestor_at_depth<S: Storage>(
    storage: &S,
    block: Hash,
    depth: u64,
) -> Result<Hash, BlockchainError> {
    let mut current = block;

    for _ in 0..depth {
        let current_data = storage.get_reachability_data(&current).await?;

        // Check if we've reached genesis (parent == self)
        if current_data.parent == current {
            return Ok(current);
        }

        current = current_data.parent;
    }

    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_REINDEX_DEPTH, 100);
        assert_eq!(DEFAULT_REINDEX_SLACK, 16384);
    }
}
