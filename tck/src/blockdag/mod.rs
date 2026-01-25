//! BlockDAG consensus testing module
//!
//! Contains shared mock infrastructure for testing BlockDAG tip selection,
//! ordering, stable height, difficulty adjustment, multi-tip scenarios,
//! and DAG invariants.

/// DAG structural invariant verification tests
pub mod dag_invariants;
/// DAG block ordering tests (sort by cumulative difficulty)
pub mod dag_ordering;
/// Difficulty arithmetic and cumulative difficulty tests
pub mod difficulty;
/// Multi-tip DAG scenarios and reachability tests
pub mod multi_tip;
/// Stable height and finality boundary tests
pub mod stable_height;
/// Tip selection and fork choice rule tests
pub mod tip_selection;

use async_trait::async_trait;
use indexmap::IndexSet;
use std::collections::HashMap;
use tos_common::{
    block::{BlockHeader, BlockVersion},
    crypto::Hash,
    difficulty::{CumulativeDifficulty, Difficulty},
    immutable::Immutable,
    time::TimestampMillis,
    varuint::VarUint,
};
use tos_daemon::core::{error::BlockchainError, storage::DifficultyProvider};

/// Helper to create a Hash filled with a single repeated byte value.
/// All 32 bytes are set to the given value.
pub fn make_hash(n: u8) -> Hash {
    Hash::new([n; 32])
}

/// Helper to create a Hash from an explicit 32-byte array.
pub fn make_hash_from_bytes(bytes: &[u8; 32]) -> Hash {
    Hash::new(*bytes)
}

/// Mock implementation of `DifficultyProvider` for BlockDAG testing.
///
/// Stores block metadata in HashMaps keyed by block hash, enabling
/// lightweight in-memory testing without a real storage backend.
pub struct MockDagProvider {
    heights: HashMap<Hash, u64>,
    difficulties: HashMap<Hash, Difficulty>,
    cumulative_difficulties: HashMap<Hash, CumulativeDifficulty>,
    timestamps: HashMap<Hash, TimestampMillis>,
    past_blocks: HashMap<Hash, IndexSet<Hash>>,
    versions: HashMap<Hash, BlockVersion>,
    estimated_covariances: HashMap<Hash, VarUint>,
}

impl MockDagProvider {
    /// Create a new empty MockDagProvider with no blocks.
    pub fn new() -> Self {
        Self {
            heights: HashMap::new(),
            difficulties: HashMap::new(),
            cumulative_difficulties: HashMap::new(),
            timestamps: HashMap::new(),
            past_blocks: HashMap::new(),
            versions: HashMap::new(),
            estimated_covariances: HashMap::new(),
        }
    }

    /// Add a block with its associated metadata.
    ///
    /// Inserts the block hash into all internal maps. The version defaults
    /// to `BlockVersion::Nobunaga` and estimated covariance defaults to 0.
    pub fn add_block(
        &mut self,
        hash: Hash,
        height: u64,
        difficulty: Difficulty,
        cumulative_difficulty: CumulativeDifficulty,
        timestamp: TimestampMillis,
        past_blocks: IndexSet<Hash>,
    ) {
        self.heights.insert(hash.clone(), height);
        self.difficulties.insert(hash.clone(), difficulty);
        self.cumulative_difficulties
            .insert(hash.clone(), cumulative_difficulty);
        self.timestamps.insert(hash.clone(), timestamp);
        self.past_blocks.insert(hash.clone(), past_blocks);
        self.versions.insert(hash.clone(), BlockVersion::Nobunaga);
        self.estimated_covariances
            .insert(hash, VarUint::from_u64(0));
    }
}

impl Default for MockDagProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DifficultyProvider for MockDagProvider {
    async fn get_height_for_block_hash(&self, hash: &Hash) -> Result<u64, BlockchainError> {
        self.heights
            .get(hash)
            .copied()
            .ok_or_else(|| BlockchainError::BlockNotFound(hash.clone()))
    }

    async fn get_version_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<BlockVersion, BlockchainError> {
        self.versions
            .get(hash)
            .copied()
            .ok_or_else(|| BlockchainError::BlockNotFound(hash.clone()))
    }

    async fn get_timestamp_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<TimestampMillis, BlockchainError> {
        self.timestamps
            .get(hash)
            .copied()
            .ok_or_else(|| BlockchainError::BlockNotFound(hash.clone()))
    }

    async fn get_difficulty_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<Difficulty, BlockchainError> {
        self.difficulties
            .get(hash)
            .cloned()
            .ok_or_else(|| BlockchainError::BlockNotFound(hash.clone()))
    }

