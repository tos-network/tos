//! Nonce Management Integration Tests for Parallel Execution (RocksDB)
//!
//! This test suite validates nonce management in parallel transaction execution,
//! ensuring nonces are properly tracked, incremented, and rolled back on failures.
//!
//! Test Coverage:
//! 1. Nonce Increments Correctly - Verifies nonces increment after successful transactions
//! 2. Staged Nonces (No Commit Until Success) - Ensures failed transactions don't modify nonces
//! 3. Nonce Rollback on Transaction Failure - Tests automatic rollback when transactions fail
//! 4. Concurrent Nonce Updates - Validates safe concurrent nonce modifications
//! 5. Nonce Ordering Preservation - Ensures nonce ordering is maintained across operations
//!
//! Design Philosophy:
//! - Uses RocksDB (production backend) instead of Sled
//! - Direct method calls on ParallelChainState (no full transaction execution)
//! - Fast tests (< 1 second each)
//! - Comprehensive print statements for debugging with --nocapture

use std::sync::Arc;
use tos_common::{
    block::{Block, BlockVersion, EXTRA_NONCE_SIZE},
    config::COIN_VALUE,
    crypto::{Hash, Hashable, KeyPair},
    immutable::Immutable,
};
use tos_daemon::core::{state::parallel_chain_state::ParallelChainState, storage::NonceProvider};
use tos_environment::Environment;
use tos_testing_integration::utils::storage_helpers::{
    create_test_rocksdb_storage, setup_account_rocksdb,
};

/// Helper to create a dummy block for testing
fn create_dummy_block(miner_key: &KeyPair) -> Block {
    use tos_common::block::BlockHeader;

    let miner = miner_key.get_public_key().compress();
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
// TEST 1: Nonce Increments Correctly After Transaction
// ============================================================================
/// Validates that nonces increment correctly after successful operations.
///
/// Scenario:
/// 1. Alice starts with nonce = 0
/// 2. Load account into ParallelChainState
/// 3. Manually increment nonce (simulating successful transaction)
/// 4. Verify nonce is tracked as modified
/// 5. Commit state to storage
/// 6. Verify nonce persisted correctly (nonce = 1)
///
/// This test validates:
/// - Nonce loading from storage
/// - Nonce increment tracking in memory
/// - Modification detection (original vs current)
/// - Nonce persistence to storage
#[tokio::test]
async fn test_nonce_increments_correctly() {
    println!("\n=== TEST 1: Nonce Increments Correctly ===");

    // Step 1: Setup storage with Alice account
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    // Alice starts with nonce = 0, balance = 1000 TOS
    setup_account_rocksdb(&storage, &alice_pubkey, 1000 * COIN_VALUE, 0)
        .await
        .expect("Failed to setup Alice account");

    println!("✓ Step 1: Alice initialized with nonce = 0, balance = 1000 TOS");

    // Step 2: Create ParallelChainState
    let block = create_dummy_block(&alice);
    let block_hash = block.hash();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0, // stable_topoheight
        1, // topoheight
        BlockVersion::V0,
        block,
        block_hash,
    )
    .await;

    println!("✓ Step 2: ParallelChainState created at topoheight 1");

    // Step 3: Load Alice's account (this loads nonce from storage)
    parallel_state
        .ensure_account_loaded(&alice_pubkey)
        .await
        .expect("Failed to load Alice account");

    let initial_nonce = parallel_state.get_nonce(&alice_pubkey);
    assert_eq!(initial_nonce, 0, "Initial nonce should be 0");
    println!(
        "✓ Step 3: Alice's account loaded, nonce = {}",
        initial_nonce
    );

    // Step 4: Simulate transaction (increment nonce)
    let new_nonce = initial_nonce + 1;
    parallel_state.set_nonce(&alice_pubkey, new_nonce);
    println!("✓ Step 4: Nonce incremented to {}", new_nonce);

    // Step 5: Verify nonce is tracked as modified
    let modified_nonces = parallel_state.get_modified_nonces();
    assert_eq!(modified_nonces.len(), 1, "Should have 1 modified nonce");
    assert_eq!(
        modified_nonces[0].0, alice_pubkey,
        "Modified nonce should be Alice's"
    );
    assert_eq!(
        modified_nonces[0].1, new_nonce,
        "Modified nonce value should be 1"
    );
    println!("✓ Step 5: Nonce tracked as modified: {:?}", modified_nonces);

    // Step 6: Commit state to storage
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit parallel state");
    }
    println!("✓ Step 6: State committed to storage");

    // Step 7: Verify nonce persisted correctly
    {
        let storage_read = storage.read().await;
        let (_, final_nonce) = storage_read
            .get_last_nonce(&alice_pubkey)
            .await
            .expect("Failed to get Alice's nonce");

        assert_eq!(
            final_nonce.get_nonce(),
            new_nonce,
            "Persisted nonce should match incremented value"
        );
        println!(
            "✓ Step 7: Nonce persisted correctly: {}",
            final_nonce.get_nonce()
        );
    }

    println!("✓ Test passed: Nonce increments correctly and persists\n");
}

