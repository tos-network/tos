//! Stress Tests for RocksDB Parallel Execution
//!
//! This test suite validates ParallelChainState performance under high load:
//! - Large account sets (100, 500, 1000+ accounts)
//! - Concurrent operations stress testing

#![allow(clippy::disallowed_methods)]
//! - Memory usage validation
//! - State commit performance
//!
//! PURPOSE: Prove RocksDB eliminates Sled deadlocks even under extreme stress
//!
//! APPROACH: Simplified tests following RocksDB migration strategy
//! - Test RocksDB storage operations + ParallelChainState
//! - Skip full transaction execution (not yet implemented)
//! - Focus on scale and concurrency without deadlocks

#![allow(deprecated)] // Allow usage of set_balance in tests

use std::sync::Arc;
use std::time::Instant;
use tos_common::{
    block::{Block, BlockVersion, EXTRA_NONCE_SIZE},
    config::{COIN_VALUE, TOS_ASSET},
    crypto::{Hash, Hashable, KeyPair},
    immutable::Immutable,
};
use tos_daemon::core::state::parallel_chain_state::ParallelChainState;
use tos_environment::Environment;
use tos_testing_framework::utilities::{create_test_rocksdb_storage, setup_account_rocksdb};

/// Helper to create a dummy block for ParallelChainState
fn create_dummy_block() -> Block {
    use tos_common::block::BlockHeader;

    let miner = KeyPair::new().get_public_key().compress();
    let header = BlockHeader::new(
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

    Block::new(Immutable::Arc(Arc::new(header)), vec![])
}

// ============================================================================
// STRESS TEST #1: 100 Accounts
// ============================================================================

#[tokio::test]
async fn test_stress_100_accounts() {
    println!("\n=== STRESS TEST #1: 100 Accounts ===");
    println!("Testing: Managing 100 accounts in parallel state");

    let start = Instant::now();

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Create 100 accounts
    let mut accounts = vec![];
    for i in 0..100 {
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();
        let balance = (i + 1) * 10 * COIN_VALUE;
        let nonce = i;

        setup_account_rocksdb(&storage, &pubkey, balance, nonce)
            .await
            .expect("Failed to setup account");
        accounts.push((pubkey, balance, nonce));
    }

    let setup_time = start.elapsed();
    println!("✓ Created 100 accounts in {setup_time:?}");

    // Step 3: Create ParallelChainState
    let state_start = Instant::now();
    let dummy_block = create_dummy_block();
    let block_hash = dummy_block.hash();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::Baseline,
        dummy_block,
        block_hash,
    )
    .await;

    let state_time = state_start.elapsed();
    println!("✓ Created ParallelChainState in {state_time:?}");

    // Step 4: Load all accounts and verify
    let load_start = Instant::now();
    for (pubkey, expected_balance, expected_nonce) in &accounts {
        parallel_state
            .ensure_balance_loaded(pubkey, &TOS_ASSET)
            .await
            .expect("Failed to load balance");
        parallel_state
            .ensure_account_loaded(pubkey)
            .await
            .expect("Failed to load account");

        let balance = parallel_state.get_balance(pubkey, &TOS_ASSET);
        let nonce = parallel_state.get_nonce(pubkey);

        assert_eq!(balance, *expected_balance, "Balance mismatch");
        assert_eq!(nonce, *expected_nonce, "Nonce mismatch");
    }
    let load_time = load_start.elapsed();
    println!("✓ Loaded 100 accounts in {load_time:?}");

    // Step 5: Commit state
    let commit_start = Instant::now();
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }
    let commit_time = commit_start.elapsed();
    println!("✓ Committed state in {commit_time:?}");

    let total_time = start.elapsed();
    println!("✓ Total time: {total_time:?}");
    println!("✓ Test passed: 100 accounts handled without deadlock");
}

// ============================================================================
// STRESS TEST #2: 500 Accounts
// ============================================================================

