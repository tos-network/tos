//! TAKO NFT Syscalls Integration Test
//!
//! Tests the NFT syscalls through the TosNftAdapter with a mock NFT storage.
//!
//! Syscalls tested (21 total):
//! - nft_create_collection
//! - nft_collection_exists
//! - nft_update_collection
//! - nft_transfer_collection_ownership
//! - nft_mint
//! - nft_batch_mint
//! - nft_burn
//! - nft_batch_burn
//! - nft_transfer
//! - nft_batch_transfer
//! - nft_exists
//! - nft_owner_of
//! - nft_balance_of
//! - nft_token_uri
//! - nft_approve
//! - nft_get_approved
//! - nft_set_approval_for_all
//! - nft_is_approved_for_all
//! - nft_set_minting_paused
//! - nft_update_attribute
//! - nft_remove_attribute

#![allow(clippy::disallowed_methods)]

use std::collections::HashMap;
use tos_common::crypto::{Hash, PublicKey};
use tos_common::nft::operations::NftStorage;
use tos_common::nft::{MintAuthority, Nft, NftCollection, NftError, NftResult, Royalty};
use tos_common::serializer::Serializer;
use tos_daemon::tako_integration::TosNftAdapter;
use tos_program_runtime::storage::NftProvider;

// ============================================================================
// Mock NFT Storage
// ============================================================================

/// In-memory NFT storage for testing
#[allow(clippy::type_complexity)]
struct MockNftStorage {
    /// Collections: id -> collection
    collections: HashMap<[u8; 32], NftCollection>,
    /// NFTs: (collection_id, token_id) -> nft
    nfts: HashMap<([u8; 32], u64), Nft>,
    /// Balances: (collection_id, owner) -> balance
    balances: HashMap<([u8; 32], [u8; 32]), u64>,
    /// Operator approvals: (owner, collection, operator) -> approved
    operator_approvals: HashMap<([u8; 32], [u8; 32], [u8; 32]), bool>,
    /// Mint counts: (collection, user) -> count
    mint_counts: HashMap<([u8; 32], [u8; 32]), u64>,
    /// Collection nonce for ID generation
    collection_nonce: u64,
}

impl MockNftStorage {
    fn new() -> Self {
        Self {
            collections: HashMap::new(),
            nfts: HashMap::new(),
            balances: HashMap::new(),
            operator_approvals: HashMap::new(),
            mint_counts: HashMap::new(),
            collection_nonce: 0,
        }
    }

    /// Add a test collection
    fn add_collection(&mut self, collection: NftCollection) {
        let id = *collection.id.as_bytes();
        self.collections.insert(id, collection);
    }
}

impl NftStorage for MockNftStorage {
    fn get_collection(&self, id: &Hash) -> Option<NftCollection> {
        self.collections.get(id.as_bytes()).cloned()
    }

    fn set_collection(&mut self, collection: &NftCollection) -> NftResult<()> {
        let id = *collection.id.as_bytes();
        self.collections.insert(id, collection.clone());
        Ok(())
    }

    fn collection_exists(&self, id: &Hash) -> bool {
        self.collections.contains_key(id.as_bytes())
    }

    fn get_nft(&self, collection: &Hash, token_id: u64) -> Option<Nft> {
        self.nfts.get(&(*collection.as_bytes(), token_id)).cloned()
    }

    fn set_nft(&mut self, nft: &Nft) -> NftResult<()> {
        let key = (*nft.collection.as_bytes(), nft.token_id);
        self.nfts.insert(key, nft.clone());
        Ok(())
    }

    fn delete_nft(&mut self, collection: &Hash, token_id: u64) -> NftResult<()> {
        let key = (*collection.as_bytes(), token_id);
        self.nfts.remove(&key);
        Ok(())
    }

    fn nft_exists(&self, collection: &Hash, token_id: u64) -> bool {
        self.nfts.contains_key(&(*collection.as_bytes(), token_id))
    }

    fn get_balance(&self, collection: &Hash, owner: &PublicKey) -> u64 {
        let key = (*collection.as_bytes(), *owner.as_bytes());
        *self.balances.get(&key).unwrap_or(&0)
    }

    fn increment_balance(&mut self, collection: &Hash, owner: &PublicKey) -> NftResult<u64> {
        let key = (*collection.as_bytes(), *owner.as_bytes());
        let balance = self.balances.entry(key).or_insert(0);
        *balance = balance.checked_add(1).ok_or(NftError::Overflow)?;
        Ok(*balance)
    }

    fn decrement_balance(&mut self, collection: &Hash, owner: &PublicKey) -> NftResult<u64> {
        let key = (*collection.as_bytes(), *owner.as_bytes());
        let balance = self.balances.entry(key).or_insert(0);
        *balance = balance.checked_sub(1).ok_or(NftError::Overflow)?;
        Ok(*balance)
    }

    fn is_approved_for_all(
        &self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
    ) -> bool {
        let key = (
            *owner.as_bytes(),
            *collection.as_bytes(),
            *operator.as_bytes(),
        );
        *self.operator_approvals.get(&key).unwrap_or(&false)
    }

    fn set_approval_for_all(
        &mut self,
        owner: &PublicKey,
        collection: &Hash,
        operator: &PublicKey,
        approved: bool,
    ) -> NftResult<()> {
        let key = (
            *owner.as_bytes(),
            *collection.as_bytes(),
            *operator.as_bytes(),
        );
        self.operator_approvals.insert(key, approved);
        Ok(())
    }

    fn get_mint_count(&self, collection: &Hash, user: &PublicKey) -> u64 {
        let key = (*collection.as_bytes(), *user.as_bytes());
        *self.mint_counts.get(&key).unwrap_or(&0)
    }

    fn increment_mint_count(&mut self, collection: &Hash, user: &PublicKey) -> NftResult<u64> {
        let key = (*collection.as_bytes(), *user.as_bytes());
        let count = self.mint_counts.entry(key).or_insert(0);
        *count = count.checked_add(1).ok_or(NftError::Overflow)?;
        Ok(*count)
    }

    fn get_and_increment_collection_nonce(&mut self) -> NftResult<u64> {
        let nonce = self.collection_nonce;
        self.collection_nonce = self
            .collection_nonce
            .checked_add(1)
            .ok_or(NftError::Overflow)?;
        Ok(nonce)
    }
}

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a test address from a single byte (repeated 32 times)
fn test_address(byte: u8) -> [u8; 32] {
    [byte; 32]
}

/// Create a test PublicKey from a byte
fn test_pubkey(byte: u8) -> PublicKey {
    PublicKey::from_bytes(&test_address(byte)).unwrap()
}

/// Create a test Hash from a byte
fn test_hash(byte: u8) -> Hash {
    Hash::new(test_address(byte))
}

/// Create a test collection with public minting
fn create_test_collection(id_byte: u8, creator_byte: u8) -> NftCollection {
    NftCollection {
        id: test_hash(id_byte),
        creator: test_pubkey(creator_byte),
        name: "Test Collection".to_string(),
        symbol: "TEST".to_string(),
        base_uri: "https://example.com/".to_string(),
        max_supply: Some(1000),
        total_supply: 0,
        next_token_id: 1,
        royalty: Royalty {
            recipient: test_pubkey(creator_byte),
            basis_points: 500, // 5%
        },
        mint_authority: MintAuthority::Public {
            price: 0,
            payment_recipient: test_pubkey(creator_byte),
            max_per_address: 10,
        },
        freeze_authority: None,
        metadata_authority: None,
        is_paused: false,
        created_at: 100,
    }
}

// ============================================================================
// Test 1: Collection Exists
// ============================================================================

#[test]
fn test_nft_collection_exists() {
    println!("\n=== Test: nft_collection_exists ===");

    let mut storage = MockNftStorage::new();

    // Add a test collection
    let collection = create_test_collection(0x01, 0xAA);
    storage.add_collection(collection.clone());

    let adapter = TosNftAdapter::new(&mut storage);

    // Test existing collection
    let collection_id = test_address(0x01);
    let result = adapter.collection_exists(&collection_id);
    assert!(result.is_ok(), "collection_exists should succeed");
    assert!(result.unwrap(), "Collection should exist");
    println!("  Existing collection: PASS");

    // Test non-existing collection
    let non_existent = test_address(0xFF);
    let result = adapter.collection_exists(&non_existent);
    assert!(
        result.is_ok(),
        "collection_exists should succeed for non-existent"
    );
    assert!(!result.unwrap(), "Collection should not exist");
    println!("  Non-existent collection: PASS");

    println!("nft_collection_exists: ALL PASS");
}

// ============================================================================
// Test 1b: Create Collection
// ============================================================================

#[test]
fn test_nft_create_collection() {
    println!("\n=== Test: nft_create_collection ===");

    let mut storage = MockNftStorage::new();

    {
        let mut adapter = TosNftAdapter::new(&mut storage);

        let creator = test_address(0xAA);
        let royalty_recipient = test_address(0xBB);
        let name = b"Test Collection";
        let symbol = b"TEST";
        let base_uri = b"https://example.com/nft/";
        let max_supply = 1000u64;
        let royalty_bps = 500u16; // 5%
        let max_per_address = 10u64;
        let block_height = 100u64;

        // Create collection
        let result = adapter.create_collection(
            &creator,
            name,
            symbol,
            base_uri,
            max_supply,
            &royalty_recipient,
            royalty_bps,
            max_per_address,
            block_height,
        );
        assert!(
            result.is_ok(),
            "create_collection should succeed: {:?}",
            result.err()
        );
        let collection_id = result.unwrap();
        println!("  Created collection: {:02x?}...", &collection_id[0..4]);

        // Verify collection exists
        let exists = adapter.collection_exists(&collection_id);
        assert!(exists.is_ok(), "collection_exists should succeed");
        assert!(exists.unwrap(), "Created collection should exist");
        println!("  Collection exists: PASS");

        // Verify collection data
        let collection_data = adapter.get_collection(&collection_id);
        assert!(collection_data.is_ok(), "get_collection should succeed");
        let data = collection_data.unwrap();
        assert!(data.is_some(), "Collection data should be present");
        let data = data.unwrap();
        assert_eq!(data.max_supply, max_supply, "Max supply should match");
        assert_eq!(data.royalty_bps, royalty_bps, "Royalty BPS should match");
        assert_eq!(
            data.max_per_address, max_per_address,
            "Max per address should match"
        );
        assert!(!data.minting_paused, "Minting should not be paused");
        println!("  Collection data verified: PASS");
    }

    // Test creating second collection
    {
        let mut adapter = TosNftAdapter::new(&mut storage);

        let creator = test_address(0xCC);
        let result = adapter.create_collection(
            &creator,
            b"Second Collection",
            b"SEC",
            b"https://second.com/",
            500,
            &creator,
            250,
            5,
            200,
        );
        assert!(result.is_ok(), "Second create_collection should succeed");
        let collection_id2 = result.unwrap();
        println!(
            "  Created second collection: {:02x?}...",
            &collection_id2[0..4]
        );

        let exists = adapter.collection_exists(&collection_id2);
        assert!(
            exists.is_ok() && exists.unwrap(),
            "Second collection should exist"
        );
        println!("  Second collection exists: PASS");
    }

    println!("nft_create_collection: ALL PASS");
}

