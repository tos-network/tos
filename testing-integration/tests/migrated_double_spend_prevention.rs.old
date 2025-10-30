//! Migrated test: Double Spend Prevention
//!
//! **Original issue**: Not originally an ignored test, but demonstrates safe testing pattern
//! **Purpose**: Show how MockStorage prevents double-spend scenarios cleanly
//! **Migration date**: 2025-10-30
//!
//! # Test Scenario
//!
//! Alice attempts to spend more than her balance by creating two conflicting transactions:
//! 1. TX1: Alice sends 100 TOS to Bob
//! 2. TX2: Alice sends 100 TOS to Charlie (but Alice only has 100 TOS!)
//!
//! Expected result: The second transaction should fail with insufficient balance error.
//!
//! # Key Testing Pattern
//!
//! This test demonstrates:
//! - ✅ Clean error handling with MockStorage
//! - ✅ Balance checks work correctly in parallel state
//! - ✅ No deadlocks when testing error conditions

use tos_testing_integration::{MockStorage, setup_account_mock};
use tos_common::{
    config::{COIN_VALUE, TOS_ASSET},
    crypto::KeyPair,
};

use crate::helpers::create_parallel_state;

#[tokio::test]
async fn test_double_spend_prevention() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== TEST: Double Spend Prevention ===");
    println!("Verifying parallel state prevents double-spend attacks");

    // Create MockStorage (no deadlocks!)
    let storage = MockStorage::new_with_tos_asset();

    // Create test accounts
    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let charlie = KeyPair::new();

    let alice_pk = alice.get_public_key().compress();
    let bob_pk = bob.get_public_key().compress();
    let charlie_pk = charlie.get_public_key().compress();

    // Setup initial balances
    // Alice has ONLY 100 TOS (insufficient for two 100 TOS transfers)
    setup_account_mock(&storage, &alice_pk, 100 * COIN_VALUE, 0);
    setup_account_mock(&storage, &bob_pk, 0, 0);
    setup_account_mock(&storage, &charlie_pk, 0, 0);

    println!("Initial state:");
    println!("  Alice: 100 TOS (nonce 0) ← Only enough for ONE 100 TOS transfer");
    println!("  Bob: 0 TOS");
    println!("  Charlie: 0 TOS");

    // Create ParallelChainState
    let parallel_state = create_parallel_state(storage.clone()).await?;

    // TX1: Alice sends 100 TOS to Bob (this should succeed)
    println!("\nTX1: Alice → Bob, 100 TOS");
    let result1 = parallel_state.sub_balance(&alice_pk, &TOS_ASSET, 100 * COIN_VALUE);
    assert!(result1.is_ok(), "First transaction should succeed");

    parallel_state.add_balance(&bob_pk, &TOS_ASSET, 100 * COIN_VALUE);
    parallel_state.increment_nonce(&alice_pk)?;

    // Verify Alice's balance is now 0
    let alice_balance_after_tx1 = parallel_state.get_balance(&alice_pk, &TOS_ASSET);
    println!("  Alice balance after TX1: {} TOS", alice_balance_after_tx1 / COIN_VALUE);
    assert_eq!(alice_balance_after_tx1, 0, "Alice should have 0 TOS after first transfer");

    // TX2: Alice tries to send another 100 TOS to Charlie (this should FAIL)
    println!("\nTX2: Alice → Charlie, 100 TOS (attempting double-spend)");
    let result2 = parallel_state.sub_balance(&alice_pk, &TOS_ASSET, 100 * COIN_VALUE);

    if result2.is_ok() {
        panic!("❌ DOUBLE-SPEND VULNERABILITY: Alice was allowed to spend 100 TOS twice with only 100 TOS balance!");
    }

    println!("  ✅ TX2 correctly rejected: {:?}", result2.unwrap_err());

    // Verify final state
    let alice_final = parallel_state.get_balance(&alice_pk, &TOS_ASSET);
    let bob_final = parallel_state.get_balance(&bob_pk, &TOS_ASSET);
    let charlie_final = parallel_state.get_balance(&charlie_pk, &TOS_ASSET);

    println!("\nFinal state:");
    println!("  Alice: {} TOS", alice_final / COIN_VALUE);
    println!("  Bob: {} TOS", bob_final / COIN_VALUE);
    println!("  Charlie: {} TOS", charlie_final / COIN_VALUE);

    assert_eq!(alice_final, 0, "Alice should have 0 TOS");
    assert_eq!(bob_final, 100 * COIN_VALUE, "Bob should have 100 TOS");
    assert_eq!(charlie_final, 0, "Charlie should have 0 TOS (double-spend prevented)");

    println!("\n✅ TEST PASSED: Double-spend attack prevented");
    println!("   ✓ Alice sent 100 TOS to Bob (succeeded)");
    println!("   ✓ Alice tried to send 100 TOS to Charlie (failed)");
    println!("   ✓ Total conservation: 0 + 100 + 0 = 100 TOS");

    Ok(())
}

#[tokio::test]
async fn test_insufficient_balance_detection() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== TEST: Insufficient Balance Detection ===");
    println!("Verifying balance checks work correctly with fees");

    // Create MockStorage
    let storage = MockStorage::new_with_tos_asset();

    let alice = KeyPair::new();
    let bob = KeyPair::new();

    let alice_pk = alice.get_public_key().compress();
    let bob_pk = bob.get_public_key().compress();

    // Alice has exactly 100 TOS
    setup_account_mock(&storage, &alice_pk, 100 * COIN_VALUE, 0);
    setup_account_mock(&storage, &bob_pk, 0, 0);

    println!("Initial state:");
    println!("  Alice: 100 TOS (exactly)");

    let parallel_state = create_parallel_state(storage.clone()).await?;

    // Try to send 100 TOS + 10 fee (total 100.00000010 TOS, but Alice only has 100.00000000)
    println!("\nAttempting: Alice → Bob, 100 TOS + 10 nanoTOS fee");
    println!("  Alice has: 100.00000000 TOS");
    println!("  Required: 100.00000010 TOS");

    let transfer_amount = 100 * COIN_VALUE;
    let fee = 10u64;

    let result = parallel_state.sub_balance(&alice_pk, &TOS_ASSET, transfer_amount + fee);

    if result.is_ok() {
        panic!("❌ BUG: Alice was allowed to spend more than her balance (100.00000010 > 100.00000000)");
    }

    println!("  ✅ Transaction correctly rejected: {:?}", result.unwrap_err());

    // Verify balance unchanged
    let alice_final = parallel_state.get_balance(&alice_pk, &TOS_ASSET);
    assert_eq!(alice_final, 100 * COIN_VALUE, "Alice balance should be unchanged");

    println!("\n✅ TEST PASSED: Insufficient balance correctly detected");
    println!("   ✓ Transaction rejected when amount + fee > balance");
    println!("   ✓ Alice retains original balance: 100 TOS");

    Ok(())
}
