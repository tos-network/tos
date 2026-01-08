// NFT Freeze/Thaw Operations
//
// This module provides functions for freezing and thawing NFTs.
// Frozen NFTs cannot be transferred or burned until thawed.
//
// Permission model:
// - Only the collection's freeze_authority can freeze/thaw tokens
// - Collections without freeze_authority cannot have frozen tokens

use super::{NftStorage, RuntimeContext};
use crate::crypto::Hash;
use crate::nft::{NftError, NftResult};

// ========================================
// Freeze Operation
// ========================================

/// Freeze an NFT, preventing transfer and burn operations
///
/// # Arguments
/// * `storage` - Storage backend
/// * `ctx` - Runtime context (caller must be freeze_authority)
/// * `collection` - Collection ID
/// * `token_id` - Token ID to freeze
///
/// # Returns
/// * `Ok(())` on success
/// * `Err(NftError::CollectionNotFound)` if collection doesn't exist
/// * `Err(NftError::TokenNotFound)` if token doesn't exist
/// * `Err(NftError::NotFreezeAuthority)` if caller is not freeze authority
/// * `Err(NftError::TokenFrozen)` if token is already frozen
pub fn freeze<S: NftStorage>(
    storage: &mut S,
    ctx: &RuntimeContext,
    collection: &Hash,
    token_id: u64,
) -> NftResult<()> {
    // 1. Get collection and verify freeze_authority exists
    let nft_collection = storage
        .get_collection(collection)
        .ok_or(NftError::CollectionNotFound)?;

    let freeze_authority = nft_collection
        .freeze_authority
        .as_ref()
        .ok_or(NftError::NotFreezeAuthority)?;

    // 2. Verify caller is freeze_authority
    if ctx.caller != *freeze_authority {
        return Err(NftError::NotFreezeAuthority);
    }

    // 3. Get the NFT
    let mut nft = storage
        .get_nft(collection, token_id)
        .ok_or(NftError::TokenNotFound)?;

    // 4. Check if already frozen
    if nft.is_frozen {
        return Err(NftError::TokenFrozen);
    }

    // 5. Freeze the token
    nft.is_frozen = true;

    // 6. Save updated NFT
    storage.set_nft(&nft)?;

    Ok(())
}

// ========================================
// Thaw Operation
// ========================================

