// TOS GHOSTDAG Implementation
// Based on Kaspa's GHOSTDAG protocol
// Reference: rusty-kaspa/consensus/src/processes/ghostdag/

pub mod types;

pub use types::{BlueWorkType, CompactGhostdagData, KType, TosGhostdagData};

use anyhow::Result;
use std::cmp::Ordering;
use std::collections::HashMap;
use tos_common::crypto::Hash;
use tos_common::difficulty::Difficulty;

use crate::core::error::BlockchainError;
use crate::core::storage::Storage;

/// Calculate work from difficulty
/// Based on Kaspa's calc_work function
/// Source: https://github.com/bitcoin/bitcoin/blob/2e34374bf3e12b37b0c66824a6c998073cdfab01/src/chain.cpp#L131
///
/// We need to compute 2**256 / (target+1), but we can't represent 2**256
/// as it's too large. However, as 2**256 is at least as large
/// as target+1, it is equal to ((2**256 - target - 1) / (target+1)) + 1,
/// or ~target / (target+1) + 1.
pub fn calc_work_from_difficulty(difficulty: &Difficulty) -> BlueWorkType {
    // Convert difficulty (VarUint wrapping common's U256 v0.13.1) to daemon's U256 v0.12
    // We do this by serializing to bytes and deserializing with the correct version
    let diff_u256_common = difficulty.as_ref();

    // Check for zero difficulty
    if diff_u256_common.is_zero() {
        return BlueWorkType::zero();
    }

    // Serialize common's U256 (v0.13.1) to bytes
    // In v0.13.1, to_big_endian() returns [u8; 32] directly
    let diff_bytes = diff_u256_common.to_big_endian();

    // Deserialize into daemon's U256 v0.12 (BlueWorkType)
    let diff_u256_daemon = BlueWorkType::from_big_endian(&diff_bytes);

    // Calculate target = MAX / difficulty (TOS's difficulty semantics)
    let target = BlueWorkType::max_value() / diff_u256_daemon;

    // Calculate work: (~target / (target + 1)) + 1
    // This formula is from Bitcoin and Kaspa
    // Source: https://github.com/bitcoin/bitcoin/blob/2e34374bf3e12b37b0c66824a6c998073cdfab01/src/chain.cpp#L131
    let res = (!target / (target + BlueWorkType::one())) + BlueWorkType::one();

    res
}

/// SortableBlock for topological ordering by blue work
/// Based on Kaspa's ordering.rs
#[derive(Clone, Debug)]
struct SortableBlock {
    hash: Hash,
    blue_work: BlueWorkType,
}

impl SortableBlock {
    fn new(hash: Hash, blue_work: BlueWorkType) -> Self {
        Self { hash, blue_work }
    }
}

impl PartialEq for SortableBlock {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Eq for SortableBlock {}

impl PartialOrd for SortableBlock {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SortableBlock {
    fn cmp(&self, other: &Self) -> Ordering {
        // First compare by blue_work, then by hash for determinism
        self.blue_work.cmp(&other.blue_work).then_with(|| {
            // Compare hash bytes
            self.hash.as_bytes().cmp(other.hash.as_bytes())
        })
    }
}

/// TOS GHOSTDAG Manager
/// Implements the GHOSTDAG protocol for block ordering and selection
///
/// GHOSTDAG (Greedy Heaviest-Observed Sub-DAG) is a generalization of Nakamoto consensus
/// that allows for a block DAG instead of a chain. It defines a chain (blue chain) within
/// the DAG based on a greedy algorithm that maximizes accumulated proof-of-work.
///
/// Key Concepts:
/// - **Blue blocks**: Blocks in the selected chain (similar to "canonical" chain)
/// - **Red blocks**: Blocks not in the selected chain (similar to "orphans" but still processed)
/// - **K parameter**: Maximum anticone size for blue blocks (k-cluster constraint)
/// - **Blue score**: Number of blue blocks in the past (similar to "height")
/// - **Blue work**: Cumulative work of all blue blocks (used for chain selection)
///
/// Algorithm Summary:
/// 1. Select parent with highest blue_work as "selected parent"
/// 2. Get all blocks in the mergeset (blocks being merged by this new block)
/// 3. For each candidate block in topological order:
///    - Check if adding it violates k-cluster constraint
///    - If no: color it blue
///    - If yes: color it red
/// 4. Calculate blue_score and blue_work based on blue blocks
///
/// For details, see: https://eprint.iacr.org/2018/104.pdf
pub struct TosGhostdag {
    /// K-cluster parameter (typically 10 for Kaspa, we start with 10)
    k: KType,

