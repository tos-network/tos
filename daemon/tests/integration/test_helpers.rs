// Test helpers for DAA integration tests
//
// Provides utilities for:
// - Creating temporary storage instances
// - Building test blocks
// - Managing GHOSTDAG and DAA test scenarios

#![allow(clippy::result_large_err)]
#![allow(clippy::assertions_on_constants)]

use std::path::PathBuf;
use std::sync::Arc;
use tempdir::TempDir;
use tos_common::{
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    crypto::{elgamal::CompressedPublicKey, Hash, Hashable},
    difficulty::Difficulty,
    immutable::Immutable,
    network::Network,
    serializer::{Serializer, Writer},
    time::TimestampMillis,
    varuint::VarUint,
};
use tos_daemon::core::{
    error::BlockchainError,
    ghostdag::{TosGhostdag, TosGhostdagData},
    reachability::TosReachability,
    storage::{
        sled::{SledStorage, StorageMode},
        BlockProvider, DifficultyProvider, GhostdagDataProvider, TipsProvider,
    },
};

/// Helper function to create a test public key from bytes
fn create_test_pubkey(bytes: [u8; 32]) -> CompressedPublicKey {
    // Use serialization to create a CompressedPublicKey from bytes
    let mut buffer = Vec::new();
    let mut writer = Writer::new(&mut buffer);
    writer.write_bytes(&bytes);
    let data = writer.as_bytes();

    // Create a Reader and deserialize
    use tos_common::serializer::Reader;
    let mut reader = Reader::new(data);
    CompressedPublicKey::read(&mut reader).expect("Failed to create test pubkey")
}

/// Test storage wrapper with automatic cleanup
pub struct TestStorage {
    _temp_dir: TempDir, // Keep alive for automatic cleanup
    pub storage: SledStorage,
}

impl TestStorage {
    /// Create a new test storage instance with temporary directory
    pub fn new() -> Result<Self, BlockchainError> {
        let temp_dir =
            TempDir::new("tos_test_storage").map_err(|_e| BlockchainError::InvalidConfig)?;

        let storage = SledStorage::new(
            temp_dir.path().to_string_lossy().to_string(),
            Some(1024 * 1024), // cache_size: 1MB
            Network::Devnet,
            1024 * 1024, // internal_cache_size: 1MB
            StorageMode::HighThroughput,
        )?;

        Ok(Self {
            _temp_dir: temp_dir,
            storage,
        })
    }

    /// Get the path to the temporary directory
    #[allow(dead_code)]
    pub fn path(&self) -> PathBuf {
        self._temp_dir.path().to_path_buf()
    }
}

/// Builder for creating test blocks with custom parameters
pub struct BlockBuilder {
    timestamp: TimestampMillis,
    parents: Vec<Hash>,
    version: BlockVersion,
    extra_nonce: [u8; EXTRA_NONCE_SIZE],
    miner: CompressedPublicKey,
    hash_merkle_root: Hash,
}

impl BlockBuilder {
    /// Create a new block builder with default values
    pub fn new() -> Self {
        // Create a default miner key (all zeros for testing)
        let miner = create_test_pubkey([0u8; 32]);

        Self {
            timestamp: 0,
            parents: vec![],
            version: BlockVersion::Baseline,
            extra_nonce: [0u8; EXTRA_NONCE_SIZE],
            miner,
            hash_merkle_root: Hash::zero(),
        }
    }

    /// Set block timestamp (Unix timestamp in milliseconds)
    pub fn with_timestamp(mut self, timestamp: TimestampMillis) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Set parent blocks
    pub fn with_parents(mut self, parents: Vec<Hash>) -> Self {
        self.parents = parents;
        self
    }

    /// Set block version
    #[allow(dead_code)]
    pub fn with_version(mut self, version: BlockVersion) -> Self {
        self.version = version;
        self
    }