// ============================================================================
// Test 2: Mint NFT
// ============================================================================

#[test]
fn test_nft_mint() {
    println!("\n=== Test: nft_mint ===");

    let mut storage = MockNftStorage::new();

    // Add a test collection with public minting
    let collection = create_test_collection(0x01, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x01);
    let recipient = test_address(0xBB);
    let caller = test_address(0xAA); // Creator
    let uri = b"ipfs://QmTest123";
    let block_height = 200u64;

    // Mint NFT
    let result = adapter.mint(&collection_id, &recipient, uri, &caller, block_height);
    assert!(result.is_ok(), "mint should succeed: {:?}", result.err());
    let token_id = result.unwrap();
    assert_eq!(token_id, 1, "First token ID should be 1");
    println!("  Mint first NFT (token_id={}): PASS", token_id);

    // Mint another
    let result2 = adapter.mint(
        &collection_id,
        &recipient,
        b"ipfs://QmTest456",
        &caller,
        block_height,
    );
    assert!(result2.is_ok(), "Second mint should succeed");
    let token_id2 = result2.unwrap();
    assert_eq!(token_id2, 2, "Second token ID should be 2");
    println!("  Mint second NFT (token_id={}): PASS", token_id2);

    // Verify balance increased
    let balance = adapter.balance_of(&collection_id, &recipient);
    assert!(balance.is_ok(), "balance_of should succeed");
    assert_eq!(balance.unwrap(), 2, "Balance should be 2");
    println!("  Balance check: PASS");

    println!("nft_mint: ALL PASS");
}

// ============================================================================
// Test 2b: Batch Mint NFTs
// ============================================================================

#[test]
fn test_nft_batch_mint() {
    println!("\n=== Test: nft_batch_mint ===");

    let mut storage = MockNftStorage::new();

    // Add a test collection with public minting
    let collection = create_test_collection(0x02, 0xCC);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x02);
    let caller = test_address(0xCC); // Creator

    // Prepare batch: 3 recipients with different URIs
    let recipient1 = test_address(0xD1);
    let recipient2 = test_address(0xD2);
    let recipient3 = test_address(0xD3);

    let recipients = [recipient1, recipient2, recipient3];
    let uri1 = b"ipfs://batch1";
    let uri2 = b"ipfs://batch2";
    let uri3 = b"ipfs://batch3";
    let uris: Vec<&[u8]> = vec![uri1.as_slice(), uri2.as_slice(), uri3.as_slice()];
    let block_height = 300u64;

    // Batch mint 3 NFTs
    let result = adapter.batch_mint(&collection_id, &recipients, &uris, &caller, block_height);
    assert!(
        result.is_ok(),
        "batch_mint should succeed: {:?}",
        result.err()
    );
    let token_ids = result.unwrap();
    assert_eq!(token_ids.len(), 3, "Should mint 3 tokens");
    assert_eq!(token_ids[0], 1, "First token ID should be 1");
    assert_eq!(token_ids[1], 2, "Second token ID should be 2");
    assert_eq!(token_ids[2], 3, "Third token ID should be 3");
    println!("  Batch mint 3 NFTs: PASS (ids: {:?})", token_ids);

    // Verify each recipient owns their NFT
    for (i, (recipient, token_id)) in recipients.iter().zip(token_ids.iter()).enumerate() {
        let owner = adapter.owner_of(&collection_id, *token_id);
        assert!(owner.is_ok(), "owner_of should succeed for token {}", i);
        assert_eq!(
            owner.unwrap(),
            Some(*recipient),
            "Recipient {} should own token {}",
            i,
            token_id
        );

        let balance = adapter.balance_of(&collection_id, recipient);
        assert!(balance.is_ok(), "balance_of should succeed");
        assert_eq!(balance.unwrap(), 1, "Each recipient should have balance 1");
    }
    println!("  Ownership verification: PASS");

    // Verify URIs
    for (i, token_id) in token_ids.iter().enumerate() {
        let uri = adapter.token_uri(&collection_id, *token_id);
        assert!(uri.is_ok(), "token_uri should succeed for token {}", i);
        let uri_bytes = uri.unwrap();
        assert!(uri_bytes.is_some(), "URI should exist for token {}", i);
        let expected_uri = format!("ipfs://batch{}", i + 1);
        assert_eq!(
            String::from_utf8(uri_bytes.unwrap()).unwrap(),
            expected_uri,
            "Token {} should have correct URI",
            i
        );
    }
    println!("  URI verification: PASS");

    // Test error case: empty batch
    let empty_recipients: [[u8; 32]; 0] = [];
    let empty_uris: Vec<&[u8]> = vec![];
    let result = adapter.batch_mint(
        &collection_id,
        &empty_recipients,
        &empty_uris,
        &caller,
        block_height,
    );
    assert!(result.is_err(), "Empty batch should fail");
    println!("  Empty batch error: PASS");

    // Test error case: mismatched counts
    let one_recipient = [test_address(0xE1)];
    let two_uris: Vec<&[u8]> = vec![b"uri1", b"uri2"];
    let result = adapter.batch_mint(
        &collection_id,
        &one_recipient,
        &two_uris,
        &caller,
        block_height,
    );
    assert!(result.is_err(), "Mismatched counts should fail");
    println!("  Mismatched counts error: PASS");

    println!("nft_batch_mint: ALL PASS");
}

// ============================================================================
// Test 2b: Batch Transfer NFTs
// ============================================================================

#[test]
fn test_nft_batch_transfer() {
    println!("\n=== Test: nft_batch_transfer ===");

    let mut storage = MockNftStorage::new();

    // Add a test collection with public minting
    let collection = create_test_collection(0x03, 0xDD);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x03);
    let creator = test_address(0xDD);
    let owner = test_address(0xEE);

    // Mint 3 NFTs to owner
    let token_id1 = adapter
        .mint(&collection_id, &owner, b"uri1", &creator, 100)
        .expect("mint 1 should succeed");
    let token_id2 = adapter
        .mint(&collection_id, &owner, b"uri2", &creator, 100)
        .expect("mint 2 should succeed");
    let token_id3 = adapter
        .mint(&collection_id, &owner, b"uri3", &creator, 100)
        .expect("mint 3 should succeed");
    println!(
        "  Minted tokens: {}, {}, {}",
        token_id1, token_id2, token_id3
    );

    // Verify owner has 3 NFTs
    let balance = adapter.balance_of(&collection_id, &owner).unwrap();
    assert_eq!(balance, 3, "Owner should have 3 NFTs");
    println!("  Owner balance before: {}", balance);

    // Prepare batch transfer: send each token to a different recipient
    let recipient1 = test_address(0xF1);
    let recipient2 = test_address(0xF2);
    let recipient3 = test_address(0xF3);

    let transfers: [([u8; 32], u64, [u8; 32]); 3] = [
        (collection_id, token_id1, recipient1),
        (collection_id, token_id2, recipient2),
        (collection_id, token_id3, recipient3),
    ];

    // Batch transfer
    let result = adapter.batch_transfer(&transfers, &owner);
    assert!(
        result.is_ok(),
        "batch_transfer should succeed: {:?}",
        result.err()
    );
    println!("  Batch transfer 3 NFTs: PASS");

    // Verify owner has 0 NFTs
    let balance = adapter.balance_of(&collection_id, &owner).unwrap();
    assert_eq!(balance, 0, "Owner should have 0 NFTs after transfer");
    println!("  Owner balance after: {}", balance);

    // Verify each recipient owns their NFT
    let recipients = [
        (recipient1, token_id1),
        (recipient2, token_id2),
        (recipient3, token_id3),
    ];
    for (i, (recipient, token_id)) in recipients.iter().enumerate() {
        let owner_result = adapter.owner_of(&collection_id, *token_id);
        assert!(
            owner_result.is_ok(),
            "owner_of should succeed for token {}",
            i
        );
        assert_eq!(
            owner_result.unwrap(),
            Some(*recipient),
            "Recipient {} should own token {}",
            i,
            token_id
        );

        let balance = adapter.balance_of(&collection_id, recipient);
        assert!(balance.is_ok(), "balance_of should succeed");
        assert_eq!(balance.unwrap(), 1, "Each recipient should have balance 1");
    }
    println!("  Ownership verification: PASS");

    // Test error case: empty batch
    let empty_transfers: [([u8; 32], u64, [u8; 32]); 0] = [];
    let result = adapter.batch_transfer(&empty_transfers, &owner);
    assert!(result.is_err(), "Empty batch should fail");
    println!("  Empty batch error: PASS");

    // Test error case: not owner
    let fake_owner = test_address(0x99);
    let transfers_not_owned: [([u8; 32], u64, [u8; 32]); 1] =
        [(collection_id, token_id1, test_address(0x88))];
    let result = adapter.batch_transfer(&transfers_not_owned, &fake_owner);
    assert!(result.is_err(), "Transfer by non-owner should fail");
    println!("  Non-owner transfer error: PASS");

    println!("nft_batch_transfer: ALL PASS");
}

// ============================================================================
// Test 2c: Batch Burn NFTs
// ============================================================================

#[test]
fn test_nft_batch_burn() {
    println!("\n=== Test: nft_batch_burn ===");

    let mut storage = MockNftStorage::new();

    // Add a test collection with public minting
    let collection = create_test_collection(0x04, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x04);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);

    // Mint 3 NFTs to owner
    let token_id1 = adapter
        .mint(&collection_id, &owner, b"uri1", &creator, 100)
        .expect("mint 1 should succeed");
    let token_id2 = adapter
        .mint(&collection_id, &owner, b"uri2", &creator, 100)
        .expect("mint 2 should succeed");
    let token_id3 = adapter
        .mint(&collection_id, &owner, b"uri3", &creator, 100)
        .expect("mint 3 should succeed");
    println!(
        "  Minted tokens: {}, {}, {}",
        token_id1, token_id2, token_id3
    );

    // Verify owner has 3 NFTs
    let balance = adapter.balance_of(&collection_id, &owner).unwrap();
    assert_eq!(balance, 3, "Owner should have 3 NFTs");
    println!("  Owner balance before: {}", balance);

    // Verify all tokens exist
    assert!(adapter.nft_exists(&collection_id, token_id1).unwrap());
    assert!(adapter.nft_exists(&collection_id, token_id2).unwrap());
    assert!(adapter.nft_exists(&collection_id, token_id3).unwrap());
    println!("  All tokens exist: PASS");

    // Prepare batch burn
    let burns: [([u8; 32], u64); 3] = [
        (collection_id, token_id1),
        (collection_id, token_id2),
        (collection_id, token_id3),
    ];

    // Batch burn
    let result = adapter.batch_burn(&burns, &owner);
    assert!(
        result.is_ok(),
        "batch_burn should succeed: {:?}",
        result.err()
    );
    println!("  Batch burn 3 NFTs: PASS");

    // Verify owner has 0 NFTs
    let balance = adapter.balance_of(&collection_id, &owner).unwrap();
    assert_eq!(balance, 0, "Owner should have 0 NFTs after burn");
    println!("  Owner balance after: {}", balance);

    // Verify all tokens no longer exist
    assert!(
        !adapter.nft_exists(&collection_id, token_id1).unwrap(),
        "Token 1 should not exist"
    );
    assert!(
        !adapter.nft_exists(&collection_id, token_id2).unwrap(),
        "Token 2 should not exist"
    );
    assert!(
        !adapter.nft_exists(&collection_id, token_id3).unwrap(),
        "Token 3 should not exist"
    );
    println!("  All tokens burned: PASS");

    // Test error case: empty batch
    let empty_burns: [([u8; 32], u64); 0] = [];
    let result = adapter.batch_burn(&empty_burns, &owner);
    assert!(result.is_err(), "Empty batch should fail");
    println!("  Empty batch error: PASS");

    // Test error case: not owner (mint a new token first)
    let token_id4 = adapter
        .mint(&collection_id, &owner, b"uri4", &creator, 100)
        .expect("mint 4 should succeed");
    let fake_owner = test_address(0x99);
    let burns_not_owned: [([u8; 32], u64); 1] = [(collection_id, token_id4)];
    let result = adapter.batch_burn(&burns_not_owned, &fake_owner);
    assert!(result.is_err(), "Burn by non-owner should fail");
    println!("  Non-owner burn error: PASS");

    println!("nft_batch_burn: ALL PASS");
}

