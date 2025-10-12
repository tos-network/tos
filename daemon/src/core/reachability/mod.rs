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
use crate::core::storage::Storage;

/// Binary search result for reachability queries
#[allow(dead_code)]
enum SearchResult {
    /// Found the hash at the given index
    Found(Hash, usize),
    /// Not found, but would be inserted at the given index
    NotFound(usize),
}

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

    /// Check if `this` block is a chain ancestor of `queried` block
    ///
    /// Returns true if `this` is on the selected parent chain to `queried`.
    /// Note: A block is considered a chain ancestor of itself.
    ///
    /// # Algorithm
    /// Uses interval containment: `this.interval.contains(queried.interval)`
    ///
    /// # Complexity
    /// O(1) - constant time interval check
    pub async fn is_chain_ancestor_of<S: Storage>(
        &self,
        storage: &S,
        this: &Hash,
        queried: &Hash,
    ) -> Result<bool, BlockchainError> {
        let this_data = storage.get_reachability_data(this).await?;
        let queried_data = storage.get_reachability_data(queried).await?;
        Ok(this_data.interval.contains(queried_data.interval))
    }

    /// Check if `this` block is a DAG ancestor of `queried` block
    ///
    /// Returns true if `queried` is reachable from `this` through any path in the DAG.
    /// Note: A block is considered an ancestor of itself.
    ///
    /// # Algorithm
    /// 1. Check if `this` is a chain ancestor (interval containment)
    /// 2. If not, search the future covering set of `this` using binary search
    ///
    /// # Complexity
    /// O(log(|future_covering_set|)) for non-chain queries
    pub async fn is_dag_ancestor_of<S: Storage>(
        &self,
        storage: &S,
        this: &Hash,
        queried: &Hash,
    ) -> Result<bool, BlockchainError> {
        // First, check if `this` is a chain ancestor of queried
        if self.is_chain_ancestor_of(storage, this, queried).await? {
            return Ok(true);
        }

        // Otherwise, use future covering set to complete the DAG reachability test
        let this_data = storage.get_reachability_data(this).await?;
        match self.binary_search_descendant(storage, &this_data.future_covering_set, queried).await? {
            SearchResult::Found(_, _) => Ok(true),
            SearchResult::NotFound(_) => Ok(false),
        }
    }

    /// Binary search for a descendant block in an ordered list
    ///
    /// The list is ordered by interval.start, and we search for a block
    /// whose interval contains the queried descendant's interval.
    ///
    /// Returns either:
    /// - Found(hash, index): The hash at the index that contains the descendant
    /// - NotFound(index): The index where the descendant should be inserted
    async fn binary_search_descendant<S: Storage>(
        &self,
        storage: &S,
        ordered_hashes: &[Hash],
        descendant: &Hash,
    ) -> Result<SearchResult, BlockchainError> {
        let descendant_data = storage.get_reachability_data(descendant).await?;
        let point = descendant_data.interval.end;

        // Binary search by interval.start
        let result = ordered_hashes.binary_search_by_key(&point, |hash| {
            // This is safe to unwrap in production since data inconsistency would be fatal
            futures::executor::block_on(async {
                storage.get_reachability_data(hash).await
                    .map(|data| data.interval.start)
                    .unwrap_or(0)
            })
        });

        match result {
            Ok(i) => Ok(SearchResult::Found(ordered_hashes[i].clone(), i)),
            Err(i) => {
                // `i` is where `point` was expected, so check if ordered_hashes[i-1] contains descendant
                if i > 0 && self.is_chain_ancestor_of(storage, &ordered_hashes[i - 1], descendant).await? {
                    Ok(SearchResult::Found(ordered_hashes[i - 1].clone(), i - 1))
                } else {
                    Ok(SearchResult::NotFound(i))
                }
            }
        }
    }

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
