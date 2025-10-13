// TOS Reachability Reindexing
//
// This module implements the interval reindexing algorithm that allows the
// reachability tree to continue operating when interval space is exhausted.

use crate::core::error::BlockchainError;
use crate::core::storage::Storage;
use std::collections::{HashMap, VecDeque};
use tos_common::crypto::Hash;

/// Context for reindexing operations
///
/// Maintains state during a reindex operation, including subtree size cache
/// and configuration parameters.
pub struct ReindexContext {
    /// Cached subtree sizes (number of blocks in subtree including self)
    subtree_sizes: HashMap<Hash, u64>,

    /// Reindex depth: reindex root stays this many blocks behind tip
    /// Default: 100 blocks (from Kaspa)
    depth: u64,

    /// Reindex slack: minimum height difference required to switch reindex root chains
    /// Default: 16384 blocks (from Kaspa) - provides reorg protection
    slack: u64,
}

impl ReindexContext {
    /// Create a new reindex context
    ///
    /// # Arguments
    /// * `depth` - Reindex root stays this many blocks behind tip (typically 100)
    /// * `slack` - Minimum height for chain switching (typically 16384)
    pub fn new(depth: u64, slack: u64) -> Self {
        Self {
            subtree_sizes: HashMap::new(),
            depth,
            slack,
        }
    }

    /// Main reindexing entry point
    ///
    /// Called when adding a new block with an empty interval (interval exhaustion detected).
    /// Finds an ancestor with sufficient space and redistributes intervals.
    ///
    /// # Algorithm
    /// 1. Ascend from new_child towards root
    /// 2. For each ancestor, count its subtree size
    /// 3. Find first ancestor with interval.size() >= subtree_size
    /// 4. Propagate intervals down from that ancestor
    ///
    /// # Arguments
    /// * `storage` - Mutable reference to blockchain storage
    /// * `new_child` - The block that triggered reindexing (has empty interval)
    /// * `reindex_root` - Current reindex root (stable point in chain)
    ///
    /// # Returns
    /// Ok(()) if reindexing succeeded, Err if failed
    pub async fn reindex_intervals<S: Storage>(
        &mut self,
        storage: &mut S,
        new_child: Hash,
        _reindex_root: Hash,
    ) -> Result<(), BlockchainError> {
        let mut current = new_child.clone();

        // Ascend the tree to find ancestor with sufficient space
        loop {
            let current_data = storage.get_reachability_data(&current).await?;
            let current_interval = current_data.interval;

            // Count subtree rooted at current
            self.count_subtrees(storage, current.clone()).await?;

            let subtree_size = self.subtree_sizes[&current];

            // Check if current has sufficient space
            if current_interval.size() >= subtree_size {
                // Found an ancestor with enough space!
                log::info!(
                    "Reindexing from block {} (interval: {}, subtree_size: {})",
                    current,
                    current_interval,
                    subtree_size
                );
                break;
            }

            // Move to parent
            let parent_hash = current_data.parent.clone();

            // Check for genesis (should never reach here with insufficient space)
            if parent_hash == current {
                log::error!(
                    "Reached genesis with insufficient space! This should never happen."
                );
                return Err(BlockchainError::InvalidReachability);
            }

            current = parent_hash;
        }

        // Propagate intervals down from current
        self.propagate_interval(storage, current).await?;

        log::info!("Reindexing completed successfully");
        Ok(())
    }