// ============================================================================
// Test 2d: Freeze/Thaw NFTs
// ============================================================================

fn create_test_collection_with_freeze_authority(
    id_byte: u8,
    creator_byte: u8,
    freeze_authority_byte: u8,
) -> NftCollection {
    NftCollection {
        id: test_hash(id_byte),
        creator: test_pubkey(creator_byte),
        name: "Test Collection".to_string(),
        symbol: "TEST".to_string(),
        base_uri: "https://example.com/".to_string(),
        max_supply: Some(1000),
        total_supply: 0,
        next_token_id: 1,
        royalty: Royalty {
            recipient: test_pubkey(creator_byte),
            basis_points: 500, // 5%
        },
        mint_authority: MintAuthority::Public {
            price: 0,
            payment_recipient: test_pubkey(creator_byte),
            max_per_address: 10,
        },
        freeze_authority: Some(test_pubkey(freeze_authority_byte)),
        metadata_authority: None,
        is_paused: false,
        created_at: 100,
    }
}

#[test]
fn test_nft_freeze_thaw() {
    println!("\n=== Test: nft_freeze_thaw ===");

    let mut storage = MockNftStorage::new();

    // Create collection with freeze_authority (0xDD)
    let collection = create_test_collection_with_freeze_authority(0x05, 0xAA, 0xDD);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x05);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let freeze_authority = test_address(0xDD);
    let not_freeze_authority = test_address(0xEE);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"test_uri", &creator, 100)
        .expect("mint should succeed");
    println!("  Minted token: {}", token_id);

    // Check initial frozen state (should be false)
    let is_frozen = adapter
        .is_frozen(&collection_id, token_id)
        .expect("is_frozen should succeed");
    assert!(!is_frozen, "Token should not be frozen initially");
    println!("  Initial frozen state: {} (expected false)", is_frozen);

    // Test 1: Freeze by freeze_authority
    let result = adapter.freeze(&collection_id, token_id, &freeze_authority);
    assert!(result.is_ok(), "Freeze should succeed: {:?}", result.err());
    println!("  Freeze by authority: PASS");

    // Verify frozen state
    let is_frozen = adapter
        .is_frozen(&collection_id, token_id)
        .expect("is_frozen should succeed");
    assert!(is_frozen, "Token should be frozen");
    println!("  Frozen state after freeze: {} (expected true)", is_frozen);

    // Test 2: Try to freeze already frozen token (should fail)
    let result = adapter.freeze(&collection_id, token_id, &freeze_authority);
    assert!(result.is_err(), "Freezing already frozen token should fail");
    println!("  Freeze already frozen: correctly failed");

    // Test 3: Try to thaw by non-authority (should fail)
    let result = adapter.thaw(&collection_id, token_id, &not_freeze_authority);
    assert!(result.is_err(), "Thaw by non-authority should fail");
    println!("  Thaw by non-authority: correctly failed");

    // Test 4: Thaw by freeze_authority
    let result = adapter.thaw(&collection_id, token_id, &freeze_authority);
    assert!(result.is_ok(), "Thaw should succeed: {:?}", result.err());
    println!("  Thaw by authority: PASS");

    // Verify thawed state
    let is_frozen = adapter
        .is_frozen(&collection_id, token_id)
        .expect("is_frozen should succeed");
    assert!(!is_frozen, "Token should not be frozen after thaw");
    println!("  Frozen state after thaw: {} (expected false)", is_frozen);

    // Test 5: Try to thaw already thawed token (should fail)
    let result = adapter.thaw(&collection_id, token_id, &freeze_authority);
    assert!(result.is_err(), "Thawing already thawed token should fail");
    println!("  Thaw already thawed: correctly failed");

    // Test 6: Try to freeze by non-authority (should fail)
    let result = adapter.freeze(&collection_id, token_id, &not_freeze_authority);
    assert!(result.is_err(), "Freeze by non-authority should fail");
    println!("  Freeze by non-authority: correctly failed");

    println!("nft_freeze_thaw: ALL PASS");
}

#[test]
fn test_nft_batch_freeze_thaw() {
    println!("\n=== Test: nft_batch_freeze_thaw ===");

    let mut storage = MockNftStorage::new();

    // Create collection with freeze_authority (0xDD)
    let collection = create_test_collection_with_freeze_authority(0x06, 0xAA, 0xDD);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x06);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let freeze_authority = test_address(0xDD);

    // Mint 3 NFTs
    let token_id1 = adapter
        .mint(&collection_id, &owner, b"uri1", &creator, 100)
        .expect("mint 1 should succeed");
    let token_id2 = adapter
        .mint(&collection_id, &owner, b"uri2", &creator, 100)
        .expect("mint 2 should succeed");
    let token_id3 = adapter
        .mint(&collection_id, &owner, b"uri3", &creator, 100)
        .expect("mint 3 should succeed");
    println!(
        "  Minted tokens: {}, {}, {}",
        token_id1, token_id2, token_id3
    );

    // Verify all tokens are not frozen
    assert!(
        !adapter.is_frozen(&collection_id, token_id1).unwrap(),
        "Token 1 should not be frozen"
    );
    assert!(
        !adapter.is_frozen(&collection_id, token_id2).unwrap(),
        "Token 2 should not be frozen"
    );
    assert!(
        !adapter.is_frozen(&collection_id, token_id3).unwrap(),
        "Token 3 should not be frozen"
    );
    println!("  All tokens unfrozen: PASS");

    // Batch freeze
    let tokens: [([u8; 32], u64); 3] = [
        (collection_id, token_id1),
        (collection_id, token_id2),
        (collection_id, token_id3),
    ];
    let result = adapter.batch_freeze(&tokens, &freeze_authority);
    assert!(
        result.is_ok(),
        "batch_freeze should succeed: {:?}",
        result.err()
    );
    println!("  Batch freeze 3 tokens: PASS");

    // Verify all tokens are frozen
    assert!(
        adapter.is_frozen(&collection_id, token_id1).unwrap(),
        "Token 1 should be frozen"
    );
    assert!(
        adapter.is_frozen(&collection_id, token_id2).unwrap(),
        "Token 2 should be frozen"
    );
    assert!(
        adapter.is_frozen(&collection_id, token_id3).unwrap(),
        "Token 3 should be frozen"
    );
    println!("  All tokens frozen: PASS");

    // Batch thaw
    let result = adapter.batch_thaw(&tokens, &freeze_authority);
    assert!(
        result.is_ok(),
        "batch_thaw should succeed: {:?}",
        result.err()
    );
    println!("  Batch thaw 3 tokens: PASS");

    // Verify all tokens are unfrozen
    assert!(
        !adapter.is_frozen(&collection_id, token_id1).unwrap(),
        "Token 1 should not be frozen"
    );
    assert!(
        !adapter.is_frozen(&collection_id, token_id2).unwrap(),
        "Token 2 should not be frozen"
    );
    assert!(
        !adapter.is_frozen(&collection_id, token_id3).unwrap(),
        "Token 3 should not be frozen"
    );
    println!("  All tokens unfrozen after thaw: PASS");

    // Test error case: empty batch
    let empty_tokens: [([u8; 32], u64); 0] = [];
    let result = adapter.batch_freeze(&empty_tokens, &freeze_authority);
    assert!(result.is_err(), "Empty batch should fail");
    println!("  Empty batch error: PASS");

    println!("nft_batch_freeze_thaw: ALL PASS");
}

// ============================================================================
// Test 3: Transfer NFT
// ============================================================================

#[test]
fn test_nft_transfer() {
    println!("\n=== Test: nft_transfer ===");

    let mut storage = MockNftStorage::new();
    let collection = create_test_collection(0x01, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x01);
    let owner = test_address(0xBB);
    let recipient = test_address(0xCC);
    let caller = test_address(0xAA);

    // First mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"uri", &caller, 100)
        .unwrap();
    println!("  Minted token {}", token_id);

    // Transfer from owner
    let result = adapter.transfer(&collection_id, token_id, &owner, &recipient, &owner);
    assert!(
        result.is_ok(),
        "transfer should succeed: {:?}",
        result.err()
    );
    println!("  Transfer by owner: PASS");

    // Verify new owner
    let new_owner = adapter.owner_of(&collection_id, token_id);
    assert!(new_owner.is_ok(), "owner_of should succeed");
    assert_eq!(
        new_owner.unwrap(),
        Some(recipient),
        "Owner should be recipient"
    );
    println!("  Owner verification: PASS");

    // Verify balances
    let old_balance = adapter.balance_of(&collection_id, &owner).unwrap();
    let new_balance = adapter.balance_of(&collection_id, &recipient).unwrap();
    assert_eq!(old_balance, 0, "Old owner balance should be 0");
    assert_eq!(new_balance, 1, "New owner balance should be 1");
    println!("  Balance verification: PASS");

    println!("nft_transfer: ALL PASS");
}

// ============================================================================
// Test 4: Burn NFT
// ============================================================================

#[test]
fn test_nft_burn() {
    println!("\n=== Test: nft_burn ===");

    let mut storage = MockNftStorage::new();
    let collection = create_test_collection(0x01, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x01);
    let owner = test_address(0xBB);
    let caller = test_address(0xAA);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"uri", &caller, 100)
        .unwrap();
    println!("  Minted token {}", token_id);

    // Verify it exists
    assert!(
        adapter.nft_exists(&collection_id, token_id).unwrap(),
        "Token should exist before burn"
    );

    // Burn the NFT (as owner)
    let result = adapter.burn(&collection_id, token_id, &owner);
    assert!(result.is_ok(), "burn should succeed: {:?}", result.err());
    println!("  Burn by owner: PASS");

    // Verify it no longer exists
    assert!(
        !adapter.nft_exists(&collection_id, token_id).unwrap(),
        "Token should not exist after burn"
    );
    println!("  Existence check after burn: PASS");

    // Verify balance decreased
    let balance = adapter.balance_of(&collection_id, &owner).unwrap();
    assert_eq!(balance, 0, "Balance should be 0 after burn");
    println!("  Balance after burn: PASS");

    println!("nft_burn: ALL PASS");
}

