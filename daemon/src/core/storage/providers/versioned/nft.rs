use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::block::TopoHeight;

#[async_trait]
pub trait VersionedNftProvider {
    async fn delete_versioned_nft_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    async fn delete_versioned_nft_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    async fn delete_versioned_nft_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
        keep_last: bool,
    ) -> Result<(), BlockchainError>;
}