// ============================================================================
// TEST 2: Staged Nonces - Not Committed Until Success
// ============================================================================
/// Validates that nonces are staged in memory and not committed until explicitly saved.
///
/// Scenario:
/// 1. Bob starts with nonce = 5
/// 2. Load account and increment nonce to 6
/// 3. DO NOT commit state
/// 4. Verify storage still shows nonce = 5 (staged only, not committed)
/// 5. Now commit state
/// 6. Verify storage shows nonce = 6 (committed)
///
/// This test validates:
/// - Nonce modifications stay in memory until commit
/// - Storage is not modified prematurely
/// - Commit correctly flushes staged nonces to storage
#[tokio::test]
async fn test_staged_nonces_not_committed_until_success() {
    println!("\n=== TEST 2: Staged Nonces (No Commit Until Success) ===");

    // Step 1: Setup storage with Bob account
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    let bob = KeyPair::new();
    let bob_pubkey = bob.get_public_key().compress();

    // Bob starts with nonce = 5, balance = 2000 TOS
    setup_account_rocksdb(&storage, &bob_pubkey, 2000 * COIN_VALUE, 5)
        .await
        .expect("Failed to setup Bob account");

    println!("✓ Step 1: Bob initialized with nonce = 5, balance = 2000 TOS");

    // Step 2: Create ParallelChainState
    let block = create_dummy_block(&bob);
    let block_hash = block.hash();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::V0,
        block,
        block_hash,
    )
    .await;

    println!("✓ Step 2: ParallelChainState created");

    // Step 3: Load Bob's account and increment nonce
    parallel_state
        .ensure_account_loaded(&bob_pubkey)
        .await
        .expect("Failed to load Bob account");

    let initial_nonce = parallel_state.get_nonce(&bob_pubkey);
    assert_eq!(initial_nonce, 5, "Initial nonce should be 5");
    println!("✓ Step 3: Bob's account loaded, nonce = {}", initial_nonce);

    // Increment nonce (staged in memory)
    let staged_nonce = initial_nonce + 1;
    parallel_state.set_nonce(&bob_pubkey, staged_nonce);
    println!(
        "✓ Step 4: Nonce staged to {} (not committed yet)",
        staged_nonce
    );

    // Step 5: Verify storage still shows original nonce
    {
        let storage_read = storage.read().await;
        let (_, storage_nonce) = storage_read
            .get_last_nonce(&bob_pubkey)
            .await
            .expect("Failed to get Bob's nonce from storage");

        assert_eq!(
            storage_nonce.get_nonce(),
            5,
            "Storage should still show original nonce (not committed)"
        );
        println!(
            "✓ Step 5: Storage nonce = {} (unchanged, staged only)",
            storage_nonce.get_nonce()
        );
    }

    // Step 6: Verify memory shows staged nonce
    let memory_nonce = parallel_state.get_nonce(&bob_pubkey);
    assert_eq!(
        memory_nonce, staged_nonce,
        "Memory should show staged nonce"
    );
    println!("✓ Step 6: Memory nonce = {} (staged)", memory_nonce);

    // Step 7: Commit state
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }
    println!("✓ Step 7: State committed");

    // Step 8: Verify storage now shows committed nonce
    {
        let storage_read = storage.read().await;
        let (_, committed_nonce) = storage_read
            .get_last_nonce(&bob_pubkey)
            .await
            .expect("Failed to get Bob's nonce after commit");

        assert_eq!(
            committed_nonce.get_nonce(),
            staged_nonce,
            "Storage should now show committed nonce"
        );
        println!(
            "✓ Step 8: Storage nonce = {} (committed)",
            committed_nonce.get_nonce()
        );
    }

    println!("✓ Test passed: Staged nonces not committed until success\n");
}

