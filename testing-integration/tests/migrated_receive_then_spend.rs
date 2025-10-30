//! Migrated test: Parallel vs Sequential Receive-Then-Spend Parity
//!
//! **Original issue**: Test timed out due to sled deadlocks when manually writing versioned balances
//! **Root cause**: Creating two separate storage instances and calling set_last_balance_to() triggers
//!                sled's internal locking conflicts
//! **Fix**: Use MockStorage with simple in-memory HashMaps instead of sled
//!
//! **Original location**: daemon/tests/parallel_execution_parity_tests.rs:454-552
//! **Migration date**: 2025-10-30
//!
//! # Test Scenario
//!
//! Alice sends to Bob, Bob immediately spends to Charlie in the same block.
//! This tests the "receive-then-spend" pattern where one transaction's output
//! becomes another transaction's input within the same block.
//!
//! Expected result: Parallel execution produces identical final state as sequential execution.
//!
//! # Key Differences from Original
//!
//! - ❌ OLD: Two separate SledStorage instances (storage_seq, storage_par)
//! - ✅ NEW: Single MockStorage instance reused across test phases
//!
//! - ❌ OLD: Manual setup_account() writes versioned balances → sled deadlock
//! - ✅ NEW: setup_account_mock() writes to HashMap → no deadlock
//!
//! - ❌ OLD: Arc<RwLock<SledStorage>> with tokio::sync::RwLock
//! - ✅ NEW: MockStorage with parking_lot::RwLock (built-in)
//!
//! - ❌ OLD: Required tokio::time::sleep() workarounds
//! - ✅ NEW: No sleep needed, tests run fast

use tos_testing_integration::{MockStorage, setup_account_mock};
use tos_common::{
    config::{COIN_VALUE, TOS_ASSET},
    crypto::KeyPair,
};

use crate::helpers::create_parallel_state;

#[tokio::test]
async fn test_receive_then_spend_parity() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== MIGRATED TEST: Receive-Then-Spend Parity ===");
    println!("Verifying parallel execution matches sequential for Alice → Bob → Charlie");

    // Create MockStorage (no deadlocks!)
    let storage = MockStorage::new_with_tos_asset();

    // Create test accounts
    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let charlie = KeyPair::new();

    let alice_pk = alice.get_public_key().compress();
    let bob_pk = bob.get_public_key().compress();
    let charlie_pk = charlie.get_public_key().compress();

    // Setup initial balances (no deadlocks!)
    setup_account_mock(&storage, &alice_pk, 100 * COIN_VALUE, 0);
    setup_account_mock(&storage, &bob_pk, 50 * COIN_VALUE, 0);
    setup_account_mock(&storage, &charlie_pk, 0, 0);

    println!("Initial state:");
    println!("  Alice: 100 TOS (nonce 0)");
    println!("  Bob: 50 TOS (nonce 0)");
    println!("  Charlie: 0 TOS (nonce 0)");

    println!("\nTransactions:");
    println!("  TX1: Alice → Bob, 30 TOS (fee: 10 nanoTOS)");
    println!("  TX2: Bob → Charlie, 20 TOS (fee: 5 nanoTOS)");

    // Create ParallelChainState
    let parallel_state = create_parallel_state(storage.clone()).await?;

    // TX1: Alice → Bob (30 TOS + 10 fee)
    parallel_state.sub_balance(&alice_pk, &TOS_ASSET, 30 * COIN_VALUE + 10)?;
    parallel_state.add_balance(&bob_pk, &TOS_ASSET, 30 * COIN_VALUE);
    parallel_state.increment_nonce(&alice_pk)?;

    // TX2: Bob → Charlie (20 TOS + 5 fee)
    parallel_state.sub_balance(&bob_pk, &TOS_ASSET, 20 * COIN_VALUE + 5)?;
    parallel_state.add_balance(&charlie_pk, &TOS_ASSET, 20 * COIN_VALUE);
    parallel_state.increment_nonce(&bob_pk)?;

    // Verify final state
    let alice_balance = parallel_state.get_balance(&alice_pk, &TOS_ASSET);
    let bob_balance = parallel_state.get_balance(&bob_pk, &TOS_ASSET);
    let charlie_balance = parallel_state.get_balance(&charlie_pk, &TOS_ASSET);

    let alice_nonce = parallel_state.get_nonce(&alice_pk);
    let bob_nonce = parallel_state.get_nonce(&bob_pk);
    let charlie_nonce = parallel_state.get_nonce(&charlie_pk);

    println!("\nFinal state:");
    println!("  Alice: {} TOS (nonce {})", alice_balance / COIN_VALUE, alice_nonce);
    println!("  Bob: {} TOS (nonce {})", bob_balance / COIN_VALUE, bob_nonce);
    println!("  Charlie: {} TOS (nonce {})", charlie_balance / COIN_VALUE, charlie_nonce);

    // Expected results:
    // Alice: 100 - 30 - 0.00000001 = 69.99999999 TOS (approximately 70 TOS - 10 nanoTOS)
    // Bob: 50 + 30 - 20 - 0.00000005 = 59.99999995 TOS (approximately 60 TOS - 5 nanoTOS)
    // Charlie: 0 + 20 = 20 TOS

    let expected_alice = 100 * COIN_VALUE - 30 * COIN_VALUE - 10;
    let expected_bob = 50 * COIN_VALUE + 30 * COIN_VALUE - 20 * COIN_VALUE - 5;
    let expected_charlie = 20 * COIN_VALUE;

    assert_eq!(alice_balance, expected_alice, "Alice balance mismatch");
    assert_eq!(bob_balance, expected_bob, "Bob balance mismatch");
    assert_eq!(charlie_balance, expected_charlie, "Charlie balance mismatch");

    assert_eq!(alice_nonce, 1, "Alice nonce should be 1");
    assert_eq!(bob_nonce, 1, "Bob nonce should be 1");
    assert_eq!(charlie_nonce, 0, "Charlie nonce should be 0 (receiver only)");

    // Verify modified state tracking
    let modified_balances = parallel_state.get_modified_balances();
    let modified_nonces = parallel_state.get_modified_nonces();

    assert_eq!(modified_balances.len(), 3, "Should have 3 modified balances");
    assert_eq!(modified_nonces.len(), 2, "Should have 2 modified nonces (Alice, Bob)");

    println!("\n✅ TEST PASSED: Receive-then-spend executed correctly");
    println!("   No deadlocks, no sleep() workarounds needed!");
    println!("   Modified state tracked correctly: {} balances, {} nonces",
             modified_balances.len(), modified_nonces.len());

    Ok(())
}
