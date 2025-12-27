use crate::core::{
    error::{BlockchainError, DiskContext},
    storage::{SledStorage, VersionedAssetsSupplyProvider},
};
use async_trait::async_trait;
use log::trace;
use tos_common::block::TopoHeight;

#[async_trait]
impl VersionedAssetsSupplyProvider for SledStorage {
    async fn delete_versioned_assets_supply_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned assets supply at topoheight {}",
                topoheight
            );
        }
        Self::delete_versioned_tree_at_topoheight(
            &mut self.snapshot,
            &self.assets_supply,
            &self.versioned_assets_supply,
            topoheight,
        )
    }

    async fn delete_versioned_assets_supply_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned assets supply above topoheight {}",
                topoheight
            );
        }
        Self::delete_versioned_tree_above_topoheight(
            &mut self.snapshot,
            &self.assets_supply,
            &self.versioned_assets_supply,
            topoheight,
            DiskContext::AssetSupply,
        )
    }

    async fn delete_versioned_assets_supply_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
        keep_last: bool,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned assets supply below topoheight {}",
                topoheight
            );
        }
        Self::delete_versioned_tree_below_topoheight(
            &mut self.snapshot,
            &self.assets_supply,
            &self.versioned_assets_supply,
            topoheight,
            keep_last,
            DiskContext::AssetSupply,
        )
    }
}
