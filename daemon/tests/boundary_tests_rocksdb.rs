//! Boundary Condition Tests for RocksDB Parallel Execution
//!
//! This test suite validates ParallelChainState behavior at system boundaries:
//! - Account balance transitions (0 → positive, positive → 0)
//! - Nonce overflow conditions
//! - Asset key variations
//! - State transitions between blocks
//! - Cache invalidation scenarios
//!
//! PURPOSE: Ensure correctness at critical boundary conditions
//!
//! APPROACH: Simplified tests following RocksDB migration strategy
//! - Test RocksDB storage operations + ParallelChainState
//! - Skip full transaction execution (not yet implemented)
//! - Focus on state transition correctness

use std::sync::Arc;
use tos_common::{
    config::{TOS_ASSET, COIN_VALUE},
    crypto::{KeyPair, Hashable, Hash},
    block::{Block, BlockVersion, EXTRA_NONCE_SIZE},
    immutable::Immutable,
};
use tos_daemon::core::{
    storage::{BalanceProvider, NonceProvider},
    state::parallel_chain_state::ParallelChainState,
};
use tos_environment::Environment;
use tos_testing_integration::utils::storage_helpers::{
    create_test_rocksdb_storage,
    setup_account_rocksdb,
};

/// Helper to create a dummy block for ParallelChainState
fn create_dummy_block() -> Block {
    use tos_common::block::BlockHeader;

    let miner = KeyPair::new().get_public_key().compress();
    let header = BlockHeader::new(
        BlockVersion::V0,
        vec![],  // parents_by_level
        0,       // blue_score
        0,       // daa_score
        0u64.into(),  // blue_work
        Hash::zero(),  // pruning_point
        0,       // timestamp
        0,       // bits
        [0u8; EXTRA_NONCE_SIZE],  // extra_nonce
        miner,   // miner
        Hash::zero(),  // hash_merkle_root
        Hash::zero(),  // accepted_id_merkle_root
        Hash::zero(),  // utxo_commitment
    );

    Block::new(Immutable::Arc(Arc::new(header)), vec![])
}

// ============================================================================
// BOUNDARY TEST #1: Zero Balance Transition (Positive → Zero)
// ============================================================================

#[tokio::test]
async fn test_balance_transition_to_zero() {
    println!("\n=== BOUNDARY TEST #1: Balance Transition to Zero ===");
    println!("Testing: Account balance goes from positive to exactly zero");

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Setup account with exact balance we'll spend
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    let initial_balance = 100 * COIN_VALUE;
    setup_account_rocksdb(&storage, &alice_pubkey, initial_balance, 0)
        .await
        .expect("Failed to setup Alice");

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

    // Step 4: Load balance from storage (lazy loading)
    parallel_state.ensure_balance_loaded(&alice_pubkey, &TOS_ASSET).await.expect("Failed to load balance");

    // Step 4b: Reduce balance to exactly zero
    parallel_state
        .set_balance(&alice_pubkey, &TOS_ASSET, 0);

    let final_balance = parallel_state.get_balance(&alice_pubkey, &TOS_ASSET);
    assert_eq!(final_balance, 0, "Balance should be exactly zero");

    // Step 5: Commit and verify
    {
        let mut storage_write = storage.write().await;
        parallel_state.commit(&mut *storage_write).await.expect("Failed to commit");
    }

    // Step 6: Verify persisted zero balance
    {
        let storage_read = storage.read().await;
        let (_, balance) = storage_read
            .get_last_balance(&alice_pubkey, &TOS_ASSET)
            .await
            .expect("Failed to get balance");
        assert_eq!(balance.get_balance(), 0, "Persisted balance should be zero");
    }

    println!("✓ Test passed: Balance transition to zero handled correctly");
}

// ============================================================================
// BOUNDARY TEST #2: Zero Balance Transition (Zero → Positive)
// ============================================================================

#[tokio::test]
async fn test_balance_transition_from_zero() {
    println!("\n=== BOUNDARY TEST #2: Balance Transition from Zero ===");
    println!("Testing: Account balance goes from zero to positive (receiving funds)");

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Setup empty account
    let bob = KeyPair::new();
    let bob_pubkey = bob.get_public_key().compress();

    setup_account_rocksdb(&storage, &bob_pubkey, 0, 0)
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

    // Step 4: Load balance from storage (even if zero, need to initialize)
    parallel_state.ensure_balance_loaded(&bob_pubkey, &TOS_ASSET).await.expect("Failed to load balance");

    // Step 4b: Add funds to zero balance account
    let new_balance = 500 * COIN_VALUE;
    parallel_state
        .set_balance(&bob_pubkey, &TOS_ASSET, new_balance);

    let final_balance = parallel_state.get_balance(&bob_pubkey, &TOS_ASSET);
    assert_eq!(final_balance, new_balance, "Balance should be updated");

    // Step 5: Commit and verify
    {
        let mut storage_write = storage.write().await;
        parallel_state.commit(&mut *storage_write).await.expect("Failed to commit");
    }

    // Step 6: Verify persisted positive balance
    {
        let storage_read = storage.read().await;
        let (_, balance) = storage_read
            .get_last_balance(&bob_pubkey, &TOS_ASSET)
            .await
            .expect("Failed to get balance");
        assert_eq!(
            balance.get_balance(),
            new_balance,
            "Persisted balance should be positive"
        );
    }

    println!("✓ Test passed: Balance transition from zero handled correctly");
}

