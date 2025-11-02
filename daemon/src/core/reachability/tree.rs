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
pub const DEFAULT_REINDEX_DEPTH: u64 = 100; // Reindex root stays ~100 blocks behind tip
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

        storage
            .set_reachability_data(&new_block, &new_block_data)
            .await?;

        // Step 2: Add new block as child of parent
        parent_data.children.push(new_block.clone());
        storage.set_reachability_data(&parent, &parent_data).await?;

        // Step 3: PERFORM REINDEXING
        let reindex_root = get_reindex_root(storage).await?;

        let mut ctx = ReindexContext::new(DEFAULT_REINDEX_DEPTH, DEFAULT_REINDEX_SLACK);
        ctx.reindex_intervals(storage, new_block.clone(), reindex_root)
            .await?;

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

        storage
            .set_reachability_data(&new_block, &new_block_data)
            .await?;

        // Add new block as child of parent
        parent_data.children.push(new_block);
        storage.set_reachability_data(&parent, &parent_data).await?;
    }

    Ok(())
}

/// Get the current reindex root from storage
///
/// The reindex root is a stable point in the chain that stays exactly
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
    // - Advancement logic in try_advancing_reindex_root() maintains root exactly 100 blocks behind tip
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

/// Find the next reindex root based on current root and hint (selected tip)
///
/// This implements rusty-kaspa's algorithm which keeps the reindex root
/// exactly `reindex_depth` blocks behind the tip.
///
/// # Algorithm (from rusty-kaspa)
/// 1. Check if current root is ancestor of hint (tip)
/// 2. If NOT (reorg case):
///    - Use reindex_slack as minimum height difference to switch chains
///    - Find common ancestor
/// 3. Walk from current/common toward hint
/// 4. Stop when (hint_height - child_height) < reindex_depth
/// 5. Return (ancestor, next) where next is the new reindex root
///
/// # Arguments
/// * `storage` - Storage reference
/// * `current` - Current reindex root
/// * `hint` - Selected tip (VSP)
/// * `reindex_depth` - Target depth behind tip (typically 100)
/// * `reindex_slack` - Reorg protection threshold (typically 16384)
///
/// # Returns
/// (ancestor, next) where ancestor is the starting point for concentration,
/// and next is the new reindex root
async fn find_next_reindex_root<S: Storage>(
    storage: &S,
    current: Hash,
    hint: Hash,
    reindex_depth: u64,
    reindex_slack: u64,
) -> Result<(Hash, Hash), BlockchainError> {
    let mut ancestor = current.clone();
    let mut next = current.clone();

    let hint_data = storage.get_reachability_data(&hint).await?;
    let hint_height = hint_data.height;

    // Test if current root is ancestor of selected tip (hint)
    // If not, this is a reorg case
    let current_data = storage.get_reachability_data(&current).await?;
    let current_interval = current_data.interval;
    let hint_interval = hint_data.interval;

    // Check if current is chain ancestor of hint using interval containment
    let is_ancestor = current_interval.contains(hint_interval);

    if !is_ancestor {
        let current_height = current_data.height;

        // Reorg protection: only switch chains if new chain is reindex_slack blocks ahead
        // This prevents oscillating between chains during reorg attacks
        if hint_height < current_height || hint_height - current_height < reindex_slack {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "Reindex root unchanged due to reorg protection: hint {} (height {}), current {} (height {}), slack required {}",
                    hint, hint_height, current, current_height, reindex_slack
                );
            }
            return Ok((current.clone(), current));
        }

        // Find common ancestor
        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Reorg detected: finding common ancestor between hint {} and current root {}",
                hint,
                current
            );
        }

        let common = find_common_tree_ancestor(storage, hint.clone(), current.clone()).await?;
        ancestor = common.clone();
        next = common;
    }

    // Walk from ancestor toward the selected tip (hint) until we reach
    // a point that is exactly reindex_depth blocks behind the tip
    let mut loop_count = 0u64;
    if log::log_enabled!(log::Level::Debug) {
        log::debug!(
            "find_next_reindex_root: starting loop from next={} (height {}), hint={} (height {}), target gap={}",
            next, storage.get_reachability_data(&next).await?.height, hint, hint_height, reindex_depth
        );
    }

    loop {
        loop_count += 1;
        let child = get_next_chain_ancestor_unchecked_internal(storage, &hint, &next).await?;
        let child_data = storage.get_reachability_data(&child).await?;
        let child_height = child_data.height;

        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "  loop[{}]: next={} (height {}), child={} (height {}), gap_before={}",
                loop_count,
                next,
                storage.get_reachability_data(&next).await?.height,
                child,
                child_height,
                hint_height - child_height
            );
        }

        if hint_height < child_height {
            log::error!("find_next_reindex_root: child_height > hint_height! child={} child_height={} hint_height={}",
                       child, child_height, hint_height);
            return Err(BlockchainError::InvalidReachability);
        }

        // Calculate gap BEFORE advancing
        let gap = hint_height - child_height;

        // Stop when child would be within reindex_depth blocks of the tip
        // This ensures 'next' stays ~reindex_depth blocks behind hint
        if gap < reindex_depth {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "find_next_reindex_root: loop terminated after {} iterations, gap {} < depth {}, final next={} (height {})",
                    loop_count, gap, reindex_depth, next, storage.get_reachability_data(&next).await?.height
                );
            }
            break;
        }

        // Advance to the child
        next = child;
    }

    if log::log_enabled!(log::Level::Debug) {
        log::debug!(
            "find_next_reindex_root: returning (ancestor={} height {}, next={} height {})",
            ancestor,
            storage.get_reachability_data(&ancestor).await?.height,
            next,
            storage.get_reachability_data(&next).await?.height
        );
    }

    Ok((ancestor, next))
}