    /// Set merkle root of transactions
    #[allow(dead_code)]
    pub fn with_merkle_root(mut self, merkle_root: Hash) -> Self {
        self.hash_merkle_root = merkle_root;
        self
    }

    /// Set extra nonce
    #[allow(dead_code)]
    pub fn with_extra_nonce(mut self, extra_nonce: [u8; EXTRA_NONCE_SIZE]) -> Self {
        self.extra_nonce = extra_nonce;
        self
    }

    /// Build the block header using new_simple()
    pub fn build(self) -> BlockHeader {
        BlockHeader::new_simple(
            self.version,
            self.parents,
            self.timestamp,
            self.extra_nonce,
            self.miner,
            self.hash_merkle_root,
        )
    }

    /// Build a complete block with header and empty transactions
    #[allow(dead_code)]
    pub fn build_block(self) -> Block {
        let header = self.build();
        Block::new(Immutable::Owned(header), vec![])
    }
}

impl Default for BlockBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Test harness for DAA integration testing
///
/// Provides high-level API for:
/// - Creating chains of blocks
/// - Running GHOSTDAG algorithm
/// - Calculating DAA scores and difficulty
/// - Querying block data
pub struct DAATestHarness {
    storage: SledStorage,
    ghostdag: TosGhostdag,
    #[allow(dead_code)]
    genesis_hash: Hash,
    current_tip: Hash,
    block_count: u64,
}

impl DAATestHarness {
    /// Create a new test harness with initialized genesis block
    pub async fn new(mut storage: SledStorage) -> Result<Self, BlockchainError> {
        // Create genesis block header first using new_simple()
        let genesis_header = BlockBuilder::new()
            .with_timestamp(1600000000000) // Fixed genesis timestamp (milliseconds)
            .with_parents(vec![])
            .build();

        // Compute genesis hash from header
        let genesis_hash = genesis_header.hash();

        // Create reachability service with actual genesis hash
        let reachability = Arc::new(TosReachability::new(genesis_hash.clone()));

        // Create GHOSTDAG with K=18 for devnet and actual genesis hash
        let ghostdag = TosGhostdag::new(18, genesis_hash.clone(), reachability);

        // Initialize genesis GHOSTDAG data
        let genesis_ghostdag_data = ghostdag.genesis_ghostdag_data();

        // Store genesis block with low difficulty for testing
        // Use difficulty=1 to avoid blue work overflow in long test chains
        let genesis_header_arc = Arc::new(genesis_header);
        storage
            .save_block(
                genesis_header_arc.clone(),
                &[],
                Difficulty::from(1u64), // Low difficulty for testing
                VarUint::from(0u64),
                genesis_hash.clone().into(),
            )
            .await?;

        // Store genesis GHOSTDAG data
        storage
            .insert_ghostdag_data(&genesis_hash, Arc::new(genesis_ghostdag_data))
            .await?;

        // Initialize tips
        storage
            .store_tips(&[genesis_hash.clone()].into_iter().collect())
            .await?;

        Ok(Self {
            storage,
            ghostdag,
            genesis_hash: genesis_hash.clone(),
            current_tip: genesis_hash,
            block_count: 1,
        })
    }