// ============================================================================
// BOUNDARY TEST #3: Nonce Boundary (Sequential Increments)
// ============================================================================

#[tokio::test]
async fn test_nonce_sequential_boundary() {
    println!("\n=== BOUNDARY TEST #3: Nonce Sequential Boundary ===");
    println!("Testing: Sequential nonce increments across multiple blocks");

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Setup account
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    setup_account_rocksdb(&storage, &alice_pubkey, 1000 * COIN_VALUE, 0)
        .await
        .expect("Failed to setup Alice");

    // Step 3: Simulate 10 sequential blocks with nonce increments
    for i in 0..10 {
        let dummy_block = create_dummy_block();
        let block_hash = dummy_block.hash();

        let parallel_state = ParallelChainState::new(
            Arc::clone(&storage),
            Arc::clone(&environment),
            i,        // stable_topoheight
            i + 1,    // topoheight
            BlockVersion::V0,
            dummy_block,
            block_hash,
        )
        .await;

        // Load account from storage (lazy loading)
        parallel_state.ensure_account_loaded(&alice_pubkey).await.expect("Failed to load account");

        // Increment nonce
        let current_nonce = parallel_state.get_nonce(&alice_pubkey);
        assert_eq!(current_nonce, i, "Nonce should be {}", i);

        parallel_state.set_nonce(&alice_pubkey, i + 1);

        // Commit
        {
            let mut storage_write = storage.write().await;
            parallel_state
                .commit(&mut *storage_write)
                .await
                .expect("Failed to commit");
        }
    }

    // Step 4: Verify final nonce
    {
        let storage_read = storage.read().await;
        let (_, nonce) = storage_read
            .get_last_nonce(&alice_pubkey)
            .await
            .expect("Failed to get nonce");
        assert_eq!(nonce.get_nonce(), 10, "Final nonce should be 10");
    }

    println!("✓ Test passed: Sequential nonce boundaries handled correctly");
}

// ============================================================================
// BOUNDARY TEST #4: Multiple Asset Types
// ============================================================================

#[tokio::test]
async fn test_multiple_asset_types() {
    println!("\n=== BOUNDARY TEST #4: Multiple Asset Types ===");
    println!("Testing: Account with balances in multiple asset types");

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Setup account with TOS asset
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    setup_account_rocksdb(&storage, &alice_pubkey, 1000 * COIN_VALUE, 0)
        .await
        .expect("Failed to setup Alice");

    // Step 3: Create custom asset (using Hash::new)
    use tos_common::crypto::Hash;
    let custom_asset_hash = Hash::new([1u8; 32]);

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

    // Step 5: Load TOS balance from storage (lazy loading)
    parallel_state.ensure_balance_loaded(&alice_pubkey, &TOS_ASSET).await.expect("Failed to load balance");

    // Step 5b: Verify TOS asset
    let tos_balance = parallel_state.get_balance(&alice_pubkey, &TOS_ASSET);
    assert_eq!(tos_balance, 1000 * COIN_VALUE, "TOS balance should be correct");

    // Step 6: Load custom asset (should be 0)
    let custom_balance = parallel_state.get_balance(&alice_pubkey, &custom_asset_hash);
    assert_eq!(custom_balance, 0, "Custom asset balance should be zero");

    // Step 7: Set custom asset balance
    parallel_state
        .set_balance(&alice_pubkey, &custom_asset_hash, 500);

    let updated_custom = parallel_state.get_balance(&alice_pubkey, &custom_asset_hash);
    assert_eq!(updated_custom, 500, "Custom asset balance should be updated");

    // Step 8: Verify TOS balance unchanged
    let tos_balance_after = parallel_state.get_balance(&alice_pubkey, &TOS_ASSET);
    assert_eq!(
        tos_balance_after, 1000 * COIN_VALUE,
        "TOS balance should remain unchanged"
    );

    // Step 9: Verify in-memory state correctness
    // NOTE: We don't commit here because the custom asset isn't registered in storage.
    // This test validates that ParallelChainState correctly handles multiple asset types
    // in memory, which is the key functionality being tested.

    println!("✓ Test passed: Multiple asset types handled correctly");
}