    /// Count subtree sizes using BFS (non-recursive to handle deep chains)
    ///
    /// Calculates the number of blocks in the subtree rooted at each block.
    /// Uses BFS to avoid stack overflow on deep chains.
    ///
    /// # Algorithm
    /// 1. BFS traversal to reach all leaves
    /// 2. When leaf found (no children), mark subtree_size = 1
    /// 3. Push updates upward through parent chain
    /// 4. Wait until all children processed before computing parent
    /// 5. Formula: subtree_size(node) = sum(subtree_size(children)) + 1
    ///
    /// # Arguments
    /// * `storage` - Reference to blockchain storage
    /// * `block` - Root block to count subtree from
    async fn count_subtrees<S: Storage>(
        &mut self,
        storage: &S,
        block: Hash,
    ) -> Result<(), BlockchainError> {
        // Skip if already counted
        if self.subtree_sizes.contains_key(&block) {
            return Ok(());
        }

        let mut queue = VecDeque::<Hash>::from([block.clone()]);
        let mut counts: HashMap<Hash, u64> = HashMap::new();

        while let Some(current) = queue.pop_front() {
            // Skip if already calculated
            if self.subtree_sizes.contains_key(&current) {
                continue;
            }

            let current_data = storage.get_reachability_data(&current).await?;
            let children = &current_data.children;

            if children.is_empty() {
                // Leaf node - subtree size is 1
                self.subtree_sizes.insert(current.clone(), 1);
            } else {
                // Check if all children have been processed
                let all_children_ready = children
                    .iter()
                    .all(|c| self.subtree_sizes.contains_key(c));

                if all_children_ready {
                    // All children ready - compute this node's subtree size
                    let subtree_sum: u64 = children
                        .iter()
                        .map(|c| self.subtree_sizes[c])
                        .sum();
                    self.subtree_sizes.insert(current.clone(), subtree_sum + 1);
                } else {
                    // Not all children ready - add children to queue and increment count
                    for child in children {
                        if !self.subtree_sizes.contains_key(child) {
                            queue.push_back(child.clone());
                        }
                    }

                    // Track how many children have been processed for this node
                    let count = counts.entry(current.clone()).or_insert(0);
                    *count += 1;

                    // Re-add current to queue to check again later
                    if *count < children.len() as u64 {
                        queue.push_back(current);
                    }
                }
            }
        }

        Ok(())
    }

    /// Propagate intervals down the subtree using BFS
    ///
    /// Starting from a block with sufficient interval space, redistributes
    /// intervals to all descendants using exponential allocation.
    ///
    /// # Algorithm
    /// 1. BFS traversal from root to leaves
    /// 2. For each node with children:
    ///    a. Get available capacity (parent.interval - 1 for strict containment)
    ///    b. Split capacity exponentially among children using subtree sizes
    ///    c. Assign new intervals to children
    /// 3. Continue BFS to all descendants
    ///
    /// # Arguments
    /// * `storage` - Mutable reference to blockchain storage
    /// * `block` - Root block to propagate from
    async fn propagate_interval<S: Storage>(
        &mut self,
        storage: &mut S,
        block: Hash,
    ) -> Result<(), BlockchainError> {
        // Ensure subtrees are counted
        self.count_subtrees(storage, block.clone()).await?;

        let mut queue = VecDeque::<Hash>::from([block]);
        let mut propagated_count = 0u64;

        while let Some(current) = queue.pop_front() {
            let current_data = storage.get_reachability_data(&current).await?;
            let children = current_data.children.clone();

            if !children.is_empty() {
                // Get children's subtree sizes
                let sizes: Vec<u64> = children
                    .iter()
                    .map(|c| self.subtree_sizes[c])
                    .collect();

                // Get available capacity for children
                // Parent must strictly contain children, so decrease end by 1
                let capacity = current_data.interval.decrease_end(1);

                if capacity.is_empty() {
                    log::warn!(
                        "Block {} has children but no capacity for them (interval: {})",
                        current,
                        current_data.interval
                    );
                    continue;
                }

                // Split capacity exponentially among children
                let new_intervals = capacity.split_exponential(&sizes);

                // Assign new intervals to children
                for (i, child) in children.iter().enumerate() {
                    let mut child_data = storage.get_reachability_data(child).await?;
                    child_data.interval = new_intervals[i];
                    storage.set_reachability_data(child, &child_data).await?;

                    propagated_count += 1;
                }

                // Continue BFS to all children
                queue.extend(children.iter().cloned());
            }
        }

        log::debug!(
            "Propagated intervals to {} blocks during reindexing",
            propagated_count
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reindex_context_creation() {
        let ctx = ReindexContext::new(100, 16384);
        assert_eq!(ctx.depth, 100);
        assert_eq!(ctx.slack, 16384);
        assert!(ctx.subtree_sizes.is_empty());
    }

    // Note: Full integration tests require storage implementation
    // These will be added in the integration test module
}
