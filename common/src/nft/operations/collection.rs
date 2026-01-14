// NFT Collection Operations
// This module contains the create_collection operation logic.

use crate::crypto::{Hash, PublicKey};
use crate::nft::{MintAuthority, NftCollection, NftError, NftResult, Royalty};

use super::validation::{
    is_identity_key, validate_base_uri, validate_name, validate_royalty, validate_symbol,
};
use super::{NftStorage, RuntimeContext};

// ========================================
// Create Collection Parameters
// ========================================

/// Parameters for creating a new NFT collection
#[derive(Clone, Debug)]
pub struct CreateCollectionParams {
    /// Collection name (1-64 bytes)
    pub name: String,
    /// Symbol (1-8 bytes, uppercase ASCII)
    pub symbol: String,
    /// Base URI for token metadata (0-256 bytes)
    pub base_uri: String,
    /// Maximum supply (None = unlimited)
    pub max_supply: Option<u64>,
    /// Royalty recipient address
    pub royalty_recipient: PublicKey,
    /// Royalty percentage in basis points (0-5000)
    pub royalty_basis_points: u16,
    /// Mint authority configuration
    pub mint_authority: MintAuthority,
    /// Freeze authority (None = cannot freeze)
    pub freeze_authority: Option<PublicKey>,
    /// Metadata update authority (None = immutable)
    pub metadata_authority: Option<PublicKey>,
}

impl CreateCollectionParams {
    /// Validate all parameters
    pub fn validate(&self) -> NftResult<()> {
        validate_name(&self.name)?;
        validate_symbol(&self.symbol)?;
        validate_base_uri(&self.base_uri)?;
        validate_royalty(self.royalty_basis_points, &self.royalty_recipient)?;
        self.mint_authority.validate()?;
        Ok(())
    }
}

// ========================================
// Create Collection Operation
// ========================================

/// Create a new NFT collection
///
/// # Parameters
/// - `storage`: Storage backend
/// - `ctx`: Runtime context (caller, block height)
/// - `params`: Collection creation parameters
///
/// # Returns
/// - `Ok(Hash)`: The new collection ID
/// - `Err(NftError)`: Error code
pub fn create_collection<S: NftStorage + ?Sized>(
    storage: &mut S,
    ctx: &RuntimeContext,
    params: CreateCollectionParams,
) -> NftResult<Hash> {
    // Step 1: Validate parameters
    params.validate()?;

    // Step 2: Generate collection ID
    let collection_id = generate_collection_id(
        storage,
        &ctx.caller,
        &params.name,
        &params.symbol,
        ctx.block_height,
    )?;

    // Step 3: Check collection doesn't already exist (extremely low probability collision)
    if storage.collection_exists(&collection_id) {
        return Err(NftError::CollectionAlreadyExists);
    }

    // Step 4: Create collection object
    let collection = NftCollection {
        id: collection_id.clone(),
        name: params.name,
        symbol: params.symbol,
        creator: ctx.caller.clone(),
        total_supply: 0,
        next_token_id: 1, // token_id starts from 1
        max_supply: params.max_supply,
        base_uri: params.base_uri,
        mint_authority: params.mint_authority,
        royalty: Royalty::new(params.royalty_recipient, params.royalty_basis_points),
        freeze_authority: params.freeze_authority,
        metadata_authority: params.metadata_authority,
        is_paused: false,
        created_at: ctx.block_height,
    };

    // Step 5: Final validation
    collection.validate()?;

    // Step 6: Store collection
    storage.set_collection(&collection)?;

    // Step 7: Return collection ID
    Ok(collection_id)
}

/// Generate a unique collection ID
///
/// Uses: creator + name + symbol + block_height + nonce
/// The nonce prevents collisions within the same block
fn generate_collection_id<S: NftStorage + ?Sized>(
    storage: &mut S,
    creator: &PublicKey,
    name: &str,
    symbol: &str,
    block_height: u64,
) -> NftResult<Hash> {
    // Get and increment nonce to prevent same-block collisions
    let nonce = storage.get_and_increment_collection_nonce()?;

    // Use Blake3 for hashing
    let mut hasher = blake3::Hasher::new();
    hasher.update(creator.as_bytes());
    hasher.update(name.as_bytes());
    hasher.update(symbol.as_bytes());
    hasher.update(&block_height.to_le_bytes());
    hasher.update(&nonce.to_le_bytes());

    let hash_bytes: [u8; 32] = hasher.finalize().into();
    Ok(Hash::new(hash_bytes))
}

// ========================================
// Collection Management Operations
// ========================================