/// Find the most recent tree ancestor common to both block and reindex_root
///
/// Note: We assume that almost always the chain between the reindex root and
/// the common ancestor is longer than the chain between block and the common
/// ancestor, hence we iterate from block.
///
/// # Arguments
/// * `storage` - Storage reference
/// * `block` - Block to find ancestor for
/// * `reindex_root` - Current reindex root
///
/// # Returns
/// Hash of the common ancestor
async fn find_common_tree_ancestor<S: Storage>(
    storage: &S,
    block: Hash,
    reindex_root: Hash,
) -> Result<Hash, BlockchainError> {
    let mut current = block;

    loop {
        let current_data = storage.get_reachability_data(&current).await?;
        let current_interval = current_data.interval;

        let root_data = storage.get_reachability_data(&reindex_root).await?;
        let root_interval = root_data.interval;

        // Check if current is chain ancestor of reindex_root using interval containment
        if current_interval.contains(root_interval) {
            return Ok(current);
        }

        let parent = current_data.parent.clone();

        // Check for genesis (self-loop)
        if parent == current {
            return Ok(current);
        }

        current = parent;
    }
}

/// Get the next chain ancestor of descendant that is a child of ancestor (unchecked)
///
/// This function doesn't validate that ancestor is actually a chain ancestor - use with care.
/// Used internally for walking the chain during reindex root advancement.
///
/// # Arguments
/// * `storage` - Storage reference
/// * `descendant` - The descendant block
/// * `ancestor` - The ancestor block whose child we want to find
///
/// # Returns
/// The child of ancestor that is on the chain path to descendant
async fn get_next_chain_ancestor_unchecked_internal<S: Storage>(
    storage: &S,
    descendant: &Hash,
    ancestor: &Hash,
) -> Result<Hash, BlockchainError> {
    let ancestor_data = storage.get_reachability_data(ancestor).await?;
    let descendant_data = storage.get_reachability_data(descendant).await?;

    if log::log_enabled!(log::Level::Trace) {
        log::trace!(
            "get_next_chain_ancestor: ancestor {} (height {}), descendant {} (height {}), children: {}",
            ancestor, ancestor_data.height, descendant, descendant_data.height, ancestor_data.children.len()
        );
    }

    // Find which child of ancestor contains descendant in its interval
    for (idx, child) in ancestor_data.children.iter().enumerate() {
        let child_data = storage.get_reachability_data(child).await?;
        let contains = child_data.interval.contains(descendant_data.interval);

        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "  child[{}] {} (height {}) interval [{}, {}) vs descendant [{}, {}): contains={}",
                idx,
                child,
                child_data.height,
                child_data.interval.start,
                child_data.interval.end,
                descendant_data.interval.start,
                descendant_data.interval.end,
                contains
            );
        }

        if contains {
            if log::log_enabled!(log::Level::Trace) {
                log::trace!("  -> Found! Returning child {}", child);
            }
            return Ok(child.clone());
        }
    }

    log::error!(
        "get_next_chain_ancestor FAILED: no child of {} (height {}) contains {} (height {})",
        ancestor,
        ancestor_data.height,
        descendant,
        descendant_data.height
    );
    Err(BlockchainError::InvalidReachability)
}