// ============================================================================
// BOUNDARY TEST #5: Cache Invalidation on State Reload
// ============================================================================

#[tokio::test]
async fn test_cache_invalidation_on_reload() {
    println!("\n=== BOUNDARY TEST #5: Cache Invalidation on Reload ===");
    println!("Testing: State reload correctly invalidates cached values");

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Setup account
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    setup_account_rocksdb(&storage, &alice_pubkey, 1000 * COIN_VALUE, 0)
        .await
        .expect("Failed to setup Alice");

    // Step 3: Create first ParallelChainState and modify
    {
        let dummy_block = create_dummy_block();
        let block_hash = dummy_block.hash();

        let parallel_state1 = ParallelChainState::new(
            Arc::clone(&storage),
            Arc::clone(&environment),
            0,
            1,
            BlockVersion::V0,
            dummy_block,
            block_hash,
        )
        .await;

        // Load balance from storage (lazy loading)
        parallel_state1.ensure_balance_loaded(&alice_pubkey, &TOS_ASSET).await.expect("Failed to load balance");

        parallel_state1
            .set_balance(&alice_pubkey, &TOS_ASSET, 500 * COIN_VALUE);

        // Commit
        let mut storage_write = storage.write().await;
        parallel_state1
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }

    // Step 4: Create second ParallelChainState and verify updated value
    {
        let dummy_block = create_dummy_block();
        let block_hash = dummy_block.hash();

        let parallel_state2 = ParallelChainState::new(
            Arc::clone(&storage),
            environment,
            1,
            2,
            BlockVersion::V0,
            dummy_block,
            block_hash,
        )
        .await;

        // Load balance from storage (lazy loading)
        parallel_state2.ensure_balance_loaded(&alice_pubkey, &TOS_ASSET).await.expect("Failed to load balance");

        let balance = parallel_state2.get_balance(&alice_pubkey, &TOS_ASSET);
        assert_eq!(
            balance,
            500 * COIN_VALUE,
            "New state should load updated balance, not cached old value"
        );

        // Commit
        let mut storage_write = storage.write().await;
        parallel_state2
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }

    println!("✓ Test passed: Cache invalidation handled correctly");
}

// ============================================================================
// BOUNDARY TEST #6: Same Account Multiple Operations
// ============================================================================

#[tokio::test]
async fn test_same_account_multiple_operations() {
    println!("\n=== BOUNDARY TEST #6: Same Account Multiple Operations ===");
    println!("Testing: Multiple sequential operations on same account in one block");

    // Step 1: Create RocksDB storage
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    // Step 2: Setup account
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    setup_account_rocksdb(&storage, &alice_pubkey, 1000 * COIN_VALUE, 0)
        .await
        .expect("Failed to setup Alice");

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

    // Step 4: Load balance and account from storage (lazy loading)
    parallel_state.ensure_balance_loaded(&alice_pubkey, &TOS_ASSET).await.expect("Failed to load balance");
    parallel_state.ensure_account_loaded(&alice_pubkey).await.expect("Failed to load account");

    // Step 5: Perform multiple operations on same account

    // Operation 1: Deduct 100 TOS
    let balance1 = parallel_state.get_balance(&alice_pubkey, &TOS_ASSET);
    parallel_state
        .set_balance(&alice_pubkey, &TOS_ASSET, balance1 - 100 * COIN_VALUE);

    // Operation 2: Deduct another 200 TOS
    let balance2 = parallel_state.get_balance(&alice_pubkey, &TOS_ASSET);
    parallel_state
        .set_balance(&alice_pubkey, &TOS_ASSET, balance2 - 200 * COIN_VALUE);

    // Operation 3: Increment nonce twice
    let nonce1 = parallel_state.get_nonce(&alice_pubkey);
    parallel_state.set_nonce(&alice_pubkey, nonce1 + 1);

    let nonce2 = parallel_state.get_nonce(&alice_pubkey);
    parallel_state.set_nonce(&alice_pubkey, nonce2 + 1);

    // Step 6: Verify final state
    let final_balance = parallel_state.get_balance(&alice_pubkey, &TOS_ASSET);
    let final_nonce = parallel_state.get_nonce(&alice_pubkey);

    assert_eq!(
        final_balance,
        700 * COIN_VALUE,
        "Balance should reflect both deductions"
    );
    assert_eq!(final_nonce, 2, "Nonce should be incremented twice");

    // Step 7: Commit and verify
    {
        let mut storage_write = storage.write().await;
        parallel_state.commit(&mut *storage_write).await.expect("Failed to commit");
    }

    println!("✓ Test passed: Multiple operations on same account handled correctly");
}
