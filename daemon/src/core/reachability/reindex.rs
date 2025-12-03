// TOS Reachability Reindexing
//
// This module implements the interval reindexing algorithm that allows the
// reachability tree to continue operating when interval space is exhausted.

use crate::core::error::BlockchainError;
use crate::core::storage::Storage;
use std::collections::{HashMap, VecDeque};
use tos_common::crypto::Hash;

use super::Interval;

/// Context for reindexing operations
///
/// Maintains state during a reindex operation, including subtree size cache
/// and configuration parameters.
pub struct ReindexContext {
    /// Cached subtree sizes (number of blocks in subtree including self)
    subtree_sizes: HashMap<Hash, u64>,

    /// Reindex depth: reindex root stays this many blocks behind tip
    /// Default: 100 blocks (standard BlockDAG value)
    #[allow(dead_code)]
    depth: u64,

    /// Reindex slack: minimum height difference required to switch reindex root chains
    /// Default: 16384 blocks (standard BlockDAG value) - provides reorg protection
    #[allow(dead_code)]
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
    /// # Algorithm (ALIGNED WITH KASPA + SLACK RECLAIM)
    /// 1. Start from new_child (which has empty interval, size=0)
    /// 2. Ascend from new_child towards root
    /// 3. For each ancestor, count its subtree size
    /// 4. Check if parent has climbed above reindex_root:
    ///    a. If yes → Use slack reclaim algorithm (prevents linear chain compression!)
    ///    b. If no → Continue search
    /// 5. Find first ancestor with interval.size() >= subtree_size (simple condition)
    /// 6. Propagate intervals down from that ancestor
    ///
    /// # Why Slack Reclaim is Critical
    /// - In linear chains, split_half causes exponential compression (1/2^n)
    /// - After ~64 blocks, intervals are exhausted
    /// - Simple propagation redistributes only the tiny interval → immediate re-exhaustion
    /// - Slack reclaim collects unused space from chain up to reindex_root
    /// - Allocates slack (4096 units) per chain block → prevents compression
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
        reindex_root: Hash,
    ) -> Result<(), BlockchainError> {
        // DEBUG: Log reindex root information
        let reindex_root_data = storage.get_reachability_data(&reindex_root).await?;
        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Starting reindex search: new_child={}, reindex_root={} (height {})",
                new_child,
                reindex_root,
                reindex_root_data.height
            );
        }

        // ALIGNED WITH KASPA: Start from new_child itself
        // Because new_child has empty interval (size=0), the simple check will fail,
        // causing automatic climb to parent (which has better space)
        let mut current = new_child;

        // Ascend the tree to find ancestor with sufficient space
        loop {
            let current_data = storage.get_reachability_data(&current).await?;
            let current_interval = current_data.interval;

            // Count subtree rooted at current
            self.count_subtrees(storage, current.clone()).await?;

            let subtree_size = self.subtree_sizes[&current];

            // DEBUG: Log each ancestor being checked
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "  Checking ancestor {} (height {}): interval={}, size={}, subtree_size={}",
                    current,
                    current_data.height,
                    current_interval,
                    current_interval.size(),
                    subtree_size
                );
            }

            // ALIGNED WITH KASPA: Simple condition
            // Check if current has sufficient space to hold its subtree
            // For new_child with empty interval (size=0): 0 >= 1 → fails, climbs to parent
            // For parent with decent space: size >= subtree_size → passes, stops here
            if current_interval.size() >= subtree_size {
                // Found an ancestor with enough space!
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "  ✓ Found sufficient space at height {}: size {} >= subtree_size {}",
                        current_data.height,
                        current_interval.size(),
                        subtree_size
                    );
                }
                break;
            }

            // Move to parent
            let parent_hash = current_data.parent.clone();

            // Check for genesis (should never reach here with insufficient space)
            if parent_hash == current {
                log::error!("Reached genesis with insufficient space! This should never happen.");
                return Err(BlockchainError::InvalidReachability);
            }

            // CRITICAL SLACK RECLAIM PATH: Check BEFORE checking if we reached reindex root
            // If current has reached the reindex root (or will on next iteration), use slack reclaim
            // This check must come BEFORE the reindex_root error check!

            // DEBUG: Log before slack reclaim check
            let parent_data = storage.get_reachability_data(&parent_hash).await?;
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "  Checking slack reclaim: parent {} (height {}), reindex_root {} (height {})",
                    parent_hash,
                    parent_data.height,
                    reindex_root,
                    reindex_root_data.height
                );
            }

            // FIX: Correct parameter order - check if parent is ancestor of reindex_root (climbed above root)
            if is_strict_chain_ancestor_of(storage, &parent_hash, &reindex_root).await? {
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "  ✓✓✓ SLACK RECLAIM TRIGGERED: parent {} is above reindex root {}",
                        parent_hash,
                        reindex_root
                    );
                }

                // Use slack reclaim algorithm
                // Notes:
                // 1. required_allocation = subtree_size to double current interval capacity
                // 2. This might be new_child itself (when new_child's parent is above root)
                return self
                    .reindex_intervals_earlier_than_root(
                        storage,
                        current,      // allocation_block: block that needs more space
                        reindex_root, // reindex_root: stable point in chain
                        parent_hash,  // common_ancestor: parent of allocation_block
                        subtree_size, // required_allocation: amount needed (doubles capacity)
                    )
                    .await;
            }

            // CRITICAL CHECK: Did we reach the reindex root without slack reclaim triggering?
            // This should only happen if slack reclaim failed or there's a logic error
            if current == reindex_root {
                log::error!(
                    "Reindex root {} is out of capacity and slack reclaim didn't trigger!",
                    reindex_root
                );
                return Err(BlockchainError::InvalidReachability);
            }

            current = parent_hash;
        }

        // Normal case: found ancestor with sufficient space below reindex root
        // Use simple propagation
        self.propagate_interval(storage, current).await?;

        log::info!("Reindexing completed successfully");
        Ok(())
    }

    /// Count subtree sizes using BFS (non-recursive to handle deep chains)
    ///
    /// Calculates the number of blocks in the subtree rooted at each block.
    /// Uses BFS to avoid stack overflow on deep chains.
    ///
    /// This is the CORRECT reachability reindexing algorithm:
    /// 1. BFS traversal to reach all leaves
    /// 2. When leaf found (no children), mark subtree_size = 1
    /// 3. Walk UP parent chain from leaf to root, updating parent sizes
    /// 4. Track child completion counts to know when parent is ready
    /// 5. Formula: subtree_size(node) = sum(subtree_size(children)) + 1
    ///
    /// Key improvement over previous implementation:
    /// - Never re-queues nodes (avoids circular dependencies)
    /// - Uses parent-chain walk instead of re-queue-and-retry
    /// - Uses `counts` map to track children completion
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
        let mut counts = HashMap::<Hash, u64>::new();

        while let Some(mut current) = queue.pop_front() {
            let current_data = storage.get_reachability_data(&current).await?;
            let children = &current_data.children;

            if children.is_empty() {
                // We reached a leaf - subtree size is 1
                self.subtree_sizes.insert(current.clone(), 1);
            } else if !self.subtree_sizes.contains_key(&current) {
                // We haven't yet calculated the subtree size of
                // the current block. Add all its children to the queue
                for child in children {
                    queue.push_back(child.clone());
                }
                continue;
            }

            // We reached a leaf or a pre-calculated subtree.
            // Push information up the parent chain
            while current != block {
                let parent_hash = storage.get_reachability_data(&current).await?.parent;

                // Self-loop check (genesis)
                if parent_hash == current {
                    break;
                }

                current = parent_hash;

                let count = counts.entry(current.clone()).or_insert(0);
                let parent_children = storage.get_reachability_data(&current).await?.children;

                *count += 1;
                if *count < parent_children.len() as u64 {
                    // Not all subtrees of the current block are ready
                    break;
                }

                // All children of `current` have calculated their subtree size.
                // Sum them all together and add 1 to get the subtree size of `current`.
                // Use get() with unwrap_or(1) to handle edge cases where a child's
                // subtree_size might not be calculated yet (defensive programming).
                let subtree_sum: u64 = parent_children
                    .iter()
                    .map(|c| *self.subtree_sizes.get(c).unwrap_or(&1))
                    .sum();
                self.subtree_sizes.insert(current.clone(), subtree_sum + 1);
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
                // Use get() with unwrap_or(1) for defensive programming
                let sizes: Vec<u64> = children
                    .iter()
                    .map(|c| *self.subtree_sizes.get(c).unwrap_or(&1))
                    .collect();

                // ALIGNED WITH KASPA: Use entire parent capacity for children
                // The interval of a block should *strictly* contain the intervals of its
                // tree children, hence we subtract 1 from the end of the range.
                let capacity = current_data.interval.decrease_end(1);

                if capacity.is_empty() {
                    if log::log_enabled!(log::Level::Warn) {
                        log::warn!(
                            "Block {} has children but no capacity for them (interval: {})",
                            current,
                            current_data.interval
                        );
                    }
                    continue;
                }

                if log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "Reindex allocating {} capacity units for {} children of block {}",
                        capacity.size(),
                        children.len(),
                        current
                    );
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

    /// Reindex intervals for blocks earlier than the reindex root
    ///
    /// This is the CRITICAL SLACK RECLAIM ALGORITHM that prevents frequent reindexing
    /// in linear chains. It collects slack (unused interval space) from blocks along the
    /// chain from the reindex root upward, and allocates it to the exhausted subtree.
    ///
    /// # Algorithm
    /// 1. Determine if allocation_block is before or after the chosen child
    /// 2. Call reclaim_interval_before or reclaim_interval_after accordingly
    /// 3. These functions walk up the chain, collecting slack, and propagate down
    ///
    /// # Arguments
    /// * `storage` - Mutable storage reference
    /// * `allocation_block` - Block that needs more interval space
    /// * `reindex_root` - Current reindex root (stable point in chain)
    /// * `common_ancestor` - Direct parent of allocation_block, ancestor of reindex_root
    /// * `required_allocation` - Amount of space needed (typically subtree_size to double capacity)
    async fn reindex_intervals_earlier_than_root<S: Storage>(
        &mut self,
        storage: &mut S,
        allocation_block: Hash,
        reindex_root: Hash,
        common_ancestor: Hash,
        required_allocation: u64,
    ) -> Result<(), BlockchainError> {
        // The chosen child is: (i) child of common_ancestor; (ii) an ancestor of reindex_root
        let chosen_child =
            get_next_chain_ancestor_unchecked(storage, &reindex_root, &common_ancestor).await?;
        let block_interval = storage
            .get_reachability_data(&allocation_block)
            .await?
            .interval;
        let chosen_interval = storage.get_reachability_data(&chosen_child).await?.interval;

        if block_interval.start < chosen_interval.start {
            // allocation_block is in the subtree before the chosen child
            self.reclaim_interval_before(
                storage,
                allocation_block,
                common_ancestor,
                chosen_child,
                reindex_root,
                required_allocation,
            )
            .await
        } else {
            // allocation_block is in the subtree after the chosen child
            self.reclaim_interval_after(
                storage,
                allocation_block,
                common_ancestor,
                chosen_child,
                reindex_root,
                required_allocation,
            )
            .await
        }
    }

    /// Reclaim slack from blocks before the chosen child
    ///
    /// Walks up the chain from chosen_child to reindex_root, collecting unused interval
    /// space (slack) before each block, and allocates it to allocation_block.
    async fn reclaim_interval_before<S: Storage>(
        &mut self,
        storage: &mut S,
        allocation_block: Hash,
        common_ancestor: Hash,
        chosen_child: Hash,
        reindex_root: Hash,
        required_allocation: u64,
    ) -> Result<(), BlockchainError> {
        let mut slack_sum = 0u64;
        let mut path_len = 0u64;
        let mut path_slack_alloc = 0u64;

        let mut current = chosen_child;
        // Walk up the chain from common ancestor's chosen child towards reindex root
        loop {
            if current == reindex_root {
                // Reached reindex root. Allocate new slack for the chain we just traversed
                let offset = required_allocation + self.slack * path_len - slack_sum;
                self.apply_interval_op_and_propagate(
                    storage,
                    &current,
                    offset,
                    Interval::increase_start,
                )
                .await?;
                self.offset_siblings_before(storage, &allocation_block, &current, offset)
                    .await?;

                // Set the slack for each chain block to be reserved below during the chain walk-down
                path_slack_alloc = self.slack;
                break;
            }

            let slack_before_current = interval_remaining_before(storage, &current).await?.size();
            slack_sum += slack_before_current;

            if slack_sum >= required_allocation {
                // Set offset to be just enough to satisfy required allocation
                let offset = slack_before_current - (slack_sum - required_allocation);
                self.apply_interval_op(storage, &current, offset, Interval::increase_start)
                    .await?;
                self.offset_siblings_before(storage, &allocation_block, &current, offset)
                    .await?;

                break;
            }

            current = get_next_chain_ancestor_unchecked(storage, &reindex_root, &current).await?;
            path_len += 1;
        }

        // Go back down the reachability tree towards the common ancestor.
        // On every hop we reindex the reachability subtree before the
        // current block with an interval that is smaller.
        // This is to make room for the required allocation.
        loop {
            let parent_hash = storage.get_reachability_data(&current).await?.parent;
            if parent_hash == common_ancestor {
                break;
            }

            current = parent_hash;

            let slack_before_current = interval_remaining_before(storage, &current).await?.size();
            let offset = slack_before_current.saturating_sub(path_slack_alloc);
            self.apply_interval_op(storage, &current, offset, Interval::increase_start)
                .await?;
            self.offset_siblings_before(storage, &allocation_block, &current, offset)
                .await?;
        }

        Ok(())
    }

    /// Reclaim slack from blocks after the chosen child
    ///
    /// Similar to reclaim_interval_before, but collects slack after each block instead of before.
    async fn reclaim_interval_after<S: Storage>(
        &mut self,
        storage: &mut S,
        allocation_block: Hash,
        common_ancestor: Hash,
        chosen_child: Hash,
        reindex_root: Hash,
        required_allocation: u64,
    ) -> Result<(), BlockchainError> {
        let mut slack_sum = 0u64;
        let mut path_len = 0u64;
        let mut path_slack_alloc = 0u64;

        let mut current = chosen_child;
        // Walk up the chain from common ancestor's chosen child towards reindex root
        loop {
            if current == reindex_root {
                // Reached reindex root. Allocate new slack for the chain we just traversed
                let offset = required_allocation + self.slack * path_len - slack_sum;
                self.apply_interval_op_and_propagate(
                    storage,
                    &current,
                    offset,
                    Interval::decrease_end,
                )
                .await?;
                self.offset_siblings_after(storage, &allocation_block, &current, offset)
                    .await?;

                // Set the slack for each chain block to be reserved below during the chain walk-down
                path_slack_alloc = self.slack;
                break;
            }

            let slack_after_current = interval_remaining_after(storage, &current).await?.size();
            slack_sum += slack_after_current;

            if slack_sum >= required_allocation {
                // Set offset to be just enough to satisfy required allocation
                let offset = slack_after_current - (slack_sum - required_allocation);
                self.apply_interval_op(storage, &current, offset, Interval::decrease_end)
                    .await?;
                self.offset_siblings_after(storage, &allocation_block, &current, offset)
                    .await?;

                break;
            }

            current = get_next_chain_ancestor_unchecked(storage, &reindex_root, &current).await?;
            path_len += 1;
        }

        // Go back down the reachability tree towards the common ancestor.
        // On every hop we reindex the reachability subtree after the
        // current block with an interval that is smaller.
        // This is to make room for the required allocation.
        loop {
            let parent_hash = storage.get_reachability_data(&current).await?.parent;
            if parent_hash == common_ancestor {
                break;
            }

            current = parent_hash;

            let slack_after_current = interval_remaining_after(storage, &current).await?.size();
            let offset = slack_after_current.saturating_sub(path_slack_alloc);
            self.apply_interval_op(storage, &current, offset, Interval::decrease_end)
                .await?;
            self.offset_siblings_after(storage, &allocation_block, &current, offset)
                .await?;
        }

        Ok(())
    }

    /// Offset sibling intervals before the current block to make space
    ///
    /// Traverses siblings before current in reverse order, shifting their intervals
    /// upward (increase) to create space. When allocation_block is reached, allocates
    /// the offset to it by increasing its end.
    async fn offset_siblings_before<S: Storage>(
        &mut self,
        storage: &mut S,
        allocation_block: &Hash,
        current: &Hash,
        offset: u64,
    ) -> Result<(), BlockchainError> {
        let parent_hash = storage.get_reachability_data(current).await?.parent;
        let children = storage
            .get_reachability_data(&parent_hash)
            .await?
            .children
            .clone();

        let (siblings_before, _) = Self::split_children(&children, current.clone())?;

        for sibling in siblings_before.iter().rev() {
            if *sibling == *allocation_block {
                // We reached our final destination, allocate offset to allocation_block by increasing end and break
                self.apply_interval_op_and_propagate(
                    storage,
                    allocation_block,
                    offset,
                    Interval::increase_end,
                )
                .await?;
                break;
            }
            // For non-allocation_block siblings offset the interval upwards in order to create space
            self.apply_interval_op_and_propagate(storage, sibling, offset, Interval::increase)
                .await?;
        }

        Ok(())
    }

    /// Offset sibling intervals after the current block to make space
    ///
    /// Traverses siblings after current in forward order, shifting their intervals
    /// downward (decrease) to create space. When allocation_block is reached, allocates
    /// the offset to it by decreasing its start.
    async fn offset_siblings_after<S: Storage>(
        &mut self,
        storage: &mut S,
        allocation_block: &Hash,
        current: &Hash,
        offset: u64,
    ) -> Result<(), BlockchainError> {
        let parent_hash = storage.get_reachability_data(current).await?.parent;
        let children = storage
            .get_reachability_data(&parent_hash)
            .await?
            .children
            .clone();

        let (_, siblings_after) = Self::split_children(&children, current.clone())?;

        for sibling in siblings_after.iter() {
            if *sibling == *allocation_block {
                // We reached our final destination, allocate offset to allocation_block by decreasing only start and break
                self.apply_interval_op_and_propagate(
                    storage,
                    allocation_block,
                    offset,
                    Interval::decrease_start,
                )
                .await?;
                break;
            }
            // For siblings before allocation_block offset the interval downwards to create space
            self.apply_interval_op_and_propagate(storage, sibling, offset, Interval::decrease)
                .await?;
        }

        Ok(())
    }

    /// Helper function to split children into before and after pivot
    ///
    /// Used during slack reclaim to identify siblings before and after the chosen child.
    fn split_children<'a>(
        children: &'a [Hash],
        pivot: Hash,
    ) -> Result<(&'a [Hash], &'a [Hash]), BlockchainError> {
        if let Some(index) = children.iter().position(|c| *c == pivot) {
            Ok((&children[..index], &children[index + 1..]))
        } else {
            Err(BlockchainError::InvalidReachability)
        }
    }

    /// Helper function to apply an interval operation to a block
    ///
    /// Applies the given interval transformation (increase, decrease, etc.) to a block's interval.
    async fn apply_interval_op<S: Storage>(
        &mut self,
        storage: &mut S,
        block: &Hash,
        offset: u64,
        op: fn(&Interval, u64) -> Interval,
    ) -> Result<(), BlockchainError> {
        let mut data = storage.get_reachability_data(block).await?;
        data.interval = op(&data.interval, offset);
        storage.set_reachability_data(block, &data).await?;
        Ok(())
    }

    /// Helper function to apply an interval operation and propagate intervals to descendants
    ///
    /// Like apply_interval_op, but also reindexes the entire subtree rooted at the block.
    async fn apply_interval_op_and_propagate<S: Storage>(
        &mut self,
        storage: &mut S,
        block: &Hash,
        offset: u64,
        op: fn(&Interval, u64) -> Interval,
    ) -> Result<(), BlockchainError> {
        self.apply_interval_op(storage, block, offset, op).await?;
        self.propagate_interval(storage, block.clone()).await?;
        Ok(())
    }

    /// Concentrate interval when advancing reindex root
    ///
    /// This is called when the reindex root advances (e.g., from height 100 to 200).
    /// It reclaims slack from finalized blocks and gives it to the chosen child.
    ///
    /// # Arguments
    /// * `storage` - Mutable storage reference
    /// * `parent` - Parent block whose children's intervals need concentration
    /// * `child` - Chosen child (on path to new reindex root) that gets expanded interval
    /// * `is_final_reindex_root` - True if child is the final new reindex root
    pub async fn concentrate_interval<S: Storage>(
        &mut self,
        storage: &mut S,
        parent: Hash,
        child: Hash,
        is_final_reindex_root: bool,
    ) -> Result<(), BlockchainError> {
        let children = storage
            .get_reachability_data(&parent)
            .await?
            .children
            .clone();

        // Split the `children` of `parent` to siblings before `child` and siblings after `child`
        let (siblings_before, siblings_after) = Self::split_children(&children, child.clone())?;

        let siblings_before_subtrees_sum: u64 = self
            .tighten_intervals_before(storage, parent.clone(), siblings_before)
            .await?;
        let siblings_after_subtrees_sum: u64 = self
            .tighten_intervals_after(storage, parent.clone(), siblings_after)
            .await?;

        self.expand_interval_to_chosen(
            storage,
            parent,
            child,
            siblings_before_subtrees_sum,
            siblings_after_subtrees_sum,
            is_final_reindex_root,
        )
        .await?;

        Ok(())
    }

    /// Tighten intervals of siblings before chosen child
    ///
    /// Compresses the intervals of all siblings that come before the chosen child,
    /// reclaiming slack space. Returns the sum of their subtree sizes.
    async fn tighten_intervals_before<S: Storage>(
        &mut self,
        storage: &mut S,
        parent: Hash,
        children_before: &[Hash],
    ) -> Result<u64, BlockchainError> {
        // Calculate subtree sizes for all children before chosen child
        let mut sizes = Vec::new();
        for block in children_before {
            self.count_subtrees(storage, block.clone()).await?;
            // Use get() with unwrap_or(1) for defensive programming
            sizes.push(*self.subtree_sizes.get(block).unwrap_or(&1));
        }
        let sum: u64 = sizes.iter().sum();

        let parent_interval = storage.get_reachability_data(&parent).await?.interval;
        // Allocate tight interval right after parent's start + slack
        let interval_before = Interval::new(
            parent_interval.start + self.slack,
            parent_interval.start + self.slack + sum - 1,
        );

        // Split the tight interval among siblings proportionally
        let split_intervals = interval_before.split_exact(&sizes);

        for (c, ci) in children_before.iter().zip(split_intervals.iter()) {
            let mut child_data = storage.get_reachability_data(c).await?;
            child_data.interval = *ci;
            storage.set_reachability_data(c, &child_data).await?;
            self.propagate_interval(storage, c.clone()).await?;
        }

        Ok(sum)
    }

    /// Tighten intervals of siblings after chosen child
    ///
    /// Compresses the intervals of all siblings that come after the chosen child,
    /// reclaiming slack space. Returns the sum of their subtree sizes.
    async fn tighten_intervals_after<S: Storage>(
        &mut self,
        storage: &mut S,
        parent: Hash,
        children_after: &[Hash],
    ) -> Result<u64, BlockchainError> {
        // Calculate subtree sizes for all children after chosen child
        let mut sizes = Vec::new();
        for block in children_after {
            self.count_subtrees(storage, block.clone()).await?;
            // Use get() with unwrap_or(1) for defensive programming
            sizes.push(*self.subtree_sizes.get(block).unwrap_or(&1));
        }
        let sum: u64 = sizes.iter().sum();

        let parent_interval = storage.get_reachability_data(&parent).await?.interval;
        // Allocate tight interval right before parent's end - slack
        let interval_after = Interval::new(
            parent_interval.end - self.slack - sum,
            parent_interval.end - self.slack - 1,
        );

        // Split the tight interval among siblings proportionally
        let split_intervals = interval_after.split_exact(&sizes);

        for (c, ci) in children_after.iter().zip(split_intervals.iter()) {
            let mut child_data = storage.get_reachability_data(c).await?;
            child_data.interval = *ci;
            storage.set_reachability_data(c, &child_data).await?;
            self.propagate_interval(storage, c.clone()).await?;
        }

        Ok(sum)
    }

    /// Expand interval of chosen child with reclaimed slack
    ///
    /// Gives the chosen child all the slack reclaimed from siblings.
    /// This allows future blocks in the chosen subtree to grow without immediate reindexing.
    async fn expand_interval_to_chosen<S: Storage>(
        &mut self,
        storage: &mut S,
        parent: Hash,
        child: Hash,
        siblings_before_subtrees_sum: u64,
        siblings_after_subtrees_sum: u64,
        is_final_reindex_root: bool,
    ) -> Result<(), BlockchainError> {
        let parent_interval = storage.get_reachability_data(&parent).await?.interval;

        // Calculate the new allocation for chosen child
        // It gets all space between the tightened sibling intervals
        let allocation = Interval::new(
            parent_interval.start + siblings_before_subtrees_sum + self.slack,
            parent_interval.end - siblings_after_subtrees_sum - self.slack - 1,
        );

        let current_interval = storage.get_reachability_data(&child).await?.interval;

        // Propagate interval only if the chosen `child` is the final reindex root AND
        // the new interval doesn't contain the previous one
        if is_final_reindex_root && !allocation.contains(current_interval) {
            /*
            We deallocate slack on both sides as an optimization. Were we to
            assign the fully allocated interval, the next time the reindex root moves we
            would need to propagate intervals again. However when we do allocate slack,
            next time this method is called (next time the reindex root moves), `allocation` is likely to contain `current`.
            Note that below following the propagation we reassign the full `allocation` to `child`.
            */
            let narrowed =
                Interval::new(allocation.start + self.slack, allocation.end - self.slack);
            let mut child_data = storage.get_reachability_data(&child).await?;
            child_data.interval = narrowed;
            storage.set_reachability_data(&child, &child_data).await?;
            self.propagate_interval(storage, child.clone()).await?;
        }

        // Assign the full allocation to the chosen child
        let mut child_data = storage.get_reachability_data(&child).await?;
        child_data.interval = allocation;
        storage.set_reachability_data(&child, &child_data).await?;

        Ok(())
    }
}

