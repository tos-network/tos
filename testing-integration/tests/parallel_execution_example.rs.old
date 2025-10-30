//! Example migrated parallel execution test
//!
//! This test demonstrates how to migrate a parallel execution test from
//! daemon/tests/parallel_execution_parity_tests.rs to use the new MockStorage.
//!
//! BEFORE (with sled deadlock issues):
//! - Manual storage setup with versioned balances
//! - Required tokio::time::sleep() workarounds
//! - Tests would timeout due to sled deadlocks
//! - Tests marked as #[ignore]
//!
//! AFTER (with MockStorage):
//! - Clean setup_account_mock() helper
//! - No sleep() workarounds needed
//! - No deadlocks (in-memory storage)
//! - Tests can run without #[ignore]

use std::sync::Arc;
use parking_lot::RwLock;

use tos_testing_integration::{MockStorage, setup_account_mock, get_balance_from_storage, get_nonce_from_storage};
use tos_daemon::core::{
    state::parallel_chain_state::ParallelChainState,
    error::BlockchainError,
};
use tos_common::{
    config::TOS_ASSET,
    crypto::PublicKey,
    serializer::{Reader, Writer},
};

/// Helper to create deterministic test accounts
fn create_test_account(id: u8) -> PublicKey {
    let mut buffer = Vec::new();
    let mut writer = Writer::new(&mut buffer);
    writer.write_bytes(&[id; 32]);
    let data = writer.as_bytes();
    let mut reader = Reader::new(data);
    tos_common::crypto::elgamal::CompressedPublicKey::read(&mut reader)
        .expect("Failed to create test pubkey")
}

#[tokio::test]
async fn test_parallel_state_basic_operations() -> Result<(), Box<dyn std::error::Error>> {
    // Create MockStorage with TOS asset pre-registered
    let storage = MockStorage::new_with_tos_asset();

    // Setup test accounts (no deadlocks!)
    let account_a = create_test_account(1);
    let account_b = create_test_account(2);

    setup_account_mock(&storage, &account_a, 1000, 0);
    setup_account_mock(&storage, &account_b, 2000, 0);

    // Verify setup
    let balance_a = get_balance_from_storage(&storage, &account_a, &TOS_ASSET).await?;
    let balance_b = get_balance_from_storage(&storage, &account_b, &TOS_ASSET).await?;
    let nonce_a = get_nonce_from_storage(&storage, &account_a).await?;
    let nonce_b = get_nonce_from_storage(&storage, &account_b).await?;

    assert_eq!(balance_a, 1000);
    assert_eq!(balance_b, 2000);
    assert_eq!(nonce_a, 0);
    assert_eq!(nonce_b, 0);

    println!("✅ Test passed: Basic MockStorage operations work without deadlocks");

    Ok(())
}

#[tokio::test]
async fn test_parallel_chain_state_creation() -> Result<(), Box<dyn std::error::Error>> {
    // Create MockStorage
    let storage = MockStorage::new_with_tos_asset();

    // Setup accounts
    let account_a = create_test_account(1);
    let account_b = create_test_account(2);

    setup_account_mock(&storage, &account_a, 5000, 10);
    setup_account_mock(&storage, &account_b, 3000, 5);

    // Create ParallelChainState (THIS WOULD DEADLOCK with sled!)
    let storage_arc = Arc::new(RwLock::new(storage.clone()));
    let parallel_state = ParallelChainState::new(storage_arc.clone(), 0).await?;

    // Verify accounts were loaded correctly
    let loaded_balance_a = parallel_state.get_balance(&account_a, &TOS_ASSET);
    let loaded_balance_b = parallel_state.get_balance(&account_b, &TOS_ASSET);
    let loaded_nonce_a = parallel_state.get_nonce(&account_a);
    let loaded_nonce_b = parallel_state.get_nonce(&account_b);

    assert_eq!(loaded_balance_a, 5000, "Account A balance mismatch");
    assert_eq!(loaded_balance_b, 3000, "Account B balance mismatch");
    assert_eq!(loaded_nonce_a, 10, "Account A nonce mismatch");
    assert_eq!(loaded_nonce_b, 5, "Account B nonce mismatch");

    println!("✅ Test passed: ParallelChainState creation works with MockStorage");

    Ok(())
}

