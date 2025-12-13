mod balance;
mod data;
mod supply;

use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::block::TopoHeight;

pub use balance::*;
pub use data::*;
pub use supply::*;

#[async_trait]
pub trait VersionedContractProvider {
    // delete versioned contracts at topoheight
    async fn delete_versioned_contracts_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    // delete versioned contracts above topoheight
    async fn delete_versioned_contracts_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    // delete versioned contracts below topoheight
    async fn delete_versioned_contracts_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
        keep_last: bool,
    ) -> Result<(), BlockchainError>;
}
