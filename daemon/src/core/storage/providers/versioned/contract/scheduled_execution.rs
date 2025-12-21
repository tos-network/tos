// Versioned Scheduled Executions Provider
// Used for deleting scheduled executions during chain rollback

use async_trait::async_trait;
use tos_common::block::TopoHeight;

use crate::core::error::BlockchainError;

#[async_trait]
pub trait VersionedScheduledExecutionsProvider {
    /// Delete all scheduled executions registered at the provided topoheight
    async fn delete_scheduled_executions_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Delete all scheduled executions registered above the provided topoheight
    async fn delete_scheduled_executions_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Delete all scheduled executions registered below the provided topoheight
    async fn delete_scheduled_executions_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;
}
