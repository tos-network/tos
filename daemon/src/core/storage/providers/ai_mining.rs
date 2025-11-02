use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{ai_mining::AIMiningState, block::TopoHeight};

/// Provider for AI mining state storage operations
#[async_trait]
pub trait AIMiningProvider {
    /// Get the current global AI mining state
    async fn get_ai_mining_state(&self) -> Result<Option<AIMiningState>, BlockchainError>;

    /// Set the global AI mining state at a specific topoheight
    async fn set_ai_mining_state(
        &mut self,
        topoheight: TopoHeight,
        state: &AIMiningState,
    ) -> Result<(), BlockchainError>;

    /// Check if AI mining state exists for topoheight
    async fn has_ai_mining_state_at_topoheight(
        &self,
        topoheight: TopoHeight,
    ) -> Result<bool, BlockchainError>;

    /// Get AI mining state at a specific topoheight
    async fn get_ai_mining_state_at_topoheight(
        &self,
        topoheight: TopoHeight,
    ) -> Result<Option<AIMiningState>, BlockchainError>;
}
