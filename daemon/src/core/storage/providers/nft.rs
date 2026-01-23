use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    block::TopoHeight,
    crypto::{Hash, PublicKey},
    nft::{Nft, NftCollection, NftRental, RentalListing, TokenBoundAccount},
};

#[async_trait]
pub trait NftProvider {
    // ===== Bootstrap Sync =====

    /// List all NFT collection IDs with skip/limit pagination
    async fn list_all_nft_collections(
        &self,
        topoheight: TopoHeight,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(Hash, NftCollection)>, BlockchainError>;

    /// List all tokens in a collection with skip/limit pagination
    async fn list_nft_tokens_for_collection(
        &self,
        collection: &Hash,
        topoheight: TopoHeight,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(u64, Nft)>, BlockchainError>;

    /// List all token owners in a collection with skip/limit pagination
    async fn list_nft_owners_for_collection(
        &self,
        collection: &Hash,
        topoheight: TopoHeight,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(u64, PublicKey)>, BlockchainError>;

    async fn get_nft_collection(
        &self,
        id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, NftCollection)>, BlockchainError>;

    async fn set_last_nft_collection_to(
        &mut self,
        id: &Hash,
        topoheight: TopoHeight,
        value: &NftCollection,
    ) -> Result<(), BlockchainError>;

    async fn delete_nft_collection(
        &mut self,
        id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    async fn get_nft_token(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, Nft)>, BlockchainError>;

    async fn set_last_nft_token_to(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
        value: &Nft,
    ) -> Result<(), BlockchainError>;

    async fn delete_nft_token(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    async fn get_nft_owner_balance(
        &self,
        collection: &Hash,
        owner: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, BlockchainError>;

    async fn set_last_nft_owner_balance_to(
        &mut self,
        collection: &Hash,
        owner: &PublicKey,
        topoheight: TopoHeight,
        value: u64,
    ) -> Result<(), BlockchainError>;

    async fn get_nft_operator_approval(
        &self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, bool)>, BlockchainError>;

    async fn set_last_nft_operator_approval_to(
        &mut self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
        topoheight: TopoHeight,
        value: bool,
    ) -> Result<(), BlockchainError>;

    async fn get_nft_mint_count(
        &self,
        collection: &Hash,
        user: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, BlockchainError>;

    async fn set_last_nft_mint_count_to(
        &mut self,
        collection: &Hash,
        user: &PublicKey,
        topoheight: TopoHeight,
        value: u64,
    ) -> Result<(), BlockchainError>;

    async fn get_nft_collection_nonce(
        &self,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, u64)>, BlockchainError>;

    async fn set_last_nft_collection_nonce_to(
        &mut self,
        topoheight: TopoHeight,
        value: u64,
    ) -> Result<(), BlockchainError>;

    async fn get_nft_tba(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, TokenBoundAccount)>, BlockchainError>;

    async fn set_last_nft_tba_to(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
        value: &TokenBoundAccount,
    ) -> Result<(), BlockchainError>;

    async fn delete_nft_tba(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    async fn get_nft_rental_listing(
        &self,
        listing_id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, RentalListing)>, BlockchainError>;

    async fn set_last_nft_rental_listing_to(
        &mut self,
        listing_id: &Hash,
        topoheight: TopoHeight,
        value: &RentalListing,
    ) -> Result<(), BlockchainError>;

    async fn delete_nft_rental_listing(
        &mut self,
        listing_id: &Hash,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    async fn get_nft_active_rental(
        &self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, NftRental)>, BlockchainError>;

    async fn set_last_nft_active_rental_to(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
        value: &NftRental,
    ) -> Result<(), BlockchainError>;

    async fn delete_nft_active_rental(
        &mut self,
        collection: &Hash,
        token_id: u64,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;
}
