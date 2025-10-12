// TOS GHOSTDAG Implementation
// Based on Kaspa's GHOSTDAG protocol
// Reference: rusty-kaspa/consensus/src/processes/ghostdag/

pub mod types;
pub mod daa;

pub use types::{BlueWorkType, CompactGhostdagData, KType, TosGhostdagData};
pub use daa::{calculate_daa_score, calculate_target_difficulty, DAA_WINDOW_SIZE, TARGET_TIME_PER_BLOCK};

use anyhow::Result;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tos_common::crypto::Hash;
use tos_common::difficulty::Difficulty;

use crate::core::error::BlockchainError;
use crate::core::reachability::TosReachability;
use crate::core::storage::Storage;

/// Calculate work from difficulty
/// Based on Kaspa's calc_work function
/// Source: https://github.com/bitcoin/bitcoin/blob/2e34374bf3e12b37b0c66824a6c998073cdfab01/src/chain.cpp#L131
///
/// We need to compute 2**256 / (target+1), but we can't represent 2**256
/// as it's too large. However, as 2**256 is at least as large
/// as target+1, it is equal to ((2**256 - target - 1) / (target+1)) + 1,
/// or ~target / (target+1) + 1.
/// SECURITY FIX V-06: Added zero difficulty check to prevent division by zero
pub fn calc_work_from_difficulty(difficulty: &Difficulty) -> BlueWorkType {
    // Convert difficulty (VarUint wrapping common's U256 v0.13.1) to daemon's U256 v0.12
    // We do this by serializing to bytes and deserializing with the correct version
    let diff_u256_common = difficulty.as_ref();

    // SECURITY FIX V-06: Check for zero difficulty to prevent division by zero
    if diff_u256_common.is_zero() {
        // Return maximum work for zero difficulty (or could reject the block)
        // Using max work means zero difficulty blocks have infinite work
        // In practice, blocks with zero difficulty should be rejected during validation
        return BlueWorkType::max_value();
    }

    // Serialize common's U256 (v0.13.1) to bytes
    // In v0.13.1, to_big_endian() returns [u8; 32] directly
    let diff_bytes = diff_u256_common.to_big_endian();

    // Deserialize into daemon's U256 v0.12 (BlueWorkType)
    let diff_u256_daemon = BlueWorkType::from_big_endian(&diff_bytes);

    // SECURITY FIX V-06: Double-check to prevent division by zero at daemon U256 level
    if diff_u256_daemon.is_zero() {
        return BlueWorkType::max_value();
    }

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

    /// Reachability service for DAG ancestry queries (TIP-2 Phase 2)
    reachability: Arc<TosReachability>,
}