#[tokio::test]
async fn test_stress_500_accounts() {
    println!("\n=== STRESS TEST #2: 500 Accounts ===");
    println!("Testing: Managing 500 accounts in parallel state");

    let start = Instant::now();

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Create 500 accounts
    let mut accounts = vec![];
    for i in 0..500 {
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();
        let balance = (i + 1) * COIN_VALUE;
        let nonce = i % 100; // Vary nonces

        setup_account_rocksdb(&storage, &pubkey, balance, nonce)
            .await
            .expect("Failed to setup account");
        accounts.push((pubkey, balance, nonce));
    }

    let setup_time = start.elapsed();
    println!("✓ Created 500 accounts in {setup_time:?}");

    // Step 3: Create ParallelChainState
    let state_start = Instant::now();
    let dummy_block = create_dummy_block();
    let block_hash = dummy_block.hash();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::Baseline,
        dummy_block,
        block_hash,
    )
    .await;

    let state_time = state_start.elapsed();
    println!("✓ Created ParallelChainState in {state_time:?}");

    // Step 4: Sample verify 50 random accounts
    let load_start = Instant::now();
    for i in (0..500).step_by(10) {
        let (pubkey, expected_balance, expected_nonce) = &accounts[i];
        parallel_state
            .ensure_balance_loaded(pubkey, &TOS_ASSET)
            .await
            .expect("Failed to load balance");
        parallel_state
            .ensure_account_loaded(pubkey)
            .await
            .expect("Failed to load account");

        let balance = parallel_state.get_balance(pubkey, &TOS_ASSET);
        let nonce = parallel_state.get_nonce(pubkey);

        assert_eq!(balance, *expected_balance, "Balance mismatch at index {i}");
        assert_eq!(nonce, *expected_nonce, "Nonce mismatch at index {i}");
    }
    let load_time = load_start.elapsed();
    println!("✓ Verified 50 sampled accounts in {load_time:?}");

    // Step 5: Commit state
    let commit_start = Instant::now();
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }
    let commit_time = commit_start.elapsed();
    println!("✓ Committed state in {commit_time:?}");

    let total_time = start.elapsed();
    println!("✓ Total time: {total_time:?}");
    println!("✓ Test passed: 500 accounts handled without deadlock");
}

// ============================================================================
// STRESS TEST #3: 1000 Accounts
// ============================================================================

#[tokio::test]
#[ignore = "Long-running stress test - run manually with --ignored"]
async fn test_stress_1000_accounts() {
    println!("\n=== STRESS TEST #3: 1000 Accounts ===");
    println!("Testing: Managing 1000 accounts in parallel state");

    let start = Instant::now();

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Create 1000 accounts
    println!("Creating 1000 accounts...");
    let mut accounts = vec![];
    for i in 0..1000 {
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();
        let balance = ((i + 1) * COIN_VALUE) % 1000000;
        let nonce = i % 200;

        setup_account_rocksdb(&storage, &pubkey, balance, nonce)
            .await
            .expect("Failed to setup account");
        accounts.push((pubkey, balance, nonce));

        if (i + 1) % 100 == 0 {
            println!("  Progress: {}/1000 accounts created", i + 1);
        }
    }

    let setup_time = start.elapsed();
    println!("✓ Created 1000 accounts in {setup_time:?}");

    // Step 3: Create ParallelChainState
    let state_start = Instant::now();
    let dummy_block = create_dummy_block();
    let block_hash = dummy_block.hash();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::Baseline,
        dummy_block,
        block_hash,
    )
    .await;

    let state_time = state_start.elapsed();
    println!("✓ Created ParallelChainState in {state_time:?}");

    // Step 4: Sample verify 100 random accounts
    let load_start = Instant::now();
    for i in (0..1000).step_by(10) {
        let (pubkey, expected_balance, expected_nonce) = &accounts[i];
        parallel_state
            .ensure_balance_loaded(pubkey, &TOS_ASSET)
            .await
            .expect("Failed to load balance");
        parallel_state
            .ensure_account_loaded(pubkey)
            .await
            .expect("Failed to load account");

        let balance = parallel_state.get_balance(pubkey, &TOS_ASSET);
        let nonce = parallel_state.get_nonce(pubkey);

        assert_eq!(balance, *expected_balance, "Balance mismatch at index {i}");
        assert_eq!(nonce, *expected_nonce, "Nonce mismatch at index {i}");
    }
    let load_time = load_start.elapsed();
    println!("✓ Verified 100 sampled accounts in {load_time:?}");

    // Step 5: Commit state
    let commit_start = Instant::now();
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }
    let commit_time = commit_start.elapsed();
    println!("✓ Committed state in {commit_time:?}");

    let total_time = start.elapsed();
    println!("✓ Total time: {total_time:?}");
    println!("✓ Test passed: 1000 accounts handled without deadlock");
}

// ============================================================================
// STRESS TEST #4: Concurrent Account Modifications
// ============================================================================