/// Check if `this` block is a chain ancestor of `queried` block (allowing equality)
///
/// Returns true if this block's interval contains the queried block's interval.
async fn is_chain_ancestor_of<S: Storage>(
    storage: &S,
    this: &Hash,
    queried: &Hash,
) -> Result<bool, BlockchainError> {
    let this_interval = storage.get_reachability_data(this).await?.interval;
    let queried_interval = storage.get_reachability_data(queried).await?.interval;
    Ok(this_interval.contains(queried_interval))
}

/// Check if `this` block is a strict chain ancestor of `queried` block (not equal)
///
/// Returns true if this block's interval strictly contains the queried block's interval.
async fn is_strict_chain_ancestor_of<S: Storage>(
    storage: &S,
    this: &Hash,
    queried: &Hash,
) -> Result<bool, BlockchainError> {
    let this_interval = storage.get_reachability_data(this).await?.interval;
    let queried_interval = storage.get_reachability_data(queried).await?.interval;
    Ok(this_interval.strictly_contains(queried_interval))
}

/// Search result for binary search in ordered children
#[allow(dead_code)]
enum SearchOutput {
    NotFound(usize),    // Position to insert at
    Found(Hash, usize), // Found hash and its index
}

/// Binary search for a descendant in an ordered list of children
///
/// Uses interval endpoints to efficiently search for which child contains the descendant.
/// FIXED: Pre-fetches all interval data to avoid blocking in Tokio async context.
async fn binary_search_descendant<S: Storage>(
    storage: &S,
    ordered_hashes: &[Hash],
    descendant: &Hash,
) -> Result<SearchOutput, BlockchainError> {
    // Get the unique endpoint for the descendant
    let point = storage
        .get_reachability_data(descendant)
        .await?
        .interval
        .end;

    // Pre-fetch all interval start points (avoids blocking inside binary search closure)
    let mut interval_starts = Vec::with_capacity(ordered_hashes.len());
    for hash in ordered_hashes {
        let data = storage.get_reachability_data(hash).await?;
        interval_starts.push(data.interval.start);
    }

    // Binary search by interval start points (now synchronous, no blocking)
    let result = interval_starts.binary_search(&point);

    match result {
        Ok(i) => Ok(SearchOutput::Found(ordered_hashes[i].clone(), i)),
        Err(i) => {
            // i is where point was expected, so check if ordered_hashes[i-1] contains descendant
            if i > 0 && is_chain_ancestor_of(storage, &ordered_hashes[i - 1], descendant).await? {
                Ok(SearchOutput::Found(ordered_hashes[i - 1].clone(), i - 1))
            } else {
                Ok(SearchOutput::NotFound(i))
            }
        }
    }
}

