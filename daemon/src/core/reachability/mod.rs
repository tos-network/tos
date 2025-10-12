// TOS Reachability Service
// Based on Kaspa's reachability implementation
// Reference: rusty-kaspa/consensus/src/processes/reachability/
//
// This is a Phase 2 minimal implementation focusing on core ancestry queries
// needed for GHOSTDAG mergeset calculation. Full Kaspa features (reindexing,
// interval concentration, etc.) will be added in later milestones.

mod interval;
mod store;

pub use interval::Interval;
pub use store::ReachabilityData;

use tos_common::crypto::Hash;
use crate::core::error::BlockchainError;

// TODO: Uncomment when ReachabilityDataProvider is added to Storage trait
// use crate::core::storage::Storage;

/// Reachability service for DAG ancestry queries
///
/// Provides efficient O(log n) queries for determining if one block
/// is an ancestor of another in the DAG structure.
///
/// Core Concepts:
/// - **Chain ancestry**: Block A is a chain ancestor of B if A is on the selected parent chain to B
/// - **DAG ancestry**: Block A is a DAG ancestor of B if B is reachable from A through any path
/// - **Intervals**: Each block has an interval [start, end] representing its position in the tree
/// - **Future covering set**: Blocks in the DAG future used for non-chain ancestry queries
///
/// Algorithm (simplified):
/// - Chain ancestry: Check if A's interval contains B's interval
/// - DAG ancestry: Check chain ancestry first, then search future covering set
pub struct TosReachability {
    /// Genesis block hash
    genesis_hash: Hash,
}

impl TosReachability {
    /// Create a new reachability service
    pub fn new(genesis_hash: Hash) -> Self {
        Self { genesis_hash }
    }

    // TODO: Uncomment and implement when Storage trait includes ReachabilityDataProvider
    // /// Check if `this` block is a DAG ancestor of `queried` block
    // ///
    // /// Returns true if `queried` is reachable from `this` through any path in the DAG.
    // /// Note: A block is considered an ancestor of itself.
    // ///
    // /// # Algorithm
    // /// 1. Check if `this` is a chain ancestor (interval containment)
    // /// 2. If not, search the future covering set of `this`
    // ///
    // /// # Complexity
    // /// O(log(|future_covering_set|)) for non-chain queries
    // pub async fn is_dag_ancestor_of<S: Storage>(
    //     &self,
    //     storage: &S,
    //     this: &Hash,
    //     queried: &Hash,
    // ) -> Result<bool, BlockchainError> {
    //     // Implementation here
    // }

    /// Initialize reachability data for genesis block
    pub fn genesis_reachability_data(&self) -> ReachabilityData {
        ReachabilityData {
            parent: self.genesis_hash.clone(), // Genesis is its own parent
            interval: Interval::maximal(), // Genesis gets the maximal interval
            height: 0,
            children: Vec::new(),
            future_covering_set: Vec::new(),
        }
    }

    // TODO: Uncomment and implement when Storage trait includes ReachabilityDataProvider
    // /// Add a new block to the reachability tree
    // ///
    // /// This is a simplified version that allocates intervals without reindexing.
    // /// Full Kaspa implementation includes complex reindexing logic when intervals run out.
    // pub async fn add_block<S: Storage>(
    //     &self,
    //     storage: &S,
    //     new_block: Hash,
    //     parent: Hash,
    //     _mergeset: &[Hash],
    // ) -> Result<ReachabilityData, BlockchainError> {
    //     // Implementation here
    // }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reachability_creation() {
        let genesis_hash = Hash::new([0u8; 32]);
        let reachability = TosReachability::new(genesis_hash.clone());

        let genesis_data = reachability.genesis_reachability_data();
        assert_eq!(genesis_data.height, 0);
        assert_eq!(genesis_data.interval, Interval::maximal());
        assert!(genesis_data.children.is_empty());
        assert!(genesis_data.future_covering_set.is_empty());
    }

    #[test]
    fn test_interval_basics() {
        let maximal = Interval::maximal();
        assert_eq!(maximal.start, 1);
        assert_eq!(maximal.end, u64::MAX - 1);
        assert!(!maximal.is_empty());

        let (left, right) = maximal.split_half();
        assert!(maximal.contains(left));
        assert!(maximal.contains(right));
        assert!(!left.contains(right));
        assert!(!right.contains(left));
    }
}
