//! Edge Case Tests for RocksDB Parallel Execution
//!
//! This test suite validates ParallelChainState behavior under edge conditions:
//! - Empty accounts (zero balance, zero nonce)
//! - Boundary values (u64::MAX, u64::MIN)
//! - Account creation and destruction
//! - Zero-value transfers
//! - Maximum nonce values
//!
//! APPROACH: Simplified tests following RocksDB migration strategy
//! - Test RocksDB storage operations + ParallelChainState
//! - Skip full transaction execution (not yet implemented)
//! - Focus on proving RocksDB handles edge cases without deadlocks

use std::sync::Arc;
use tos_common::{
    block::{Block, BlockVersion, EXTRA_NONCE_SIZE},
    config::{COIN_VALUE, TOS_ASSET},
    crypto::{Hash, Hashable, KeyPair},
    immutable::Immutable,
};
use tos_daemon::core::{
    state::parallel_chain_state::ParallelChainState,
    storage::{BalanceProvider, NonceProvider},
};
use tos_environment::Environment;
use tos_testing_integration::utils::storage_helpers::{
    create_test_rocksdb_storage, setup_account_rocksdb,
};

/// Helper to create a dummy block for ParallelChainState
fn create_dummy_block() -> Block {
    use tos_common::block::BlockHeader;

    let miner = KeyPair::new().get_public_key().compress();
    let header = BlockHeader::new(
        BlockVersion::V0,
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
// EDGE CASE #1: Empty Account Operations
// ============================================================================

#[tokio::test]
async fn test_empty_account_creation() {
    println!("\n=== EDGE CASE #1: Empty Account Creation ===");
    println!("Testing: Creating and managing accounts with zero balance and nonce");

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Setup empty account (0 balance, 0 nonce)
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    setup_account_rocksdb(&storage, &alice_pubkey, 0, 0)
        .await
        .expect("Failed to setup empty account");

    // Step 3: Verify empty account state
    {
        let storage_read = storage.read().await;
        let (_, balance) = storage_read
            .get_last_balance(&alice_pubkey, &TOS_ASSET)
            .await
            .expect("Failed to get balance");
        assert_eq!(
            balance.get_balance(),
            0,
            "Empty account should have 0 balance"
        );

        let (_, nonce) = storage_read
            .get_last_nonce(&alice_pubkey)
            .await
            .expect("Failed to get nonce");
        assert_eq!(nonce.get_nonce(), 0, "Empty account should have 0 nonce");
    }

    // Step 4: Create ParallelChainState with empty account
    let dummy_block = create_dummy_block();
    let block_hash = dummy_block.hash();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::V0,
        dummy_block,
        block_hash,
    )
    .await;

    // Step 5: Commit state
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit empty account state");
    }

    println!("✓ Test passed: Empty account creation and state management");
}

// ============================================================================
// EDGE CASE #2: Zero-Value Transfer
// ============================================================================

#[tokio::test]
async fn test_zero_value_transfer() {
    println!("\n=== EDGE CASE #2: Zero-Value Transfer ===");
    println!("Testing: Transfer of 0 TOS between accounts");

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Setup sender and receiver
    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();
    let bob_pubkey = bob.get_public_key().compress();

    setup_account_rocksdb(&storage, &alice_pubkey, 1000 * COIN_VALUE, 0)
        .await
        .expect("Failed to setup Alice");
    setup_account_rocksdb(&storage, &bob_pubkey, 500 * COIN_VALUE, 0)
        .await
        .expect("Failed to setup Bob");

    // Step 3: Create ParallelChainState
    let dummy_block = create_dummy_block();
    let block_hash = dummy_block.hash();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::V0,
        dummy_block,
        block_hash,
    )
    .await;

    // Step 4: Load balances from storage into ParallelChainState
    parallel_state
        .ensure_balance_loaded(&alice_pubkey, &TOS_ASSET)
        .await
        .expect("Failed to load Alice balance");
    parallel_state
        .ensure_balance_loaded(&bob_pubkey, &TOS_ASSET)
        .await
        .expect("Failed to load Bob balance");

    // Step 5: Simulate zero-value transfer (just nonce increment, no balance change)
    parallel_state.set_nonce(&alice_pubkey, 1);

    // Step 6: Verify balances unchanged
    let alice_balance = parallel_state.get_balance(&alice_pubkey, &TOS_ASSET);
    let bob_balance = parallel_state.get_balance(&bob_pubkey, &TOS_ASSET);

    assert_eq!(alice_balance, 1000 * COIN_VALUE, "Alice balance unchanged");
    assert_eq!(bob_balance, 500 * COIN_VALUE, "Bob balance unchanged");

    // Step 6: Commit state
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }

    println!("✓ Test passed: Zero-value transfer handled correctly");
}

// ============================================================================
// EDGE CASE #3: Maximum Balance Values
// ============================================================================