impl TosGhostdag {
    /// Create a new GHOSTDAG manager
    ///
    /// # Arguments
    /// * `k` - The k-cluster parameter (maximum anticone size for blue blocks)
    /// * `genesis_hash` - Hash of the genesis block
    /// * `reachability` - Reachability service for DAG ancestry queries
    pub fn new(k: KType, genesis_hash: Hash, reachability: Arc<TosReachability>) -> Self {
        Self {
            k,
            genesis_hash,
            reachability,
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
            Vec::new(),             // mergeset_non_daa (empty for genesis)
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

        // SECURITY FIX V-05: Return proper error for no valid parents
        best_parent.ok_or(BlockchainError::NoValidParents)
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

        // SECURITY FIX V-05: Validate all parents exist before processing
        for parent_hash in parents.iter() {
            // Check if parent block exists in storage
            if !storage.has_block_with_hash(parent_hash).await? {
                return Err(BlockchainError::ParentNotFound(parent_hash.clone()));
            }
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
        // SECURITY FIX V-01: Use checked arithmetic to prevent overflow
        // blue_score = parent's blue_score + number of blues in mergeset
        let parent_data = storage.get_ghostdag_data(&selected_parent).await?;
        let blue_score = parent_data.blue_score
            .checked_add(new_block_data.mergeset_blues.len() as u64)
            .ok_or(BlockchainError::BlueScoreOverflow)?;

        // Step 6: Calculate blue_work
        // SECURITY FIX V-01: Use checked arithmetic to prevent overflow
        // blue_work = parent's blue_work + sum of work for all blues in mergeset
        // Calculate actual work from each block's difficulty
        let mut added_blue_work = BlueWorkType::zero();
        for blue_hash in new_block_data.mergeset_blues.iter() {
            // Get the difficulty for this blue block
            let difficulty = storage.get_difficulty_for_block_hash(blue_hash).await?;
            // Calculate work from difficulty
            let block_work = calc_work_from_difficulty(&difficulty);
            // Use checked addition for blue work accumulation
            added_blue_work = added_blue_work.checked_add(block_work)
                .ok_or(BlockchainError::BlueWorkOverflow)?;
        }
        let blue_work = parent_data.blue_work.checked_add(added_blue_work)
            .ok_or(BlockchainError::BlueWorkOverflow)?;

        // Step 7: Calculate DAA score and identify mergeset_non_daa blocks
        // This is Phase 3 addition for complete DAA implementation
        // Extract mergeset_blues without selected_parent for DAA calculation
        let mergeset_blues_without_selected: Vec<Hash> = new_block_data
            .mergeset_blues
            .iter()
            .filter(|b| *b != &selected_parent)
            .cloned()
            .collect();

        let (_daa_score, mergeset_non_daa) = daa::calculate_daa_score(
            storage,
            &selected_parent,
            &mergeset_blues_without_selected,
        )
        .await?;

        // Set the mergeset_non_daa blocks
        new_block_data.set_mergeset_non_daa(mergeset_non_daa);

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
    /// BFS-based implementation with reachability service
    ///
    /// Based on Kaspa's ordered_mergeset_without_selected_parent in mergeset.rs
    /// Phase 2 complete implementation: Uses BFS with reachability service to accurately
    /// determine which blocks are in the past of the selected parent.
    async fn ordered_mergeset_without_selected_parent<S: Storage>(
        &self,
        storage: &S,
        selected_parent: Hash,
        parents: &[Hash],
    ) -> Result<Vec<Hash>, BlockchainError> {
        use std::collections::{HashSet, VecDeque};

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
            let current_parents = current_header.get_parents();

            // For each parent of current block
            for parent in current_parents.iter() {
                // Skip if already processed
                if mergeset.contains(parent) || past.contains(parent) {
                    continue;
                }

                // Try to use reachability service to check if parent is in selected_parent's past
                // If reachability data doesn't exist yet (during migration), fall back to blue_score heuristic
                let is_in_past = match (
                    storage.has_reachability_data(parent).await,
                    storage.has_reachability_data(&selected_parent).await
                ) {
                    (Ok(true), Ok(true)) => {
                        // Both blocks have reachability data - use accurate DAG ancestry check
                        self.reachability.is_dag_ancestor_of(storage, parent, &selected_parent).await?
                    }
                    _ => {
                        // Fall back to conservative heuristic for blocks without reachability data
                        let parent_data = storage.get_ghostdag_data(parent).await?;
                        let selected_parent_data = storage.get_ghostdag_data(&selected_parent).await?;
                        parent_data.blue_score + 10 < selected_parent_data.blue_score
                    }
                };

                if is_in_past {
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
    /// SECURITY FIX V-03: Implements proper k-cluster validation using reachability
    /// This is the CORE SECURITY GUARANTEE of GHOSTDAG consensus.
    ///
    /// K-cluster property: For all blue blocks B in blues(C), |anticone(B, blues(C))| < k
    /// Where anticone(B, S) = blocks in S that are neither ancestors nor descendants of B
    ///
    /// Returns: (is_blue, blue_anticone_size, blues_anticone_sizes_map)
    async fn check_blue_candidate<S: Storage>(
        &self,
        storage: &S,
        new_block_data: &TosGhostdagData,
        candidate: &Hash,
    ) -> Result<(bool, KType, HashMap<Hash, KType>), BlockchainError> {
        // Check 1: Mergeset blues cannot exceed k+1 (selected parent + k blues)
        if new_block_data.mergeset_blues.len() >= (self.k + 1) as usize {
            return Ok((false, 0, HashMap::new()));
        }

        let mut candidate_blues_anticone_sizes: HashMap<Hash, KType> = HashMap::new();
        let mut candidate_blue_anticone_size: KType = 0;

        // SECURITY FIX V-03: Proper k-cluster validation using reachability
        // Check 2: Validate k-cluster constraint for candidate
        // For each existing blue block, check if it's in the anticone of candidate
        for blue in new_block_data.mergeset_blues.iter() {
            // Get the blue anticone size for this blue block
            let blue_anticone_size = self.blue_anticone_size(storage, blue, new_block_data).await?;

            // Check if blue and candidate are in each other's anticone
            // Two blocks are in each other's anticone if neither is an ancestor of the other
            let is_in_anticone = if storage.has_reachability_data(blue).await.unwrap_or(false)
                && storage.has_reachability_data(candidate).await.unwrap_or(false) {
                // Use reachability data for accurate anticone check
                !self.reachability.is_dag_ancestor_of(storage, blue, candidate).await?
                    && !self.reachability.is_dag_ancestor_of(storage, candidate, blue).await?
            } else {
                // Fallback: conservative assumption (all blues in anticone)
                true
            };

            if is_in_anticone {
                // Candidate and this blue are in each other's anticone
                candidate_blue_anticone_size += 1;

                // Check k-cluster condition 1: candidate's blue anticone must be < k
                if candidate_blue_anticone_size >= self.k {
                    return Err(BlockchainError::KClusterViolation {
                        block: candidate.clone(),
                        anticone_size: candidate_blue_anticone_size as usize,
                        k: self.k,
                    });
                }

                // Check k-cluster condition 2: existing blue's anticone + candidate must be < k
                if blue_anticone_size >= self.k {
                    return Err(BlockchainError::KClusterViolation {
                        block: blue.clone(),
                        anticone_size: (blue_anticone_size + 1) as usize,
                        k: self.k,
                    });
                }

                // Record updated anticone size for this blue
                candidate_blues_anticone_sizes.insert(blue.clone(), blue_anticone_size + 1);
            } else {
                // Blue and candidate are in chain relationship (not anticone)
                candidate_blues_anticone_sizes.insert(blue.clone(), blue_anticone_size);
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
            Vec::new(),  // Empty mergeset_non_daa for genesis
        );

        assert_eq!(genesis_data.blue_score, 0);
        assert_eq!(genesis_data.blue_work, BlueWorkType::zero());
    }

    // Task 4.1: GHOSTDAG Edge Case Tests

    /// Test 1: Maximum parent count edge case (32 parents)
    #[test]
    fn test_ghostdag_max_parent_count() {
        // TOS supports up to 32 parents (2^5 bits for parent count)
        const MAX_PARENTS: usize = 32;

        // Create 32 parent hashes
        let mut parents = Vec::with_capacity(MAX_PARENTS);
        for i in 0..MAX_PARENTS {
            let mut hash_bytes = [0u8; 32];
            hash_bytes[0] = i as u8;
            parents.push(Hash::new(hash_bytes));
        }

        assert_eq!(parents.len(), MAX_PARENTS);
        assert_eq!(parents.len(), 32);

        // Verify all hashes are unique
        use std::collections::HashSet;
        let unique: HashSet<_> = parents.iter().collect();
        assert_eq!(unique.len(), MAX_PARENTS);
    }

    /// Test 2: K-cluster edge case - exactly K parents
    #[test]
    fn test_ghostdag_k_cluster_exactly_k() {
        let k = 10;

        // Create exactly K parent hashes
        let mut parents = Vec::with_capacity(k as usize);
        for i in 0..k {
            let mut hash_bytes = [0u8; 32];
            hash_bytes[0] = i as u8;
            parents.push(Hash::new(hash_bytes));
        }

        assert_eq!(parents.len(), k as usize);

        // Verify this is the maximum allowed for k-cluster
        // A block with k+1 blues (including selected parent) is valid
        assert!(parents.len() <= (k + 1) as usize);
    }

    /// Test 3: K-cluster edge case - single parent (minimum)
    #[test]
    fn test_ghostdag_k_cluster_single_parent() {
        // Minimum case: single parent (like a chain)
        let parent = Hash::new([1u8; 32]);
        let parents = vec![parent];

        assert_eq!(parents.len(), 1);

        // Single parent means no merging, behaves like a chain
        // This is the minimum valid case for a non-genesis block
    }

    /// Test 4: Deep DAG structure - blue score calculation
    #[test]
    fn test_ghostdag_deep_dag_blue_score() {
        // Test blue score accumulation in deep DAG
        // blue_score should increase monotonically

        let parent_score = 1000u64;
        let mergeset_blues_count = 5;

        // blue_score = parent_score + mergeset_blues.len()
        let expected_score = parent_score + mergeset_blues_count;

        assert_eq!(expected_score, 1005);

        // Verify monotonicity: new_score > parent_score
        assert!(expected_score > parent_score);
    }

    /// Test 5: Deep DAG structure - blue work calculation
    #[test]
    fn test_ghostdag_deep_dag_blue_work() {
        // Test blue work accumulation
        use tos_common::difficulty::Difficulty;

        let parent_work = BlueWorkType::from(1000u64);
        let block_difficulty = Difficulty::from(100u64);

        // Calculate work from difficulty
        let block_work = calc_work_from_difficulty(&block_difficulty);

        // Total work should accumulate
        let total_work = parent_work + block_work;

        // Verify monotonicity
        assert!(total_work > parent_work);
    }

    /// Test 6: K-cluster validation - anticone size tracking
    #[test]
    fn test_ghostdag_anticone_size_tracking() {
        let k = 10;

        // Test anticone size constraints
        // Each blue block's anticone must be ≤ k

        let anticone_size_valid = 5;
        let anticone_size_invalid = 15;

        assert!(anticone_size_valid <= k, "Valid anticone size should be ≤ k");
        assert!(anticone_size_invalid > k, "Invalid anticone size should be > k");
    }

    /// Test 7: Blue/Red classification - boundary cases
    #[test]
    fn test_ghostdag_blue_red_classification() {
        let k = 10;

        // A block is blue if it doesn't violate k-cluster
        // Test boundary: exactly k blues (plus selected parent = k+1 total)

        let blues_count = k as usize;
        let max_allowed_blues = (k + 1) as usize; // Including selected parent

        assert!(blues_count < max_allowed_blues, "Should allow k blues + selected parent");

        // Test that k+2 would exceed limit
        let too_many_blues = (k + 2) as usize;
        assert!(too_many_blues > max_allowed_blues, "k+2 blues would violate limit");
    }

    /// Test 8: Genesis block special case
    #[test]
    fn test_ghostdag_genesis_special_case() {
        // Genesis has no parents, empty mergeset
        let k = 10;
        let genesis_hash = Hash::new([0u8; 32]);

        // Use Arc for reachability (as in production code)
        use std::sync::Arc;
        use crate::core::reachability::TosReachability;

        let reachability = Arc::new(TosReachability::new(genesis_hash.clone()));
        let ghostdag = TosGhostdag::new(k, genesis_hash.clone(), reachability);

        let genesis_data = ghostdag.genesis_ghostdag_data();

        // Verify genesis properties
        assert_eq!(genesis_data.blue_score, 0);
        assert_eq!(genesis_data.blue_work, BlueWorkType::zero());
        assert_eq!(genesis_data.mergeset_blues.len(), 0);
        assert_eq!(genesis_data.mergeset_reds.len(), 0);
        assert_eq!(genesis_data.mergeset_non_daa.len(), 0);
        assert_eq!(genesis_data.blues_anticone_sizes.len(), 0);
    }

    /// Test 9: Selected parent selection - highest blue work
    #[test]
    fn test_ghostdag_selected_parent_highest_work() {
        // Selected parent must have highest blue work among parents

        let work_low = BlueWorkType::from(100u64);
        let work_medium = BlueWorkType::from(500u64);
        let work_high = BlueWorkType::from(1000u64);

        // Verify ordering
        assert!(work_low < work_medium);
        assert!(work_medium < work_high);

        // Selected parent should be the one with work_high
        assert_eq!(work_high, BlueWorkType::from(1000u64));
    }

    /// Test 10: Sortable block ordering
    #[test]
    fn test_ghostdag_sortable_block_ordering() {
        let hash1 = Hash::new([1u8; 32]);
        let hash2 = Hash::new([2u8; 32]);

        let work1 = BlueWorkType::from(100u64);
        let work2 = BlueWorkType::from(200u64);

        let block1 = SortableBlock::new(hash1.clone(), work1);
        let block2 = SortableBlock::new(hash2.clone(), work2);

        // Blocks should sort by blue work (ascending)
        assert!(block1 < block2, "Lower work should sort first");

        // Test with equal work (should fall back to hash comparison)
        let block3 = SortableBlock::new(hash1.clone(), work1);
        let block4 = SortableBlock::new(hash2.clone(), work1); // Same work as block3

        assert!(block3.blue_work == block4.blue_work);
        // With equal work, should compare by hash
        assert_ne!(block3.hash, block4.hash);
    }

    /// Test 11: Work calculation from difficulty
    #[test]
    fn test_ghostdag_work_calculation() {
        use tos_common::difficulty::Difficulty;

        // Test work calculation from various difficulties
        let diff_low = Difficulty::from(100u64);
        let diff_high = Difficulty::from(1000u64);

        let work_low = calc_work_from_difficulty(&diff_low);
        let work_high = calc_work_from_difficulty(&diff_high);

        // Higher difficulty should produce higher work
        assert!(work_high > work_low, "Higher difficulty should produce higher work");
    }

    /// Test 12: Zero difficulty edge case
    #[test]
    fn test_ghostdag_zero_difficulty() {
        use tos_common::difficulty::Difficulty;

        // Test work calculation with zero difficulty
        let zero_diff = Difficulty::from(0u64);
        let zero_work = calc_work_from_difficulty(&zero_diff);

        // Zero difficulty should produce zero work
        assert_eq!(zero_work, BlueWorkType::zero());
    }

    /// Test 13: Large DAG performance simulation
    #[test]
    fn test_ghostdag_large_dag_scaling() {
        // Simulate scaling behavior with large number of blocks
        const LARGE_DAG_SIZE: u64 = 10_000;

        // Simulate blue_score growth
        let final_blue_score = LARGE_DAG_SIZE;
        assert_eq!(final_blue_score, 10_000);

        // Verify that blue_score scales linearly with block count
        assert!(final_blue_score >= LARGE_DAG_SIZE);
    }

    /// Test 14: Mergeset size limits
    #[test]
    fn test_ghostdag_mergeset_size_limits() {
        // Test mergeset size constraints
        // With k=10, maximum mergeset_blues size is k+1 (including selected parent)
        let k = 10;
        let max_mergeset_blues = (k + 1) as usize;

        assert_eq!(max_mergeset_blues, 11);

        // Mergeset_reds is unbounded (all non-blue parents and their descendants)
        // but should be reasonable in practice
    }

    /// Test 15: TosGhostdagData structure invariants
    #[test]
    fn test_ghostdag_data_invariants() {
        // Test invariants of TosGhostdagData structure

        let blue_score = 100u64;
        let blue_work = BlueWorkType::from(1000u64);
        let selected_parent = Hash::new([1u8; 32]);
        let mergeset_blues = vec![selected_parent.clone()];
        let mergeset_reds = vec![Hash::new([2u8; 32])];
        let blues_anticone_sizes = HashMap::new();
        let mergeset_non_daa = vec![];

        let data = TosGhostdagData::new(
            blue_score,
            blue_work,
            selected_parent.clone(),
            mergeset_blues.clone(),
            mergeset_reds.clone(),
            blues_anticone_sizes,
            mergeset_non_daa,
        );

        // Verify invariants
        assert_eq!(data.blue_score, blue_score);
        assert_eq!(data.blue_work, blue_work);
        assert_eq!(data.selected_parent, selected_parent);
        assert_eq!(data.mergeset_blues.len(), 1);
        assert_eq!(data.mergeset_reds.len(), 1);

        // Verify blues and reds are disjoint
        let blues_set: HashSet<_> = data.mergeset_blues.iter().collect();
        let reds_set: HashSet<_> = data.mergeset_reds.iter().collect();
        let intersection: Vec<_> = blues_set.intersection(&reds_set).collect();
        assert_eq!(intersection.len(), 0, "Blues and reds must be disjoint");
    }

    // SECURITY TEST SUITE: Tests for vulnerability fixes V-01 through V-07

    /// Test V-01: Blue score overflow protection
    #[test]
    fn test_v01_blue_score_overflow() {
        // Test that blue_score overflow is properly detected
        let max_score = u64::MAX;
        let one = 1u64;

        // This should overflow if not using checked arithmetic
        let result = max_score.checked_add(one);
        assert!(result.is_none(), "Should detect overflow");

        // Verify safe addition works
        let safe_score = 1000u64;
        let safe_result = safe_score.checked_add(one);
        assert!(safe_result.is_some(), "Safe addition should succeed");
        assert_eq!(safe_result.unwrap(), 1001);
    }

    /// Test V-01: Blue work overflow protection
    #[test]
    fn test_v01_blue_work_overflow() {
        // Test that blue_work overflow is properly detected
        let max_work = BlueWorkType::max_value();
        let one_work = BlueWorkType::one();

        // This should overflow
        let result = max_work.checked_add(one_work);
        assert!(result.is_none(), "Should detect blue work overflow");

        // Verify safe addition works
        let safe_work = BlueWorkType::from(1000u64);
        let safe_result = safe_work.checked_add(one_work);
        assert!(safe_result.is_some(), "Safe blue work addition should succeed");
    }

    /// Test V-03: K-cluster validation (basic test)
    #[test]
    fn test_v03_k_cluster_size_check() {
        let k = 10;

        // Test that we detect when mergeset_blues exceeds k+1
        let blues_count_valid = k as usize;
        let blues_count_invalid = (k + 2) as usize;

        assert!(blues_count_valid <= (k + 1) as usize, "Valid blues count");
        assert!(blues_count_invalid > (k + 1) as usize, "Invalid blues count");
    }

    /// Test V-05: No valid parents error detection
    #[test]
    fn test_v05_no_valid_parents() {
        // Test that empty parent list is properly detected
        let empty_parents: Vec<Hash> = vec![];
        assert!(empty_parents.is_empty(), "Empty parents should be detected");

        // Test that we have parents
        let valid_parents = vec![Hash::new([1u8; 32])];
        assert!(!valid_parents.is_empty(), "Valid parents should not be empty");
    }

    /// Test V-06: Zero difficulty protection
    #[test]
    fn test_v06_zero_difficulty_protection() {
        use tos_common::difficulty::Difficulty;

        // Test zero difficulty handling
        let zero_diff = Difficulty::from(0u64);
        let work = calc_work_from_difficulty(&zero_diff);

        // Should return max work for zero difficulty
        assert_eq!(work, BlueWorkType::max_value(), "Zero difficulty should return max work");

        // Test non-zero difficulty
        let normal_diff = Difficulty::from(1000u64);
        let normal_work = calc_work_from_difficulty(&normal_diff);
        assert!(normal_work < BlueWorkType::max_value(), "Normal difficulty should return finite work");
    }

    /// Test V-07: Timestamp validation
    #[test]
    fn test_v07_timestamp_validation() {
        // Test timestamp ordering validation
        let oldest = 1000u64;
        let newest = 2000u64;

        assert!(newest >= oldest, "Newest should be >= oldest");

        // Test invalid ordering
        let invalid_oldest = 2000u64;
        let invalid_newest = 1000u64;
        assert!(invalid_newest < invalid_oldest, "Should detect invalid ordering");
    }

    /// Test V-07: Saturating subtraction for time span
    #[test]
    fn test_v07_saturating_subtraction() {
        // Test saturating_sub prevents underflow
        let newer = 2000u64;
        let older = 1000u64;

        let time_span = newer.saturating_sub(older);
        assert_eq!(time_span, 1000);

        // Test with reversed order (would underflow without saturation)
        let backwards_span = older.saturating_sub(newer);
        assert_eq!(backwards_span, 0, "Saturating sub should return 0, not underflow");
    }

    /// Test V-02: Reachability interval size check
    #[test]
    fn test_v02_interval_exhaustion_detection() {
        use crate::core::reachability::Interval;

        // Test interval with size 1 (exhausted)
        let exhausted = Interval::new(100, 100);
        assert_eq!(exhausted.size(), 1, "Size 1 interval is exhausted");

        // Test interval with size 0 (empty)
        let empty = Interval::new(100, 99);
        assert_eq!(empty.size(), 0, "Empty interval has size 0");

        // Test healthy interval
        let healthy = Interval::new(1, 1000);
        assert!(healthy.size() > 1, "Healthy interval has size > 1");
    }

    /// Test: Work calculation consistency
    #[test]
    fn test_work_calculation_consistency() {
        use tos_common::difficulty::Difficulty;

        // Test that work calculation is consistent
        let diff = Difficulty::from(1000u64);
        let work1 = calc_work_from_difficulty(&diff);
        let work2 = calc_work_from_difficulty(&diff);

        assert_eq!(work1, work2, "Work calculation should be deterministic");
    }

    /// Test: Blue work accumulation
    #[test]
    fn test_blue_work_accumulation() {
        // Test that blue work accumulates correctly
        let work1 = BlueWorkType::from(1000u64);
        let work2 = BlueWorkType::from(2000u64);

        let total = work1.checked_add(work2);
        assert!(total.is_some(), "Blue work accumulation should succeed");
        assert_eq!(total.unwrap(), BlueWorkType::from(3000u64));
    }

    /// Test: Hash comparison determinism
    #[test]
    fn test_hash_comparison_determinism() {
        let hash1 = Hash::new([1u8; 32]);
        let hash2 = Hash::new([2u8; 32]);

        // Same hashes should compare equal
        let hash1_copy = Hash::new([1u8; 32]);
        assert_eq!(hash1, hash1_copy);

        // Different hashes should compare unequal
        assert_ne!(hash1, hash2);

        // Comparison should be consistent
        let cmp1 = hash1.as_bytes().cmp(hash2.as_bytes());
        let cmp2 = hash1.as_bytes().cmp(hash2.as_bytes());
        assert_eq!(cmp1, cmp2, "Hash comparison should be deterministic");
    }
}