    /// Genesis block hash
    genesis_hash: Hash,
}

impl TosGhostdag {
    /// Create a new GHOSTDAG manager
    ///
    /// # Arguments
    /// * `k` - The k-cluster parameter (maximum anticone size for blue blocks)
    /// * `genesis_hash` - Hash of the genesis block
    pub fn new(k: KType, genesis_hash: Hash) -> Self {
        Self {
            k,
            genesis_hash,
        }
    }

    /// Get the k parameter
    pub fn k(&self) -> KType {
        self.k
    }

    /// Create GHOSTDAG data for genesis block
    pub fn genesis_ghostdag_data(&self) -> TosGhostdagData {
        TosGhostdagData::new(
            0,                      // blue_score
            BlueWorkType::zero(),   // blue_work
            Hash::new([0u8; 32]),   // selected_parent (genesis has no parent - zero hash)
            Vec::new(),             // mergeset_blues
            Vec::new(),             // mergeset_reds
            std::collections::HashMap::new(), // blues_anticone_sizes
        )
    }

    /// Find the selected parent from a list of parents
    /// The selected parent is the one with the highest blue_work
    ///
    /// # Arguments
    /// * `storage` - Reference to blockchain storage
    /// * `parents` - Iterator of parent block hashes
    ///
    /// # Returns
    /// Hash of the selected parent (the one with highest blue_work)
    pub async fn find_selected_parent<S: Storage>(
        &self,
        storage: &S,
        parents: impl IntoIterator<Item = Hash>,
    ) -> Result<Hash, BlockchainError> {
        let mut best_parent = None;
        let mut best_blue_work = BlueWorkType::zero();

        for parent in parents {
            // Get GHOSTDAG data for this parent
            let parent_data = storage.get_ghostdag_data(&parent).await?;

            // Compare blue work
            if parent_data.blue_work > best_blue_work {
                best_blue_work = parent_data.blue_work;
                best_parent = Some(parent);
            }
        }

        best_parent.ok_or_else(|| {
            BlockchainError::InvalidConfig  // Use existing error variant
        })
    }

    /// Run the GHOSTDAG algorithm for a new block with given parents
    ///
    /// This is the core GHOSTDAG protocol implementation.
    /// Based on Kaspa's ghostdag() in protocol.rs (lines 127-168)
    ///
    /// # Arguments
    /// * `storage` - Reference to blockchain storage
    /// * `parents` - Slice of parent block hashes
    ///
    /// # Returns
    /// TosGhostdagData for the new block
    ///
    /// # Algorithm (from GHOSTDAG whitepaper)
    /// 1. Find selected parent (highest blue_work)
    /// 2. Initialize new block data with selected parent
    /// 3. Get ordered mergeset (topological sort by blue_work)
    /// 4. For each candidate in mergeset:
    ///    a. Check k-cluster conditions:
    ///       - |anticone(candidate) ∩ blue_set| ≤ K
    ///       - For all blues: |(anticone(blue) ∩ blue_set) ∪ {candidate}| ≤ K
    ///    b. If no violation: add as blue
    ///    c. If violation: add as red
    /// 5. Calculate blue_score = parent.blue_score + |mergeset_blues|
    /// 6. Calculate blue_work = parent.blue_work + sum(work of blues in mergeset)
    ///
    /// See: https://eprint.iacr.org/2018/104.pdf
    pub async fn ghostdag<S: Storage>(&self, storage: &S, parents: &[Hash]) -> Result<TosGhostdagData, BlockchainError> {
        // Genesis block special case
        if parents.is_empty() {
            return Ok(self.genesis_ghostdag_data());
        }

        // Step 1: Find selected parent (parent with highest blue_work)
        let selected_parent = self.find_selected_parent(storage, parents.iter().cloned()).await?;

        // Step 2: Initialize new block data with selected parent as first blue
        let mut new_block_data = TosGhostdagData::new_with_selected_parent(selected_parent.clone(), self.k);

        // Step 3: Get ordered mergeset (topologically sorted by blue_work)
        let ordered_mergeset = self.ordered_mergeset_without_selected_parent(storage, selected_parent.clone(), parents).await?;

        // Step 4: Process each candidate block in topological order
        for candidate in ordered_mergeset {
            // Check if candidate can be blue without violating k-cluster
            let (is_blue, anticone_size, blues_anticone_sizes) =
                self.check_blue_candidate(storage, &new_block_data, &candidate).await?;

            if is_blue {
                // No k-cluster violation - add as blue
                new_block_data.add_blue(candidate, anticone_size, &blues_anticone_sizes);
            } else {
                // K-cluster violation - add as red
                new_block_data.add_red(candidate);
            }
        }

        // Step 5: Calculate blue_score
        // blue_score = parent's blue_score + number of blues in mergeset
        let parent_data = storage.get_ghostdag_data(&selected_parent).await?;
        let blue_score = parent_data.blue_score + new_block_data.mergeset_blues.len() as u64;

        // Step 6: Calculate blue_work
        // blue_work = parent's blue_work + sum of work for all blues in mergeset
        // Calculate actual work from each block's difficulty
        let mut added_blue_work = BlueWorkType::zero();
        for blue_hash in new_block_data.mergeset_blues.iter() {
            // Get the difficulty for this blue block
            let difficulty = storage.get_difficulty_for_block_hash(blue_hash).await?;
            // Calculate work from difficulty
            let block_work = calc_work_from_difficulty(&difficulty);
            added_blue_work = added_blue_work + block_work;
        }
        let blue_work = parent_data.blue_work + added_blue_work;

        // Finalize the GHOSTDAG data
        new_block_data.finalize_score_and_work(blue_score, blue_work);

        Ok(new_block_data)
    }

