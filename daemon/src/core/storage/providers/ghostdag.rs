// TOS GHOSTDAG Storage Provider
// Follows TOS storage architecture patterns

use async_trait::async_trait;
use std::sync::Arc;
use tos_common::crypto::Hash;
use crate::core::{
    error::BlockchainError,
    ghostdag::{BlueWorkType, CompactGhostdagData, KType, TosGhostdagData},
};

/// GHOSTDAG Data Provider
///
/// Provides storage and retrieval of GHOSTDAG consensus data.
/// This trait follows TOS's provider pattern, similar to DagOrderProvider and BlockDagProvider.
///
/// GHOSTDAG data is append-only (write-once), never modified after insertion.
/// This allows for efficient concurrent access and caching.
#[async_trait]
pub trait GhostdagDataProvider {
    /// Get the blue score for a block
    ///
    /// Blue score is the number of blue blocks in the past of this block
    /// (similar to "height" in a chain, but for DAG)
    async fn get_ghostdag_blue_score(&self, hash: &Hash) -> Result<u64, BlockchainError>;

    /// Get the blue work for a block
    ///
    /// Blue work is the cumulative difficulty of all blue blocks in the past.
    /// Used for selecting the "heaviest" chain (blue chain).
    async fn get_ghostdag_blue_work(&self, hash: &Hash) -> Result<BlueWorkType, BlockchainError>;

    /// Get the selected parent for a block
    ///
    /// Selected parent is the parent with the highest blue work.
    /// This forms the "main chain" in GHOSTDAG terminology.
    async fn get_ghostdag_selected_parent(&self, hash: &Hash) -> Result<Hash, BlockchainError>;

    /// Get the mergeset blues for a block
    ///
    /// Mergeset blues are the blue blocks being merged by this block
    /// (excluding the selected parent, which is also blue).
    async fn get_ghostdag_mergeset_blues(&self, hash: &Hash) -> Result<Arc<Vec<Hash>>, BlockchainError>;

    /// Get the mergeset reds for a block
    ///
    /// Mergeset reds are the red blocks being merged by this block.
    /// Red blocks violate the k-cluster constraint.
    async fn get_ghostdag_mergeset_reds(&self, hash: &Hash) -> Result<Arc<Vec<Hash>>, BlockchainError>;

    /// Get the blues anticone sizes map for a block
    ///
    /// Maps each blue block to its anticone size (must be ≤ K).
    /// This is used to verify the k-cluster constraint.
    async fn get_ghostdag_blues_anticone_sizes(
        &self,
        hash: &Hash,
    ) -> Result<Arc<std::collections::HashMap<Hash, KType>>, BlockchainError>;

    /// Get complete GHOSTDAG data for a block
    ///
    /// Returns all GHOSTDAG information for the block.
    /// This is the primary method for retrieving full consensus data.
    async fn get_ghostdag_data(&self, hash: &Hash) -> Result<Arc<TosGhostdagData>, BlockchainError>;

    /// Get compact GHOSTDAG data for a block
    ///
    /// Returns only essential fields (blue_score, blue_work, selected_parent).
    /// More efficient for queries that don't need full mergeset data.
    async fn get_ghostdag_compact_data(&self, hash: &Hash) -> Result<CompactGhostdagData, BlockchainError>;

    /// Check if GHOSTDAG data exists for a block
    ///
    /// Returns true if the block has GHOSTDAG data stored.
    async fn has_ghostdag_data(&self, hash: &Hash) -> Result<bool, BlockchainError>;

    /// Insert GHOSTDAG data for a block
    ///
    /// GHOSTDAG data is append-only - once written, it's never modified.
    /// This allows for concurrent writes in some implementations.
    ///
    /// # Arguments
    /// * `hash` - The block hash
    /// * `data` - The GHOSTDAG data to store
    async fn insert_ghostdag_data(&mut self, hash: &Hash, data: Arc<TosGhostdagData>) -> Result<(), BlockchainError>;

    /// Delete GHOSTDAG data for a block
    ///
    /// Used during pruning to remove old block data.
    /// Should be called as part of the pruning process.
    async fn delete_ghostdag_data(&mut self, hash: &Hash) -> Result<(), BlockchainError>;
}