/// Thaw a frozen NFT, allowing transfer and burn operations
///
/// # Arguments
/// * `storage` - Storage backend
/// * `ctx` - Runtime context (caller must be freeze_authority)
/// * `collection` - Collection ID
/// * `token_id` - Token ID to thaw
///
/// # Returns
/// * `Ok(())` on success
/// * `Err(NftError::CollectionNotFound)` if collection doesn't exist
/// * `Err(NftError::TokenNotFound)` if token doesn't exist
/// * `Err(NftError::NotFreezeAuthority)` if caller is not freeze authority
/// * `Err(NftError::TokenNotFrozen)` if token is not frozen
pub fn thaw<S: NftStorage>(
    storage: &mut S,
    ctx: &RuntimeContext,
    collection: &Hash,
    token_id: u64,
) -> NftResult<()> {
    // 1. Get collection and verify freeze_authority exists
    let nft_collection = storage
        .get_collection(collection)
        .ok_or(NftError::CollectionNotFound)?;

    let freeze_authority = nft_collection
        .freeze_authority
        .as_ref()
        .ok_or(NftError::NotFreezeAuthority)?;

    // 2. Verify caller is freeze_authority
    if ctx.caller != *freeze_authority {
        return Err(NftError::NotFreezeAuthority);
    }

    // 3. Get the NFT
    let mut nft = storage
        .get_nft(collection, token_id)
        .ok_or(NftError::TokenNotFound)?;

    // 4. Check if not frozen
    if !nft.is_frozen {
        return Err(NftError::TokenNotFrozen);
    }

    // 5. Thaw the token
    nft.is_frozen = false;

    // 6. Save updated NFT
    storage.set_nft(&nft)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::PublicKey;
    use crate::nft::operations::NftStorage;
    use crate::nft::{MintAuthority, Nft, NftCollection, Royalty};
    use crate::serializer::Serializer;
    use std::collections::HashMap;

    // Simple in-memory storage for testing
    struct MockStorage {
        collections: HashMap<Hash, NftCollection>,
        nfts: HashMap<(Hash, u64), Nft>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self {
                collections: HashMap::new(),
                nfts: HashMap::new(),
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

        fn get_balance(&self, _collection: &Hash, _owner: &PublicKey) -> u64 {
            0
        }
        fn increment_balance(&mut self, _collection: &Hash, _owner: &PublicKey) -> NftResult<u64> {
            Ok(1)
        }
        fn decrement_balance(&mut self, _collection: &Hash, _owner: &PublicKey) -> NftResult<u64> {
            Ok(0)
        }
        fn is_approved_for_all(
            &self,
            _owner: &PublicKey,
            _collection: &Hash,
            _operator: &PublicKey,
        ) -> bool {
            false
        }
        fn set_approval_for_all(
            &mut self,
            _owner: &PublicKey,
            _collection: &Hash,
            _operator: &PublicKey,
            _approved: bool,
        ) -> NftResult<()> {
            Ok(())
        }
        fn get_mint_count(&self, _collection: &Hash, _user: &PublicKey) -> u64 {
            0
        }
        fn increment_mint_count(
            &mut self,
            _collection: &Hash,
            _user: &PublicKey,
        ) -> NftResult<u64> {
            Ok(1)
        }
        fn get_and_increment_collection_nonce(&mut self) -> NftResult<u64> {
            Ok(1)
        }
    }

    fn test_bytes(seed: u8) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        bytes
    }

    fn test_pubkey(seed: u8) -> PublicKey {
        PublicKey::from_bytes(&test_bytes(seed)).unwrap()
    }

    fn test_hash(seed: u8) -> Hash {
        Hash::new(test_bytes(seed))
    }

    fn setup_collection_with_freeze(
        storage: &mut MockStorage,
        freeze_authority: PublicKey,
    ) -> Hash {
        let collection_id = test_hash(1);
        let collection = NftCollection {
            id: collection_id.clone(),
            name: "Test".to_string(),
            symbol: "TST".to_string(),
            base_uri: "https://test.com/".to_string(),
            creator: test_pubkey(1),
            total_supply: 1,
            next_token_id: 2,
            max_supply: Some(1000),
            royalty: Royalty::new(test_pubkey(1), 500),
            mint_authority: MintAuthority::CreatorOnly,
            freeze_authority: Some(freeze_authority),
            metadata_authority: None,
            is_paused: false,
            created_at: 100,
        };
        storage.set_collection(&collection).unwrap();
        collection_id
    }

    fn setup_nft(storage: &mut MockStorage, collection: &Hash, token_id: u64, owner: PublicKey) {
        let nft = Nft {
            collection: collection.clone(),
            token_id,
            owner: owner.clone(),
            metadata_uri: "uri".to_string(),
            attributes: vec![],
            created_at: 100,
            creator: owner,
            royalty: None,
            approved: None,
            is_frozen: false,
        };
        storage.set_nft(&nft).unwrap();
    }

    #[test]
    fn test_freeze_success() {
        let mut storage = MockStorage::new();
        let freeze_auth = test_pubkey(10);
        let collection = setup_collection_with_freeze(&mut storage, freeze_auth.clone());
        let owner = test_pubkey(20);
        setup_nft(&mut storage, &collection, 1, owner);

        let ctx = RuntimeContext::new(freeze_auth, 100);
        let result = freeze(&mut storage, &ctx, &collection, 1);
        assert!(result.is_ok());

        let nft = storage.get_nft(&collection, 1).unwrap();
        assert!(nft.is_frozen);
    }

    #[test]
    fn test_freeze_not_authority() {
        let mut storage = MockStorage::new();
        let freeze_auth = test_pubkey(10);
        let collection = setup_collection_with_freeze(&mut storage, freeze_auth);
        let owner = test_pubkey(20);
        setup_nft(&mut storage, &collection, 1, owner.clone());

        // Try to freeze as owner (not freeze authority)
        let ctx = RuntimeContext::new(owner, 100);
        let result = freeze(&mut storage, &ctx, &collection, 1);
        assert_eq!(result, Err(NftError::NotFreezeAuthority));
    }

    #[test]
    fn test_freeze_already_frozen() {
        let mut storage = MockStorage::new();
        let freeze_auth = test_pubkey(10);
        let collection = setup_collection_with_freeze(&mut storage, freeze_auth.clone());
        let owner = test_pubkey(20);
        setup_nft(&mut storage, &collection, 1, owner);

        let ctx = RuntimeContext::new(freeze_auth, 100);

        // First freeze succeeds
        let result = freeze(&mut storage, &ctx, &collection, 1);
        assert!(result.is_ok());

        // Second freeze fails
        let result = freeze(&mut storage, &ctx, &collection, 1);
        assert_eq!(result, Err(NftError::TokenFrozen));
    }

    #[test]
    fn test_thaw_success() {
        let mut storage = MockStorage::new();
        let freeze_auth = test_pubkey(10);
        let collection = setup_collection_with_freeze(&mut storage, freeze_auth.clone());
        let owner = test_pubkey(20);
        setup_nft(&mut storage, &collection, 1, owner);

        let ctx = RuntimeContext::new(freeze_auth, 100);

        // Freeze first
        freeze(&mut storage, &ctx, &collection, 1).unwrap();

        // Then thaw
        let result = thaw(&mut storage, &ctx, &collection, 1);
        assert!(result.is_ok());

        let nft = storage.get_nft(&collection, 1).unwrap();
        assert!(!nft.is_frozen);
    }

    #[test]
    fn test_thaw_not_frozen() {
        let mut storage = MockStorage::new();
        let freeze_auth = test_pubkey(10);
        let collection = setup_collection_with_freeze(&mut storage, freeze_auth.clone());
        let owner = test_pubkey(20);
        setup_nft(&mut storage, &collection, 1, owner);

        let ctx = RuntimeContext::new(freeze_auth, 100);

        // Try to thaw unfrozen token
        let result = thaw(&mut storage, &ctx, &collection, 1);
        assert_eq!(result, Err(NftError::TokenNotFrozen));
    }
}
