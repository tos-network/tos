use crate::core::{
    error::BlockchainError,
    storage::{providers::NetworkProvider, rocksdb::Column, NftProvider, RocksStorage},
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    block::TopoHeight,
    crypto::{Hash, PublicKey},
    nft::{
        active_rental_key, collection_key, collection_nonce_key, mint_count_key,
        operator_approval_key, owner_balance_key, rental_listing_key, tba_key, token_key, Nft,
        NftCollection, NftRental, NftStorageProvider, RentalListing, TokenBoundAccount,
    },
    tokio::try_block_on,
    versioned_type::Versioned,
};

fn versioned_key(pointer_key: &[u8], topoheight: TopoHeight) -> Vec<u8> {
    let mut key = Vec::with_capacity(8 + pointer_key.len());
    key.extend_from_slice(&topoheight.to_be_bytes());
    key.extend_from_slice(pointer_key);
    key
}

async fn get_versioned_at_maximum<T: tos_common::serializer::Serializer>(
    storage: &RocksStorage,
    pointer_column: Column,
    versioned_column: Column,
    pointer_key: &[u8],
    maximum_topoheight: TopoHeight,
) -> Result<Option<(TopoHeight, Versioned<T>)>, BlockchainError> {
    let mut prev_topo = storage.load_optional_from_disk(pointer_column, pointer_key)?;
    while let Some(topo) = prev_topo {
        let versioned_key = versioned_key(pointer_key, topo);
        let version: Versioned<T> = storage.load_from_disk(versioned_column, &versioned_key)?;
        if topo <= maximum_topoheight {
            return Ok(Some((topo, version)));
        }
        prev_topo = version.get_previous_topoheight();
    }
    Ok(None)
}