#[tokio::test]
async fn test_parallel_state_modifications() -> Result<(), Box<dyn std::error::Error>> {
    // Create MockStorage
    let storage = MockStorage::new_with_tos_asset();

    // Setup accounts
    let account_a = create_test_account(1);
    let account_b = create_test_account(2);

    setup_account_mock(&storage, &account_a, 1000, 0);
    setup_account_mock(&storage, &account_b, 500, 0);

    // Create ParallelChainState
    let storage_arc = Arc::new(RwLock::new(storage.clone()));
    let parallel_state = ParallelChainState::new(storage_arc.clone(), 0).await?;

    // Modify balances (simulate transaction execution)
    parallel_state.add_balance(&account_a, &TOS_ASSET, 200);  // +200
    parallel_state.sub_balance(&account_b, &TOS_ASSET, 150)?; // -150

    // Increment nonces
    parallel_state.increment_nonce(&account_a)?;
    parallel_state.increment_nonce(&account_b)?;

    // Verify modifications
    assert_eq!(parallel_state.get_balance(&account_a, &TOS_ASSET), 1200);
    assert_eq!(parallel_state.get_balance(&account_b, &TOS_ASSET), 350);
    assert_eq!(parallel_state.get_nonce(&account_a), 1);
    assert_eq!(parallel_state.get_nonce(&account_b), 1);

    // Get modified state (this is what triggers version bumping)
    let modified_balances = parallel_state.get_modified_balances();
    let modified_nonces = parallel_state.get_modified_nonces();

    // Verify only modified accounts are returned
    assert_eq!(modified_balances.len(), 2, "Should have 2 modified balances");
    assert_eq!(modified_nonces.len(), 2, "Should have 2 modified nonces");

    println!("✅ Test passed: ParallelChainState modifications tracked correctly");

    Ok(())
}

#[tokio::test]
async fn test_multiple_accounts_no_deadlock() -> Result<(), Box<dyn std::error::Error>> {
    // This test used to timeout due to sled deadlocks when setting up many accounts
    let storage = MockStorage::new_with_tos_asset();

    // Setup 100 accounts (this would be very slow or deadlock with sled)
    let accounts: Vec<PublicKey> = (0..100)
        .map(|i| {
            let account = create_test_account(i);
            setup_account_mock(&storage, &account, i as u64 * 100, i as u64);
            account
        })
        .collect();

    // Create ParallelChainState
    let storage_arc = Arc::new(RwLock::new(storage.clone()));
    let parallel_state = ParallelChainState::new(storage_arc, 0).await?;

    // Verify all accounts loaded
    for (i, account) in accounts.iter().enumerate() {
        let balance = parallel_state.get_balance(account, &TOS_ASSET);
        let nonce = parallel_state.get_nonce(account);

        assert_eq!(balance, i as u64 * 100, "Account {} balance mismatch", i);
        assert_eq!(nonce, i as u64, "Account {} nonce mismatch", i);
    }

    println!("✅ Test passed: 100 accounts loaded without deadlocks");

    Ok(())
}

#[tokio::test]
async fn test_version_tracking_correctness() -> Result<(), Box<dyn std::error::Error>> {
    // Test that only actually modified state creates new versions
    let storage = MockStorage::new_with_tos_asset();

    let account_modified = create_test_account(1);
    let account_unchanged = create_test_account(2);

    setup_account_mock(&storage, &account_modified, 1000, 5);
    setup_account_mock(&storage, &account_unchanged, 2000, 10);

    let storage_arc = Arc::new(RwLock::new(storage.clone()));
    let parallel_state = ParallelChainState::new(storage_arc, 0).await?;

    // Modify only account_modified
    parallel_state.add_balance(&account_modified, &TOS_ASSET, 500);
    parallel_state.increment_nonce(&account_modified)?;

    // account_unchanged should NOT be in modified lists
    let modified_balances = parallel_state.get_modified_balances();
    let modified_nonces = parallel_state.get_modified_nonces();

    // Check that only account_modified appears
    assert_eq!(modified_balances.len(), 1, "Only one account should have modified balance");
    assert_eq!(modified_nonces.len(), 1, "Only one account should have modified nonce");

    let (modified_account, _) = &modified_balances[0];
    assert_eq!(modified_account, &account_modified, "Wrong account in modified balances");

    let (modified_account_nonce, _) = &modified_nonces[0];
    assert_eq!(modified_account_nonce, &account_modified, "Wrong account in modified nonces");

    println!("✅ Test passed: Version tracking only captures actually modified state");

    Ok(())
}
