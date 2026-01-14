// NFT Query Operations
// This module contains read-only query functions.

use crate::crypto::{Hash, PublicKey};
use crate::nft::{Nft, NftCollection, NftError, NftResult};

use super::NftStorage;

// ========================================
// Owner Query
// ========================================

/// Get the owner of an NFT
///
/// # Parameters
/// - `storage`: Storage backend
/// - `collection`: Collection ID
/// - `token_id`: Token ID
///
/// # Returns
/// - `Ok(PublicKey)`: Owner's public key
/// - `Err(NftError)`: Error code
pub fn owner_of<S: NftStorage + ?Sized>(
    storage: &S,
    collection: &Hash,
    token_id: u64,
) -> NftResult<PublicKey> {
    if token_id == 0 {
        return Err(NftError::TokenNotFound);
    }

    let nft = storage
        .get_nft(collection, token_id)
        .ok_or(NftError::TokenNotFound)?;

    Ok(nft.owner)
}

// ========================================
// Existence Query
// ========================================

/// Check if an NFT exists
///
/// # Parameters
/// - `storage`: Storage backend
/// - `collection`: Collection ID
/// - `token_id`: Token ID
///
/// # Returns
/// - `Ok(bool)`: Whether the NFT exists
pub fn exists<S: NftStorage + ?Sized>(
    storage: &S,
    collection: &Hash,
    token_id: u64,
) -> NftResult<bool> {
    if token_id == 0 {
        return Ok(false);
    }

    Ok(storage.nft_exists(collection, token_id))
}

// ========================================
// Balance Query
// ========================================

/// Get the number of NFTs owned by an address in a collection
///
/// # Parameters
/// - `storage`: Storage backend
/// - `collection`: Collection ID
/// - `owner`: Owner's public key
///
/// # Returns
/// - `Ok(u64)`: Number of NFTs owned
/// - `Err(NftError)`: Error code
pub fn balance_of<S: NftStorage + ?Sized>(
    storage: &S,
    collection: &Hash,
    owner: &PublicKey,
) -> NftResult<u64> {
    if !storage.collection_exists(collection) {
        return Err(NftError::CollectionNotFound);
    }

    Ok(storage.get_balance(collection, owner))
}

// ========================================
// Collection Supply Query
// ========================================

/// Get the current supply of a collection
///
/// # Parameters
/// - `storage`: Storage backend
/// - `collection`: Collection ID
///
/// # Returns
/// - `Ok(u64)`: Current total supply
/// - `Err(NftError)`: Error code
pub fn get_collection_supply<S: NftStorage + ?Sized>(
    storage: &S,
    collection: &Hash,
) -> NftResult<u64> {
    let col = storage
        .get_collection(collection)
        .ok_or(NftError::CollectionNotFound)?;

    Ok(col.total_supply)
}

/// Get the maximum supply of a collection
///
/// # Parameters
/// - `storage`: Storage backend
/// - `collection`: Collection ID
///
/// # Returns
/// - `Ok(Option<u64>)`: Maximum supply (None = unlimited)
/// - `Err(NftError)`: Error code
pub fn get_collection_max_supply<S: NftStorage + ?Sized>(
    storage: &S,
    collection: &Hash,
) -> NftResult<Option<u64>> {
    let col = storage
        .get_collection(collection)
        .ok_or(NftError::CollectionNotFound)?;

    Ok(col.max_supply)
}

// ========================================
// Collection Info Query
// ========================================

/// Get full collection information
///
/// # Parameters
/// - `storage`: Storage backend
/// - `collection`: Collection ID
///
/// # Returns
/// - `Ok(NftCollection)`: Full collection data
/// - `Err(NftError)`: Error code
pub fn get_collection<S: NftStorage + ?Sized>(
    storage: &S,
    collection: &Hash,
) -> NftResult<NftCollection> {
    storage
        .get_collection(collection)
        .ok_or(NftError::CollectionNotFound)
}

// ========================================
// NFT Info Query
// ========================================

/// Get full NFT information
///
/// # Parameters
/// - `storage`: Storage backend
/// - `collection`: Collection ID
/// - `token_id`: Token ID
///
/// # Returns
/// - `Ok(Nft)`: Full NFT data
/// - `Err(NftError)`: Error code
pub fn get_nft<S: NftStorage + ?Sized>(
    storage: &S,
    collection: &Hash,
    token_id: u64,
) -> NftResult<Nft> {
    if token_id == 0 {
        return Err(NftError::TokenNotFound);
    }

    storage
        .get_nft(collection, token_id)
        .ok_or(NftError::TokenNotFound)
}

// ========================================
// Approval Queries
// ========================================