// ============================================================================
// TEST 3: Nonce Rollback on Transaction Failure
// ============================================================================
/// Validates that nonces are NOT modified when transaction fails (no commit).
///
/// Scenario:
/// 1. Charlie starts with nonce = 10
/// 2. Load account and increment nonce to 11 (simulate failed transaction)
/// 3. DO NOT commit (simulating transaction failure)
/// 4. Create NEW ParallelChainState (fresh instance)
/// 5. Load Charlie's account again
/// 6. Verify nonce is still 10 (rollback succeeded)
///
/// This test validates:
/// - Failed transactions don't persist nonce changes
/// - Fresh ParallelChainState loads original nonce from storage
/// - Automatic rollback behavior (no explicit rollback call needed)
#[tokio::test]
async fn test_nonce_rollback_on_failure() {
    println!("\n=== TEST 3: Nonce Rollback on Transaction Failure ===");

    // Step 1: Setup storage with Charlie account
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    let charlie = KeyPair::new();
    let charlie_pubkey = charlie.get_public_key().compress();

    // Charlie starts with nonce = 10, balance = 3000 TOS
    setup_account_rocksdb(&storage, &charlie_pubkey, 3000 * COIN_VALUE, 10)
        .await
        .expect("Failed to setup Charlie account");

    println!("✓ Step 1: Charlie initialized with nonce = 10, balance = 3000 TOS");

    // Step 2: Create first ParallelChainState (simulate failed transaction)
    let block1 = create_dummy_block(&charlie);
    let block_hash1 = block1.hash();

    let parallel_state_1 = ParallelChainState::new(
        Arc::clone(&storage),
        Arc::clone(&environment),
        0,
        1,
        BlockVersion::V0,
        block1,
        block_hash1,
    )
    .await;

    println!("✓ Step 2: First ParallelChainState created");

    // Step 3: Load Charlie's account and increment nonce (simulate transaction)
    parallel_state_1
        .ensure_account_loaded(&charlie_pubkey)
        .await
        .expect("Failed to load Charlie account");

    let initial_nonce = parallel_state_1.get_nonce(&charlie_pubkey);
    assert_eq!(initial_nonce, 10, "Initial nonce should be 10");
    println!(
        "✓ Step 3: Charlie's account loaded, nonce = {}",
        initial_nonce
    );

    // Increment nonce (simulate transaction execution)
    let failed_nonce = initial_nonce + 1;
    parallel_state_1.set_nonce(&charlie_pubkey, failed_nonce);
    println!(
        "✓ Step 4: Nonce incremented to {} (simulating transaction)",
        failed_nonce
    );

    // Step 5: DO NOT COMMIT (simulate transaction failure)
    println!("✓ Step 5: Transaction FAILED - state NOT committed (rollback)");
    drop(parallel_state_1); // Discard state

    // Step 6: Verify storage still shows original nonce
    {
        let storage_read = storage.read().await;
        let (_, storage_nonce) = storage_read
            .get_last_nonce(&charlie_pubkey)
            .await
            .expect("Failed to get Charlie's nonce");

        assert_eq!(
            storage_nonce.get_nonce(),
            10,
            "Storage should still show original nonce after rollback"
        );
        println!(
            "✓ Step 6: Storage nonce = {} (rollback succeeded)",
            storage_nonce.get_nonce()
        );
    }

    // Step 7: Create new ParallelChainState (fresh instance)
    let block2 = create_dummy_block(&charlie);
    let block_hash2 = block2.hash();

    let parallel_state_2 = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::V0,
        block2,
        block_hash2,
    )
    .await;

    println!("✓ Step 7: New ParallelChainState created");

    // Step 8: Load Charlie's account again and verify nonce
    parallel_state_2
        .ensure_account_loaded(&charlie_pubkey)
        .await
        .expect("Failed to load Charlie account in new state");

    let final_nonce = parallel_state_2.get_nonce(&charlie_pubkey);
    assert_eq!(
        final_nonce, 10,
        "Nonce should be original value after rollback"
    );
    println!(
        "✓ Step 8: Charlie's nonce = {} (rollback successful)",
        final_nonce
    );

    println!("✓ Test passed: Nonce rollback on transaction failure works correctly\n");
}