#[async_trait]
impl NftProvider for RocksStorage {
    async fn get_nft_collection(
        &self,
        id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, NftCollection)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get nft collection {} at topoheight {}", id, topoheight);
        }
        let key = collection_key(id);
        let Some((topo, version)) = get_versioned_at_maximum::<Option<NftCollection>>(
            self,
            Column::NftCollections,
            Column::VersionedNftCollections,
            &key,
            topoheight,
        )
        .await?
        else {
            return Ok(None);
        };
        Ok(version.take().map(|collection| (topo, collection)))
    }

    async fn set_last_nft_collection_to(
        &mut self,
        id: &Hash,
        topoheight: TopoHeight,
        value: &NftCollection,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set nft collection {} to topoheight {}", id, topoheight);
        }
        let key = collection_key(id);
        let previous = self.load_optional_from_disk(Column::NftCollections, &key)?;
        let versioned = Versioned::new(Some(value.clone()), previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftCollections, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(Column::VersionedNftCollections, &versioned_key, &versioned)
    }

    async fn delete_nft_collection(
        &mut self,
        id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete nft collection {} at topoheight {}", id, topoheight);
        }
        let key = collection_key(id);
        let previous = self.load_optional_from_disk(Column::NftCollections, &key)?;
        let versioned: Versioned<Option<NftCollection>> = Versioned::new(None, previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftCollections, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(Column::VersionedNftCollections, &versioned_key, &versioned)
    }

    async fn get_nft_token(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, Nft)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get nft token {}:{} at topoheight {}",
                collection,
                token_id,
                topoheight
            );
        }
        let key = token_key(collection, token_id);
        let Some((topo, version)) = get_versioned_at_maximum::<Option<Nft>>(
            self,
            Column::NftTokens,
            Column::VersionedNftTokens,
            &key,
            topoheight,
        )
        .await?
        else {
            return Ok(None);
        };
        Ok(version.take().map(|nft| (topo, nft)))
    }

    async fn set_last_nft_token_to(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
        value: &Nft,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set nft token {}:{} to topoheight {}",
                collection,
                token_id,
                topoheight
            );
        }
        let key = token_key(collection, token_id);
        let previous = self.load_optional_from_disk(Column::NftTokens, &key)?;
        let versioned = Versioned::new(Some(value.clone()), previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftTokens, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(Column::VersionedNftTokens, &versioned_key, &versioned)
    }

    async fn delete_nft_token(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete nft token {}:{} at topoheight {}",
                collection,
                token_id,
                topoheight
            );
        }
        let key = token_key(collection, token_id);
        let previous = self.load_optional_from_disk(Column::NftTokens, &key)?;
        let versioned: Versioned<Option<Nft>> = Versioned::new(None, previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftTokens, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(Column::VersionedNftTokens, &versioned_key, &versioned)
    }

    async fn get_nft_owner_balance(
        &self,
        collection: &Hash,
        owner: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get nft owner balance {}:{} at topoheight {}",
                collection,
                owner.as_address(self.is_mainnet()),
                topoheight
            );
        }
        let key = owner_balance_key(collection, owner);
        let Some((topo, version)) = get_versioned_at_maximum::<u64>(
            self,
            Column::NftOwnerBalances,
            Column::VersionedNftOwnerBalances,
            &key,
            topoheight,
        )
        .await?
        else {
            return Ok(None);
        };
        Ok(Some((topo, version.take())))
    }

    async fn set_last_nft_owner_balance_to(
        &mut self,
        collection: &Hash,
        owner: &PublicKey,
        topoheight: TopoHeight,
        value: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set nft owner balance {}:{} to topoheight {}",
                collection,
                owner.as_address(self.is_mainnet()),
                topoheight
            );
        }
        let key = owner_balance_key(collection, owner);
        let previous = self.load_optional_from_disk(Column::NftOwnerBalances, &key)?;
        let versioned = Versioned::new(value, previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftOwnerBalances, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(
            Column::VersionedNftOwnerBalances,
            &versioned_key,
            &versioned,
        )
    }

    async fn get_nft_operator_approval(
        &self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, bool)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get nft operator approval {}:{}:{} at topoheight {}",
                owner.as_address(self.is_mainnet()),
                collection,
                operator.as_address(self.is_mainnet()),
                topoheight
            );
        }
        let key = operator_approval_key(owner, collection, operator);
        let Some((topo, version)) = get_versioned_at_maximum::<bool>(
            self,
            Column::NftOperatorApprovals,
            Column::VersionedNftOperatorApprovals,
            &key,
            topoheight,
        )
        .await?
        else {
            return Ok(None);
        };
        Ok(Some((topo, version.take())))
    }

    async fn set_last_nft_operator_approval_to(
        &mut self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
        topoheight: TopoHeight,
        value: bool,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set nft operator approval {}:{}:{} to topoheight {}",
                owner.as_address(self.is_mainnet()),
                collection,
                operator.as_address(self.is_mainnet()),
                topoheight
            );
        }
        let key = operator_approval_key(owner, collection, operator);
        let previous = self.load_optional_from_disk(Column::NftOperatorApprovals, &key)?;
        let versioned = Versioned::new(value, previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(
            Column::NftOperatorApprovals,
            &key,
            &topoheight.to_be_bytes(),
        )?;
        self.insert_into_disk(
            Column::VersionedNftOperatorApprovals,
            &versioned_key,
            &versioned,
        )
    }

    async fn get_nft_mint_count(
        &self,
        collection: &Hash,
        user: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get nft mint count {}:{} at topoheight {}",
                collection,
                user.as_address(self.is_mainnet()),
                topoheight
            );
        }
        let key = mint_count_key(collection, user);
        let Some((topo, version)) = get_versioned_at_maximum::<u64>(
            self,
            Column::NftMintCounts,
            Column::VersionedNftMintCounts,
            &key,
            topoheight,
        )
        .await?
        else {
            return Ok(None);
        };
        Ok(Some((topo, version.take())))
    }

    async fn set_last_nft_mint_count_to(
        &mut self,
        collection: &Hash,
        user: &PublicKey,
        topoheight: TopoHeight,
        value: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set nft mint count {}:{} to topoheight {}",
                collection,
                user.as_address(self.is_mainnet()),
                topoheight
            );
        }
        let key = mint_count_key(collection, user);
        let previous = self.load_optional_from_disk(Column::NftMintCounts, &key)?;
        let versioned = Versioned::new(value, previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftMintCounts, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(Column::VersionedNftMintCounts, &versioned_key, &versioned)
    }

    async fn get_nft_collection_nonce(
        &self,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get nft collection nonce at topoheight {}", topoheight);
        }
        let key = collection_nonce_key();
        let Some((topo, version)) = get_versioned_at_maximum::<u64>(
            self,
            Column::NftCollectionNonce,
            Column::VersionedNftCollectionNonce,
            &key,
            topoheight,
        )
        .await?
        else {
            return Ok(None);
        };
        Ok(Some((topo, version.take())))
    }

    async fn set_last_nft_collection_nonce_to(
        &mut self,
        topoheight: TopoHeight,
        value: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set nft collection nonce to topoheight {}", topoheight);
        }
        let key = collection_nonce_key();
        let previous = self.load_optional_from_disk(Column::NftCollectionNonce, &key)?;
        let versioned = Versioned::new(value, previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftCollectionNonce, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(
            Column::VersionedNftCollectionNonce,
            &versioned_key,
            &versioned,
        )
    }

    async fn get_nft_tba(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, TokenBoundAccount)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get nft tba {}:{} at topoheight {}",
                collection,
                token_id,
                topoheight
            );
        }
        let key = tba_key(collection, token_id);
        let Some((topo, version)) = get_versioned_at_maximum::<Option<TokenBoundAccount>>(
            self,
            Column::NftTba,
            Column::VersionedNftTba,
            &key,
            topoheight,
        )
        .await?
        else {
            return Ok(None);
        };
        Ok(version.take().map(|tba| (topo, tba)))
    }

    async fn set_last_nft_tba_to(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
        value: &TokenBoundAccount,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set nft tba {}:{} to topoheight {}",
                collection,
                token_id,
                topoheight
            );
        }
        let key = tba_key(collection, token_id);
        let previous = self.load_optional_from_disk(Column::NftTba, &key)?;
        let versioned = Versioned::new(Some(value.clone()), previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftTba, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(Column::VersionedNftTba, &versioned_key, &versioned)
    }

    async fn delete_nft_tba(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete nft tba {}:{} at topoheight {}",
                collection,
                token_id,
                topoheight
            );
        }
        let key = tba_key(collection, token_id);
        let previous = self.load_optional_from_disk(Column::NftTba, &key)?;
        let versioned: Versioned<Option<TokenBoundAccount>> = Versioned::new(None, previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftTba, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(Column::VersionedNftTba, &versioned_key, &versioned)
    }

    async fn get_nft_rental_listing(
        &self,
        listing_id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, RentalListing)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get nft rental listing {} at topoheight {}",
                listing_id,
                topoheight
            );
        }
        let key = rental_listing_key(listing_id);
        let Some((topo, version)) = get_versioned_at_maximum::<Option<RentalListing>>(
            self,
            Column::NftRentalListings,
            Column::VersionedNftRentalListings,
            &key,
            topoheight,
        )
        .await?
        else {
            return Ok(None);
        };
        Ok(version.take().map(|listing| (topo, listing)))
    }

    async fn set_last_nft_rental_listing_to(
        &mut self,
        listing_id: &Hash,
        topoheight: TopoHeight,
        value: &RentalListing,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set nft rental listing {} to topoheight {}",
                listing_id,
                topoheight
            );
        }
        let key = rental_listing_key(listing_id);
        let previous = self.load_optional_from_disk(Column::NftRentalListings, &key)?;
        let versioned = Versioned::new(Some(value.clone()), previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftRentalListings, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(
            Column::VersionedNftRentalListings,
            &versioned_key,
            &versioned,
        )
    }

    async fn delete_nft_rental_listing(
        &mut self,
        listing_id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete nft rental listing {} at topoheight {}",
                listing_id,
                topoheight
            );
        }
        let key = rental_listing_key(listing_id);
        let previous = self.load_optional_from_disk(Column::NftRentalListings, &key)?;
        let versioned: Versioned<Option<RentalListing>> = Versioned::new(None, previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftRentalListings, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(
            Column::VersionedNftRentalListings,
            &versioned_key,
            &versioned,
        )
    }

    async fn get_nft_active_rental(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, NftRental)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get nft active rental {}:{} at topoheight {}",
                collection,
                token_id,
                topoheight
            );
        }
        let key = active_rental_key(collection, token_id);
        let Some((topo, version)) = get_versioned_at_maximum::<Option<NftRental>>(
            self,
            Column::NftActiveRentals,
            Column::VersionedNftActiveRentals,
            &key,
            topoheight,
        )
        .await?
        else {
            return Ok(None);
        };
        Ok(version.take().map(|rental| (topo, rental)))
    }

    async fn set_last_nft_active_rental_to(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
        value: &NftRental,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set nft active rental {}:{} to topoheight {}",
                collection,
                token_id,
                topoheight
            );
        }
        let key = active_rental_key(collection, token_id);
        let previous = self.load_optional_from_disk(Column::NftActiveRentals, &key)?;
        let versioned = Versioned::new(Some(value.clone()), previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftActiveRentals, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(
            Column::VersionedNftActiveRentals,
            &versioned_key,
            &versioned,
        )
    }

    async fn delete_nft_active_rental(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete nft active rental {}:{} at topoheight {}",
                collection,
                token_id,
                topoheight
            );
        }
        let key = active_rental_key(collection, token_id);
        let previous = self.load_optional_from_disk(Column::NftActiveRentals, &key)?;
        let versioned: Versioned<Option<NftRental>> = Versioned::new(None, previous);
        let versioned_key = versioned_key(&key, topoheight);

        self.insert_into_disk(Column::NftActiveRentals, &key, &topoheight.to_be_bytes())?;
        self.insert_into_disk(
            Column::VersionedNftActiveRentals,
            &versioned_key,
            &versioned,
        )
    }
}

