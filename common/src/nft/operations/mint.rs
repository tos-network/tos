// NFT Mint Operations
// This module contains the mint operation logic.

use crate::crypto::{Hash, PublicKey};
use crate::nft::{AttributeValue, MintAuthority, Nft, NftError, NftResult, MAX_BATCH_SIZE};

use super::validation::{
    check_mint_authority, validate_attributes, validate_collection_id, validate_metadata_uri,
    validate_recipient,
};
use super::{NftStorage, RuntimeContext};

// ========================================
// Mint Parameters
// ========================================

/// Parameters for minting a single NFT
#[derive(Clone, Debug)]
pub struct MintParams {
    /// Collection ID
    pub collection: Hash,
    /// Recipient address
    pub to: PublicKey,
    /// Metadata URI (0-512 bytes)
    pub metadata_uri: String,
    /// On-chain attributes (max 32)
    pub attributes: Vec<(String, AttributeValue)>,
}

impl MintParams {
    /// Create new mint parameters
    pub fn new(collection: Hash, to: PublicKey) -> Self {
        Self {
            collection,
            to,
            metadata_uri: String::new(),
            attributes: Vec::new(),
        }
    }

    /// Set metadata URI
    pub fn with_uri(mut self, uri: String) -> Self {
        self.metadata_uri = uri;
        self
    }

    /// Set attributes
    pub fn with_attributes(mut self, attributes: Vec<(String, AttributeValue)>) -> Self {
        self.attributes = attributes;
        self
    }
}

// ========================================
// Mint Operation
// ========================================

/// Mint a single NFT
///
/// # Parameters
/// - `storage`: Storage backend
/// - `ctx`: Runtime context (caller, block height)
/// - `params`: Mint parameters
///
/// # Returns
/// - `Ok(u64)`: The new token ID
/// - `Err(NftError)`: Error code
pub fn mint<S: NftStorage + ?Sized>(
    storage: &mut S,
    ctx: &RuntimeContext,
    params: MintParams,
) -> NftResult<u64> {
    // Step 1: Input validation
    validate_collection_id(&params.collection)?;
    validate_recipient(&params.to)?;
    validate_metadata_uri(&params.metadata_uri)?;
    validate_attributes(&params.attributes)?;

    // Step 2: Get and validate collection
    let mut collection = storage
        .get_collection(&params.collection)
        .ok_or(NftError::CollectionNotFound)?;

    // Step 3: Check collection is not paused
    if collection.is_paused {
        return Err(NftError::CollectionPaused);
    }

    // Step 4: Check supply limit
    collection.can_mint(1)?;

    // Step 5: Check mint authority
    let current_mint_count = storage.get_mint_count(&params.collection, &ctx.caller);
    check_mint_authority(
        &collection.mint_authority,
        &ctx.caller,
        &collection.creator,
        &params.to,
        current_mint_count,
    )?;

    // Step 6: Allocate token ID
    let token_id = collection.next_token_id;
    collection.next_token_id = collection
        .next_token_id
        .checked_add(1)
        .ok_or(NftError::Overflow)?;
    collection.total_supply = collection
        .total_supply
        .checked_add(1)
        .ok_or(NftError::Overflow)?;

    // Step 7: Create NFT
    let collection_id = params.collection.clone();
    let recipient = params.to.clone();
    let nft = Nft {
        collection: params.collection,
        token_id,
        owner: params.to,
        metadata_uri: params.metadata_uri,
        attributes: params.attributes,
        created_at: ctx.block_height,
        creator: ctx.caller.clone(),
        royalty: None, // Use collection default
        approved: None,
        is_frozen: false,
    };

    // Step 8: Store NFT
    storage.set_nft(&nft)?;

    // Step 9: Update collection
    storage.set_collection(&collection)?;

    // Step 10: Update recipient balance
    storage.increment_balance(&collection_id, &recipient)?;

    // Step 11: Update mint count if needed
    update_mint_count_if_needed(
        storage,
        &collection.mint_authority,
        &collection_id,
        &ctx.caller,
    )?;

    // Step 12: Return token ID
    // Note: Payment processing for Public mint is handled by the runtime
    Ok(token_id)
}

