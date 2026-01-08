//! TAKO NFT Syscalls Integration Test
//!
//! Tests the NFT syscalls through the TosNftAdapter with a mock NFT storage.
//!
//! Syscalls tested (17 total):
//! - tos_nft_create_collection
//! - tos_nft_collection_exists
//! - tos_nft_mint
//! - tos_nft_batch_mint
//! - tos_nft_burn
//! - tos_nft_batch_burn
//! - tos_nft_transfer
//! - tos_nft_batch_transfer
//! - tos_nft_exists
//! - tos_nft_owner_of
//! - tos_nft_balance_of
//! - tos_nft_token_uri
//! - tos_nft_approve
//! - tos_nft_get_approved
//! - tos_nft_set_approval_for_all
//! - tos_nft_is_approved_for_all
//! - tos_nft_set_minting_paused

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
// Summary Test
// ============================================================================

#[test]
fn test_nft_syscalls_summary() {
    println!("\n");
    println!("============================================================");
    println!("  TAKO NFT Syscalls Integration Test Summary");
    println!("============================================================");
    println!();
    println!("NFT Syscalls Tested:");
    println!("  Collection Operations:");
    println!("    - tos_nft_collection_exists   (500 CU)");
    println!("    - tos_nft_set_minting_paused  (2000 CU)");
    println!();
    println!("  Token Operations:");
    println!("    - tos_nft_mint                (2000 CU + URI bytes)");
    println!("    - tos_nft_burn                (2000 CU)");
    println!("    - tos_nft_transfer            (2000 CU)");
    println!();
    println!("  Query Operations:");
    println!("    - tos_nft_exists              (500 CU)");
    println!("    - tos_nft_owner_of            (1000 CU)");
    println!("    - tos_nft_balance_of          (1000 CU)");
    println!("    - tos_nft_token_uri           (1000 CU)");
    println!();
    println!("  Approval Operations:");
    println!("    - tos_nft_approve             (2000 CU)");
    println!("    - tos_nft_get_approved        (1000 CU)");
    println!("    - tos_nft_set_approval_for_all (2000 CU)");
    println!("    - tos_nft_is_approved_for_all (500 CU)");
    println!();
    println!("Architecture:");
    println!("  Smart Contract -> TAKO Syscall -> TosNftAdapter -> NftStorage");
    println!();
    println!("============================================================");
}
