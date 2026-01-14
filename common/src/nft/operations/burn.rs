// NFT Burn Operations
// This module contains the burn operation logic.

use crate::crypto::Hash;
use crate::nft::{NftError, NftResult, MAX_BATCH_SIZE};

use super::validation::validate_token_id;
use super::{check_nft_permission, NftStorage, RuntimeContext};

// ========================================
// Burn Operation
// ========================================

/// Burn (destroy) an NFT
///
/// # Parameters
/// - `storage`: Storage backend
/// - `ctx`: Runtime context (caller, block height)
/// - `collection`: Collection ID
/// - `token_id`: Token ID
///
/// # Returns
/// - `Ok(())`: Success
/// - `Err(NftError)`: Error code
pub fn burn<S: NftStorage + ?Sized>(
    storage: &mut S,
    ctx: &RuntimeContext,
    collection: &Hash,
    token_id: u64,
) -> NftResult<()> {
    // Step 1: Input validation
    validate_token_id(token_id)?;

    // Step 2: Get NFT
    let nft = storage
        .get_nft(collection, token_id)
        .ok_or(NftError::TokenNotFound)?;

    // Step 3: Business rules check
    // 3.1 Frozen tokens cannot be burned
    if nft.is_frozen {
        return Err(NftError::CannotBurnFrozen);
    }

    // 3.2 Check rental status
    if storage.has_active_rental(collection, token_id) {
        return Err(NftError::RentalActive);
    }

    // 3.3 Check TBA has assets
    if storage.tba_has_assets(collection, token_id) {
        return Err(NftError::TbaHasAssets);
    }

    // Step 4: Permission check
    check_nft_permission(storage, &nft, &ctx.caller)?;

    // Step 5: Execute burn
    // 5.1 Delete NFT
    storage.delete_nft(collection, token_id)?;

    // 5.2 Update collection supply
    let mut collection_data = storage
        .get_collection(collection)
        .ok_or(NftError::CollectionNotFound)?;
    collection_data.total_supply = collection_data
        .total_supply
        .checked_sub(1)
        .ok_or(NftError::Overflow)?;
    storage.set_collection(&collection_data)?;

    // 5.3 Update balance
    storage.decrement_balance(collection, &nft.owner)?;

    // 5.4 Remove TBA if exists (and has no assets)
    storage.remove_tba(collection, token_id)?;

    Ok(())
}

// ========================================
// Batch Burn Operation
// ========================================