/// Update mint count if the mint authority tracks per-user mints
fn update_mint_count_if_needed<S: NftStorage + ?Sized>(
    storage: &mut S,
    mint_authority: &MintAuthority,
    collection_id: &Hash,
    caller: &PublicKey,
) -> NftResult<()> {
    match mint_authority {
        MintAuthority::WhitelistMerkle { .. } | MintAuthority::Public { .. } => {
            storage.increment_mint_count(collection_id, caller)?;
        }
        _ => {}
    }
    Ok(())
}

// ========================================
// Batch Mint Operation
// ========================================

/// Single mint entry for batch operations
#[derive(Clone, Debug)]
pub struct MintEntry {
    /// Recipient address
    pub to: PublicKey,
    /// Metadata URI
    pub metadata_uri: String,
    /// Initial attributes
    pub attributes: Vec<(String, AttributeValue)>,
}

impl MintEntry {
    /// Create a new mint entry
    pub fn new(
        to: PublicKey,
        metadata_uri: String,
        attributes: Vec<(String, AttributeValue)>,
    ) -> Self {
        Self {
            to,
            metadata_uri,
            attributes,
        }
    }
}

/// Parameters for batch minting
#[derive(Clone, Debug)]
pub struct BatchMintParams {
    /// Collection ID
    pub collection: Hash,
    /// List of mint entries
    pub mints: Vec<MintEntry>,
}

/// Batch mint multiple NFTs
///
/// # Parameters
/// - `storage`: Storage backend
/// - `ctx`: Runtime context
/// - `params`: Batch mint parameters
///
/// # Returns
/// - `Ok(Vec<u64>)`: List of new token IDs
/// - `Err(NftError)`: Error code (entire batch fails)
pub fn batch_mint<S: NftStorage + ?Sized>(
    storage: &mut S,
    ctx: &RuntimeContext,
    params: BatchMintParams,
) -> NftResult<Vec<u64>> {
    // Step 1: Validate batch size
    if params.mints.is_empty() {
        return Err(NftError::BatchEmpty);
    }
    if params.mints.len() > MAX_BATCH_SIZE {
        return Err(NftError::BatchSizeExceeded);
    }

    // Step 2: Validate collection
    validate_collection_id(&params.collection)?;
    let mut collection = storage
        .get_collection(&params.collection)
        .ok_or(NftError::CollectionNotFound)?;

    if collection.is_paused {
        return Err(NftError::CollectionPaused);
    }

    // Step 3: Check supply for entire batch
    collection.can_mint(params.mints.len() as u64)?;

    // Step 4: Validate all mints and check authority
    for entry in &params.mints {
        validate_recipient(&entry.to)?;
        validate_metadata_uri(&entry.metadata_uri)?;
        validate_attributes(&entry.attributes)?;
    }

    // Check mint authority (only once for batch)
    let current_mint_count = storage.get_mint_count(&params.collection, &ctx.caller);
    check_mint_authority(
        &collection.mint_authority,
        &ctx.caller,
        &collection.creator,
        &params.mints[0].to, // Use first recipient for authority check
        current_mint_count,
    )?;

    // Step 5: Mint all NFTs
    let collection_id = params.collection.clone();
    let mut token_ids = Vec::with_capacity(params.mints.len());
    let mut balance_updates: std::collections::HashMap<PublicKey, u64> =
        std::collections::HashMap::new();

    for entry in params.mints {
        let token_id = collection.next_token_id;
        collection.next_token_id = collection
            .next_token_id
            .checked_add(1)
            .ok_or(NftError::Overflow)?;

        let recipient = entry.to.clone();
        let nft = Nft {
            collection: collection_id.clone(),
            token_id,
            owner: entry.to,
            metadata_uri: entry.metadata_uri,
            attributes: entry.attributes,
            created_at: ctx.block_height,
            creator: ctx.caller.clone(),
            royalty: None,
            approved: None,
            is_frozen: false,
        };

        storage.set_nft(&nft)?;
        token_ids.push(token_id);

        // Track balance updates
        *balance_updates.entry(recipient).or_insert(0) += 1;
    }

    // Step 6: Update collection supply
    collection.total_supply = collection
        .total_supply
        .checked_add(token_ids.len() as u64)
        .ok_or(NftError::Overflow)?;
    storage.set_collection(&collection)?;

    // Step 7: Update balances
    for (owner, count) in balance_updates {
        for _ in 0..count {
            storage.increment_balance(&collection_id, &owner)?;
        }
    }

    // Step 8: Update mint count
    update_mint_count_if_needed(
        storage,
        &collection.mint_authority,
        &collection_id,
        &ctx.caller,
    )?;

    Ok(token_ids)
}

