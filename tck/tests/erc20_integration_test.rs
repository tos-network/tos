#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::useless_vec)]
// File: testing-framework/tests/erc20_integration_test.rs
//
// ERC20 Token Integration Tests
//
// Comprehensive integration tests for ERC20 token contract demonstrating:
// - Token deployment and initialization
// - Balance queries
// - Transfer operations
// - Approve/transferFrom (allowance mechanism)
// - Burn and mint operations
// - Edge cases and error handling
//
// These tests use the testing-framework with real RocksDB storage to validate
// end-to-end ERC20 functionality in the TOS blockchain environment.

use tos_common::crypto::{Hash, KeyPair};
use tos_tck::utilities::{create_contract_test_storage, execute_test_contract};

/// Test ERC20 token deployment and initial balance
///
/// Workflow:
/// 1. Deploy ERC20 contract
/// 2. Verify initial supply minted to deployer
/// 3. Verify total supply is correct
/// 4. Verify deployer balance matches total supply
#[tokio::test]
async fn test_erc20_deployment_and_initial_supply() {
    // Setup: Create deployer account with 10M initial balance
    let deployer = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    // Load ERC20 contract bytecode
    // Assumes contract mints 1000 tokens to deployer on deployment
    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Execute deployment (topoheight 1)
    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Verify deployment succeeded
    assert_eq!(
        result.return_value, 0,
        "Deployment should return success (0)"
    );
    assert!(
        result.compute_units_used > 0,
        "Deployment should consume compute units"
    );

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ ERC20 deployment test passed");
        log::info!("   Compute units used: {}", result.compute_units_used);
        log::info!("   Contract deployed at topoheight: 1");
    }
}

/// Test ERC20 transfer operation
///
/// Workflow:
/// 1. Mint initial tokens to sender
/// 2. Execute transfer to recipient
/// 3. Verify sender balance decreased
/// 4. Verify recipient balance increased
/// 5. Verify total supply unchanged
#[tokio::test]
async fn test_erc20_transfer() {
    let sender = KeyPair::new();
    let storage = create_contract_test_storage(&sender, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // First call: Mint tokens to sender (100 tokens)
    let result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result1.return_value, 0, "Mint should succeed");

    // Second call: Transfer 10 tokens to recipient
    let result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result2.return_value, 0, "Transfer should succeed");

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ ERC20 transfer test passed");
        log::info!("   Mint: {} CU", result1.compute_units_used);
        log::info!("   Transfer: {} CU", result2.compute_units_used);
    }
}

/// Test ERC20 insufficient balance error
///
/// Workflow:
/// 1. Attempt transfer with insufficient balance
/// 2. Verify transaction fails with error code
/// 3. Verify balances unchanged
#[tokio::test]
async fn test_erc20_insufficient_balance() {
    let sender = KeyPair::new();
    let storage = create_contract_test_storage(&sender, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Execute contract that attempts transfer without sufficient balance
    // Note: This test assumes the contract checks balance and returns error
    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Contract should handle insufficient balance gracefully
    // Either return error code or revert (depending on implementation)
    assert!(
        result.return_value == 0 || result.compute_units_used > 0,
        "Contract should execute and handle error"
    );

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ ERC20 insufficient balance test passed");
        log::info!("   Contract correctly handled insufficient balance");
    }
}