/// Get the next chain ancestor of descendant that is a child of ancestor
///
/// This function doesn't validate that ancestor is actually a chain ancestor - use with care.
async fn get_next_chain_ancestor_unchecked<S: Storage>(
    storage: &S,
    descendant: &Hash,
    ancestor: &Hash,
) -> Result<Hash, BlockchainError> {
    let children = storage.get_reachability_data(ancestor).await?.children;

    match binary_search_descendant(storage, &children, descendant).await? {
        SearchOutput::Found(hash, _) => Ok(hash),
        SearchOutput::NotFound(_) => Err(BlockchainError::InvalidReachability),
    }
}

/// Get the interval capacity available for children of a block
///
/// Returns parent.interval.decrease_end(1) to maintain strict containment invariant.
async fn interval_children_capacity<S: Storage>(
    storage: &S,
    block: &Hash,
) -> Result<Interval, BlockchainError> {
    let data = storage.get_reachability_data(block).await?;
    Ok(data.interval.decrease_end(1))
}

/// Get the available interval space before the first child
///
/// Returns the interval from [capacity.start, first_child.start - 1].
async fn interval_remaining_before<S: Storage>(
    storage: &S,
    block: &Hash,
) -> Result<Interval, BlockchainError> {
    let alloc_capacity = interval_children_capacity(storage, block).await?;
    let children = storage.get_reachability_data(block).await?.children;

    match children.first() {
        Some(first_child) => {
            let first_alloc = storage.get_reachability_data(first_child).await?.interval;
            Ok(Interval::new(alloc_capacity.start, first_alloc.start - 1))
        }
        None => Ok(alloc_capacity),
    }
}

/// Get the available interval space after the last child
///
/// Returns the interval from [last_child.end + 1, capacity.end].
async fn interval_remaining_after<S: Storage>(
    storage: &S,
    block: &Hash,
) -> Result<Interval, BlockchainError> {
    let alloc_capacity = interval_children_capacity(storage, block).await?;
    let children = storage.get_reachability_data(block).await?.children;

    match children.last() {
        Some(last_child) => {
            let last_alloc = storage.get_reachability_data(last_child).await?.interval;
            Ok(Interval::new(last_alloc.end + 1, alloc_capacity.end))
        }
        None => Ok(alloc_capacity),
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