impl NftStorageProvider for RocksStorage {
    fn get_collection(
        &self,
        id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, NftCollection)>, anyhow::Error> {
        Ok(try_block_on(self.get_nft_collection(id, topoheight))??)
    }

    fn get_token(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, Nft)>, anyhow::Error> {
        Ok(try_block_on(
            self.get_nft_token(collection, token_id, topoheight),
        )??)
    }

    fn get_owner_balance(
        &self,
        collection: &Hash,
        owner: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
        Ok(try_block_on(
            self.get_nft_owner_balance(collection, owner, topoheight),
        )??)
    }

    fn get_operator_approval(
        &self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, bool)>, anyhow::Error> {
        Ok(try_block_on(self.get_nft_operator_approval(
            owner, collection, operator, topoheight,
        ))??)
    }

    fn get_mint_count(
        &self,
        collection: &Hash,
        user: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
        Ok(try_block_on(
            self.get_nft_mint_count(collection, user, topoheight),
        )??)
    }

    fn get_collection_nonce(
        &self,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, anyhow::Error> {
        Ok(try_block_on(self.get_nft_collection_nonce(topoheight))??)
    }

    fn get_tba(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, TokenBoundAccount)>, anyhow::Error> {
        Ok(try_block_on(
            self.get_nft_tba(collection, token_id, topoheight),
        )??)
    }

    fn get_rental_listing(
        &self,
        listing_id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, RentalListing)>, anyhow::Error> {
        Ok(try_block_on(
            self.get_nft_rental_listing(listing_id, topoheight),
        )??)
    }

    fn get_active_rental(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, NftRental)>, anyhow::Error> {
        Ok(try_block_on(
            self.get_nft_active_rental(collection, token_id, topoheight),
        )??)
    }
}