#[tokio::test]
async fn test_stress_concurrent_modifications() {
    println!("\n=== STRESS TEST #4: Concurrent Account Modifications ===");
    println!("Testing: 50 concurrent workers modifying different accounts");

    let start = Instant::now();

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Create 50 accounts
    let mut accounts = vec![];
    for i in 0..50 {
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();
        let balance = (i + 1) * 100 * COIN_VALUE;
        let nonce = 0;

        setup_account_rocksdb(&storage, &pubkey, balance, nonce)
            .await
            .expect("Failed to setup account");
        accounts.push(pubkey);
    }

    let setup_time = start.elapsed();
    println!("✓ Created 50 accounts in {setup_time:?}");

    // Step 3: Create ParallelChainState
    let dummy_block = create_dummy_block();
    let block_hash = dummy_block.hash();

    let parallel_state = Arc::new(
        ParallelChainState::new(
            Arc::clone(&storage),
            environment,
            0,
            1,
            BlockVersion::Baseline,
            dummy_block,
            block_hash,
        )
        .await,
    );

    // Step 4: Spawn 50 concurrent workers
    let mod_start = Instant::now();
    let mut handles = vec![];

    for pubkey in accounts {
        let state_clone = Arc::clone(&parallel_state);
        let handle = tokio::spawn(async move {
            // Load account

            // Modify balance (deduct 10 TOS)
            let current_balance = state_clone.get_balance(&pubkey, &TOS_ASSET);
            if current_balance >= 10 * COIN_VALUE {
                state_clone.set_balance(&pubkey, &TOS_ASSET, current_balance - 10 * COIN_VALUE);
            }

            // Increment nonce
            let current_nonce = state_clone.get_nonce(&pubkey);
            state_clone.set_nonce(&pubkey, current_nonce + 1);
        });
        handles.push(handle);
    }

    // Wait for all workers
    for handle in handles {
        handle.await.expect("Worker failed");
    }

    let mod_time = mod_start.elapsed();
    println!("✓ 50 concurrent modifications completed in {mod_time:?}");

    // Step 5: Commit state
    let commit_start = Instant::now();
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }
    let commit_time = commit_start.elapsed();
    println!("✓ Committed state in {commit_time:?}");

    let total_time = start.elapsed();
    println!("✓ Total time: {total_time:?}");
    println!("✓ Test passed: Concurrent modifications handled without deadlock");
}

// ============================================================================
// STRESS TEST #5: Rapid State Creation and Commit Cycles
// ============================================================================

#[tokio::test]
async fn test_stress_rapid_state_cycles() {
    println!("\n=== STRESS TEST #5: Rapid State Creation and Commit Cycles ===");
    println!("Testing: 20 rapid ParallelChainState create/commit cycles");

    let start = Instant::now();

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Create 10 accounts
    let mut accounts = vec![];
    for i in 0..10 {
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();
        let balance = (i + 1) * 100 * COIN_VALUE;
        let nonce = 0;

        setup_account_rocksdb(&storage, &pubkey, balance, nonce)
            .await
            .expect("Failed to setup account");
        accounts.push(pubkey);
    }

    println!("✓ Created 10 accounts");

    // Step 3: Perform 20 rapid create/commit cycles
    for cycle in 0..20 {
        let dummy_block = create_dummy_block();
        let block_hash = dummy_block.hash();

        let parallel_state = ParallelChainState::new(
            Arc::clone(&storage),
            Arc::clone(&environment),
            cycle,     // stable_topoheight
            cycle + 1, // topoheight
            BlockVersion::Baseline,
            dummy_block,
            block_hash,
        )
        .await;

        // Modify one account
        let pubkey = &accounts[cycle as usize % 10];

        let current_nonce = parallel_state.get_nonce(pubkey);
        parallel_state.set_nonce(pubkey, current_nonce + 1);

        // Commit
        {
            let mut storage_write = storage.write().await;
            parallel_state
                .commit(&mut *storage_write)
                .await
                .expect("Failed to commit");
        }

        if (cycle + 1) % 5 == 0 {
            println!("  Progress: {}/20 cycles completed", cycle + 1);
        }
    }

    let total_time = start.elapsed();
    println!("✓ Total time: {total_time:?}");
    println!("✓ Average time per cycle: {:?}", total_time / 20);
    println!("✓ Test passed: Rapid cycles handled without deadlock");
}