/// Attempts to advance the reindex root according to the provided hint (VSP)
///
/// This implements rusty-kaspa's algorithm: the reindex root stays exactly
/// DEFAULT_REINDEX_DEPTH blocks behind the tip on the selected chain.
///
/// It is important for the reindex root point to follow the consensus-agreed chain
/// since this way it can benefit from chain-robustness which is implied by the security
/// of the ordering protocol. That is, it enjoys from the fact that all future blocks are
/// expected to elect the root subtree (by converging to the agreement to have it on the
/// selected chain).
///
/// # Algorithm (from rusty-kaspa)
/// 1. Get current root from storage
/// 2. Call find_next_reindex_root to find the new root position
/// 3. If no change needed, return early
/// 4. Perform interval concentration along the path from ancestor to new root
/// 5. Update reindex root in storage
///
/// # Arguments
/// * `storage` - Mutable storage reference
/// * `hint` - Hash of the new virtual selected parent (tip)
///
/// # Returns
/// Ok(()) if successful (whether or not advancement occurred)
pub async fn try_advancing_reindex_root<S: Storage>(
    storage: &mut S,
    hint: Hash,
) -> Result<(), BlockchainError> {
    // Get current root from storage
    let current = match storage.get_reindex_root().await {
        Ok(root) => root,
        Err(_) => {
            // Reindex root not initialized - initialize with the hint (genesis)
            if log::log_enabled!(log::Level::Info) {
                log::info!("Initializing reindex root to {}", hint);
            }
            storage.set_reindex_root(hint).await?;
            return Ok(());
        }
    };

    // Find the possible new root using rusty-kaspa's algorithm
    let (mut ancestor, next) = find_next_reindex_root(
        storage,
        current.clone(),
        hint,
        DEFAULT_REINDEX_DEPTH,
        DEFAULT_REINDEX_SLACK,
    )
    .await?;

    // No update to root, return early
    if current == next {
        if log::log_enabled!(log::Level::Trace) {
            let current_data = storage.get_reachability_data(&current).await?;
            log::trace!(
                "Reindex root unchanged: current {} at height {}",
                current,
                current_data.height
            );
        }
        return Ok(());
    }

    // Log the advancement at DEBUG level (called every block, so shouldn't be INFO)
    let current_data = storage.get_reachability_data(&current).await?;
    let next_data = storage.get_reachability_data(&next).await?;

    if log::log_enabled!(log::Level::Debug) {
        log::debug!(
            "Advancing reindex root from {} (height {}) to {} (height {})",
            current,
            current_data.height,
            next,
            next_data.height
        );
    }

    // Perform interval concentration along the path from ancestor to next
    // This reclaims slack from finalized blocks and gives it to the chosen child
    while ancestor != next {
        let child = get_next_chain_ancestor_for_concentration(storage, &next, &ancestor).await?;

        if log::log_enabled!(log::Level::Debug) {
            let child_data = storage.get_reachability_data(&child).await?;
            log::debug!(
                "Concentrating intervals: parent {} → child {} (height {}), is_final={}",
                ancestor,
                child,
                child_data.height,
                child == next
            );
        }

        let mut ctx = ReindexContext::new(DEFAULT_REINDEX_DEPTH, DEFAULT_REINDEX_SLACK);
        ctx.concentrate_interval(storage, ancestor.clone(), child.clone(), child == next)
            .await?;

        ancestor = child;
    }

    // Update reindex root in storage
    storage.set_reindex_root(next).await?;

    Ok(())
}

/// Find ancestor at a specific depth from a given block
///
/// Traverses up the selected parent chain by the specified depth.
/// Note: This function is kept for potential future use but is not currently
/// used by the rusty-kaspa-based advancement algorithm.
///
/// # Arguments
/// * `storage` - Storage reference
/// * `block` - Starting block
/// * `depth` - Number of blocks to go back
///
/// # Returns
/// Hash of the ancestor at the specified depth
#[allow(dead_code)]
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
