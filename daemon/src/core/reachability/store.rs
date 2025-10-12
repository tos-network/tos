// TOS Reachability Storage Types
// Based on Kaspa's reachability store
// Reference: rusty-kaspa/consensus/src/model/stores/reachability.rs

use super::Interval;
use serde::{Deserialize, Serialize};
use tos_common::crypto::Hash;

/// Reachability data for a single block
///
/// This data enables efficient O(log n) DAG ancestry queries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReachabilityData {
    /// Parent block in the selected parent chain (tree structure)
    pub parent: Hash,

    /// Interval allocated to this block in the reachability tree
    /// Used for fast chain ancestry queries via interval containment
    pub interval: Interval,

    /// Height in the selected parent chain (tree structure)
    pub height: u64,

    /// Children in the selected parent chain (tree structure)
    /// Sorted by interval.start for binary search
    pub children: Vec<Hash>,

    /// Future covering set: blocks in the DAG future of this block
    /// Used for DAG ancestry queries beyond the chain
    /// Sorted by interval.start for binary search
    pub future_covering_set: Vec<Hash>,
}

impl ReachabilityData {
    /// Create new reachability data
    pub fn new(
        parent: Hash,
        interval: Interval,
        height: u64,
        children: Vec<Hash>,
        future_covering_set: Vec<Hash>,
    ) -> Self {
        Self {
            parent,
            interval,
            height,
            children,
            future_covering_set,
        }
    }

    /// Create reachability data for a leaf node (no children)
    pub fn new_leaf(parent: Hash, interval: Interval, height: u64) -> Self {
        Self {
            parent,
            interval,
            height,
            children: Vec::new(),
            future_covering_set: Vec::new(),
        }
    }
}
