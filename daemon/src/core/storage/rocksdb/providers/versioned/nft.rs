use crate::core::{
    error::BlockchainError,
    storage::{rocksdb::Column, RocksStorage, VersionedNftProvider},
};
use async_trait::async_trait;
use log::trace;
use tos_common::block::TopoHeight;

const NFT_COLUMNS: &[(Column, Column)] = &[
    (Column::NftCollections, Column::VersionedNftCollections),
    (Column::NftTokens, Column::VersionedNftTokens),
    (Column::NftOwnerBalances, Column::VersionedNftOwnerBalances),
    (
        Column::NftOperatorApprovals,
        Column::VersionedNftOperatorApprovals,
    ),
    (Column::NftMintCounts, Column::VersionedNftMintCounts),
    (
        Column::NftCollectionNonce,
        Column::VersionedNftCollectionNonce,
    ),
    (Column::NftTba, Column::VersionedNftTba),
    (
        Column::NftRentalListings,
        Column::VersionedNftRentalListings,
    ),
    (Column::NftActiveRentals, Column::VersionedNftActiveRentals),
];

#[async_trait]
impl VersionedNftProvider for RocksStorage {
    async fn delete_versioned_nft_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned nft at topoheight {}", topoheight);
        }
        for (pointer, versioned) in NFT_COLUMNS {
            self.delete_versioned_at_topoheight(*pointer, *versioned, topoheight)?;
        }
        Ok(())
    }

    async fn delete_versioned_nft_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned nft above topoheight {}", topoheight);
        }
        for (pointer, versioned) in NFT_COLUMNS {
            self.delete_versioned_above_topoheight(*pointer, *versioned, topoheight)?;
        }
        Ok(())
    }

    async fn delete_versioned_nft_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
        keep_last: bool,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned nft below topoheight {}", topoheight);
        }
        for (pointer, versioned) in NFT_COLUMNS {
            self.delete_versioned_below_topoheight(*pointer, *versioned, topoheight, keep_last)?;
        }
        Ok(())
    }
}