/// Batch burn multiple NFTs
///
/// # Parameters
/// - `storage`: Storage backend
/// - `ctx`: Runtime context
/// - `collection`: Collection ID
/// - `token_ids`: List of token IDs to burn
///
/// # Returns
/// - `Ok(())`: All burns succeeded
/// - `Err(NftError)`: First error encountered
pub fn batch_burn<S: NftStorage + ?Sized>(
    storage: &mut S,
    ctx: &RuntimeContext,
    collection: &Hash,
    token_ids: &[u64],
) -> NftResult<()> {
    // Validate batch size
    if token_ids.is_empty() {
        return Err(NftError::BatchEmpty);
    }
    if token_ids.len() > MAX_BATCH_SIZE {
        return Err(NftError::BatchSizeExceeded);
    }

    // Check for duplicates
    let mut seen = std::collections::HashSet::new();
    for token_id in token_ids {
        if !seen.insert(*token_id) {
            return Err(NftError::DuplicateToken);
        }
    }

    // Execute all burns
    for token_id in token_ids {
        burn(storage, ctx, collection, *token_id)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::collection::{create_collection, CreateCollectionParams};
    use super::super::mint::{mint, MintParams};
    use super::*;
    use crate::crypto::PublicKey;
    use crate::nft::{MintAuthority, Nft, NftCollection};
    use std::collections::HashMap;

    // Mock storage
    struct MockStorage {
        collections: HashMap<Hash, NftCollection>,
        nfts: HashMap<(Hash, u64), Nft>,
        balances: HashMap<(Hash, PublicKey), u64>,
        approvals: HashMap<(PublicKey, Hash, PublicKey), bool>,
        mint_counts: HashMap<(Hash, PublicKey), u64>,
        nonce: u64,
        active_rentals: std::collections::HashSet<(Hash, u64)>,
        tba_with_assets: std::collections::HashSet<(Hash, u64)>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self {
                collections: HashMap::new(),
                nfts: HashMap::new(),
                balances: HashMap::new(),
                approvals: HashMap::new(),
                mint_counts: HashMap::new(),
                nonce: 0,
                active_rentals: std::collections::HashSet::new(),
                tba_with_assets: std::collections::HashSet::new(),
            }
        }

        fn add_rental(&mut self, collection: Hash, token_id: u64) {
            self.active_rentals.insert((collection, token_id));
        }

        fn add_tba_with_assets(&mut self, collection: Hash, token_id: u64) {
            self.tba_with_assets.insert((collection, token_id));
        }
    }

    impl NftStorage for MockStorage {
        fn get_collection(&self, id: &Hash) -> Option<NftCollection> {
            self.collections.get(id).cloned()
        }

        fn set_collection(&mut self, collection: &NftCollection) -> NftResult<()> {
            self.collections
                .insert(collection.id.clone(), collection.clone());
            Ok(())
        }

        fn collection_exists(&self, id: &Hash) -> bool {
            self.collections.contains_key(id)
        }

        fn get_nft(&self, collection: &Hash, token_id: u64) -> Option<Nft> {
            self.nfts.get(&(collection.clone(), token_id)).cloned()
        }

        fn set_nft(&mut self, nft: &Nft) -> NftResult<()> {
            self.nfts
                .insert((nft.collection.clone(), nft.token_id), nft.clone());
            Ok(())
        }

        fn delete_nft(&mut self, collection: &Hash, token_id: u64) -> NftResult<()> {
            self.nfts.remove(&(collection.clone(), token_id));
            Ok(())
        }

        fn nft_exists(&self, collection: &Hash, token_id: u64) -> bool {
            self.nfts.contains_key(&(collection.clone(), token_id))
        }

        fn get_balance(&self, collection: &Hash, owner: &PublicKey) -> u64 {
            *self
                .balances
                .get(&(collection.clone(), owner.clone()))
                .unwrap_or(&0)
        }

        fn increment_balance(&mut self, collection: &Hash, owner: &PublicKey) -> NftResult<u64> {
            let balance = self
                .balances
                .entry((collection.clone(), owner.clone()))
                .or_insert(0);
            *balance = balance.checked_add(1).ok_or(NftError::Overflow)?;
            Ok(*balance)
        }

        fn decrement_balance(&mut self, collection: &Hash, owner: &PublicKey) -> NftResult<u64> {
            let balance = self
                .balances
                .entry((collection.clone(), owner.clone()))
                .or_insert(0);
            *balance = balance.checked_sub(1).ok_or(NftError::Overflow)?;
            Ok(*balance)
        }

        fn is_approved_for_all(
            &self,
            owner: &PublicKey,
            collection: &Hash,
            operator: &PublicKey,
        ) -> bool {
            *self
                .approvals
                .get(&(owner.clone(), collection.clone(), operator.clone()))
                .unwrap_or(&false)
        }

        fn set_approval_for_all(
            &mut self,
            owner: &PublicKey,
            collection: &Hash,
            operator: &PublicKey,
            approved: bool,
        ) -> NftResult<()> {
            self.approvals.insert(
                (owner.clone(), collection.clone(), operator.clone()),
                approved,
            );
            Ok(())
        }

        fn get_mint_count(&self, collection: &Hash, user: &PublicKey) -> u64 {
            *self
                .mint_counts
                .get(&(collection.clone(), user.clone()))
                .unwrap_or(&0)
        }

        fn increment_mint_count(&mut self, collection: &Hash, user: &PublicKey) -> NftResult<u64> {
            let count = self
                .mint_counts
                .entry((collection.clone(), user.clone()))
                .or_insert(0);
            *count = count.checked_add(1).ok_or(NftError::Overflow)?;
            Ok(*count)
        }

        fn get_and_increment_collection_nonce(&mut self) -> NftResult<u64> {
            let current = self.nonce;
            self.nonce = self.nonce.checked_add(1).ok_or(NftError::Overflow)?;
            Ok(current)
        }

        fn has_active_rental(&self, collection: &Hash, token_id: u64) -> bool {
            self.active_rentals
                .contains(&(collection.clone(), token_id))
        }

        fn tba_has_assets(&self, collection: &Hash, token_id: u64) -> bool {
            self.tba_with_assets
                .contains(&(collection.clone(), token_id))
        }
    }

    fn test_public_key() -> PublicKey {
        PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[10u8; 32])
                .expect("valid"),
        )
    }

    fn test_public_key_2() -> PublicKey {
        PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[1u8; 32])
                .expect("valid"),
        )
    }

    fn setup_test() -> (MockStorage, Hash, u64, PublicKey) {
        let mut storage = MockStorage::new();
        let creator = test_public_key();

        // Create collection
        let ctx = RuntimeContext::new(creator.clone(), 100);
        let params = CreateCollectionParams {
            name: "Test".to_string(),
            symbol: "TEST".to_string(),
            base_uri: "".to_string(),
            max_supply: None,
            royalty_recipient: creator.clone(),
            royalty_basis_points: 0,
            mint_authority: MintAuthority::CreatorOnly,
            freeze_authority: Some(creator.clone()),
            metadata_authority: None,
        };
        let collection_id = create_collection(&mut storage, &ctx, params).unwrap();

        // Mint NFT
        let token_id = mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), creator.clone()),
        )
        .unwrap();

        (storage, collection_id, token_id, creator)
    }

    #[test]
    fn test_burn_success() {
        let (mut storage, collection_id, token_id, owner) = setup_test();

        let ctx = RuntimeContext::new(owner.clone(), 100);
        let result = burn(&mut storage, &ctx, &collection_id, token_id);
        assert!(result.is_ok());

        // NFT should be deleted
        assert!(storage.get_nft(&collection_id, token_id).is_none());

        // Supply should decrease
        let collection = storage.get_collection(&collection_id).unwrap();
        assert_eq!(collection.total_supply, 0);

        // Balance should decrease
        assert_eq!(storage.get_balance(&collection_id, &owner), 0);
    }

    #[test]
    fn test_burn_by_approved() {
        let (mut storage, collection_id, token_id, _owner) = setup_test();
        let operator = test_public_key_2();

        // Approve operator
        let mut nft = storage.get_nft(&collection_id, token_id).unwrap();
        nft.approved = Some(operator.clone());
        storage.set_nft(&nft).unwrap();

        // Burn by operator
        let ctx = RuntimeContext::new(operator, 100);
        let result = burn(&mut storage, &ctx, &collection_id, token_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_burn_by_global_operator() {
        let (mut storage, collection_id, token_id, owner) = setup_test();
        let operator = test_public_key_2();

        // Set global approval
        storage
            .set_approval_for_all(&owner, &collection_id, &operator, true)
            .unwrap();

        // Burn by operator
        let ctx = RuntimeContext::new(operator, 100);
        let result = burn(&mut storage, &ctx, &collection_id, token_id);
        assert!(result.is_ok());
    }

    #[test]
    fn test_burn_not_authorized() {
        let (mut storage, collection_id, token_id, _owner) = setup_test();
        let other = test_public_key_2();

        let ctx = RuntimeContext::new(other, 100);
        let result = burn(&mut storage, &ctx, &collection_id, token_id);
        assert_eq!(result, Err(NftError::NotApproved));
    }

    #[test]
    fn test_burn_frozen_fails() {
        let (mut storage, collection_id, token_id, owner) = setup_test();

        // Freeze token
        let mut nft = storage.get_nft(&collection_id, token_id).unwrap();
        nft.is_frozen = true;
        storage.set_nft(&nft).unwrap();

        let ctx = RuntimeContext::new(owner, 100);
        let result = burn(&mut storage, &ctx, &collection_id, token_id);
        assert_eq!(result, Err(NftError::CannotBurnFrozen));
    }

    #[test]
    fn test_burn_rented_fails() {
        let (mut storage, collection_id, token_id, owner) = setup_test();

        // Add active rental
        storage.add_rental(collection_id.clone(), token_id);

        let ctx = RuntimeContext::new(owner, 100);
        let result = burn(&mut storage, &ctx, &collection_id, token_id);
        assert_eq!(result, Err(NftError::RentalActive));
    }

    #[test]
    fn test_burn_tba_with_assets_fails() {
        let (mut storage, collection_id, token_id, owner) = setup_test();

        // Add TBA with assets
        storage.add_tba_with_assets(collection_id.clone(), token_id);

        let ctx = RuntimeContext::new(owner, 100);
        let result = burn(&mut storage, &ctx, &collection_id, token_id);
        assert_eq!(result, Err(NftError::TbaHasAssets));
    }

    #[test]
    fn test_burn_token_not_found() {
        let (mut storage, collection_id, _, owner) = setup_test();

        let ctx = RuntimeContext::new(owner, 100);
        let result = burn(&mut storage, &ctx, &collection_id, 999);
        assert_eq!(result, Err(NftError::TokenNotFound));
    }

    #[test]
    fn test_burn_token_id_zero() {
        let (mut storage, collection_id, _, owner) = setup_test();

        let ctx = RuntimeContext::new(owner, 100);
        let result = burn(&mut storage, &ctx, &collection_id, 0);
        assert_eq!(result, Err(NftError::InvalidTokenId));
    }

    #[test]
    fn test_burn_token_id_not_reused() {
        let (mut storage, collection_id, token_id, owner) = setup_test();

        // Burn token
        let ctx = RuntimeContext::new(owner.clone(), 100);
        burn(&mut storage, &ctx, &collection_id, token_id).unwrap();

        // Mint new token
        let new_id = mint(&mut storage, &ctx, MintParams::new(collection_id, owner)).unwrap();

        // Should get new ID, not reused
        assert_eq!(new_id, 2);
    }

    #[test]
    fn test_batch_burn_success() {
        let (mut storage, collection_id, _, owner) = setup_test();

        // Mint more NFTs
        let ctx = RuntimeContext::new(owner.clone(), 100);
        let id2 = mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), owner.clone()),
        )
        .unwrap();
        let id3 = mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), owner.clone()),
        )
        .unwrap();

        // Batch burn
        let result = batch_burn(&mut storage, &ctx, &collection_id, &[1, id2, id3]);
        assert!(result.is_ok());

        // All burned
        assert!(storage.get_nft(&collection_id, 1).is_none());
        assert!(storage.get_nft(&collection_id, id2).is_none());
        assert!(storage.get_nft(&collection_id, id3).is_none());

        let collection = storage.get_collection(&collection_id).unwrap();
        assert_eq!(collection.total_supply, 0);
    }

    #[test]
    fn test_batch_burn_duplicate_fails() {
        let (mut storage, collection_id, token_id, owner) = setup_test();

        let ctx = RuntimeContext::new(owner, 100);
        let result = batch_burn(&mut storage, &ctx, &collection_id, &[token_id, token_id]);
        assert_eq!(result, Err(NftError::DuplicateToken));
    }

    #[test]
    fn test_batch_burn_empty_fails() {
        let (mut storage, collection_id, _, owner) = setup_test();

        let ctx = RuntimeContext::new(owner, 100);
        let result = batch_burn(&mut storage, &ctx, &collection_id, &[]);
        assert_eq!(result, Err(NftError::BatchEmpty));
    }
}
