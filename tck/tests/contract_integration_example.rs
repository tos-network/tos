#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::useless_vec)]
// File: testing-framework/tests/contract_integration_example.rs
//
// Example Contract Integration Test
//
// This test demonstrates how to use the testing-framework to test TAKO smart contracts
// with real RocksDB storage, showing the recommended approach for end-to-end contract testing.

use tos_common::crypto::{Hash, KeyPair};
use tos_tck::utilities::{contract_exists, create_contract_test_storage, execute_test_contract};

/// Test executing a simple "hello world" contract
///
/// This demonstrates the basic workflow:
/// 1. Create test storage with funded account
/// 2. Execute contract bytecode
/// 3. Verify execution result
#[tokio::test]
async fn test_hello_world_contract() {
    // Setup: Create storage with funded account
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000)
        .await
        .unwrap();

    // Load contract bytecode from fixture
    let bytecode = include_bytes!("../../daemon/tests/fixtures/hello_world.so");

    // Execute contract at topoheight 1
    let contract_hash = Hash::zero(); // For testing, any hash works with execute_simple
    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Verify execution succeeded
    assert_eq!(result.return_value, 0, "Contract should return 0 (success)");
    assert!(
        result.compute_units_used > 0,
        "Contract should consume compute units"
    );

    if log::log_enabled!(log::Level::Info) {
        log::info!("Hello world contract executed successfully!");
        log::info!("Compute units used: {}", result.compute_units_used);
        log::info!("Return value: {}", result.return_value);
    }
}

/// Test contract existence check
///
/// This demonstrates checking if a contract exists at a given topoheight.
#[tokio::test]
async fn test_contract_existence_check() {
    // Create test storage
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000)
        .await
        .unwrap();

    let fake_hash = Hash::zero();

    // Check if non-existent contract exists
    let exists = contract_exists(&storage, fake_hash, 1).await.unwrap();
    assert!(!exists, "Non-deployed contract should not exist");
}

/// Test contract execution with compute unit limits
///
/// This demonstrates that contracts consume compute units and respect limits.
#[tokio::test]
async fn test_contract_compute_units() {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/hello_world.so");
    let contract_hash = Hash::zero();

    // Execute contract
    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Verify compute units were consumed
    assert!(
        result.compute_units_used > 0,
        "Contract execution should consume compute units"
    );

    // Verify it didn't exceed reasonable limits
    const MAX_EXPECTED_UNITS: u64 = 1_000_000;
    assert!(
        result.compute_units_used < MAX_EXPECTED_UNITS,
        "Hello world should use less than {} compute units, used: {}",
        MAX_EXPECTED_UNITS,
        result.compute_units_used
    );
}

/// Test multiple contract executions with different topoheights
///
/// This demonstrates that contracts can be executed at different topoheights
/// to test versioned behavior.
#[tokio::test]
async fn test_contract_execution_at_different_topoheights() {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/hello_world.so");
    let contract_hash = Hash::zero();

    // Execute at different topoheights
    for topoheight in [1, 10, 100, 1000] {
        let result = execute_test_contract(bytecode, &storage, topoheight, &contract_hash)
            .await
            .unwrap();

        assert_eq!(
            result.return_value, 0,
            "Contract should succeed at topoheight {}",
            topoheight
        );
    }
}
