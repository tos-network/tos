//! Example: Safe Storage Setup for Parallel Execution Tests
//!
//! This example demonstrates how to use the storage_helpers module to safely
//! set up sled storage for parallel execution tests without deadlocks.
//!
//! Run with: cargo run --example storage_helpers_usage

use tos_common::{
    config::TOS_ASSET,
    crypto::elgamal::CompressedPublicKey,
    serializer::{Reader, Serializer, Writer},
};
use tos_daemon::core::storage::{BalanceProvider, NonceProvider};
use tos_testing_integration::{
    create_test_storage,
    create_test_storage_with_accounts,
    create_test_storage_with_tos_asset,
    setup_account_safe,
    flush_storage_and_wait,
};

fn create_test_pubkey(seed: u8) -> CompressedPublicKey {
    let mut buffer = Vec::new();
    let mut writer = Writer::new(&mut buffer);
    writer.write_bytes(&[seed; 32]);
    let data = writer.as_bytes();
    let mut reader = Reader::new(data);
    CompressedPublicKey::read(&mut reader).expect("Failed to create test pubkey")
}

#[tokio::main]
async fn main() {
    println!("=== Storage Helpers Usage Examples ===\n");

    // Example 1: Basic storage creation
    println!("1. Creating basic test storage with TOS asset...");
    let _storage1 = create_test_storage().await;
    println!("   Storage created successfully\n");

    // Example 2: Explicit TOS asset creation (same as above, but clearer intent)
    println!("2. Creating test storage with explicit TOS asset...");
    let _storage2 = create_test_storage_with_tos_asset().await;
    println!("   Storage created successfully\n");

    // Example 3: Manual account setup with safe pattern
    println!("3. Setting up accounts manually with safe pattern...");
    let storage3 = create_test_storage().await;
    let account_a = create_test_pubkey(1);
    let account_b = create_test_pubkey(2);

    // Setup accounts safely (includes internal delays)
    setup_account_safe(&storage3, &account_a, 1000, 0).await.unwrap();
    setup_account_safe(&storage3, &account_b, 2000, 0).await.unwrap();

    // CRITICAL: Always flush before parallel execution
    flush_storage_and_wait(&storage3).await;
    println!("   Accounts setup safely with flush\n");

    // Verify accounts
    {
        let storage_read = storage3.read().await;
        let (_, balance_a) = storage_read.get_last_balance(&account_a, &TOS_ASSET).await.unwrap();
        let (_, balance_b) = storage_read.get_last_balance(&account_b, &TOS_ASSET).await.unwrap();
        println!("   Account A balance: {}", balance_a.get_balance());
        println!("   Account B balance: {}\n", balance_b.get_balance());
    }

    // Example 4: Convenient batch account setup
    println!("4. Creating storage with pre-populated accounts...");
    let account_c = create_test_pubkey(3);
    let account_d = create_test_pubkey(4);
    let account_e = create_test_pubkey(5);

    let storage4 = create_test_storage_with_accounts(vec![
        (account_c.clone(), 5000, 0),
        (account_d.clone(), 10000, 5),
        (account_e.clone(), 15000, 10),
    ]).await.unwrap();

    println!("   Storage created with 3 accounts (already flushed)\n");

    // Verify accounts
    {
        let storage_read = storage4.read().await;
        let (_, balance_c) = storage_read.get_last_balance(&account_c, &TOS_ASSET).await.unwrap();
        let (_, nonce_d) = storage_read.get_last_nonce(&account_d).await.unwrap();
        println!("   Account C balance: {}", balance_c.get_balance());
        println!("   Account D nonce: {}\n", nonce_d.get_nonce());
    }

    // Example 5: Best practices summary
    println!("5. Best practices summary:");
    println!("   - Always use create_test_storage_with_accounts() for convenience");
    println!("   - Or use setup_account_safe() + flush_storage_and_wait() manually");
    println!("   - CRITICAL: Call flush_storage_and_wait() before ParallelChainState");
    println!("   - Never use legacy setup_account_in_storage_legacy() in parallel tests");
    println!("   - These helpers prevent sled deadlocks by allowing proper internal flush\n");

    println!("=== All Examples Completed Successfully ===");
}