/// Get the approved address for a single NFT
///
/// # Parameters
/// - `storage`: Storage backend
/// - `collection`: Collection ID
/// - `token_id`: Token ID
///
/// # Returns
/// - `Ok(Option<PublicKey>)`: Approved address if any
/// - `Err(NftError)`: Error code
pub fn get_approved<S: NftStorage + ?Sized>(
    storage: &S,
    collection: &Hash,
    token_id: u64,
) -> NftResult<Option<PublicKey>> {
    if token_id == 0 {
        return Err(NftError::TokenNotFound);
    }

    let nft = storage
        .get_nft(collection, token_id)
        .ok_or(NftError::TokenNotFound)?;

    Ok(nft.approved)
}

/// Check if an operator is approved for all NFTs of an owner in a collection
///
/// # Parameters
/// - `storage`: Storage backend
/// - `owner`: Owner's public key
/// - `collection`: Collection ID
/// - `operator`: Operator's public key
///
/// # Returns
/// - `Ok(bool)`: Whether the operator is approved
pub fn is_approved_for_all<S: NftStorage + ?Sized>(
    storage: &S,
    owner: &PublicKey,
    collection: &Hash,
    operator: &PublicKey,
) -> NftResult<bool> {
    if !storage.collection_exists(collection) {
        return Err(NftError::CollectionNotFound);
    }

    Ok(storage.is_approved_for_all(owner, collection, operator))
}

// ========================================
// Mint Count Query
// ========================================

/// Get the number of NFTs minted by a user in a collection
/// Used for enforcing mint limits
///
/// # Parameters
/// - `storage`: Storage backend
/// - `collection`: Collection ID
/// - `user`: User's public key
///
/// # Returns
/// - `Ok(u64)`: Number of NFTs minted
pub fn get_mint_count<S: NftStorage + ?Sized>(
    storage: &S,
    collection: &Hash,
    user: &PublicKey,
) -> NftResult<u64> {
    if !storage.collection_exists(collection) {
        return Err(NftError::CollectionNotFound);
    }

    Ok(storage.get_mint_count(collection, user))
}

// ========================================
// Frozen Status Query
// ========================================

/// Check if an NFT is frozen
///
/// # Parameters
/// - `storage`: Storage backend
/// - `collection`: Collection ID
/// - `token_id`: Token ID
///
/// # Returns
/// - `Ok(bool)`: Whether the NFT is frozen
/// - `Err(NftError)`: Error code
pub fn is_frozen<S: NftStorage + ?Sized>(
    storage: &S,
    collection: &Hash,
    token_id: u64,
) -> NftResult<bool> {
    if token_id == 0 {
        return Err(NftError::TokenNotFound);
    }

    let nft = storage
        .get_nft(collection, token_id)
        .ok_or(NftError::TokenNotFound)?;

    Ok(nft.is_frozen)
}

/// Check if a collection is paused
///
/// # Parameters
/// - `storage`: Storage backend
/// - `collection`: Collection ID
///
/// # Returns
/// - `Ok(bool)`: Whether the collection is paused
/// - `Err(NftError)`: Error code
pub fn is_collection_paused<S: NftStorage + ?Sized>(
    storage: &S,
    collection: &Hash,
) -> NftResult<bool> {
    let col = storage
        .get_collection(collection)
        .ok_or(NftError::CollectionNotFound)?;

    Ok(col.is_paused)
}

#[cfg(test)]
mod tests {
    use super::super::collection::{create_collection, CreateCollectionParams};
    use super::super::mint::{mint, MintParams};
    use super::super::RuntimeContext;
    use super::*;
    use crate::nft::MintAuthority;
    use std::collections::HashMap;