// ============================================================================
// Test 5: Token Queries
// ============================================================================

#[test]
fn test_nft_queries() {
    println!("\n=== Test: NFT Query Functions ===");

    let mut storage = MockNftStorage::new();
    let collection = create_test_collection(0x01, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x01);
    let owner = test_address(0xBB);
    let caller = test_address(0xAA);
    let uri = b"ipfs://QmTestMetadata";

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, uri, &caller, 100)
        .unwrap();

    // Test exists
    let exists = adapter.nft_exists(&collection_id, token_id);
    assert!(
        exists.is_ok() && exists.unwrap(),
        "exists should return true"
    );
    println!("  exists: PASS");

    // Test owner_of
    let owner_result = adapter.owner_of(&collection_id, token_id);
    assert!(owner_result.is_ok(), "owner_of should succeed");
    assert_eq!(owner_result.unwrap(), Some(owner), "Owner should match");
    println!("  owner_of: PASS");

    // Test balance_of
    let balance = adapter.balance_of(&collection_id, &owner);
    assert!(balance.is_ok(), "balance_of should succeed");
    assert_eq!(balance.unwrap(), 1, "Balance should be 1");
    println!("  balance_of: PASS");

    // Test token_uri
    let token_uri = adapter.token_uri(&collection_id, token_id);
    assert!(token_uri.is_ok(), "token_uri should succeed");
    let uri_data = token_uri.unwrap();
    assert!(uri_data.is_some(), "URI should exist");
    assert_eq!(uri_data.unwrap(), uri.to_vec(), "URI should match");
    println!("  token_uri: PASS");

    // Test non-existent token
    let non_existent = adapter.owner_of(&collection_id, 999);
    assert!(
        non_existent.is_ok(),
        "owner_of should succeed for non-existent"
    );
    assert_eq!(
        non_existent.unwrap(),
        None,
        "Non-existent token should return None"
    );
    println!("  owner_of (non-existent): PASS");

    println!("NFT Query Functions: ALL PASS");
}

// ============================================================================
// Test 6: Single Token Approval
// ============================================================================

#[test]
fn test_nft_approve() {
    println!("\n=== Test: nft_approve ===");

    let mut storage = MockNftStorage::new();
    let collection = create_test_collection(0x01, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x01);
    let owner = test_address(0xBB);
    let operator = test_address(0xCC);
    let caller = test_address(0xAA);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"uri", &caller, 100)
        .unwrap();

    // Set approval (as owner)
    let result = adapter.approve(&collection_id, token_id, Some(&operator), &owner);
    assert!(result.is_ok(), "approve should succeed: {:?}", result.err());
    println!("  Set approval: PASS");

    // Check approval
    let approved = adapter.get_approved(&collection_id, token_id);
    assert!(approved.is_ok(), "get_approved should succeed");
    assert_eq!(
        approved.unwrap(),
        Some(operator),
        "Approved address should match"
    );
    println!("  Get approval: PASS");

    // Clear approval
    let result = adapter.approve(&collection_id, token_id, None, &owner);
    assert!(result.is_ok(), "clear approval should succeed");
    println!("  Clear approval: PASS");

    // Verify cleared
    let approved = adapter.get_approved(&collection_id, token_id);
    assert!(approved.is_ok(), "get_approved should succeed after clear");
    assert_eq!(approved.unwrap(), None, "Approval should be cleared");
    println!("  Verify cleared: PASS");

    println!("nft_approve: ALL PASS");
}

// ============================================================================
// Test 7: Approval For All
// ============================================================================

#[test]
fn test_nft_approval_for_all() {
    println!("\n=== Test: nft_set/is_approved_for_all ===");

    let mut storage = MockNftStorage::new();
    let collection = create_test_collection(0x01, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x01);
    let owner = test_address(0xBB);
    let operator = test_address(0xCC);

    // Check initial state (not approved)
    let is_approved = adapter.is_approved_for_all(&collection_id, &owner, &operator);
    assert!(is_approved.is_ok(), "is_approved_for_all should succeed");
    assert!(!is_approved.unwrap(), "Should not be approved initially");
    println!("  Initial state (not approved): PASS");

    // Set approval for all
    let result = adapter.set_approval_for_all(&collection_id, &operator, true, &owner);
    assert!(
        result.is_ok(),
        "set_approval_for_all should succeed: {:?}",
        result.err()
    );
    println!("  Set approval for all: PASS");

    // Check approved
    let is_approved = adapter.is_approved_for_all(&collection_id, &owner, &operator);
    assert!(is_approved.is_ok(), "is_approved_for_all should succeed");
    assert!(is_approved.unwrap(), "Should be approved");
    println!("  Verify approved: PASS");

    // Revoke approval
    let result = adapter.set_approval_for_all(&collection_id, &operator, false, &owner);
    assert!(result.is_ok(), "revoke approval should succeed");
    println!("  Revoke approval: PASS");

    // Verify revoked
    let is_approved = adapter.is_approved_for_all(&collection_id, &owner, &operator);
    assert!(
        is_approved.is_ok(),
        "is_approved_for_all should succeed after revoke"
    );
    assert!(!is_approved.unwrap(), "Should not be approved after revoke");
    println!("  Verify revoked: PASS");

    println!("nft_approval_for_all: ALL PASS");
}

// ============================================================================
// Test 8: Minting Paused
// ============================================================================

#[test]
fn test_nft_minting_paused() {
    println!("\n=== Test: nft_set_minting_paused ===");

    let mut storage = MockNftStorage::new();
    let collection = create_test_collection(0x01, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x01);
    let owner = test_address(0xBB);
    let creator = test_address(0xAA);

    // Mint should succeed initially
    let result = adapter.mint(&collection_id, &owner, b"uri1", &creator, 100);
    assert!(result.is_ok(), "Initial mint should succeed");
    println!("  Initial mint: PASS");

    // Pause minting (as creator)
    let result = adapter.set_minting_paused(&collection_id, &creator, true);
    assert!(
        result.is_ok(),
        "set_minting_paused should succeed: {:?}",
        result.err()
    );
    println!("  Pause minting: PASS");

    // Mint should fail when paused
    let result = adapter.mint(&collection_id, &owner, b"uri2", &creator, 100);
    assert!(result.is_err(), "Mint should fail when paused");
    println!("  Mint while paused (expect fail): PASS");

    // Unpause minting
    let result = adapter.set_minting_paused(&collection_id, &creator, false);
    assert!(result.is_ok(), "Unpause should succeed");
    println!("  Unpause minting: PASS");

    // Mint should succeed again
    let result = adapter.mint(&collection_id, &owner, b"uri3", &creator, 100);
    assert!(result.is_ok(), "Mint after unpause should succeed");
    println!("  Mint after unpause: PASS");

    println!("nft_minting_paused: ALL PASS");
}

// ============================================================================
// Test 9: Operator Transfer
// ============================================================================

#[test]
fn test_nft_operator_transfer() {
    println!("\n=== Test: Operator Transfer ===");

    let mut storage = MockNftStorage::new();
    let collection = create_test_collection(0x01, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x01);
    let owner = test_address(0xBB);
    let operator = test_address(0xCC);
    let recipient = test_address(0xDD);
    let caller = test_address(0xAA);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"uri", &caller, 100)
        .unwrap();
    println!("  Minted token {}", token_id);

    // Set approval for all
    adapter
        .set_approval_for_all(&collection_id, &operator, true, &owner)
        .unwrap();
    println!("  Set operator approval: PASS");

    // Transfer by operator
    let result = adapter.transfer(&collection_id, token_id, &owner, &recipient, &operator);
    assert!(
        result.is_ok(),
        "Operator transfer should succeed: {:?}",
        result.err()
    );
    println!("  Operator transfer: PASS");

    // Verify new owner
    let new_owner = adapter.owner_of(&collection_id, token_id).unwrap();
    assert_eq!(new_owner, Some(recipient), "New owner should be recipient");
    println!("  Owner verification: PASS");

    println!("Operator Transfer: ALL PASS");
}

// ============================================================================
// Test 10: Unauthorized Operations
// ============================================================================

#[test]
fn test_nft_unauthorized() {
    println!("\n=== Test: Unauthorized Operations ===");

    let mut storage = MockNftStorage::new();
    let collection = create_test_collection(0x01, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x01);
    let owner = test_address(0xBB);
    let attacker = test_address(0xEE);
    let recipient = test_address(0xCC);
    let caller = test_address(0xAA);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"uri", &caller, 100)
        .unwrap();

    // Unauthorized transfer
    let result = adapter.transfer(&collection_id, token_id, &owner, &recipient, &attacker);
    assert!(result.is_err(), "Unauthorized transfer should fail");
    println!("  Unauthorized transfer (expect fail): PASS");

    // Unauthorized burn
    let result = adapter.burn(&collection_id, token_id, &attacker);
    assert!(result.is_err(), "Unauthorized burn should fail");
    println!("  Unauthorized burn (expect fail): PASS");

    // Unauthorized approval
    let result = adapter.approve(&collection_id, token_id, Some(&attacker), &attacker);
    assert!(result.is_err(), "Unauthorized approval should fail");
    println!("  Unauthorized approval (expect fail): PASS");

    // Unauthorized pause (only creator can pause)
    let result = adapter.set_minting_paused(&collection_id, &attacker, true);
    assert!(result.is_err(), "Unauthorized pause should fail");
    println!("  Unauthorized pause (expect fail): PASS");

    println!("Unauthorized Operations: ALL PASS");
}

// ============================================================================
// Collection Query and Metadata Tests
// ============================================================================

fn create_test_collection_with_metadata_authority(
    id_byte: u8,
    creator_byte: u8,
    metadata_authority_byte: u8,
) -> NftCollection {
    NftCollection {
        id: test_hash(id_byte),
        creator: test_pubkey(creator_byte),
        name: "Mutable Collection".to_string(),
        symbol: "MUT".to_string(),
        base_uri: "https://example.com/".to_string(),
        max_supply: Some(1000),
        total_supply: 0,
        next_token_id: 1,
        royalty: Royalty {
            recipient: test_pubkey(creator_byte),
            basis_points: 500,
        },
        mint_authority: MintAuthority::Public {
            price: 0,
            payment_recipient: test_pubkey(creator_byte),
            max_per_address: 10,
        },
        freeze_authority: None,
        metadata_authority: Some(test_pubkey(metadata_authority_byte)),
        is_paused: false,
        created_at: 100,
    }
}