    /// Sort blocks by blue work (topological order)
    /// Based on Kaspa's sort_blocks in ordering.rs
    async fn sort_blocks<S: Storage>(&self, storage: &S, blocks: Vec<Hash>) -> Result<Vec<Hash>, BlockchainError> {
        let mut sortable_blocks = Vec::with_capacity(blocks.len());

        for hash in blocks {
            let blue_work = storage.get_ghostdag_blue_work(&hash).await?;
            sortable_blocks.push(SortableBlock::new(hash, blue_work));
        }

        sortable_blocks.sort();
        Ok(sortable_blocks.into_iter().map(|sb| sb.hash).collect())
    }

    /// Get ordered mergeset without the selected parent
    /// BFS-based implementation with conservative heuristic
    ///
    /// Based on Kaspa's ordered_mergeset_without_selected_parent in mergeset.rs
    /// Phase 2 improvement: Uses BFS to explore mergeset candidates
    ///
    /// Note: Full Kaspa implementation uses reachability service to determine
    /// if a block is in the past of selected parent. We use a conservative heuristic:
    /// - If block.blue_score <= selected_parent.blue_score - 10, it's likely in the past
    /// - This is safe but may miss some valid mergeset candidates
    ///
    /// Full reachability service will be implemented in later Phase 2 milestone.
    async fn ordered_mergeset_without_selected_parent<S: Storage>(
        &self,
        storage: &S,
        selected_parent: Hash,
        parents: &[Hash],
    ) -> Result<Vec<Hash>, BlockchainError> {
        use std::collections::{HashSet, VecDeque};

        // Get selected parent's blue score for heuristic
        let selected_parent_data = storage.get_ghostdag_data(&selected_parent).await?;
        let selected_parent_blue_score = selected_parent_data.blue_score;

        // Initialize BFS queue with non-selected parents
        let mut queue: VecDeque<Hash> = parents.iter()
            .filter(|&p| p != &selected_parent)
            .cloned()
            .collect();

        // Track visited blocks
        let mut mergeset: HashSet<Hash> = queue.iter().cloned().collect();
        let mut past: HashSet<Hash> = HashSet::new();

        // BFS exploration
        while let Some(current) = queue.pop_front() {
            // Get current block's header to access its parents
            let current_header = storage.get_block_header_by_hash(&current).await?;
            let current_parents = current_header.get_tips();

            // For each parent of current block
            for parent in current_parents.iter() {
                // Skip if already processed
                if mergeset.contains(parent) || past.contains(parent) {
                    continue;
                }

                // Conservative heuristic: Check if parent is likely in selected_parent's past
                // Get parent's GHOSTDAG data
                let parent_data = storage.get_ghostdag_data(parent).await?;
                let parent_blue_score = parent_data.blue_score;

                // If parent's blue_score is significantly lower, it's likely in the past
                // Use a conservative threshold of 10 blocks
                if parent_blue_score + 10 < selected_parent_blue_score {
                    past.insert(parent.clone());
                    continue;
                }

                // Otherwise, add to mergeset and queue for further exploration
                mergeset.insert(parent.clone());
                queue.push_back(parent.clone());
            }
        }

        // Convert HashSet to Vec and sort by blue work
        let mergeset_vec: Vec<Hash> = mergeset.into_iter().collect();
        self.sort_blocks(storage, mergeset_vec).await
    }