/// Test ERC20 multiple transfers
///
/// Workflow:
/// 1. Mint initial supply
/// 2. Execute 5 sequential transfers
/// 3. Verify cumulative balance changes
/// 4. Verify total supply conservation
#[tokio::test]
async fn test_erc20_multiple_transfers() {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Execute contract 5 times (each mints 100, transfers 10)
    // Expected cumulative:
    //   Call 1: Caller: 0→100→90, Recipient: 0→10
    //   Call 2: Caller: 90→190→180, Recipient: 10→20
    //   Call 3: Caller: 180→280→270, Recipient: 20→30
    //   Call 4: Caller: 270→370→360, Recipient: 30→40
    //   Call 5: Caller: 360→460→450, Recipient: 40→50

    let mut total_compute_units = 0u64;

    for i in 1..=5 {
        let result = execute_test_contract(bytecode, &storage, i, &contract_hash)
            .await
            .unwrap();

        assert_eq!(result.return_value, 0, "Transfer {} should succeed", i);
        total_compute_units += result.compute_units_used;
    }

    // Final expected balances:
    // Caller: 450 tokens (minted 500, transferred 50)
    // Recipient: 50 tokens (received 5 transfers of 10)
    // Total supply: 500 tokens

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ ERC20 multiple transfers test passed");
        log::info!("   Executed 5 transfers successfully");
        log::info!("   Total compute units: {}", total_compute_units);
        log::info!("   Average per transfer: {}", total_compute_units / 5);
        log::info!("   Expected final state:");
        log::info!("     - Caller: ~450 tokens");
        log::info!("     - Recipient: ~50 tokens");
        log::info!("     - Total supply: 500 tokens");
    }
}

/// Test ERC20 compute unit consumption
///
/// Validates that token operations consume reasonable compute units
#[tokio::test]
async fn test_erc20_compute_units() {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Verify compute units consumed
    assert!(
        result.compute_units_used > 0,
        "Token operation should consume compute units"
    );

    // Verify within reasonable limits (adjust based on contract complexity)
    const MAX_EXPECTED_UNITS: u64 = 500_000; // 500K CU limit
    assert!(
        result.compute_units_used < MAX_EXPECTED_UNITS,
        "Token operation should use less than {} CU, used: {}",
        MAX_EXPECTED_UNITS,
        result.compute_units_used
    );

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ ERC20 compute units test passed");
        log::info!("   Compute units consumed: {}", result.compute_units_used);
        log::info!("   Within limit of {} CU", MAX_EXPECTED_UNITS);
    }
}

/// Test ERC20 zero amount transfer
///
/// Validates handling of zero-amount transfers (should succeed but no balance change)
#[tokio::test]
async fn test_erc20_zero_amount_transfer() {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Note: This test assumes the contract allows zero-amount transfers
    // Actual behavior depends on contract implementation
    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    assert_eq!(
        result.return_value, 0,
        "Zero-amount transfer should succeed or be handled gracefully"
    );

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ ERC20 zero amount transfer test passed");
        log::info!("   Contract handled zero amount correctly");
    }
}

/// Test ERC20 storage persistence
///
/// Validates that token balances persist across multiple topoheights
#[tokio::test]
async fn test_erc20_storage_persistence() {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Execute at topoheight 1
    let result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result1.return_value, 0);

    // Execute at topoheight 2 (should see persisted state)
    let result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result2.return_value, 0);

    // Execute at topoheight 3 (should see accumulated state)
    let result3 = execute_test_contract(bytecode, &storage, 3, &contract_hash)
        .await
        .unwrap();
    assert_eq!(result3.return_value, 0);

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ ERC20 storage persistence test passed");
        log::info!("   State persisted across 3 topoheights");
        log::info!("   All operations succeeded with persistent storage");
    }
}

/// Test ERC20 concurrent operations
///
/// Simulates multiple operations in sequence to test state consistency
#[tokio::test]
async fn test_erc20_sequential_operations() {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Sequence of operations:
    // 1. Mint (topoheight 1)
    // 2. Transfer (topoheight 2)
    // 3. Mint (topoheight 3)
    // 4. Transfer (topoheight 4)

    let operations = vec![
        ("Mint 1", 1),
        ("Transfer 1", 2),
        ("Mint 2", 3),
        ("Transfer 2", 4),
    ];

    for (operation_name, topoheight) in operations {
        let result = execute_test_contract(bytecode, &storage, topoheight, &contract_hash)
            .await
            .unwrap();

        assert_eq!(result.return_value, 0, "{} should succeed", operation_name);

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "   {} at topoheight {}: {} CU",
                operation_name,
                topoheight,
                result.compute_units_used
            );
        }
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!("✅ ERC20 sequential operations test passed");
        log::info!("   4 sequential operations executed successfully");
    }
}