    /// Add a single block to the chain
    ///
    /// # Arguments
    /// * `timestamp` - Block timestamp (Unix milliseconds)
    /// * `parents` - Parent block hashes
    ///
    /// # Returns
    /// Hash of the newly created block
    pub async fn add_block(
        &mut self,
        timestamp: TimestampMillis,
        parents: Vec<Hash>,
    ) -> Result<Hash, BlockchainError> {
        // Create block header first
        let header = BlockBuilder::new()
            .with_timestamp(timestamp)
            .with_parents(parents.clone())
            .build();

        let block_hash = header.hash();

        // Run GHOSTDAG algorithm to get DAA score and other data
        let ghostdag_data = self.ghostdag.ghostdag(&self.storage, &parents).await?;

        // Calculate difficulty based on DAA
        use tos_daemon::core::ghostdag::calculate_target_difficulty;
        let difficulty = calculate_target_difficulty(
            &self.storage,
            &ghostdag_data.selected_parent,
            ghostdag_data.daa_score,
        )
        .await?;

        // Store block with calculated difficulty
        let header_arc = Arc::new(header);
        self.storage
            .save_block(
                header_arc.clone(),
                &[],
                difficulty,
                VarUint::from(self.block_count),
                block_hash.clone().into(),
            )
            .await?;

        // Store GHOSTDAG data
        self.storage
            .insert_ghostdag_data(&block_hash, Arc::new(ghostdag_data))
            .await?;

        // Update tips
        let mut tips = self.storage.get_tips().await?;
        tips.insert(block_hash.clone());
        for parent in &parents {
            tips.remove(parent);
        }
        self.storage.store_tips(&tips).await?;

        self.current_tip = block_hash.clone();
        self.block_count += 1;

        Ok(block_hash)
    }

    /// Add a chain of blocks with consistent time intervals
    ///
    /// # Arguments
    /// * `count` - Number of blocks to add
    /// * `timestamp_delta` - Time between blocks in seconds
    ///
    /// # Returns
    /// Vector of block hashes in order
    pub async fn add_chain_blocks(
        &mut self,
        count: usize,
        timestamp_delta_seconds: u64,
    ) -> Result<Vec<Hash>, BlockchainError> {
        let mut hashes = Vec::with_capacity(count);
        let mut current_parent = self.current_tip.clone();

        // Get current timestamp from parent
        let parent_header = self
            .storage
            .get_block_header_by_hash(&current_parent)
            .await?;
        let mut current_timestamp = parent_header.get_timestamp();

        // Convert seconds to milliseconds
        let timestamp_delta_ms = timestamp_delta_seconds * 1000;

        for _ in 0..count {
            current_timestamp += timestamp_delta_ms;
            let hash = self
                .add_block(current_timestamp, vec![current_parent.clone()])
                .await?;
            hashes.push(hash.clone());
            current_parent = hash;
        }

        self.current_tip = current_parent;
        Ok(hashes)
    }

    /// Get difficulty for a block
    pub async fn get_difficulty(&self, hash: &Hash) -> Result<Difficulty, BlockchainError> {
        self.storage.get_difficulty_for_block_hash(hash).await
    }

    /// Get DAA score for a block
    pub async fn get_daa_score(&self, hash: &Hash) -> Result<u64, BlockchainError> {
        let ghostdag_data = self.storage.get_ghostdag_data(hash).await?;
        Ok(ghostdag_data.daa_score)
    }

    /// Get blue score for a block
    #[allow(dead_code)]
    pub async fn get_blue_score(&self, hash: &Hash) -> Result<u64, BlockchainError> {
        let ghostdag_data = self.storage.get_ghostdag_data(hash).await?;
        Ok(ghostdag_data.blue_score)
    }

    /// Get GHOSTDAG data for a block
    #[allow(dead_code)]
    pub async fn get_ghostdag_data(
        &self,
        hash: &Hash,
    ) -> Result<Arc<TosGhostdagData>, BlockchainError> {
        self.storage.get_ghostdag_data(hash).await
    }

    /// Get the current tip hash
    pub fn current_tip(&self) -> &Hash {
        &self.current_tip
    }

    /// Get the genesis hash
    #[allow(dead_code)]
    pub fn genesis_hash(&self) -> &Hash {
        &self.genesis_hash
    }

    /// Get the total number of blocks
    #[allow(dead_code)]
    pub fn block_count(&self) -> u64 {
        self.block_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_builder() {
        let block = BlockBuilder::new().with_timestamp(1234567890000).build();

        assert_eq!(block.get_timestamp(), 1234567890000);
    }

    #[tokio::test]
    async fn test_storage_creation() {
        let storage = TestStorage::new();
        assert!(storage.is_ok());
    }
}
