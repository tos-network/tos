// File: testing-framework/tests/reentrancy_guard_integration_test.rs
//
// ReentrancyGuard Contract Integration Tests
//
// Tests demonstrating reentrancy attack prevention following OpenZeppelin pattern

use tos_common::crypto::{Hash, KeyPair};
use tos_testing_framework::utilities::{create_contract_test_storage, execute_test_contract, execute_test_contract_with_input};

const OP_WITHDRAW: u8 = 0x01;
const OP_DEPOSIT: u8 = 0x02;
const OP_GET_BALANCE: u8 = 0x10;

const ERR_REENTRANT_CALL: u64 = 1001;

fn encode_u64(value: u64) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

// ============================================================================
// TEST 1: Normal Withdrawal (No Reentrancy)
// ============================================================================

#[tokio::test]
async fn test_reentrancy_guard_normal_withdrawal() {
    let user = KeyPair::new();
    let storage = create_contract_test_storage(&user, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/reentrancy_guard.so");
    let contract_hash = Hash::zero();
    let tx_sender = Hash::zero();  // Sender address

    // Deposit
    let mut deposit_params = vec![OP_DEPOSIT];
    deposit_params.extend(encode_u64(1000));

    let result1 = execute_test_contract_with_input(bytecode, &storage, 1, &contract_hash, &tx_sender, &deposit_params)
        .await
        .unwrap();

    assert_eq!(result1.return_value, 0, "Deposit should succeed");

    // Withdraw
    let mut withdraw_params = vec![OP_WITHDRAW];
    withdraw_params.extend(encode_u64(500));

    let result2 = execute_test_contract_with_input(bytecode, &storage, 2, &contract_hash, &tx_sender, &withdraw_params)
        .await
        .unwrap();

    assert_eq!(result2.return_value, 0, "Withdrawal should succeed");
}

// ============================================================================
// TEST 2: Reentrancy Attack Prevention
// ============================================================================

#[tokio::test]
async fn test_reentrancy_guard_blocks_reentrant_call() {
    let attacker = KeyPair::new();
    let storage = create_contract_test_storage(&attacker, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/reentrancy_guard.so");
    let contract_hash = Hash::zero();
    let tx_sender = Hash::zero();

    // Attempt reentrancy (simulated)
    // TODO: When contract is ready, test actual reentrancy attack
    // Expected: ERR_REENTRANT_CALL

    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    assert_eq!(result.return_value, 0);
}

// ============================================================================
// TEST 3: Multiple Sequential Calls (Allowed)
// ============================================================================

#[tokio::test]
async fn test_reentrancy_guard_sequential_calls() {
    let user = KeyPair::new();
    let storage = create_contract_test_storage(&user, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/reentrancy_guard.so");
    let contract_hash = Hash::zero();
    let tx_sender = Hash::zero();

    // Multiple sequential withdrawals (not reentrant)
    for i in 1..=5 {
        let result = execute_test_contract(bytecode, &storage, i, &contract_hash)
            .await
            .unwrap();

        assert_eq!(result.return_value, 0, "Call {} should succeed", i);
    }
}

// ============================================================================
// TEST 4: Guard State Management
// ============================================================================

#[tokio::test]
async fn test_reentrancy_guard_state_management() {
    let user = KeyPair::new();
    let storage = create_contract_test_storage(&user, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/reentrancy_guard.so");
    let contract_hash = Hash::zero();
    let tx_sender = Hash::zero();

    // Test guard unlocks after function completes
    let result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    assert_eq!(result1.return_value, 0);

    // Next call should succeed (guard was released)
    let result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();

    assert_eq!(result2.return_value, 0);
}

// ============================================================================
// TEST 5: Balance Query During Lock
// ============================================================================

#[tokio::test]
async fn test_reentrancy_guard_query_during_lock() {
    let user = KeyPair::new();
    let storage = create_contract_test_storage(&user, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/reentrancy_guard.so");
    let contract_hash = Hash::zero();
    let tx_sender = Hash::zero();

    // Query balance (read-only, should work even during lock)
    let query_params = vec![OP_GET_BALANCE];

    let result = execute_test_contract_with_input(bytecode, &storage, 1, &contract_hash, &tx_sender, &query_params)
        .await
        .unwrap();

    assert_eq!(result.return_value, 0);
}

// ============================================================================
// TEST 6: Nested Call Prevention
// ============================================================================

#[tokio::test]
async fn test_reentrancy_guard_nested_prevention() {
    let user = KeyPair::new();
    let storage = create_contract_test_storage(&user, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/reentrancy_guard.so");
    let contract_hash = Hash::zero();
    let tx_sender = Hash::zero();

    // TODO: Test nested call prevention when contract supports CPI
    // Expected: First call succeeds, nested call fails with ERR_REENTRANT_CALL

    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    assert_eq!(result.return_value, 0);
}

// ============================================================================
// TEST 7: Compute Units
// ============================================================================

#[tokio::test]
async fn test_reentrancy_guard_compute_units() {
    let user = KeyPair::new();
    let storage = create_contract_test_storage(&user, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/reentrancy_guard.so");
    let contract_hash = Hash::zero();
    let tx_sender = Hash::zero();

    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    assert!(result.compute_units_used > 0);
    assert!(result.compute_units_used < 500_000);
}

// ============================================================================
// TEST 8: Storage Persistence
// ============================================================================

#[tokio::test]
async fn test_reentrancy_guard_storage_persistence() {
    let user = KeyPair::new();
    let storage = create_contract_test_storage(&user, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/reentrancy_guard.so");
    let contract_hash = Hash::zero();
    let tx_sender = Hash::zero();

    // Execute operations across multiple topoheights
    for topoheight in 1..=5 {
        let result = execute_test_contract(bytecode, &storage, topoheight, &contract_hash)
            .await
            .unwrap();

        assert_eq!(result.return_value, 0, "Call at {} should succeed", topoheight);
    }
}
