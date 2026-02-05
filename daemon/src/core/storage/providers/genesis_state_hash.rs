use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::crypto::Hash;

/// Provider trait for genesis state hash persistence
/// Per PLAN-B v1.5, the genesis state hash must be stored and verified on restart
#[async_trait]
pub trait GenesisStateHashProvider {
    /// Get the stored genesis state hash
    async fn get_genesis_state_hash(&self) -> Result<Option<Hash>, BlockchainError>;

    /// Store the genesis state hash
    async fn set_genesis_state_hash(&mut self, hash: &Hash) -> Result<(), BlockchainError>;

    /// Check if genesis state hash is stored
    async fn has_genesis_state_hash(&self) -> Result<bool, BlockchainError>;
}