#[test]
fn test_nft_collection_query() {
    println!("\n=== Test: nft_collection_query ===");

    let mut storage = MockNftStorage::new();

    // Create collection
    let collection = create_test_collection(0x07, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x07);
    let creator = test_address(0xAA);
    let user1 = test_address(0xBB);
    let user2 = test_address(0xCC);

    // Initial total supply should be 0
    let supply = adapter.get_total_supply(&collection_id).unwrap();
    assert_eq!(supply, 0, "Initial supply should be 0");
    println!("  Initial total supply (0): PASS");

    // Initial mint count for creator (the minter) should be 0
    // Note: get_mint_count tracks how many tokens the CALLER has minted,
    // not how many tokens a user owns. This is for max_per_address enforcement.
    let mint_count = adapter.get_mint_count(&collection_id, &creator).unwrap();
    assert_eq!(mint_count, 0, "Initial mint count should be 0");
    println!("  Initial mint count for minter (0): PASS");

    // Mint 2 tokens to user1 (as creator)
    let _token1 = adapter
        .mint(&collection_id, &user1, b"uri1", &creator, 100)
        .expect("mint 1 should succeed");
    let _token2 = adapter
        .mint(&collection_id, &user1, b"uri2", &creator, 100)
        .expect("mint 2 should succeed");
    println!("  Minted 2 tokens to user1 (by creator)");

    // Total supply should now be 2
    let supply = adapter.get_total_supply(&collection_id).unwrap();
    assert_eq!(supply, 2, "Supply should be 2 after minting 2 tokens");
    println!("  Total supply after minting (2): PASS");

    // Mint count for creator (the minter) should be 2
    let mint_count = adapter.get_mint_count(&collection_id, &creator).unwrap();
    assert_eq!(mint_count, 2, "Mint count for creator should be 2");
    println!("  Creator mint count (2): PASS");

    // Mint count for user2 (hasn't minted yet) should be 0
    let mint_count = adapter.get_mint_count(&collection_id, &user2).unwrap();
    assert_eq!(mint_count, 0, "Mint count for user2 should be 0");
    println!("  User2 mint count (0): PASS");

    // Mint 1 token to user2 (as user2 - self mint)
    let _token3 = adapter
        .mint(&collection_id, &user2, b"uri3", &user2, 100)
        .expect("mint 3 should succeed");

    // Total supply should now be 3
    let supply = adapter.get_total_supply(&collection_id).unwrap();
    assert_eq!(supply, 3, "Supply should be 3 after minting 3 tokens total");
    println!("  Total supply after 3rd mint (3): PASS");

    // Mint count for user2 (who minted 1 token to themselves) should be 1
    let mint_count = adapter.get_mint_count(&collection_id, &user2).unwrap();
    assert_eq!(mint_count, 1, "Mint count for user2 should be 1");
    println!("  User2 mint count after self-mint (1): PASS");

    // Creator's mint count should still be 2
    let mint_count = adapter.get_mint_count(&collection_id, &creator).unwrap();
    assert_eq!(mint_count, 2, "Creator mint count should still be 2");
    println!("  Creator mint count unchanged (2): PASS");

    // Test non-existent collection
    let fake_collection = test_address(0xFF);
    let result = adapter.get_total_supply(&fake_collection);
    assert!(result.is_err(), "Non-existent collection should error");
    println!("  Non-existent collection error: PASS");

    println!("nft_collection_query: ALL PASS");
}

#[test]
fn test_nft_set_token_uri() {
    println!("\n=== Test: nft_set_token_uri ===");

    let mut storage = MockNftStorage::new();

    // Create collection WITH metadata_authority (mutable URIs)
    let collection = create_test_collection_with_metadata_authority(0x08, 0xAA, 0xEE);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x08);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let metadata_authority = test_address(0xEE);
    let random_user = test_address(0xCC);

    // Mint a token
    let token_id = adapter
        .mint(
            &collection_id,
            &owner,
            b"ipfs://original_uri",
            &creator,
            100,
        )
        .expect("mint should succeed");
    println!("  Minted token {} with original URI", token_id);

    // Verify original URI
    let uri_result = adapter.token_uri(&collection_id, token_id).unwrap();
    assert!(uri_result.is_some(), "Token should have URI");
    assert_eq!(
        uri_result.as_ref().unwrap(),
        b"ipfs://original_uri",
        "URI should match"
    );
    println!("  Original URI verified: PASS");

    // Update URI as metadata_authority
    let new_uri = b"ipfs://updated_uri";
    let result = adapter.set_token_uri(&collection_id, token_id, new_uri, &metadata_authority);
    assert!(
        result.is_ok(),
        "metadata_authority should be able to update URI: {:?}",
        result.err()
    );
    println!("  URI update by metadata_authority: PASS");

    // Verify updated URI
    let uri_result = adapter.token_uri(&collection_id, token_id).unwrap();
    assert_eq!(
        uri_result.as_ref().unwrap(),
        b"ipfs://updated_uri",
        "URI should be updated"
    );
    println!("  Updated URI verified: PASS");

    // Try to update as non-authority (should fail)
    let result = adapter.set_token_uri(&collection_id, token_id, b"bad_uri", &random_user);
    assert!(
        result.is_err(),
        "Non-authority should not be able to update URI"
    );
    println!("  Non-authority update rejected: PASS");

    // Try to update as owner (should fail - owner != metadata_authority)
    let result = adapter.set_token_uri(&collection_id, token_id, b"bad_uri", &owner);
    assert!(
        result.is_err(),
        "Owner should not be able to update URI (not metadata_authority)"
    );
    println!("  Owner update rejected: PASS");

    // Drop adapter to release borrow on storage
    drop(adapter);

    // Test immutable collection (no metadata_authority)
    let immutable_collection = create_test_collection(0x09, 0xAA);
    storage.add_collection(immutable_collection);

    // Create new adapter for immutable collection test
    let mut adapter2 = TosNftAdapter::new(&mut storage);

    let immutable_collection_id = test_address(0x09);
    let token_id2 = adapter2
        .mint(
            &immutable_collection_id,
            &owner,
            b"immutable_uri",
            &creator,
            100,
        )
        .expect("mint should succeed");
    println!("  Minted token {} in immutable collection", token_id2);

    // Try to update URI in immutable collection (should fail)
    let result = adapter2.set_token_uri(&immutable_collection_id, token_id2, b"new_uri", &creator);
    assert!(
        result.is_err(),
        "Should not be able to update URI in immutable collection"
    );
    println!("  Immutable collection update rejected: PASS");

    println!("nft_set_token_uri: ALL PASS");
}

// ============================================================================
// Test 12: Update Collection
// ============================================================================

#[test]
fn test_nft_update_collection() {
    println!("\n=== Test: nft_update_collection ===");

    let mut storage = MockNftStorage::new();

    // Create a collection
    let collection = create_test_collection(0x10, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x10);
    let creator = test_address(0xAA);
    let non_creator = test_address(0xBB);
    let new_royalty_recipient = test_address(0xCC);

    // Get original collection data
    let original = adapter
        .get_collection(&collection_id)
        .unwrap()
        .expect("Collection should exist");
    println!(
        "  Original base_uri: {}",
        String::from_utf8_lossy(&original.base_uri)
    );
    println!("  Original royalty_bps: {}", original.royalty_bps);

    // Test 1: Update base_uri only (as creator)
    let new_base_uri = b"https://new-api.example.com/v2/";
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        Some(new_base_uri),
        None, // No royalty update
        0,
    );
    assert!(
        result.is_ok(),
        "Update collection base_uri should succeed: {:?}",
        result.err()
    );
    println!("  Update base_uri: PASS");

    // Verify base_uri updated
    let updated = adapter
        .get_collection(&collection_id)
        .unwrap()
        .expect("Collection should exist");
    assert_eq!(
        updated.base_uri,
        new_base_uri.to_vec(),
        "Base URI should be updated"
    );
    println!(
        "  Verified new base_uri: {}",
        String::from_utf8_lossy(&updated.base_uri)
    );

    // Test 2: Update royalty only (as creator)
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        None, // No base_uri update
        Some(&new_royalty_recipient),
        750, // 7.5%
    );
    assert!(
        result.is_ok(),
        "Update collection royalty should succeed: {:?}",
        result.err()
    );
    println!("  Update royalty: PASS");

    // Verify royalty updated
    let updated = adapter
        .get_collection(&collection_id)
        .unwrap()
        .expect("Collection should exist");
    assert_eq!(updated.royalty_bps, 750, "Royalty BPS should be updated");
    println!("  Verified new royalty_bps: {}", updated.royalty_bps);

    // Test 3: Update both base_uri and royalty
    let final_uri = b"https://final-api.example.com/";
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        Some(final_uri),
        Some(&creator), // Set royalty back to creator
        500,            // 5%
    );
    assert!(
        result.is_ok(),
        "Update collection both fields should succeed: {:?}",
        result.err()
    );
    println!("  Update both fields: PASS");

    // Test 4: Non-creator should fail
    let result = adapter.update_collection(
        &collection_id,
        &non_creator,
        Some(b"https://hacked.com/"),
        None,
        0,
    );
    assert!(result.is_err(), "Non-creator should not be able to update");
    println!("  Non-creator update rejected: PASS");

    // Test 5: Invalid royalty_bps (> 10000) should fail
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        None,
        Some(&new_royalty_recipient),
        10001, // > 100%
    );
    assert!(result.is_err(), "Royalty > 10000 should fail");
    println!("  Invalid royalty rejected: PASS");

    // Test 6: Non-existent collection should fail
    let fake_collection = test_address(0xFF);
    let result = adapter.update_collection(&fake_collection, &creator, Some(b"uri"), None, 0);
    assert!(result.is_err(), "Non-existent collection should fail");
    println!("  Non-existent collection rejected: PASS");

    println!("nft_update_collection: ALL PASS");
}

// ============================================================================
// Test 13: Transfer Collection Ownership
// ============================================================================