// ============================================================================
// TEST 4: Concurrent Nonce Updates Are Handled Safely
// ============================================================================
/// Validates that concurrent nonce updates don't cause data corruption.
///
/// Scenario:
/// 1. Create storage with 3 accounts (Alice, Bob, Charlie)
/// 2. Load all accounts into ParallelChainState
/// 3. Concurrently increment nonces for all 3 accounts
/// 4. Verify all nonces tracked correctly
/// 5. Commit and verify persistence
///
/// This test validates:
/// - DashMap handles concurrent nonce updates safely
/// - All nonce modifications are tracked correctly
/// - No data corruption from concurrent access
#[tokio::test]
async fn test_concurrent_nonce_updates() {
    println!("\n=== TEST 4: Concurrent Nonce Updates Are Handled Safely ===");

    // Step 1: Setup storage with 3 accounts
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let charlie = KeyPair::new();

    let alice_pubkey = alice.get_public_key().compress();
    let bob_pubkey = bob.get_public_key().compress();
    let charlie_pubkey = charlie.get_public_key().compress();

    setup_account_rocksdb(&storage, &alice_pubkey, 1000 * COIN_VALUE, 0)
        .await
        .unwrap();
    setup_account_rocksdb(&storage, &bob_pubkey, 2000 * COIN_VALUE, 5)
        .await
        .unwrap();
    setup_account_rocksdb(&storage, &charlie_pubkey, 3000 * COIN_VALUE, 10)
        .await
        .unwrap();

    println!("✓ Step 1: 3 accounts initialized:");
    println!("  - Alice: nonce = 0");
    println!("  - Bob: nonce = 5");
    println!("  - Charlie: nonce = 10");

    // Step 2: Create ParallelChainState
    let block = create_dummy_block(&alice);
    let block_hash = block.hash();

    let parallel_state = Arc::new(
        ParallelChainState::new(
            Arc::clone(&storage),
            environment,
            0,
            1,
            BlockVersion::V0,
            block,
            block_hash,
        )
        .await,
    );

    println!("✓ Step 2: ParallelChainState created");

    // Step 3: Load all accounts
    parallel_state
        .ensure_account_loaded(&alice_pubkey)
        .await
        .unwrap();
    parallel_state
        .ensure_account_loaded(&bob_pubkey)
        .await
        .unwrap();
    parallel_state
        .ensure_account_loaded(&charlie_pubkey)
        .await
        .unwrap();

    println!("✓ Step 3: All accounts loaded");

    // Step 4: Concurrently increment nonces (simulate parallel transaction execution)
    let state_clone1 = Arc::clone(&parallel_state);
    let state_clone2 = Arc::clone(&parallel_state);
    let state_clone3 = Arc::clone(&parallel_state);

    let alice_key = alice_pubkey.clone();
    let bob_key = bob_pubkey.clone();
    let charlie_key = charlie_pubkey.clone();

    let handles = vec![
        tokio::spawn(async move {
            // Alice: 0 → 1
            let nonce = state_clone1.get_nonce(&alice_key);
            state_clone1.set_nonce(&alice_key, nonce + 1);
        }),
        tokio::spawn(async move {
            // Bob: 5 → 6
            let nonce = state_clone2.get_nonce(&bob_key);
            state_clone2.set_nonce(&bob_key, nonce + 1);
        }),
        tokio::spawn(async move {
            // Charlie: 10 → 11
            let nonce = state_clone3.get_nonce(&charlie_key);
            state_clone3.set_nonce(&charlie_key, nonce + 1);
        }),
    ];

    // Wait for all concurrent updates
    for handle in handles {
        handle.await.unwrap();
    }

    println!("✓ Step 4: Concurrent nonce updates completed");

    // Step 5: Verify all nonces updated correctly
    assert_eq!(parallel_state.get_nonce(&alice_pubkey), 1);
    assert_eq!(parallel_state.get_nonce(&bob_pubkey), 6);
    assert_eq!(parallel_state.get_nonce(&charlie_pubkey), 11);

    println!("✓ Step 5: All nonces verified:");
    println!("  - Alice: {} ✓", parallel_state.get_nonce(&alice_pubkey));
    println!("  - Bob: {} ✓", parallel_state.get_nonce(&bob_pubkey));
    println!(
        "  - Charlie: {} ✓",
        parallel_state.get_nonce(&charlie_pubkey)
    );

    // Step 6: Verify modification tracking
    let modified_nonces = parallel_state.get_modified_nonces();
    assert_eq!(modified_nonces.len(), 3, "Should have 3 modified nonces");
    println!("✓ Step 6: All 3 nonces tracked as modified");

    // Step 7: Commit and verify persistence
    {
        let mut storage_write = storage.write().await;
        parallel_state.commit(&mut *storage_write).await.unwrap();
    }

    {
        let storage_read = storage.read().await;

        let (_, alice_nonce) = storage_read.get_last_nonce(&alice_pubkey).await.unwrap();
        let (_, bob_nonce) = storage_read.get_last_nonce(&bob_pubkey).await.unwrap();
        let (_, charlie_nonce) = storage_read.get_last_nonce(&charlie_pubkey).await.unwrap();

        assert_eq!(alice_nonce.get_nonce(), 1);
        assert_eq!(bob_nonce.get_nonce(), 6);
        assert_eq!(charlie_nonce.get_nonce(), 11);

        println!("✓ Step 7: All nonces persisted correctly");
    }

    println!("✓ Test passed: Concurrent nonce updates handled safely\n");
}

