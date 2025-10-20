// TOS Reachability Tree Management
//
// This module manages the reachability tree structure and triggers reindexing
// when interval space is exhausted.

use super::{Interval, ReachabilityData};
use crate::core::error::BlockchainError;
use crate::core::storage::Storage;
use tos_common::crypto::Hash;

use super::reindex::ReindexContext;

/// Reachability reindex configuration constants
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
    // CRITICAL FIX: After reindex, children's intervals may be reordered by subtree_size
    // We must find the child with the MAXIMUM interval.end, not just children.last()
    let remaining = if !parent_data.children.is_empty() {
        // Find the child with maximum interval.end
        let mut max_end = 0u64;
        for child_hash in &parent_data.children {
            let child_data = storage.get_reachability_data(child_hash).await?;
            max_end = max_end.max(child_data.interval.end);
        }
        Interval::new(max_end + 1, parent_data.interval.end)
    } else {
        // No children yet - full parent interval available (minus 1 for strict containment)
        parent_data.interval.decrease_end(1)
    };

    // Check if we need reindexing
    // ALIGNED WITH KASPA: Trigger ONLY when completely exhausted (size == 0)
    // This ensures new_block always gets an empty interval, allowing simple
    // reindex algorithm to work correctly (check fails, climbs to parent automatically)
    if remaining.is_empty() {
        // CRITICAL PATH: Interval exhaustion detected - trigger reindexing
        if log::log_enabled!(log::Level::Warn) {
            log::warn!(
                "Interval exhaustion detected for parent {} (remaining: empty). Triggering reindexing...",
                parent
            );
        }

        // Step 1: Initialize new block with the empty interval
        // Note: internal logic relies on interval being this specific interval
        //       which comes exactly at the end of current capacity (like Kaspa)
        let empty_interval = remaining;

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

        if log::log_enabled!(log::Level::Info) {
            log::info!("Reindexing completed successfully for block {}", new_block);
        }
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
    // Reindex root tracking is fully implemented:
    // - Storage layer provides get/set_reindex_root() (RocksDB + Sled)
    // - Advancement logic in try_advancing_reindex_root() maintains root ~100 blocks behind tip
    // - Blockchain initializes root to genesis and calls hint_virtual_selected_parent() on new blocks

    match storage.get_reindex_root().await {
        Ok(root) => Ok(root),
        Err(_) => {
            // Reindex root not yet initialized (only happens before first block)
            // Blockchain will initialize it to genesis during first block processing
            log::warn!("Reindex root not initialized in storage");
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
            if log::log_enabled!(log::Level::Info) {
                log::info!("Initializing reindex root to {}", hint);
            }
            storage.set_reindex_root(hint).await?;
            return Ok(());
        }
    };

    let current_root_data = storage.get_reachability_data(&current_root).await?;
    let hint_data = storage.get_reachability_data(&hint).await?;

    // Check if hint is far enough ahead to warrant advancement
    // CRITICAL FIX: Require hint to be at least (DEPTH + SLACK/2) ahead
    // This prevents advancing every single block and reduces advancement frequency
    //
    // Example: DEPTH=100, SLACK=16384 → threshold = 100 + 8192 = 8292
    // With old logic: advance every block after height 100
    // With new logic: advance every ~8000 blocks
    //
    // For smaller chains (< SLACK blocks), use minimum threshold of 2*DEPTH
    let advancement_threshold = if current_root_data.height < DEFAULT_REINDEX_SLACK {
        // Early chain: advance every 200 blocks (2 × DEFAULT_REINDEX_DEPTH)
        DEFAULT_REINDEX_DEPTH * 2
    } else {
        // Mature chain: advance every ~8000 blocks
        DEFAULT_REINDEX_DEPTH + DEFAULT_REINDEX_SLACK / 2
    };

    let required_height = current_root_data.height + advancement_threshold;
    if hint_data.height <= required_height {
        // Not far enough ahead - no advancement needed
        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "Reindex root advancement skipped: hint height {} <= required {} (current {} + threshold {})",
                hint_data.height,
                required_height,
                current_root_data.height,
                advancement_threshold
            );
        }
        return Ok(());
    }

    // Find ancestor of hint at depth DEFAULT_REINDEX_DEPTH from hint
    let new_root = find_ancestor_at_depth(storage, hint, DEFAULT_REINDEX_DEPTH).await?;

    // Check if new root is actually ahead of current root
    let new_root_data = storage.get_reachability_data(&new_root).await?;
    if new_root_data.height > current_root_data.height {
        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Advancing reindex root from {} (height {}) to {} (height {})",
                current_root,
                current_root_data.height,
                new_root,
                new_root_data.height
            );
        }

        // Perform interval concentration to reclaim slack from finalized blocks
        // Walk from current_root to new_root, concentrating intervals at each step
        let mut ancestor = current_root.clone();
        let mut ctx = ReindexContext::new(DEFAULT_REINDEX_DEPTH, DEFAULT_REINDEX_SLACK);

        while ancestor != new_root {
            // Find the child of ancestor that is on the path to new_root
            let child = get_next_chain_ancestor_for_concentration(storage, &new_root, &ancestor).await?;

            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "Concentrating intervals: parent {} → child {}, is_final={}",
                    ancestor,
                    child,
                    child == new_root
                );
            }

            // Concentrate intervals: tighten siblings, expand chosen child
            ctx.concentrate_interval(storage, ancestor.clone(), child.clone(), child == new_root).await?;

            ancestor = child;
        }

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

/// Get the next chain ancestor for interval concentration
///
/// Given an ancestor block and a descendant block, finds which child of the ancestor
/// is on the path to the descendant. This is used during interval concentration when
/// advancing the reindex root.
///
/// # Arguments
/// * `storage` - Storage reference
/// * `descendant` - The descendant block (new reindex root)
/// * `ancestor` - The ancestor block whose child we want to find
///
/// # Returns
/// The child of ancestor that is on the chain path to descendant
async fn get_next_chain_ancestor_for_concentration<S: Storage>(
    storage: &S,
    descendant: &Hash,
    ancestor: &Hash,
) -> Result<Hash, BlockchainError> {
    let descendant_data = storage.get_reachability_data(descendant).await?;
    let ancestor_data = storage.get_reachability_data(ancestor).await?;

    // Check each child of ancestor to find which one contains descendant in its interval
    for child in &ancestor_data.children {
        let child_data = storage.get_reachability_data(child).await?;
        // Check if child's interval contains descendant's interval
        if child_data.interval.contains(descendant_data.interval) {
            return Ok(child.clone());
        }
    }

    // If no child found, this is an error - descendant is not actually a descendant of ancestor
    Err(BlockchainError::InvalidReachability)
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