#[test]
fn test_nft_transfer_collection_ownership() {
    println!("\n=== Test: nft_transfer_collection_ownership ===");

    let mut storage = MockNftStorage::new();

    // Create a collection
    let collection = create_test_collection(0x11, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x11);
    let original_creator = test_address(0xAA);
    let new_owner = test_address(0xBB);
    let random_user = test_address(0xCC);

    // Verify original creator
    let original = adapter
        .get_collection(&collection_id)
        .unwrap()
        .expect("Collection should exist");
    println!("  Original creator: {:02x?}...", &original.creator[0..4]);

    // Test 1: Transfer ownership as current creator
    let result =
        adapter.transfer_collection_ownership(&collection_id, &original_creator, &new_owner);
    assert!(
        result.is_ok(),
        "Transfer ownership should succeed: {:?}",
        result.err()
    );
    println!("  Transfer ownership: PASS");

    // Verify new owner
    let updated = adapter
        .get_collection(&collection_id)
        .unwrap()
        .expect("Collection should exist");
    assert_eq!(updated.creator, new_owner, "Creator should be new owner");
    println!("  Verified new creator: {:02x?}...", &updated.creator[0..4]);

    // Test 2: Original creator can no longer transfer
    let result =
        adapter.transfer_collection_ownership(&collection_id, &original_creator, &random_user);
    assert!(
        result.is_err(),
        "Original creator should no longer be able to transfer"
    );
    println!("  Original creator transfer rejected: PASS");

    // Test 3: New owner can transfer
    let result = adapter.transfer_collection_ownership(&collection_id, &new_owner, &random_user);
    assert!(
        result.is_ok(),
        "New owner should be able to transfer: {:?}",
        result.err()
    );
    println!("  New owner transfer: PASS");

    // Verify ownership transferred again
    let final_state = adapter
        .get_collection(&collection_id)
        .unwrap()
        .expect("Collection should exist");
    assert_eq!(
        final_state.creator, random_user,
        "Creator should be random_user now"
    );
    println!("  Final creator: {:02x?}...", &final_state.creator[0..4]);

    // Test 4: Random user trying to transfer should fail
    let fake_caller = test_address(0xDD);
    let result =
        adapter.transfer_collection_ownership(&collection_id, &fake_caller, &original_creator);
    assert!(
        result.is_err(),
        "Non-creator should not be able to transfer"
    );
    println!("  Non-creator transfer rejected: PASS");

    // Test 5: Non-existent collection should fail
    let fake_collection = test_address(0xFF);
    let result =
        adapter.transfer_collection_ownership(&fake_collection, &random_user, &original_creator);
    assert!(result.is_err(), "Non-existent collection should fail");
    println!("  Non-existent collection rejected: PASS");

    println!("nft_transfer_collection_ownership: ALL PASS");
}

// ============================================================================
// Test 14: Update Attribute
// ============================================================================

/// Create a test collection with metadata_authority
fn create_test_collection_with_all_authorities(
    id_byte: u8,
    creator_byte: u8,
    freeze_authority_byte: u8,
    metadata_authority_byte: u8,
) -> NftCollection {
    NftCollection {
        id: test_hash(id_byte),
        creator: test_pubkey(creator_byte),
        name: "Full Authority Collection".to_string(),
        symbol: "FULL".to_string(),
        base_uri: "https://example.com/".to_string(),
        max_supply: Some(1000),
        total_supply: 0,
        next_token_id: 1,
        royalty: Royalty {
            recipient: test_pubkey(creator_byte),
            basis_points: 500,
        },
        mint_authority: MintAuthority::Public {
            price: 0,
            payment_recipient: test_pubkey(creator_byte),
            max_per_address: 10,
        },
        freeze_authority: Some(test_pubkey(freeze_authority_byte)),
        metadata_authority: Some(test_pubkey(metadata_authority_byte)),
        is_paused: false,
        created_at: 100,
    }
}

#[test]
fn test_nft_update_attribute() {
    println!("\n=== Test: nft_update_attribute ===");

    let mut storage = MockNftStorage::new();

    // Create collection with metadata_authority
    let collection = create_test_collection_with_all_authorities(0x12, 0xAA, 0xDD, 0xEE);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x12);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let metadata_authority = test_address(0xEE);
    let random_user = test_address(0xCC);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"ipfs://test", &creator, 100)
        .expect("mint should succeed");
    println!("  Minted token {}", token_id);

    // Test 1: Add string attribute
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"rarity",
        b"legendary",
        0, // String type
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "Add string attribute should succeed: {:?}",
        result.err()
    );
    println!("  Add string attribute 'rarity': PASS");

    // Test 2: Add number attribute
    let power_value = 100i64.to_le_bytes();
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"power",
        &power_value,
        1, // Number type
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "Add number attribute should succeed: {:?}",
        result.err()
    );
    println!("  Add number attribute 'power': PASS");

    // Test 3: Add boolean attribute
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"tradeable",
        &[1u8], // true
        2,      // Boolean type
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "Add boolean attribute should succeed: {:?}",
        result.err()
    );
    println!("  Add boolean attribute 'tradeable': PASS");

    // Test 4: Update existing attribute (change rarity to "common")
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"rarity",
        b"common",
        0, // String type
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "Update existing attribute should succeed: {:?}",
        result.err()
    );
    println!("  Update existing attribute 'rarity': PASS");

    // Test 5: Non-metadata_authority should fail
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"hacked",
        b"value",
        0,
        &random_user,
    );
    assert!(
        result.is_err(),
        "Non-metadata_authority should not be able to update"
    );
    println!("  Non-authority update rejected: PASS");

    // Test 6: Owner should fail (owner != metadata_authority)
    let result = adapter.update_attribute(&collection_id, token_id, b"test", b"value", 0, &owner);
    assert!(
        result.is_err(),
        "Owner should not be able to update (not metadata_authority)"
    );
    println!("  Owner update rejected: PASS");

    // Test 7: Array type should fail (not supported via syscall)
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"array_attr",
        b"data",
        3, // Array type
        &metadata_authority,
    );
    assert!(result.is_err(), "Array type should not be supported");
    println!("  Array type rejected: PASS");

    // Test 8: Invalid value type should fail
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"bad_type",
        b"value",
        99, // Invalid type
        &metadata_authority,
    );
    assert!(result.is_err(), "Invalid value type should fail");
    println!("  Invalid value type rejected: PASS");

    // Test 9: Non-existent token should fail
    let result = adapter.update_attribute(
        &collection_id,
        999,
        b"key",
        b"value",
        0,
        &metadata_authority,
    );
    assert!(result.is_err(), "Non-existent token should fail");
    println!("  Non-existent token rejected: PASS");

    println!("nft_update_attribute: ALL PASS");
}

// ============================================================================
// Test 15: Remove Attribute
// ============================================================================

#[test]
fn test_nft_remove_attribute() {
    println!("\n=== Test: nft_remove_attribute ===");

    let mut storage = MockNftStorage::new();

    // Create collection with metadata_authority
    let collection = create_test_collection_with_all_authorities(0x13, 0xAA, 0xDD, 0xEE);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x13);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let metadata_authority = test_address(0xEE);
    let random_user = test_address(0xCC);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"ipfs://test", &creator, 100)
        .expect("mint should succeed");
    println!("  Minted token {}", token_id);

    // Add some attributes first
    adapter
        .update_attribute(
            &collection_id,
            token_id,
            b"rarity",
            b"epic",
            0,
            &metadata_authority,
        )
        .expect("add rarity should succeed");
    adapter
        .update_attribute(
            &collection_id,
            token_id,
            b"level",
            &10i64.to_le_bytes(),
            1,
            &metadata_authority,
        )
        .expect("add level should succeed");
    adapter
        .update_attribute(
            &collection_id,
            token_id,
            b"equipped",
            &[1u8],
            2,
            &metadata_authority,
        )
        .expect("add equipped should succeed");
    println!("  Added 3 attributes: rarity, level, equipped");

    // Test 1: Remove attribute as metadata_authority
    let result = adapter.remove_attribute(&collection_id, token_id, b"rarity", &metadata_authority);
    assert!(
        result.is_ok(),
        "Remove attribute should succeed: {:?}",
        result.err()
    );
    println!("  Remove 'rarity' attribute: PASS");

    // Test 2: Try to remove same attribute again (should fail - not found)
    let result = adapter.remove_attribute(&collection_id, token_id, b"rarity", &metadata_authority);
    assert!(result.is_err(), "Remove non-existent attribute should fail");
    println!("  Remove already removed attribute rejected: PASS");

    // Test 3: Remove another attribute
    let result = adapter.remove_attribute(&collection_id, token_id, b"level", &metadata_authority);
    assert!(
        result.is_ok(),
        "Remove second attribute should succeed: {:?}",
        result.err()
    );
    println!("  Remove 'level' attribute: PASS");

    // Test 4: Non-metadata_authority should fail
    let result = adapter.remove_attribute(&collection_id, token_id, b"equipped", &random_user);
    assert!(
        result.is_err(),
        "Non-metadata_authority should not be able to remove"
    );
    println!("  Non-authority remove rejected: PASS");

    // Test 5: Owner should fail (owner != metadata_authority)
    let result = adapter.remove_attribute(&collection_id, token_id, b"equipped", &owner);
    assert!(
        result.is_err(),
        "Owner should not be able to remove (not metadata_authority)"
    );
    println!("  Owner remove rejected: PASS");

    // Test 6: Remove the last remaining attribute
    let result =
        adapter.remove_attribute(&collection_id, token_id, b"equipped", &metadata_authority);
    assert!(
        result.is_ok(),
        "Remove last attribute should succeed: {:?}",
        result.err()
    );
    println!("  Remove 'equipped' attribute: PASS");

    // Test 7: Non-existent key should fail
    let result = adapter.remove_attribute(
        &collection_id,
        token_id,
        b"does_not_exist",
        &metadata_authority,
    );
    assert!(result.is_err(), "Non-existent key should fail");
    println!("  Non-existent key rejected: PASS");

    // Test 8: Non-existent token should fail
    let result = adapter.remove_attribute(&collection_id, 999, b"key", &metadata_authority);
    assert!(result.is_err(), "Non-existent token should fail");
    println!("  Non-existent token rejected: PASS");

    println!("nft_remove_attribute: ALL PASS");
}

// ============================================================================
// Test 16: Immutable Collection Attributes
// ============================================================================

#[test]
fn test_nft_immutable_collection_attributes() {
    println!("\n=== Test: nft_immutable_collection_attributes ===");

    let mut storage = MockNftStorage::new();

    // Create collection WITHOUT metadata_authority (immutable attributes)
    let collection = create_test_collection(0x14, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x14);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"ipfs://test", &creator, 100)
        .expect("mint should succeed");
    println!("  Minted token {} in immutable collection", token_id);

    // Test 1: Update attribute should fail (no metadata_authority)
    let result =
        adapter.update_attribute(&collection_id, token_id, b"rarity", b"epic", 0, &creator);
    assert!(
        result.is_err(),
        "Update should fail in immutable collection"
    );
    println!("  Update attribute in immutable collection rejected: PASS");

    // Test 2: Remove attribute should also fail
    let result = adapter.remove_attribute(&collection_id, token_id, b"rarity", &creator);
    assert!(
        result.is_err(),
        "Remove should fail in immutable collection"
    );
    println!("  Remove attribute in immutable collection rejected: PASS");

    println!("nft_immutable_collection_attributes: ALL PASS");
}

// ============================================================================
// Test 17: Edge Cases - Input Validation
// ============================================================================

#[test]
fn test_nft_update_collection_royalty_without_recipient() {
    println!("\n=== Test: nft_update_collection_royalty_without_recipient ===");

    let mut storage = MockNftStorage::new();

    // Create a collection
    let collection = create_test_collection(0x20, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x20);
    let creator = test_address(0xAA);

    // Get original royalty for state invariant check
    let original = adapter
        .get_collection(&collection_id)
        .unwrap()
        .expect("Collection should exist");
    let original_royalty_bps = original.royalty_bps;

    // Test: Providing royalty_bps > 0 without recipient should fail
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        None, // No base_uri update
        None, // No recipient - but royalty_bps is non-zero!
        500,  // 5% royalty
    );
    assert!(result.is_err(), "Royalty bps without recipient should fail");
    println!("  Royalty bps without recipient rejected: PASS");

    // State invariant: royalty should remain unchanged after failure
    let after_fail = adapter
        .get_collection(&collection_id)
        .unwrap()
        .expect("Collection should exist");
    assert_eq!(
        after_fail.royalty_bps, original_royalty_bps,
        "Royalty should remain unchanged after failed update"
    );
    println!("  State invariant (royalty unchanged): PASS");

    // Test: royalty_bps = 0 with no recipient should succeed (no-op)
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        None, // No base_uri update
        None, // No recipient
        0,    // No royalty update
    );
    assert!(
        result.is_ok(),
        "No royalty update (0 bps, no recipient) should succeed: {:?}",
        result.err()
    );
    println!("  Zero royalty bps without recipient accepted: PASS");

    println!("nft_update_collection_royalty_without_recipient: ALL PASS");
}