/// Pause a collection (only creator can pause)
pub fn pause_collection<S: NftStorage + ?Sized>(
    storage: &mut S,
    ctx: &RuntimeContext,
    collection_id: &Hash,
) -> NftResult<()> {
    let mut collection = storage
        .get_collection(collection_id)
        .ok_or(NftError::CollectionNotFound)?;

    // Only creator can pause
    if collection.creator != ctx.caller {
        return Err(NftError::NotCreator);
    }

    // Already paused is a no-op
    if collection.is_paused {
        return Ok(());
    }

    collection.is_paused = true;
    storage.set_collection(&collection)?;

    Ok(())
}

/// Unpause a collection (only creator can unpause)
pub fn unpause_collection<S: NftStorage + ?Sized>(
    storage: &mut S,
    ctx: &RuntimeContext,
    collection_id: &Hash,
) -> NftResult<()> {
    let mut collection = storage
        .get_collection(collection_id)
        .ok_or(NftError::CollectionNotFound)?;

    // Only creator can unpause
    if collection.creator != ctx.caller {
        return Err(NftError::NotCreator);
    }

    // Already unpaused is a no-op
    if !collection.is_paused {
        return Ok(());
    }

    collection.is_paused = false;
    storage.set_collection(&collection)?;

    Ok(())
}

/// Update collection mint authority (only creator can update)
pub fn update_mint_authority<S: NftStorage + ?Sized>(
    storage: &mut S,
    ctx: &RuntimeContext,
    collection_id: &Hash,
    new_authority: MintAuthority,
) -> NftResult<()> {
    let mut collection = storage
        .get_collection(collection_id)
        .ok_or(NftError::CollectionNotFound)?;

    // Only creator can update mint authority
    if collection.creator != ctx.caller {
        return Err(NftError::NotCreator);
    }

    // Validate new authority
    new_authority.validate()?;

    collection.mint_authority = new_authority;
    storage.set_collection(&collection)?;

    Ok(())
}

/// Update collection base URI (requires metadata authority)
pub fn update_base_uri<S: NftStorage + ?Sized>(
    storage: &mut S,
    ctx: &RuntimeContext,
    collection_id: &Hash,
    new_base_uri: String,
) -> NftResult<()> {
    let mut collection = storage
        .get_collection(collection_id)
        .ok_or(NftError::CollectionNotFound)?;

    // Check metadata authority
    match &collection.metadata_authority {
        Some(auth) if *auth == ctx.caller => {}
        Some(_) => return Err(NftError::NotMetadataAuthority),
        None => return Err(NftError::NotMetadataAuthority), // Immutable
    }

    // Validate new URI
    validate_base_uri(&new_base_uri)?;

    collection.base_uri = new_base_uri;
    storage.set_collection(&collection)?;

    Ok(())
}