    async fn get_cumulative_difficulty_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<CumulativeDifficulty, BlockchainError> {
        self.cumulative_difficulties
            .get(hash)
            .cloned()
            .ok_or_else(|| BlockchainError::BlockNotFound(hash.clone()))
    }

    async fn get_past_blocks_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<Immutable<IndexSet<Hash>>, BlockchainError> {
        self.past_blocks
            .get(hash)
            .cloned()
            .map(Immutable::Owned)
            .ok_or_else(|| BlockchainError::BlockNotFound(hash.clone()))
    }

    async fn get_block_header_by_hash(
        &self,
        hash: &Hash,
    ) -> Result<Immutable<BlockHeader>, BlockchainError> {
        // Full BlockHeader construction is complex; return not-found for now
        Err(BlockchainError::BlockNotFound(hash.clone()))
    }

    async fn get_estimated_covariance_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<VarUint, BlockchainError> {
        Ok(self
            .estimated_covariances
            .get(hash)
            .cloned()
            .unwrap_or_else(|| VarUint::from_u64(0)))
    }
}

/// Builder for constructing complex DAG test scenarios step by step.
///
/// Provides a fluent interface for adding blocks with or without parent tips,
/// then building a `MockDagProvider` from the accumulated state.
pub struct DagBuilder {
    heights: HashMap<Hash, u64>,
    difficulties: HashMap<Hash, Difficulty>,
    cumulative_difficulties: HashMap<Hash, CumulativeDifficulty>,
    timestamps: HashMap<Hash, TimestampMillis>,
    past_blocks: HashMap<Hash, IndexSet<Hash>>,
    versions: HashMap<Hash, BlockVersion>,
    estimated_covariances: HashMap<Hash, VarUint>,
}

impl DagBuilder {
    /// Create a new empty DagBuilder.
    pub fn new() -> Self {
        Self {
            heights: HashMap::new(),
            difficulties: HashMap::new(),
            cumulative_difficulties: HashMap::new(),
            timestamps: HashMap::new(),
            past_blocks: HashMap::new(),
            versions: HashMap::new(),
            estimated_covariances: HashMap::new(),
        }
    }

    /// Add a block with no parent tips.
    ///
    /// Difficulty and cumulative difficulty are provided as u64 values
    /// and converted to `VarUint` internally.
    pub fn add_block(
        mut self,
        hash: Hash,
        height: u64,
        difficulty_u64: u64,
        cumulative_difficulty_u64: u64,
        timestamp: TimestampMillis,
    ) -> Self {
        self.heights.insert(hash.clone(), height);
        self.difficulties
            .insert(hash.clone(), VarUint::from_u64(difficulty_u64));
        self.cumulative_difficulties
            .insert(hash.clone(), VarUint::from_u64(cumulative_difficulty_u64));
        self.timestamps.insert(hash.clone(), timestamp);
        self.past_blocks.insert(hash.clone(), IndexSet::new());
        self.versions.insert(hash.clone(), BlockVersion::Nobunaga);
        self.estimated_covariances
            .insert(hash, VarUint::from_u64(0));
        self
    }

    /// Add a block with specific parent tips.
    ///
    /// The tips vector represents the parent blocks in the DAG structure.
    pub fn add_block_with_tips(
        mut self,
        hash: Hash,
        height: u64,
        difficulty_u64: u64,
        cumulative_difficulty_u64: u64,
        timestamp: TimestampMillis,
        tips: Vec<Hash>,
    ) -> Self {
        self.heights.insert(hash.clone(), height);
        self.difficulties
            .insert(hash.clone(), VarUint::from_u64(difficulty_u64));
        self.cumulative_difficulties
            .insert(hash.clone(), VarUint::from_u64(cumulative_difficulty_u64));
        self.timestamps.insert(hash.clone(), timestamp);
        let tip_set: IndexSet<Hash> = tips.into_iter().collect();
        self.past_blocks.insert(hash.clone(), tip_set);
        self.versions.insert(hash.clone(), BlockVersion::Nobunaga);
        self.estimated_covariances
            .insert(hash, VarUint::from_u64(0));
        self
    }

    /// Build the `MockDagProvider` from the accumulated builder state.
    pub fn build(self) -> MockDagProvider {
        MockDagProvider {
            heights: self.heights,
            difficulties: self.difficulties,
            cumulative_difficulties: self.cumulative_difficulties,
            timestamps: self.timestamps,
            past_blocks: self.past_blocks,
            versions: self.versions,
            estimated_covariances: self.estimated_covariances,
        }
    }
}

impl Default for DagBuilder {
    fn default() -> Self {
        Self::new()
    }
}