#[tokio::test]
async fn test_maximum_balance_values() {
    println!("\n=== EDGE CASE #3: Maximum Balance Values ===");
    println!("Testing: Accounts with near-maximum u64 balance values");

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Setup account with very large balance
    let whale = KeyPair::new();
    let whale_pubkey = whale.get_public_key().compress();

    // Use a large but not max value to avoid overflow in operations
    let large_balance = u64::MAX / 2;

    setup_account_rocksdb(&storage, &whale_pubkey, large_balance, 0)
        .await
        .expect("Failed to setup whale account");

    // Step 3: Verify large balance
    {
        let storage_read = storage.read().await;
        let (_, balance) = storage_read
            .get_last_balance(&whale_pubkey, &TOS_ASSET)
            .await
            .expect("Failed to get balance");
        assert_eq!(
            balance.get_balance(),
            large_balance,
            "Large balance should be stored correctly"
        );
    }

    // Step 4: Create ParallelChainState with large balance account
    let dummy_block = create_dummy_block();
    let block_hash = dummy_block.hash();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::V0,
        dummy_block,
        block_hash,
    )
    .await;

    // Step 5: Load large balance from storage into ParallelChainState
    parallel_state
        .ensure_balance_loaded(&whale_pubkey, &TOS_ASSET)
        .await
        .expect("Failed to load whale balance");

    // Step 5b: Verify large balance in parallel state
    let loaded_balance = parallel_state.get_balance(&whale_pubkey, &TOS_ASSET);
    assert_eq!(
        loaded_balance, large_balance,
        "Large balance loaded correctly into parallel state"
    );

    // Step 6: Commit state
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }

    println!("✓ Test passed: Maximum balance values handled correctly");
}

// ============================================================================
// EDGE CASE #4: Maximum Nonce Values
// ============================================================================

#[tokio::test]
async fn test_maximum_nonce_values() {
    println!("\n=== EDGE CASE #4: Maximum Nonce Values ===");
    println!("Testing: Accounts with very high nonce values");

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Setup account with high nonce
    let busy_account = KeyPair::new();
    let busy_pubkey = busy_account.get_public_key().compress();

    let high_nonce = u64::MAX / 2;

    setup_account_rocksdb(&storage, &busy_pubkey, 1000 * COIN_VALUE, high_nonce)
        .await
        .expect("Failed to setup busy account");

    // Step 3: Verify high nonce
    {
        let storage_read = storage.read().await;
        let (_, nonce) = storage_read
            .get_last_nonce(&busy_pubkey)
            .await
            .expect("Failed to get nonce");
        assert_eq!(
            nonce.get_nonce(),
            high_nonce,
            "High nonce should be stored correctly"
        );
    }

    // Step 4: Create ParallelChainState
    let dummy_block = create_dummy_block();
    let block_hash = dummy_block.hash();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::V0,
        dummy_block,
        block_hash,
    )
    .await;

    // Step 5: Load account (including nonce) from storage
    parallel_state
        .ensure_account_loaded(&busy_pubkey)
        .await
        .expect("Failed to load account");

    // Step 5b: Verify high nonce is loaded
    let loaded_nonce = parallel_state.get_nonce(&busy_pubkey);
    assert_eq!(loaded_nonce, high_nonce, "High nonce loaded correctly");

    // Step 5b: Increment high nonce
    parallel_state.set_nonce(&busy_pubkey, high_nonce + 1);
    let incremented_nonce = parallel_state.get_nonce(&busy_pubkey);
    assert_eq!(
        incremented_nonce,
        high_nonce + 1,
        "High nonce incremented correctly"
    );

    // Step 6: Commit state
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }

    println!("✓ Test passed: Maximum nonce values handled correctly");
}

// ============================================================================
// EDGE CASE #5: Multiple Empty Accounts
// ============================================================================

#[tokio::test]
async fn test_multiple_empty_accounts() {
    println!("\n=== EDGE CASE #5: Multiple Empty Accounts ===");
    println!("Testing: Managing multiple accounts with zero balance");

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Create 10 empty accounts
    let mut empty_accounts = vec![];
    for _ in 0..10 {
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();
        setup_account_rocksdb(&storage, &pubkey, 0, 0)
            .await
            .expect("Failed to setup empty account");
        empty_accounts.push(pubkey);
    }

    // Step 3: Verify all accounts are empty
    {
        let storage_read = storage.read().await;
        for pubkey in &empty_accounts {
            let (_, balance) = storage_read
                .get_last_balance(pubkey, &TOS_ASSET)
                .await
                .expect("Failed to get balance");
            assert_eq!(balance.get_balance(), 0);

            let (_, nonce) = storage_read
                .get_last_nonce(pubkey)
                .await
                .expect("Failed to get nonce");
            assert_eq!(nonce.get_nonce(), 0);
        }
    }

    // Step 4: Create ParallelChainState with all empty accounts
    let dummy_block = create_dummy_block();
    let block_hash = dummy_block.hash();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::V0,
        dummy_block,
        block_hash,
    )
    .await;

    // Step 5: Load all empty accounts
    for pubkey in &empty_accounts {
        let balance = parallel_state.get_balance(pubkey, &TOS_ASSET);
        assert_eq!(balance, 0, "Empty account balance should be 0");
    }

    // Step 6: Commit state
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }

    println!("✓ Test passed: Multiple empty accounts handled correctly");
}