/// Transfer freeze authority to a new address
pub fn transfer_freeze_authority<S: NftStorage + ?Sized>(
    storage: &mut S,
    ctx: &RuntimeContext,
    collection_id: &Hash,
    new_authority: Option<PublicKey>,
) -> NftResult<()> {
    let mut collection = storage
        .get_collection(collection_id)
        .ok_or(NftError::CollectionNotFound)?;

    // Only current freeze authority can transfer
    match &collection.freeze_authority {
        Some(auth) if *auth == ctx.caller => {}
        Some(_) => return Err(NftError::NotFreezeAuthority),
        None => return Err(NftError::NotFreezeAuthority),
    }

    // Validate new authority if provided
    if let Some(ref auth) = new_authority {
        if is_identity_key(auth) {
            return Err(NftError::InvalidAmount);
        }
    }

    collection.freeze_authority = new_authority;
    storage.set_collection(&collection)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nft::Nft;
    use std::collections::HashMap;

    // Mock storage for testing
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

    fn test_public_key_2() -> PublicKey {
        // Generate a different key by using a different approach
        let bytes = [1u8; 32];
        // This creates a compressed point from bytes
        PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&bytes)
                .expect("valid compressed point"),
        )
    }

    #[test]
    fn test_create_collection_success() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let ctx = RuntimeContext::new(creator.clone(), 100);

        let params = CreateCollectionParams {
            name: "Test Collection".to_string(),
            symbol: "TEST".to_string(),
            base_uri: "https://example.com/".to_string(),
            max_supply: Some(1000),
            royalty_recipient: creator.clone(),
            royalty_basis_points: 250,
            mint_authority: MintAuthority::CreatorOnly,
            freeze_authority: None,
            metadata_authority: None,
        };

        let result = create_collection(&mut storage, &ctx, params);
        assert!(result.is_ok());

        let collection_id = result.unwrap();
        let collection = storage.get_collection(&collection_id).unwrap();

        assert_eq!(collection.name, "Test Collection");
        assert_eq!(collection.symbol, "TEST");
        assert_eq!(collection.total_supply, 0);
        assert_eq!(collection.next_token_id, 1);
        assert_eq!(collection.max_supply, Some(1000));
        assert!(!collection.is_paused);
    }

    #[test]
    fn test_create_collection_unlimited_supply() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let ctx = RuntimeContext::new(creator.clone(), 100);

        let params = CreateCollectionParams {
            name: "Unlimited".to_string(),
            symbol: "UNLIM".to_string(),
            base_uri: "".to_string(),
            max_supply: None,
            royalty_recipient: creator.clone(),
            royalty_basis_points: 0,
            mint_authority: MintAuthority::CreatorOnly,
            freeze_authority: None,
            metadata_authority: None,
        };

        let result = create_collection(&mut storage, &ctx, params);
        assert!(result.is_ok());

        let collection = storage.get_collection(&result.unwrap()).unwrap();
        assert!(collection.max_supply.is_none());
    }

    #[test]
    fn test_create_collection_same_block_different_ids() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let ctx = RuntimeContext::new(creator.clone(), 100);

        let params1 = CreateCollectionParams {
            name: "SameBlock".to_string(),
            symbol: "SAME".to_string(),
            base_uri: "".to_string(),
            max_supply: None,
            royalty_recipient: creator.clone(),
            royalty_basis_points: 0,
            mint_authority: MintAuthority::CreatorOnly,
            freeze_authority: None,
            metadata_authority: None,
        };

        let params2 = params1.clone();

        let id1 = create_collection(&mut storage, &ctx, params1).unwrap();
        let id2 = create_collection(&mut storage, &ctx, params2).unwrap();

        // Different IDs due to nonce
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_create_collection_empty_name_fails() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let ctx = RuntimeContext::new(creator.clone(), 100);

        let params = CreateCollectionParams {
            name: "".to_string(),
            symbol: "TEST".to_string(),
            base_uri: "".to_string(),
            max_supply: None,
            royalty_recipient: creator.clone(),
            royalty_basis_points: 0,
            mint_authority: MintAuthority::CreatorOnly,
            freeze_authority: None,
            metadata_authority: None,
        };

        let result = create_collection(&mut storage, &ctx, params);
        assert_eq!(result, Err(NftError::InvalidAmount));
    }

    #[test]
    fn test_create_collection_invalid_symbol_fails() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let ctx = RuntimeContext::new(creator.clone(), 100);

        let params = CreateCollectionParams {
            name: "Test".to_string(),
            symbol: "test".to_string(), // lowercase
            base_uri: "".to_string(),
            max_supply: None,
            royalty_recipient: creator.clone(),
            royalty_basis_points: 0,
            mint_authority: MintAuthority::CreatorOnly,
            freeze_authority: None,
            metadata_authority: None,
        };

        let result = create_collection(&mut storage, &ctx, params);
        assert_eq!(result, Err(NftError::SymbolInvalidChar));
    }

    #[test]
    fn test_pause_unpause_collection() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let ctx = RuntimeContext::new(creator.clone(), 100);

        let params = CreateCollectionParams {
            name: "Test".to_string(),
            symbol: "TEST".to_string(),
            base_uri: "".to_string(),
            max_supply: None,
            royalty_recipient: creator.clone(),
            royalty_basis_points: 0,
            mint_authority: MintAuthority::CreatorOnly,
            freeze_authority: None,
            metadata_authority: None,
        };

        let collection_id = create_collection(&mut storage, &ctx, params).unwrap();

        // Pause
        assert!(pause_collection(&mut storage, &ctx, &collection_id).is_ok());
        assert!(storage.get_collection(&collection_id).unwrap().is_paused);

        // Unpause
        assert!(unpause_collection(&mut storage, &ctx, &collection_id).is_ok());
        assert!(!storage.get_collection(&collection_id).unwrap().is_paused);
    }

    #[test]
    fn test_pause_not_creator_fails() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let ctx = RuntimeContext::new(creator.clone(), 100);

        let params = CreateCollectionParams {
            name: "Test".to_string(),
            symbol: "TEST".to_string(),
            base_uri: "".to_string(),
            max_supply: None,
            royalty_recipient: creator.clone(),
            royalty_basis_points: 0,
            mint_authority: MintAuthority::CreatorOnly,
            freeze_authority: None,
            metadata_authority: None,
        };

        let collection_id = create_collection(&mut storage, &ctx, params).unwrap();

        // Try to pause as different user
        let other_ctx = RuntimeContext::new(test_public_key_2(), 100);
        let result = pause_collection(&mut storage, &other_ctx, &collection_id);
        assert_eq!(result, Err(NftError::NotCreator));
    }
}
