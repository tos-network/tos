// TOS Reachability Storage Provider
// Provides storage operations for reachability data

use async_trait::async_trait;
use tos_common::crypto::Hash;
use crate::core::error::BlockchainError;
use crate::core::reachability::ReachabilityData;

/// Reachability data provider trait
///
/// Implemented by storage backends to provide reachability query support
#[async_trait]
pub trait ReachabilityDataProvider {
    /// Get reachability data for a block
    async fn get_reachability_data(&self, hash: &Hash) -> Result<ReachabilityData, BlockchainError>;

    /// Check if reachability data exists for a block
    async fn has_reachability_data(&self, hash: &Hash) -> Result<bool, BlockchainError>;

    /// Store reachability data for a block
    async fn set_reachability_data(&mut self, hash: &Hash, data: &ReachabilityData) -> Result<(), BlockchainError>;

    /// Delete reachability data for a block
    async fn delete_reachability_data(&mut self, hash: &Hash) -> Result<(), BlockchainError>;

    /// Get the current reindex root
    ///
    /// The reindex root is a stable point in the chain that stays approximately
    /// 100 blocks behind the current tip. It provides a reference point for
    /// reindexing operations.
    ///
    /// # Returns
    /// Hash of the current reindex root, or error if not set
    async fn get_reindex_root(&self) -> Result<Hash, BlockchainError>;

    /// Set the reindex root
    ///
    /// Updates the stored reindex root to a new value.
    ///
    /// # Arguments
    /// * `root` - New reindex root hash
    async fn set_reindex_root(&mut self, root: Hash) -> Result<(), BlockchainError>;
}
