use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::block::TopoHeight;

/// Versioned energy resource provider for blockchain reorg support
/// Energy state must be cleaned up during reorgs
#[async_trait]
pub trait VersionedEnergyProvider {
    /// Delete all versioned energy resources at the given topoheight
    async fn delete_versioned_energy_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Delete all versioned energy resources above the given topoheight
    async fn delete_versioned_energy_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Delete all versioned energy resources below the given topoheight
    /// keep_last: if true, keep the last version for each account
    async fn delete_versioned_energy_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
        keep_last: bool,
    ) -> Result<(), BlockchainError>;
}
