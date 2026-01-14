use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::block::TopoHeight;

#[async_trait]
pub trait VersionedContractAssetExtProvider {
    async fn delete_versioned_contract_asset_ext_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    async fn delete_versioned_contract_asset_ext_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    async fn delete_versioned_contract_asset_ext_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
        keep_last: bool,
    ) -> Result<(), BlockchainError>;
}
