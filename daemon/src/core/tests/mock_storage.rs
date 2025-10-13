// Mock Storage for Integration Testing
// Provides an in-memory implementation of Storage trait for testing

use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;

use tos_common::crypto::Hash;
use tos_common::difficulty::Difficulty;
use tos_common::immutable::Immutable;

use crate::core::{
    error::BlockchainError,
    storage::{Storage, BlockHeader},
    ghostdag::TosGhostdagData,
    reachability::ReachabilityData,
};

/// Mock storage for testing
/// Stores all data in-memory HashMaps
pub struct MockStorage {
    pub blocks: HashMap<Hash, Immutable<BlockHeader>>,
    pub ghostdag_data: HashMap<Hash, Arc<TosGhostdagData>>,
    pub reachability_data: HashMap<Hash, ReachabilityData>,
    pub difficulties: HashMap<Hash, Difficulty>,
    pub blue_works: HashMap<Hash, crate::core::ghostdag::BlueWorkType>,
}

impl MockStorage {
    pub fn new() -> Self {
        Self {
            blocks: HashMap::new(),
            ghostdag_data: HashMap::new(),
            reachability_data: HashMap::new(),
            difficulties: HashMap::new(),
            blue_works: HashMap::new(),
        }
    }

    pub fn insert_block(&mut self, hash: Hash, header: BlockHeader) {
        self.blocks.insert(hash, Immutable::new(header));
    }

    pub fn insert_ghostdag(&mut self, hash: Hash, data: TosGhostdagData) {
        self.ghostdag_data.insert(hash, Arc::new(data));
    }

    pub fn insert_reachability(&mut self, hash: Hash, data: ReachabilityData) {
        self.reachability_data.insert(hash, data);
    }

    pub fn insert_difficulty(&mut self, hash: Hash, difficulty: Difficulty) {
        self.difficulties.insert(hash, difficulty);
    }

    pub fn insert_blue_work(&mut self, hash: Hash, work: crate::core::ghostdag::BlueWorkType) {
        self.blue_works.insert(hash, work);
    }
}

#[async_trait]
impl Storage for MockStorage {
    async fn has_block_with_hash(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        Ok(self.blocks.contains_key(hash))
    }

    async fn get_block_header_by_hash(&self, hash: &Hash) -> Result<Immutable<BlockHeader>, BlockchainError> {
        self.blocks
            .get(hash)
            .cloned()
            .ok_or_else(|| BlockchainError::ParentNotFound(hash.clone()))
    }

    async fn get_ghostdag_data(&self, hash: &Hash) -> Result<Arc<TosGhostdagData>, BlockchainError> {
        self.ghostdag_data
            .get(hash)
            .cloned()
            .ok_or(BlockchainError::InvalidBlock)
    }

    async fn get_ghostdag_blue_work(&self, hash: &Hash) -> Result<crate::core::ghostdag::BlueWorkType, BlockchainError> {
        self.blue_works
            .get(hash)
            .copied()
            .ok_or(BlockchainError::InvalidBlock)
    }

    async fn get_difficulty_for_block_hash(&self, hash: &Hash) -> Result<Difficulty, BlockchainError> {
        self.difficulties
            .get(hash)
            .cloned()
            .ok_or(BlockchainError::InvalidBlock)
    }

    async fn has_reachability_data(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        Ok(self.reachability_data.contains_key(hash))
    }

    async fn get_reachability_data(&self, hash: &Hash) -> Result<ReachabilityData, BlockchainError> {
        self.reachability_data
            .get(hash)
            .cloned()
            .ok_or(BlockchainError::InvalidReachability)
    }

    async fn set_reachability_data(&mut self, hash: &Hash, data: &ReachabilityData) -> Result<(), BlockchainError> {
        self.reachability_data.insert(hash.clone(), data.clone());
        Ok(())
    }

    async fn delete_reachability_data(&mut self, hash: &Hash) -> Result<(), BlockchainError> {
        self.reachability_data.remove(hash);
        Ok(())
    }

    async fn get_reindex_root(&self) -> Result<Hash, BlockchainError> {
        // For testing, return genesis
        Ok(Hash::new([0u8; 32]))
    }

    async fn set_reindex_root(&mut self, _root: Hash) -> Result<(), BlockchainError> {
        // For testing, no-op
        Ok(())
    }

    // Placeholder implementations for other required methods
    async fn insert_ghostdag_data(&mut self, hash: &Hash, data: Arc<TosGhostdagData>) -> Result<(), BlockchainError> {
        self.ghostdag_data.insert(hash.clone(), data);
        Ok(())
    }

    async fn has_ghostdag_data(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        Ok(self.ghostdag_data.contains_key(hash))
    }
}
