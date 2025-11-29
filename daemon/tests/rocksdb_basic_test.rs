//! Simple RocksDB integration test to verify no deadlocks
//!
//! This test verifies that the RocksDB storage backend works correctly
//! without deadlocks when setting up accounts and creating ParallelChainState.

#![allow(clippy::disallowed_methods)]

use std::sync::Arc;
use tempdir::TempDir;
use tos_common::tokio::sync::RwLock;

use tos_common::{
    account::{VersionedBalance, VersionedNonce},
    asset::{AssetData, VersionedAssetData},
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    config::{COIN_DECIMALS, COIN_VALUE, TOS_ASSET},
    crypto::{elgamal::CompressedPublicKey, Hash, KeyPair},
    immutable::Immutable,
    network::Network,
    versioned_type::Versioned,
};

use tos_daemon::core::{
    config::RocksDBConfig,
    state::parallel_chain_state::ParallelChainState,
    storage::{
        rocksdb::RocksStorage, AccountProvider, AssetProvider, BalanceProvider, NonceProvider,
    },
};

use tos_environment::Environment;

/// Register TOS asset in storage
async fn register_tos_asset(storage: &mut RocksStorage) {
    let asset_data = AssetData::new(
        COIN_DECIMALS,
        "TOS".to_string(),
        "TOS".to_string(),
        None,
        None,
    );
    let versioned_asset_data: VersionedAssetData = Versioned::new(asset_data, Some(0));
    storage
        .add_asset(&TOS_ASSET, 0, versioned_asset_data)
        .await
        .unwrap();
}

/// Setup account with balance and nonce
async fn setup_account(
    storage: &Arc<RwLock<RocksStorage>>,
    account: &CompressedPublicKey,
    balance: u64,
    nonce: u64,
) {
    let mut guard = storage.write().await;
    guard
        .set_last_nonce_to(account, 0, &VersionedNonce::new(nonce, Some(0)))
        .await
        .unwrap();
    guard
        .set_last_balance_to(
            account,
            &TOS_ASSET,
            0,
            &VersionedBalance::new(balance, Some(0)),
        )
        .await
        .unwrap();
    guard
        .set_account_registration_topoheight(account, 0)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_rocksdb_no_deadlock() {
    println!("\n=== TEST START: RocksDB Basic Integration Test ===");

    // Step 1: Create RocksDB storage
    println!("Step 1/5: Creating RocksDB storage...");
    let temp_dir = TempDir::new("tos_basic_test").unwrap();
    let dir_path = temp_dir.path().to_string_lossy().to_string();
    let config = RocksDBConfig::default();

    let mut storage = RocksStorage::new(&dir_path, Network::Devnet, Some(1024 * 1024), &config);
    register_tos_asset(&mut storage).await;
    let storage = Arc::new(RwLock::new(storage));
    println!("✓ RocksDB storage created");

    // Step 2: Setup accounts
    println!("Step 2/5: Setting up test accounts...");
    let alice = KeyPair::new();
    let bob = KeyPair::new();

    setup_account(
        &storage,
        &alice.get_public_key().compress(),
        100 * COIN_VALUE,
        0,
    )
    .await;
    setup_account(
        &storage,
        &bob.get_public_key().compress(),
        50 * COIN_VALUE,
        0,
    )
    .await;
    println!("✓ Accounts created");

    // Step 3: Verify balances
    println!("Step 3/5: Verifying account balances...");
    {
        let guard = storage.read().await;
        let alice_balance = guard
            .get_balance_at_exact_topoheight(&alice.get_public_key().compress(), &TOS_ASSET, 0)
            .await
            .unwrap();
        assert_eq!(alice_balance.get_balance(), 100 * COIN_VALUE);

        let bob_balance = guard
            .get_balance_at_exact_topoheight(&bob.get_public_key().compress(), &TOS_ASSET, 0)
            .await
            .unwrap();
        assert_eq!(bob_balance.get_balance(), 50 * COIN_VALUE);
    }
    println!("✓ Balances verified");

    // Step 4: Create ParallelChainState (this was causing deadlock with Sled!)
    println!("Step 4/5: Creating ParallelChainState...");
    let environment = Arc::new(Environment::new());

    // Create a minimal dummy block header
    let miner = KeyPair::new().get_public_key().compress();
    let dummy_header = BlockHeader::new(
        BlockVersion::Baseline,
        vec![],                  // parents_by_level
        0,                       // blue_score
        0,                       // daa_score
        0u64.into(),             // blue_work
        Hash::zero(),            // pruning_point
        0,                       // timestamp
        0,                       // bits
        [0u8; EXTRA_NONCE_SIZE], // extra_nonce
        miner,                   // miner
        Hash::zero(),            // hash_merkle_root
        Hash::zero(),            // accepted_id_merkle_root
        Hash::zero(),            // utxo_commitment
    );

    let dummy_block = Block::new(
        Immutable::Arc(Arc::new(dummy_header)),
        vec![], // transactions
    );

    let block_hash = Hash::zero();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0, // stable_topoheight (previous)
        1, // topoheight
        BlockVersion::Baseline,
        dummy_block,
        block_hash,
    )
    .await;
    println!("✓ ParallelChainState created (NO DEADLOCK!)");

    // Step 5: Commit state
    println!("Step 5/5: Committing state...");
    {
        let mut guard = storage.write().await;
        parallel_state.commit(&mut *guard).await.unwrap();
    }
    println!("✓ State committed successfully");

    println!("=== TEST COMPLETED SUCCESSFULLY ===\n");
}

#[tokio::test]
async fn test_rocksdb_concurrent_access() {
    println!("\n=== TEST START: RocksDB Concurrent Access Test ===");

    // Create storage
    let temp_dir = TempDir::new("tos_concurrent_test").unwrap();
    let dir_path = temp_dir.path().to_string_lossy().to_string();
    let config = RocksDBConfig::default();

    let mut storage = RocksStorage::new(&dir_path, Network::Devnet, Some(1024 * 1024), &config);
    register_tos_asset(&mut storage).await;
    let storage = Arc::new(RwLock::new(storage));

    // Create 10 accounts concurrently
    println!("Creating 10 accounts concurrently...");
    let mut handles = vec![];
    for i in 0..10 {
        let storage_clone = Arc::clone(&storage);
        let handle = tokio::spawn(async move {
            let keypair = KeyPair::new();
            setup_account(
                &storage_clone,
                &keypair.get_public_key().compress(),
                (i + 1) * 10 * COIN_VALUE,
                0,
            )
            .await;
            keypair
        });
        handles.push(handle);
    }

    let keypairs: Vec<KeyPair> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    println!("✓ 10 accounts created concurrently");

    // Verify all balances
    println!("Verifying all balances...");
    {
        let guard = storage.read().await;
        for (i, keypair) in keypairs.iter().enumerate() {
            let balance = guard
                .get_balance_at_exact_topoheight(
                    &keypair.get_public_key().compress(),
                    &TOS_ASSET,
                    0,
                )
                .await
                .unwrap();
            assert_eq!(balance.get_balance(), (i as u64 + 1) * 10 * COIN_VALUE);
        }
    }
    println!("✓ All balances verified");

    println!("=== TEST COMPLETED SUCCESSFULLY ===\n");
}
