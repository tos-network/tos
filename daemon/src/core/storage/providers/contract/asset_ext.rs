use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    block::TopoHeight,
    contract_asset::{ContractAssetData, TokenKey, TokenValue},
    crypto::Hash,
};

#[async_trait]
pub trait ContractAssetExtProvider {
    /// List all contract assets with skip/limit pagination (bootstrap sync)
    async fn list_all_contract_assets(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(Hash, ContractAssetData)>, BlockchainError>;

    /// Import contract asset directly (bootstrap sync)
    async fn import_contract_asset(
        &mut self,
        asset: &Hash,
        data: &ContractAssetData,
    ) -> Result<(), BlockchainError>;

    async fn get_contract_asset_ext(
        &self,
        contract: &Hash,
        key: &TokenKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, TokenValue)>, BlockchainError>;

    async fn set_last_contract_asset_ext_to(
        &mut self,
        contract: &Hash,
        key: &TokenKey,
        topoheight: TopoHeight,
        value: &TokenValue,
    ) -> Result<(), BlockchainError>;

    async fn delete_contract_asset_ext(
        &mut self,
        contract: &Hash,
        key: &TokenKey,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;
}