#[cfg(test)]
mod tests {
    use super::super::collection::{create_collection, CreateCollectionParams};
    use super::*;
    use std::collections::HashMap;

    // Mock storage (same as in collection.rs)
    struct MockStorage {
        collections: HashMap<Hash, crate::nft::NftCollection>,
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
        fn get_collection(&self, id: &Hash) -> Option<crate::nft::NftCollection> {
            self.collections.get(id).cloned()
        }

        fn set_collection(&mut self, collection: &crate::nft::NftCollection) -> NftResult<()> {
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

    fn create_test_collection(storage: &mut MockStorage, creator: PublicKey) -> Hash {
        let ctx = RuntimeContext::new(creator.clone(), 100);
        let params = CreateCollectionParams {
            name: "Test".to_string(),
            symbol: "TEST".to_string(),
            base_uri: "https://example.com/".to_string(),
            max_supply: None,
            royalty_recipient: creator.clone(),
            royalty_basis_points: 0,
            mint_authority: MintAuthority::CreatorOnly,
            freeze_authority: None,
            metadata_authority: None,
        };
        create_collection(storage, &ctx, params).unwrap()
    }

    #[test]
    fn test_mint_success() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let collection_id = create_test_collection(&mut storage, creator.clone());

        let ctx = RuntimeContext::new(creator.clone(), 100);
        let params = MintParams::new(collection_id.clone(), creator.clone())
            .with_uri("https://example.com/1.json".to_string());

        let result = mint(&mut storage, &ctx, params);
        assert!(result.is_ok());

        let token_id = result.unwrap();
        assert_eq!(token_id, 1);

        // Verify NFT created
        let nft = storage.get_nft(&collection_id, token_id).unwrap();
        assert_eq!(nft.owner, creator);
        assert_eq!(nft.metadata_uri, "https://example.com/1.json");
        assert!(!nft.is_frozen);

        // Verify collection updated
        let collection = storage.get_collection(&collection_id).unwrap();
        assert_eq!(collection.total_supply, 1);
        assert_eq!(collection.next_token_id, 2);

        // Verify balance updated
        assert_eq!(storage.get_balance(&collection_id, &creator), 1);
    }

    #[test]
    fn test_mint_sequential_token_ids() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let collection_id = create_test_collection(&mut storage, creator.clone());

        let ctx = RuntimeContext::new(creator.clone(), 100);