// ============================================================================
// TEST 5: Nonce Ordering Is Preserved
// ============================================================================
/// Validates that nonce ordering is preserved across sequential operations.
///
/// Scenario:
/// 1. Dave starts with nonce = 0
/// 2. Perform 5 sequential nonce increments (0→1→2→3→4→5)
/// 3. Verify nonce progression is correct at each step
/// 4. Commit and verify final nonce = 5
///
/// This test validates:
/// - Sequential nonce increments work correctly
/// - No nonce skipping or duplication
/// - Proper ordering preservation
#[tokio::test]
async fn test_nonce_ordering_preserved() {
    println!("\n=== TEST 5: Nonce Ordering Is Preserved ===");

    // Step 1: Setup storage with Dave account
    let storage = create_test_rocksdb_storage().await;
    let environment = Arc::new(Environment::new());

    let dave = KeyPair::new();
    let dave_pubkey = dave.get_public_key().compress();

    // Dave starts with nonce = 0, balance = 5000 TOS
    setup_account_rocksdb(&storage, &dave_pubkey, 5000 * COIN_VALUE, 0)
        .await
        .expect("Failed to setup Dave account");

    println!("✓ Step 1: Dave initialized with nonce = 0, balance = 5000 TOS");

    // Step 2: Create ParallelChainState
    let block = create_dummy_block(&dave);
    let block_hash = block.hash();

    let parallel_state = ParallelChainState::new(
        Arc::clone(&storage),
        environment,
        0,
        1,
        BlockVersion::V0,
        block,
        block_hash,
    )
    .await;

    println!("✓ Step 2: ParallelChainState created");

    // Step 3: Load Dave's account
    parallel_state
        .ensure_account_loaded(&dave_pubkey)
        .await
        .expect("Failed to load Dave account");

    let initial_nonce = parallel_state.get_nonce(&dave_pubkey);
    assert_eq!(initial_nonce, 0, "Initial nonce should be 0");
    println!("✓ Step 3: Dave's account loaded, nonce = {}", initial_nonce);

    // Step 4: Perform 5 sequential nonce increments
    println!("✓ Step 4: Performing 5 sequential nonce increments...");

    let expected_sequence = vec![1, 2, 3, 4, 5];
    for expected_nonce in &expected_sequence {
        let current_nonce = parallel_state.get_nonce(&dave_pubkey);
        let next_nonce = current_nonce + 1;
        parallel_state.set_nonce(&dave_pubkey, next_nonce);

        assert_eq!(
            next_nonce, *expected_nonce,
            "Nonce should progress sequentially"
        );
        println!("  - Nonce: {} → {} ✓", current_nonce, next_nonce);
    }

    // Step 5: Verify final nonce
    let final_nonce_memory = parallel_state.get_nonce(&dave_pubkey);
    assert_eq!(final_nonce_memory, 5, "Final nonce in memory should be 5");
    println!("✓ Step 5: Final nonce in memory = {}", final_nonce_memory);

    // Step 6: Commit state
    {
        let mut storage_write = storage.write().await;
        parallel_state
            .commit(&mut *storage_write)
            .await
            .expect("Failed to commit");
    }
    println!("✓ Step 6: State committed");

    // Step 7: Verify final nonce in storage
    {
        let storage_read = storage.read().await;
        let (_, committed_nonce) = storage_read
            .get_last_nonce(&dave_pubkey)
            .await
            .expect("Failed to get Dave's nonce");

        assert_eq!(
            committed_nonce.get_nonce(),
            5,
            "Final committed nonce should be 5"
        );
        println!(
            "✓ Step 7: Final nonce in storage = {}",
            committed_nonce.get_nonce()
        );
    }

    println!("✓ Test passed: Nonce ordering preserved correctly\n");
}