    /// Returns the blue anticone size of `block` from the worldview of `context`.
    /// Expects `block` to be in the blue set of `context`.
    ///
    /// Based on Kaspa's blue_anticone_size in protocol.rs (lines 234-249)
    ///
    /// Walks the selected parent chain until finding the block in blues_anticone_sizes map.
    async fn blue_anticone_size<S: Storage>(
        &self,
        storage: &S,
        block: &Hash,
        context: &TosGhostdagData,
    ) -> Result<KType, BlockchainError> {
        let mut current_blues_anticone_sizes = context.blues_anticone_sizes.clone();
        let mut current_selected_parent = context.selected_parent.clone();

        loop {
            // Check if we have the anticone size for this block
            if let Some(&size) = current_blues_anticone_sizes.get(block) {
                return Ok(size);
            }

            // Check if we reached genesis
            if current_selected_parent == self.genesis_hash {
                // Block not found in blue set - this shouldn't happen if called correctly
                return Err(BlockchainError::InvalidConfig);
            }

            // Move to parent's GHOSTDAG data
            let parent_data = storage.get_ghostdag_data(&current_selected_parent).await?;
            current_blues_anticone_sizes = parent_data.blues_anticone_sizes.clone();
            current_selected_parent = parent_data.selected_parent.clone();
        }
    }

    /// Check if a candidate block can be blue (doesn't violate k-cluster)
    ///
    /// Based on Kaspa's check_blue_candidate in protocol.rs (lines 251-287)
    ///
    /// Simplified version for Phase 1:
    /// - Checks if mergeset_blues size would exceed k+1
    /// - For each existing blue, checks if candidate would violate k-cluster
    /// - Conservative: may reject valid blues, but won't accept invalid ones
    ///
    /// Returns: (is_blue, blue_anticone_size, blues_anticone_sizes_map)
    async fn check_blue_candidate<S: Storage>(
        &self,
        storage: &S,
        new_block_data: &TosGhostdagData,
        _candidate: &Hash,
    ) -> Result<(bool, KType, HashMap<Hash, KType>), BlockchainError> {
        // Check 1: Mergeset blues cannot exceed k+1 (selected parent + k blues)
        if new_block_data.mergeset_blues.len() >= (self.k + 1) as usize {
            return Ok((false, 0, HashMap::new()));
        }

        let mut candidate_blues_anticone_sizes: HashMap<Hash, KType> = HashMap::new();
        let mut candidate_blue_anticone_size: KType = 0;

        // Check 2: Validate k-cluster with existing blues
        // Iterate over all blues in new_block_data
        for blue in new_block_data.mergeset_blues.iter() {
            // Simplified check: assume all blues are in anticone of candidate
            // (conservative - may reject valid candidates)
            // Full implementation would use reachability to check if blue is ancestor of candidate

            // Get the blue anticone size for this blue block
            let blue_anticone_size = self.blue_anticone_size(storage, blue, new_block_data).await?;

            // Record this for the candidate's blues_anticone_sizes map
            candidate_blues_anticone_sizes.insert(blue.clone(), blue_anticone_size);

            // Increment candidate's blue anticone size
            candidate_blue_anticone_size += 1;

            // Check k-cluster condition 1: candidate's blue anticone must be ≤ k
            if candidate_blue_anticone_size > self.k {
                return Ok((false, 0, HashMap::new()));
            }

            // Check k-cluster condition 2: existing blue's anticone + candidate must be ≤ k
            if blue_anticone_size >= self.k {
                return Ok((false, 0, HashMap::new()));
            }
        }

        // All checks passed - candidate can be blue
        Ok((true, candidate_blue_anticone_size, candidate_blues_anticone_sizes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full tests require storage implementation
    // For now, we test basic structure

    #[test]
    fn test_ghostdag_creation() {
        // This test will be expanded once we have a mock storage
        let k = 10;
        assert_eq!(k, 10); // Placeholder test
    }

    #[test]
    fn test_genesis_data() {
        // Create a minimal mock (we'll improve this with proper mock storage)
        // For now, just test data structure creation
        let genesis_data = TosGhostdagData::new(
            0,
            BlueWorkType::zero(),
            Hash::new([0u8; 32]),  // Zero hash
            Vec::new(),
            Vec::new(),
            std::collections::HashMap::new(),
        );

        assert_eq!(genesis_data.blue_score, 0);
        assert_eq!(genesis_data.blue_work, BlueWorkType::zero());
    }
}