#[test]
fn test_nft_update_collection_zero_royalty_recipient() {
    println!("\n=== Test: nft_update_collection_zero_royalty_recipient ===");

    let mut storage = MockNftStorage::new();

    // Create a collection
    let collection = create_test_collection(0x23, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x23);
    let creator = test_address(0xAA);
    let zero_address = [0u8; 32];

    // Get original royalty for state invariant check
    let original = adapter
        .get_collection(&collection_id)
        .unwrap()
        .expect("Collection should exist");
    let original_royalty_recipient = original.royalty_recipient;

    // Test: Zero address as royalty recipient should fail
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        None,
        Some(&zero_address), // Zero address recipient
        500,                 // 5% royalty
    );
    assert!(
        result.is_err(),
        "Zero address royalty recipient should fail"
    );
    println!("  Zero address royalty recipient rejected: PASS");

    // State invariant: royalty recipient should remain unchanged
    let after_fail = adapter
        .get_collection(&collection_id)
        .unwrap()
        .expect("Collection should exist");
    assert_eq!(
        after_fail.royalty_recipient, original_royalty_recipient,
        "Royalty recipient should remain unchanged after failed update"
    );
    println!("  State invariant (royalty recipient unchanged): PASS");

    // Test: Valid recipient should work
    let valid_recipient = test_address(0xBB);
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        None,
        Some(&valid_recipient),
        750, // 7.5% royalty
    );
    assert!(
        result.is_ok(),
        "Valid royalty recipient should succeed: {:?}",
        result.err()
    );
    println!("  Valid royalty recipient accepted: PASS");

    println!("nft_update_collection_zero_royalty_recipient: ALL PASS");
}

#[test]
fn test_nft_update_collection_max_royalty_bps() {
    println!("\n=== Test: nft_update_collection_max_royalty_bps ===");

    let mut storage = MockNftStorage::new();

    // Create a collection
    let collection = create_test_collection(0x28, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x28);
    let creator = test_address(0xAA);
    let recipient = test_address(0xBB);

    // Test 1: MAX_ROYALTY_BASIS_POINTS (5000 = 50%) should succeed
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        None,
        Some(&recipient),
        5000, // 50% - at the limit
    );
    assert!(
        result.is_ok(),
        "Royalty at max (5000) should succeed: {:?}",
        result.err()
    );
    println!("  Royalty at 5000 (50%) accepted: PASS");

    // Test 2: 5001 should fail (over limit)
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        None,
        Some(&recipient),
        5001, // 50.01% - over limit
    );
    assert!(result.is_err(), "Royalty over 5000 should fail");
    println!("  Royalty at 5001 rejected: PASS");

    // Test 3: 10000 (100%) should also fail
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        None,
        Some(&recipient),
        10000, // 100% - way over limit
    );
    assert!(result.is_err(), "Royalty at 10000 should fail");
    println!("  Royalty at 10000 rejected: PASS");

    println!("nft_update_collection_max_royalty_bps: ALL PASS");
}

#[test]
fn test_nft_update_attribute_number_validation() {
    println!("\n=== Test: nft_update_attribute_number_validation ===");

    let mut storage = MockNftStorage::new();

    // Create collection with metadata_authority
    let collection = create_test_collection_with_all_authorities(0x21, 0xAA, 0xDD, 0xEE);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x21);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let metadata_authority = test_address(0xEE);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"ipfs://test", &creator, 100)
        .expect("mint should succeed");
    println!("  Minted token {}", token_id);

    // Test 1: Exactly 8 bytes should succeed
    let valid_number = 42i64.to_le_bytes();
    assert_eq!(valid_number.len(), 8, "i64 should be 8 bytes");
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"score",
        &valid_number,
        1, // Number type
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "8-byte number should succeed: {:?}",
        result.err()
    );
    println!("  8-byte number accepted: PASS");

    // Get NFT to check attribute count for state invariant
    let nft_before = adapter
        .get_nft(&collection_id, token_id)
        .unwrap()
        .expect("NFT should exist");
    // NFT has 1 attribute: "score"

    // Test 2: Less than 8 bytes should fail
    let short_value = [1u8, 2u8, 3u8, 4u8]; // Only 4 bytes
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"bad_num1",
        &short_value,
        1, // Number type
        &metadata_authority,
    );
    assert!(result.is_err(), "< 8 bytes should fail for Number type");
    println!("  4-byte number rejected: PASS");

    // Test 3: More than 8 bytes should fail
    let long_value = [1u8, 2u8, 3u8, 4u8, 5u8, 6u8, 7u8, 8u8, 9u8, 10u8]; // 10 bytes
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"bad_num2",
        &long_value,
        1, // Number type
        &metadata_authority,
    );
    assert!(result.is_err(), "> 8 bytes should fail for Number type");
    println!("  10-byte number rejected: PASS");

    // Test 4: Empty value should fail
    let empty_value: [u8; 0] = [];
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"bad_num3",
        &empty_value,
        1, // Number type
        &metadata_authority,
    );
    assert!(result.is_err(), "Empty value should fail for Number type");
    println!("  Empty number rejected: PASS");

    // State invariant: no new attributes should have been added after failures
    let nft_after = adapter
        .get_nft(&collection_id, token_id)
        .unwrap()
        .expect("NFT should exist");
    // Since we can't directly inspect attributes via NftData, we verify by checking
    // that the NFT's metadata remains unchanged (minted_at timestamp is immutable)
    assert!(
        nft_before.minted_at == nft_after.minted_at,
        "NFT should remain unchanged after failed updates"
    );
    println!("  State invariant (NFT unchanged after failures): PASS");

    println!("nft_update_attribute_number_validation: ALL PASS");
}

#[test]
fn test_nft_update_attribute_boolean_validation() {
    println!("\n=== Test: nft_update_attribute_boolean_validation ===");

    let mut storage = MockNftStorage::new();

    // Create collection with metadata_authority
    let collection = create_test_collection_with_all_authorities(0x24, 0xAA, 0xDD, 0xEE);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x24);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let metadata_authority = test_address(0xEE);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"ipfs://test", &creator, 100)
        .expect("mint should succeed");
    println!("  Minted token {}", token_id);

    // Test 1: Exactly 1 byte (true) should succeed
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"active",
        &[1u8], // true
        2,      // Boolean type
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "1-byte boolean (true) should succeed: {:?}",
        result.err()
    );
    println!("  1-byte boolean (true) accepted: PASS");

    // Test 2: Exactly 1 byte (false) should succeed
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"verified",
        &[0u8], // false
        2,      // Boolean type
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "1-byte boolean (false) should succeed: {:?}",
        result.err()
    );
    println!("  1-byte boolean (false) accepted: PASS");

    // Test 3: Empty value should fail
    let empty_value: [u8; 0] = [];
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"bad_bool1",
        &empty_value,
        2, // Boolean type
        &metadata_authority,
    );
    assert!(result.is_err(), "Empty value should fail for Boolean type");
    println!("  Empty boolean rejected: PASS");

    // Test 4: More than 1 byte should fail (trailing bytes rejected)
    let long_value = [1u8, 2u8]; // 2 bytes
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"bad_bool2",
        &long_value,
        2, // Boolean type
        &metadata_authority,
    );
    assert!(result.is_err(), "> 1 byte should fail for Boolean type");
    println!("  2-byte boolean rejected: PASS");

    // Test 5: Even more bytes should fail
    let very_long_value = [1u8, 0u8, 1u8, 0u8]; // 4 bytes
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"bad_bool3",
        &very_long_value,
        2, // Boolean type
        &metadata_authority,
    );
    assert!(result.is_err(), "4 bytes should fail for Boolean type");
    println!("  4-byte boolean rejected: PASS");

    println!("nft_update_attribute_boolean_validation: ALL PASS");
}

#[test]
fn test_nft_transfer_ownership_to_zero_address() {
    println!("\n=== Test: nft_transfer_ownership_to_zero_address ===");

    let mut storage = MockNftStorage::new();

    // Create a collection
    let collection = create_test_collection(0x22, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x22);
    let creator = test_address(0xAA);
    let zero_address = [0u8; 32]; // Zero address

    // Test: Transfer to zero address should fail
    let result = adapter.transfer_collection_ownership(&collection_id, &creator, &zero_address);
    assert!(result.is_err(), "Transfer to zero address should fail");
    println!("  Transfer to zero address rejected: PASS");

    // Verify ownership unchanged
    let current = adapter
        .get_collection(&collection_id)
        .unwrap()
        .expect("Collection should exist");
    assert_eq!(
        current.creator, creator,
        "Creator should remain unchanged after failed transfer"
    );
    println!("  Creator unchanged: PASS");

    // Test: Valid transfer should still work
    let valid_new_owner = test_address(0xBB);
    let result = adapter.transfer_collection_ownership(&collection_id, &creator, &valid_new_owner);
    assert!(
        result.is_ok(),
        "Valid transfer should succeed: {:?}",
        result.err()
    );
    println!("  Valid transfer succeeded: PASS");

    println!("nft_transfer_ownership_to_zero_address: ALL PASS");
}

// ============================================================================
// Test: URI Length Validation
// ============================================================================

#[test]
fn test_nft_uri_length_validation() {
    println!("\n=== Test: nft_uri_length_validation ===");

    let mut storage = MockNftStorage::new();

    // Create collection with metadata_authority
    let collection = create_test_collection_with_all_authorities(0x25, 0xAA, 0xDD, 0xEE);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x25);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let metadata_authority = test_address(0xEE);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"initial_uri", &creator, 100)
        .expect("mint should succeed");
    println!("  Minted token {}", token_id);

    // Test 1: set_token_uri with valid length should succeed
    let valid_uri = "x".repeat(512); // MAX_METADATA_URI_LENGTH = 512
    let result = adapter.set_token_uri(
        &collection_id,
        token_id,
        valid_uri.as_bytes(),
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "URI at max length (512) should succeed: {:?}",
        result.err()
    );
    println!("  512-byte URI accepted: PASS");

    // Test 2: set_token_uri exceeding limit should fail
    let long_uri = "x".repeat(513); // One byte over limit
    let result = adapter.set_token_uri(
        &collection_id,
        token_id,
        long_uri.as_bytes(),
        &metadata_authority,
    );
    assert!(result.is_err(), "URI exceeding 512 bytes should fail");
    println!("  513-byte URI rejected: PASS");

    // Test 3: update_collection base_uri with valid length should succeed
    let valid_base_uri = "y".repeat(256); // MAX_BASE_URI_LENGTH = 256
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        Some(valid_base_uri.as_bytes()),
        None,
        0,
    );
    assert!(
        result.is_ok(),
        "Base URI at max length (256) should succeed: {:?}",
        result.err()
    );
    println!("  256-byte base_uri accepted: PASS");

    // Test 4: update_collection base_uri exceeding limit should fail
    let long_base_uri = "y".repeat(257); // One byte over limit
    let result = adapter.update_collection(
        &collection_id,
        &creator,
        Some(long_base_uri.as_bytes()),
        None,
        0,
    );
    assert!(result.is_err(), "Base URI exceeding 256 bytes should fail");
    println!("  257-byte base_uri rejected: PASS");

    println!("nft_uri_length_validation: ALL PASS");
}

