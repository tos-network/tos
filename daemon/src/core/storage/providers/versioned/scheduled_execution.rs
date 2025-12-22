// Versioned Scheduled Execution Provider
//
// This trait defines methods for cleaning up scheduled executions during chain rewinds.
// Unlike other versioned data, scheduled executions are not versioned in the traditional sense -
// they are registered at one topoheight and scheduled to execute at another.
//
// During a rewind, we need to:
// 1. Delete executions registered at the rewound topoheight (since the registering TX is undone)
// 2. The registration index allows efficient cleanup

use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::block::TopoHeight;

#[async_trait]
pub trait VersionedScheduledExecutionProvider {
    /// Delete scheduled executions registered at the specified topoheight.
    /// This is called during single-block rewind.
    async fn delete_scheduled_executions_registered_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Delete scheduled executions registered above the specified topoheight.
    /// This is called during multi-block rewind or chain reorganization.
    async fn delete_scheduled_executions_registered_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Delete scheduled executions registered below the specified topoheight.
    /// This is called during pruning to clean up old registration records.
    async fn delete_scheduled_executions_registered_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;
}