    // Mock storage (simplified for query tests)
    struct MockStorage {
        collections: HashMap<Hash, NftCollection>,
        nfts: HashMap<(Hash, u64), Nft>,
        balances: HashMap<(Hash, PublicKey), u64>,
        approvals: HashMap<(PublicKey, Hash, PublicKey), bool>,
        mint_counts: HashMap<(Hash, PublicKey), u64>,
        nonce: u64,
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
            }
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
    }

    fn test_public_key() -> PublicKey {
        PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[10u8; 32])
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
            base_uri: "https://example.com/".to_string(),
            max_supply: Some(1000),
            royalty_recipient: creator.clone(),
            royalty_basis_points: 250,
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
    fn test_owner_of() {
        let (storage, collection_id, token_id, owner) = setup_test();

        let result = owner_of(&storage, &collection_id, token_id);
        assert_eq!(result, Ok(owner));
    }

    #[test]
    fn test_owner_of_not_found() {
        let (storage, collection_id, _, _) = setup_test();

        let result = owner_of(&storage, &collection_id, 999);
        assert_eq!(result, Err(NftError::TokenNotFound));
    }

    #[test]
    fn test_owner_of_zero_token_id() {
        let (storage, collection_id, _, _) = setup_test();

        let result = owner_of(&storage, &collection_id, 0);
        assert_eq!(result, Err(NftError::TokenNotFound));
    }

    #[test]
    fn test_exists() {
        let (storage, collection_id, token_id, _) = setup_test();

        assert_eq!(exists(&storage, &collection_id, token_id), Ok(true));
        assert_eq!(exists(&storage, &collection_id, 999), Ok(false));
        assert_eq!(exists(&storage, &collection_id, 0), Ok(false));
    }

    #[test]
    fn test_balance_of() {
        let (mut storage, collection_id, _, owner) = setup_test();

        assert_eq!(balance_of(&storage, &collection_id, &owner), Ok(1));

        // Mint more
        let ctx = RuntimeContext::new(owner.clone(), 100);
        mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), owner.clone()),
        )
        .unwrap();
        mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), owner.clone()),
        )
        .unwrap();

        assert_eq!(balance_of(&storage, &collection_id, &owner), Ok(3));
    }

    #[test]
    fn test_balance_of_collection_not_found() {
        let (storage, _, _, owner) = setup_test();
        let fake_id = Hash::new([99u8; 32]);

        let result = balance_of(&storage, &fake_id, &owner);
        assert_eq!(result, Err(NftError::CollectionNotFound));
    }

    #[test]
    fn test_get_collection_supply() {
        let (mut storage, collection_id, _, owner) = setup_test();

        assert_eq!(get_collection_supply(&storage, &collection_id), Ok(1));

        // Mint more
        let ctx = RuntimeContext::new(owner.clone(), 100);
        mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), owner.clone()),
        )
        .unwrap();

        assert_eq!(get_collection_supply(&storage, &collection_id), Ok(2));
    }

    #[test]
    fn test_get_collection_max_supply() {
        let (storage, collection_id, _, _) = setup_test();

        assert_eq!(
            get_collection_max_supply(&storage, &collection_id),
            Ok(Some(1000))
        );
    }

    #[test]
    fn test_get_collection() {
        let (storage, collection_id, _, _) = setup_test();

        let result = get_collection(&storage, &collection_id);
        assert!(result.is_ok());
        let col = result.unwrap();
        assert_eq!(col.name, "Test");
        assert_eq!(col.symbol, "TEST");
    }

    #[test]
    fn test_get_nft() {
        let (storage, collection_id, token_id, owner) = setup_test();

        let result = get_nft(&storage, &collection_id, token_id);
        assert!(result.is_ok());
        let nft = result.unwrap();
        assert_eq!(nft.owner, owner);
        assert_eq!(nft.token_id, token_id);
    }

    #[test]
    fn test_get_approved() {
        let (mut storage, collection_id, token_id, _) = setup_test();

        // Initially no approval
        assert_eq!(get_approved(&storage, &collection_id, token_id), Ok(None));

        // Set approval
        let operator = PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[1u8; 32])
                .expect("valid"),
        );
        let mut nft = storage.get_nft(&collection_id, token_id).unwrap();
        nft.approved = Some(operator.clone());
        storage.set_nft(&nft).unwrap();

        assert_eq!(
            get_approved(&storage, &collection_id, token_id),
            Ok(Some(operator))
        );
    }

    #[test]
    fn test_is_approved_for_all() {
        let (mut storage, collection_id, _, owner) = setup_test();
        let operator = PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[1u8; 32])
                .expect("valid"),
        );

        // Initially not approved
        assert_eq!(
            is_approved_for_all(&storage, &owner, &collection_id, &operator),
            Ok(false)
        );

        // Set approval
        storage
            .set_approval_for_all(&owner, &collection_id, &operator, true)
            .unwrap();

        assert_eq!(
            is_approved_for_all(&storage, &owner, &collection_id, &operator),
            Ok(true)
        );
    }

    #[test]
    fn test_is_frozen() {
        let (mut storage, collection_id, token_id, _) = setup_test();

        // Initially not frozen
        assert_eq!(is_frozen(&storage, &collection_id, token_id), Ok(false));

        // Freeze
        let mut nft = storage.get_nft(&collection_id, token_id).unwrap();
        nft.is_frozen = true;
        storage.set_nft(&nft).unwrap();

        assert_eq!(is_frozen(&storage, &collection_id, token_id), Ok(true));
    }

    #[test]
    fn test_is_collection_paused() {
        let (mut storage, collection_id, _, _) = setup_test();

        // Initially not paused
        assert_eq!(is_collection_paused(&storage, &collection_id), Ok(false));

        // Pause
        let mut col = storage.get_collection(&collection_id).unwrap();
        col.is_paused = true;
        storage.set_collection(&col).unwrap();

        assert_eq!(is_collection_paused(&storage, &collection_id), Ok(true));
    }
}
