//! Migrated test: Balance Preservation (Security Test #2)
//!
//! **Original issue**: Test timed out due to sled deadlocks when manually writing versioned balances
//! **Root cause**: Manual versioned balance writes to sled storage trigger internal locking conflicts
//! **Fix**: Use MockStorage with simple in-memory HashMaps instead of sled
//!
//! **Original location**: daemon/tests/parallel_execution_security_tests.rs:367-498
//! **Migration date**: 2025-10-30
//!
//! # Security Test: Balance Preservation
//!
//! This test verifies that receiver balances are **incremented** (not overwritten) when receiving
//! transfers. This was Vulnerability #2 in SECURITY_AUDIT_PARALLEL_EXECUTION.md.
//!
//! # Test Scenario
//!
//! 1. Bob has existing balance of 500 TOS
//! 2. Alice sends 1 TOS to Bob
//! 3. Verify Bob's final balance is 501 TOS (not 1 TOS!)
//!
//! If the implementation incorrectly overwrites instead of incrementing, Bob would lose his
//! 500 TOS existing balance.
//!
//! # Key Differences from Original
//!
//! - ❌ OLD: SledStorage with manual versioned balance writes → deadlock
//! - ✅ NEW: MockStorage with HashMap → no deadlock
//!
//! - ❌ OLD: Complex storage locking with Arc<RwLock<SledStorage>>
//! - ✅ NEW: Simple MockStorage.setup_account() helper
//!
//! - ❌ OLD: Required manual version tracking
//! - ✅ NEW: MockStorage handles versions automatically

use tos_testing_integration::{MockStorage, setup_account_mock};
use tos_common::{
    config::{COIN_VALUE, TOS_ASSET},
    crypto::KeyPair,
};

use crate::helpers::create_parallel_state;

#[tokio::test]
async fn test_balance_preservation_security() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== SECURITY TEST #2: Balance Preservation (Increment vs Overwrite) ===");
    println!("Verifying receiver balances are incremented, not overwritten");

    // Create MockStorage (no deadlocks!)
    let storage = MockStorage::new_with_tos_asset();

    // Create test accounts
    let alice = KeyPair::new();
    let bob = KeyPair::new();

    let alice_pk = alice.get_public_key().compress();
    let bob_pk = bob.get_public_key().compress();

    // Setup initial balances
    // CRITICAL: Bob has 500 TOS existing balance
    setup_account_mock(&storage, &alice_pk, 1000 * COIN_VALUE, 0);
    setup_account_mock(&storage, &bob_pk, 500 * COIN_VALUE, 0);

    println!("Initial state:");
    println!("  Alice: 1000 TOS (nonce 0)");
    println!("  Bob: 500 TOS (existing balance) ← IMPORTANT!");

    // Create ParallelChainState
    let parallel_state = create_parallel_state(storage.clone()).await?;

    // Verify initial state loaded correctly
    let bob_initial = parallel_state.get_balance(&bob_pk, &TOS_ASSET);
    assert_eq!(bob_initial, 500 * COIN_VALUE, "Bob should start with 500 TOS");

    println!("\nTransaction: Alice → Bob, 1 TOS (fee: 10 nanoTOS)");

    // Execute transfer: Alice sends 1 TOS to Bob
    let transfer_amount = 1 * COIN_VALUE;
    let fee = 10u64;

    parallel_state.sub_balance(&alice_pk, &TOS_ASSET, transfer_amount + fee)?;
    parallel_state.add_balance(&bob_pk, &TOS_ASSET, transfer_amount);
    parallel_state.increment_nonce(&alice_pk)?;

    // Verify final balances
    let alice_final = parallel_state.get_balance(&alice_pk, &TOS_ASSET);
    let bob_final = parallel_state.get_balance(&bob_pk, &TOS_ASSET);

    println!("\nFinal state:");
    println!("  Alice: {}.{:08} TOS", alice_final / COIN_VALUE, alice_final % COIN_VALUE);
    println!("  Bob: {} TOS", bob_final / COIN_VALUE);

    // Expected results:
    // Alice: 1000 - 1 - 0.00000010 = 998.99999990 TOS
    let expected_alice = 1000 * COIN_VALUE - transfer_amount - fee;

    // Bob: 500 + 1 = 501 TOS (NOT 1 TOS!)
    let expected_bob = 500 * COIN_VALUE + transfer_amount;

    assert_eq!(alice_final, expected_alice, "Alice balance mismatch");
    assert_eq!(bob_final, expected_bob,
               "Bob should have 501 TOS (500 existing + 1 received), not {} TOS",
               bob_final / COIN_VALUE);

    // Verify the critical security property
    if bob_final == transfer_amount {
        panic!("❌ SECURITY VULNERABILITY DETECTED: Bob's balance was OVERWRITTEN to {} TOS instead of INCREMENTED to 501 TOS!",
               transfer_amount / COIN_VALUE);
    }

    println!("\n✅ SECURITY TEST PASSED: Bob's balance correctly incremented");
    println!("   ✓ Bob started with 500 TOS");
    println!("   ✓ Bob received 1 TOS");
    println!("   ✓ Bob's final balance is 501 TOS (not overwritten)");
    println!("   This verifies the fix for Vulnerability #2");

    Ok(())
}