        let id1 = mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), creator.clone()),
        )
        .unwrap();
        let id2 = mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), creator.clone()),
        )
        .unwrap();
        let id3 = mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), creator.clone()),
        )
        .unwrap();

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);

        let collection = storage.get_collection(&collection_id).unwrap();
        assert_eq!(collection.total_supply, 3);
        assert_eq!(collection.next_token_id, 4);
    }

    #[test]
    fn test_mint_with_attributes() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let collection_id = create_test_collection(&mut storage, creator.clone());

        let ctx = RuntimeContext::new(creator.clone(), 100);
        let attrs = vec![
            (
                "rarity".to_string(),
                AttributeValue::String("rare".to_string()),
            ),
            ("power".to_string(), AttributeValue::Number(100)),
        ];

        let params = MintParams::new(collection_id.clone(), creator.clone()).with_attributes(attrs);
        let token_id = mint(&mut storage, &ctx, params).unwrap();

        let nft = storage.get_nft(&collection_id, token_id).unwrap();
        assert_eq!(nft.attributes.len(), 2);
    }

    #[test]
    fn test_mint_not_creator_fails() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let collection_id = create_test_collection(&mut storage, creator.clone());

        // Different caller
        let other = PublicKey::new(
            tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&[1u8; 32])
                .expect("valid"),
        );
        let ctx = RuntimeContext::new(other.clone(), 100);

        let result = mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), other),
        );
        assert_eq!(result, Err(NftError::NotCreator));
    }

    #[test]
    fn test_mint_collection_not_found() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let ctx = RuntimeContext::new(creator.clone(), 100);

        let fake_id = Hash::new([99u8; 32]);
        let result = mint(&mut storage, &ctx, MintParams::new(fake_id, creator));
        assert_eq!(result, Err(NftError::CollectionNotFound));
    }

    #[test]
    fn test_mint_paused_collection_fails() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let collection_id = create_test_collection(&mut storage, creator.clone());

        // Pause collection
        let ctx = RuntimeContext::new(creator.clone(), 100);
        super::super::collection::pause_collection(&mut storage, &ctx, &collection_id).unwrap();

        // Try to mint
        let result = mint(
            &mut storage,
            &ctx,
            MintParams::new(collection_id.clone(), creator.clone()),
        );
        assert_eq!(result, Err(NftError::CollectionPaused));
    }

    #[test]
    fn test_batch_mint_success() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let collection_id = create_test_collection(&mut storage, creator.clone());

        let ctx = RuntimeContext::new(creator.clone(), 100);
        let params = BatchMintParams {
            collection: collection_id.clone(),
            mints: vec![
                MintEntry::new(creator.clone(), "uri1".to_string(), vec![]),
                MintEntry::new(creator.clone(), "uri2".to_string(), vec![]),
                MintEntry::new(creator.clone(), "uri3".to_string(), vec![]),
            ],
        };

        let result = batch_mint(&mut storage, &ctx, params);
        assert!(result.is_ok());

        let ids = result.unwrap();
        assert_eq!(ids, vec![1, 2, 3]);

        let collection = storage.get_collection(&collection_id).unwrap();
        assert_eq!(collection.total_supply, 3);
        assert_eq!(storage.get_balance(&collection_id, &creator), 3);
    }

    #[test]
    fn test_batch_mint_empty_fails() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let collection_id = create_test_collection(&mut storage, creator.clone());

        let ctx = RuntimeContext::new(creator.clone(), 100);
        let params = BatchMintParams {
            collection: collection_id,
            mints: vec![],
        };

        let result = batch_mint(&mut storage, &ctx, params);
        assert_eq!(result, Err(NftError::BatchEmpty));
    }

    #[test]
    fn test_batch_mint_exceeds_limit_fails() {
        let mut storage = MockStorage::new();
        let creator = test_public_key();
        let collection_id = create_test_collection(&mut storage, creator.clone());

        let ctx = RuntimeContext::new(creator.clone(), 100);
        let params = BatchMintParams {
            collection: collection_id,
            mints: vec![MintEntry::new(creator, String::new(), vec![]); MAX_BATCH_SIZE + 1],
        };

        let result = batch_mint(&mut storage, &ctx, params);
        assert_eq!(result, Err(NftError::BatchSizeExceeded));
    }
}
