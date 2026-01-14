use crate::core::{
    error::BlockchainError,
    storage::{rocksdb::Column, RocksStorage, VersionedContractAssetExtProvider},
};
use async_trait::async_trait;
use log::trace;
use tos_common::block::TopoHeight;

#[async_trait]
impl VersionedContractAssetExtProvider for RocksStorage {
    async fn delete_versioned_contract_asset_ext_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned contract asset ext at topoheight {}",
                topoheight
            );
        }
        self.delete_versioned_at_topoheight(
            Column::ContractsAssetExt,
            Column::VersionedContractsAssetExt,
            topoheight,
        )
    }

    async fn delete_versioned_contract_asset_ext_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned contract asset ext above topoheight {}",
                topoheight
            );
        }
        self.delete_versioned_above_topoheight(
            Column::ContractsAssetExt,
            Column::VersionedContractsAssetExt,
            topoheight,
        )
    }

    async fn delete_versioned_contract_asset_ext_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
        keep_last: bool,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned contract asset ext below topoheight {}",
                topoheight
            );
        }
        self.delete_versioned_below_topoheight(
            Column::ContractsAssetExt,
            Column::VersionedContractsAssetExt,
            topoheight,
            keep_last,
        )
    }
}