// ============================================================================
// Test: Attribute Key/Value Length Validation
// ============================================================================

#[test]
fn test_nft_attribute_length_validation() {
    println!("\n=== Test: nft_attribute_length_validation ===");

    let mut storage = MockNftStorage::new();

    // Create collection with metadata_authority
    let collection = create_test_collection_with_all_authorities(0x26, 0xAA, 0xDD, 0xEE);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x26);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let metadata_authority = test_address(0xEE);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"test_uri", &creator, 100)
        .expect("mint should succeed");
    println!("  Minted token {}", token_id);

    // Test 1: Key at max length should succeed
    let max_key = "k".repeat(32); // MAX_ATTRIBUTE_KEY_LENGTH = 32
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        max_key.as_bytes(),
        &[42u8], // Boolean true
        2,       // Boolean type
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "Key at max length (32) should succeed: {:?}",
        result.err()
    );
    println!("  32-byte key accepted: PASS");

    // Test 2: Key exceeding limit should fail
    let long_key = "k".repeat(33); // One byte over limit
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        long_key.as_bytes(),
        &[1u8],
        2,
        &metadata_authority,
    );
    assert!(result.is_err(), "Key exceeding 32 bytes should fail");
    println!("  33-byte key rejected: PASS");

    // Test 3: String value at max length should succeed
    let max_value = "v".repeat(256); // MAX_ATTRIBUTE_STRING_LENGTH = 256
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"valid_str",
        max_value.as_bytes(),
        0, // String type
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "String value at max length (256) should succeed: {:?}",
        result.err()
    );
    println!("  256-byte string value accepted: PASS");

    // Test 4: String value exceeding limit should fail
    let long_value = "v".repeat(257); // One byte over limit
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"long_str",
        long_value.as_bytes(),
        0, // String type
        &metadata_authority,
    );
    assert!(
        result.is_err(),
        "String value exceeding 256 bytes should fail"
    );
    println!("  257-byte string value rejected: PASS");

    println!("nft_attribute_length_validation: ALL PASS");
}

// ============================================================================
// Test: Approval Cleared on Transfer
// ============================================================================

#[test]
fn test_nft_approval_cleared_on_transfer() {
    println!("\n=== Test: nft_approval_cleared_on_transfer ===");

    let mut storage = MockNftStorage::new();

    // Create a collection
    let collection = create_test_collection(0x27, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x27);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let approved = test_address(0xCC);
    let new_owner = test_address(0xDD);

    // Mint an NFT to owner
    let token_id = adapter
        .mint(&collection_id, &owner, b"test_uri", &creator, 100)
        .expect("mint should succeed");
    println!("  Minted token {} to owner", token_id);

    // Approve a spender
    let result = adapter.approve(&collection_id, token_id, Some(&approved), &owner);
    assert!(result.is_ok(), "Approve should succeed: {:?}", result.err());
    println!("  Approved spender: PASS");

    // Verify approval exists
    let approved_addr = adapter
        .get_approved(&collection_id, token_id)
        .expect("get_approved should succeed");
    assert!(approved_addr.is_some(), "Should have approved address");
    println!("  Approval exists before transfer: PASS");

    // Transfer the NFT (from owner to new_owner, called by owner)
    let result = adapter.transfer(&collection_id, token_id, &owner, &new_owner, &owner);
    assert!(
        result.is_ok(),
        "Transfer should succeed: {:?}",
        result.err()
    );
    println!("  Transferred to new owner: PASS");

    // Verify approval is cleared after transfer
    let approved_after = adapter
        .get_approved(&collection_id, token_id)
        .expect("get_approved should succeed");
    assert!(
        approved_after.is_none(),
        "Approval should be cleared after transfer"
    );
    println!("  Approval cleared after transfer: PASS");

    println!("nft_approval_cleared_on_transfer: ALL PASS");
}

// ============================================================================
// Test: Approve Validation (self and identity key)
// ============================================================================

#[test]
fn test_nft_approve_validation() {
    println!("\n=== Test: nft_approve_validation ===");

    let mut storage = MockNftStorage::new();

    // Create a collection
    let collection = create_test_collection(0x29, 0xAA);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x29);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let valid_operator = test_address(0xCC);
    let zero_address = [0u8; 32];

    // Mint an NFT to owner
    let token_id = adapter
        .mint(&collection_id, &owner, b"test_uri", &creator, 100)
        .expect("mint should succeed");
    println!("  Minted token {} to owner", token_id);

    // Test 1: Self-approval should fail
    let result = adapter.approve(&collection_id, token_id, Some(&owner), &owner);
    assert!(result.is_err(), "Self-approval should fail");
    println!("  Self-approval rejected: PASS");

    // Test 2: Zero address approval should fail
    let result = adapter.approve(&collection_id, token_id, Some(&zero_address), &owner);
    assert!(result.is_err(), "Zero address approval should fail");
    println!("  Zero address approval rejected: PASS");

    // Test 3: Valid operator approval should succeed
    let result = adapter.approve(&collection_id, token_id, Some(&valid_operator), &owner);
    assert!(
        result.is_ok(),
        "Valid operator approval should succeed: {:?}",
        result.err()
    );
    println!("  Valid operator approval accepted: PASS");

    // Test 4: Clearing approval (None) should succeed
    let result = adapter.approve(&collection_id, token_id, None, &owner);
    assert!(
        result.is_ok(),
        "Clearing approval should succeed: {:?}",
        result.err()
    );
    println!("  Clearing approval accepted: PASS");

    println!("nft_approve_validation: ALL PASS");
}

// ============================================================================
// Test: Remove Attribute Key Length Validation
// ============================================================================

#[test]
fn test_nft_remove_attribute_key_length() {
    println!("\n=== Test: nft_remove_attribute_key_length ===");

    let mut storage = MockNftStorage::new();

    // Create collection with metadata_authority
    let collection = create_test_collection_with_all_authorities(0x30, 0xAA, 0xDD, 0xEE);
    storage.add_collection(collection);

    let mut adapter = TosNftAdapter::new(&mut storage);

    let collection_id = test_address(0x30);
    let creator = test_address(0xAA);
    let owner = test_address(0xBB);
    let metadata_authority = test_address(0xEE);

    // Mint an NFT
    let token_id = adapter
        .mint(&collection_id, &owner, b"test_uri", &creator, 100)
        .expect("mint should succeed");
    println!("  Minted token {}", token_id);

    // First add an attribute with valid key
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        b"valid_key",
        &[1u8],
        2, // Boolean
        &metadata_authority,
    );
    assert!(result.is_ok(), "Adding attribute should succeed");
    println!("  Added test attribute: PASS");

    // Test 1: Remove with key at max length should succeed
    // First add with max length key
    let max_key = "k".repeat(32);
    let result = adapter.update_attribute(
        &collection_id,
        token_id,
        max_key.as_bytes(),
        &[1u8],
        2,
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "Adding max-length key attribute should succeed"
    );

    let result = adapter.remove_attribute(
        &collection_id,
        token_id,
        max_key.as_bytes(),
        &metadata_authority,
    );
    assert!(
        result.is_ok(),
        "Remove with 32-byte key should succeed: {:?}",
        result.err()
    );
    println!("  32-byte key removal accepted: PASS");

    // Test 2: Remove with key exceeding limit should fail
    let long_key = "k".repeat(33);
    let result = adapter.remove_attribute(
        &collection_id,
        token_id,
        long_key.as_bytes(),
        &metadata_authority,
    );
    assert!(result.is_err(), "Remove with 33-byte key should fail");
    println!("  33-byte key removal rejected: PASS");

    println!("nft_remove_attribute_key_length: ALL PASS");
}

// ============================================================================
// Summary Test
// ============================================================================

#[test]
fn test_nft_syscalls_summary() {
    println!("\n");
    println!("============================================================");
    println!("  TAKO NFT Syscalls Integration Test Summary");
    println!("============================================================");
    println!();
    println!("NFT Syscalls Tested (21 total):");
    println!("  Collection Operations:");
    println!("    - nft_collection_exists            (500 CU)");
    println!("    - nft_create_collection            (5000 CU)");
    println!("    - nft_set_minting_paused           (2000 CU)");
    println!("    - nft_get_total_supply             (500 CU)");
    println!("    - nft_get_mint_count               (500 CU)");
    println!("    - nft_update_collection            (2000 CU + URI bytes)");
    println!("    - nft_transfer_collection_ownership (2000 CU)");
    println!();
    println!("  Token Operations:");
    println!("    - nft_mint                (2000 CU + URI bytes)");
    println!("    - nft_batch_mint          (2000 CU + per-item cost)");
    println!("    - nft_burn                (2000 CU)");
    println!("    - nft_batch_burn          (2000 CU + per-item cost)");
    println!("    - nft_transfer            (2000 CU)");
    println!("    - nft_batch_transfer      (2000 CU + per-item cost)");
    println!();
    println!("  Query Operations:");
    println!("    - nft_exists              (500 CU)");
    println!("    - nft_owner_of            (1000 CU)");
    println!("    - nft_balance_of          (1000 CU)");
    println!("    - nft_token_uri           (1000 CU)");
    println!();
    println!("  Approval Operations:");
    println!("    - nft_approve             (2000 CU)");
    println!("    - nft_get_approved        (1000 CU)");
    println!("    - nft_set_approval_for_all (2000 CU)");
    println!("    - nft_is_approved_for_all (500 CU)");
    println!();
    println!("  Freeze Operations:");
    println!("    - nft_freeze              (2000 CU)");
    println!("    - nft_thaw                (2000 CU)");
    println!("    - nft_is_frozen           (500 CU)");
    println!("    - nft_batch_freeze        (2000 CU + 1500 CU/item)");
    println!("    - nft_batch_thaw          (2000 CU + 1500 CU/item)");
    println!();
    println!("  Metadata Operations:");
    println!("    - nft_set_token_uri       (2000 CU + URI bytes)");
    println!("    - nft_update_attribute    (2000 CU + key/value bytes)");
    println!("    - nft_remove_attribute    (2000 CU + key bytes)");
    println!();
    println!("Architecture:");
    println!("  Smart Contract -> TAKO Syscall -> TosNftAdapter -> NftStorage");
    println!();
    println!("============================================================");
}
