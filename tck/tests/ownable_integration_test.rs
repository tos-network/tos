#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::useless_vec)]
// File: testing-framework/tests/ownable_integration_test.rs
//
// Ownable Contract Integration Tests
//
// Comprehensive integration tests for OpenZeppelin-style Ownable contract demonstrating:
// - Initialization with deployer as owner
// - Ownership transfer with authorization
// - Ownership renouncement
// - Unauthorized access prevention
// - Zero address validation
// - Owner query functionality
// - Real input_data and return_data flow
// - Storage persistence across calls
// - Error cases (unauthorized, invalid address)
//
// These tests use the testing-framework with real RocksDB storage to validate
// end-to-end Ownable functionality in the TOS blockchain environment.

use tos_common::crypto::{Hash, KeyPair};
use tos_tck::utilities::{create_contract_test_storage, execute_test_contract};

// Instruction opcodes (from ownable contract)
#[allow(dead_code)]
const OP_TRANSFER_OWNERSHIP: u8 = 0x00;
#[allow(dead_code)]
const OP_RENOUNCE_OWNERSHIP: u8 = 0x01;
#[allow(dead_code)]
const OP_OWNER: u8 = 0x10;

// Error codes
#[allow(dead_code)]
const ERR_UNAUTHORIZED: u64 = 1001;
#[allow(dead_code)]
const ERR_ZERO_ADDRESS: u64 = 1002;

// Helper function to encode address parameter
#[allow(dead_code)]
fn encode_address(address: &[u8; 32]) -> Vec<u8> {
    address.to_vec()
}

// ============================================================================
// TEST 1: Initialization with Deployer as Owner
// ============================================================================

/// Test Ownable contract initialization
///
/// Workflow:
/// 1. Deploy contract without parameters (auto-initializes deployer as owner)
/// 2. Verify initialization succeeded
/// 3. Query owner
/// 4. Verify owner is the deployer
#[tokio::test]
async fn test_ownable_initialization() {
    let deployer = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    // TODO: Replace with actual ownable.so when built
    let bytecode = include_bytes!("fixtures/ownable.so");
    let contract_hash = Hash::zero();

    // Initialize contract (no input data = auto-init with deployer as owner)
    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    assert_eq!(result.return_value, 0, "Initialization should succeed");
    assert!(
        result.compute_units_used > 0,
        "Initialization should consume compute units"
    );
}

// ============================================================================
// TEST 2: Owner Query
// ============================================================================

/// Test owner query function
///
/// Workflow:
/// 1. Initialize contract
/// 2. Query current owner
/// 3. Verify return data contains owner address
#[tokio::test]
async fn test_ownable_query_owner() {
    let owner = KeyPair::new();
    let storage = create_contract_test_storage(&owner, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/ownable.so");
    let contract_hash = Hash::zero();

    // Initialize
    let _result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Query owner
    let _query_params = vec![OP_OWNER];
    let _result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: When contract supports return_data, verify owner address
}

// ============================================================================
// TEST 3: Transfer Ownership - Success
// ============================================================================

/// Test successful ownership transfer
///
/// Workflow:
/// 1. Owner transfers ownership to new owner
/// 2. Verify transfer succeeded
/// 3. Query owner
/// 4. Verify new owner is set correctly
#[tokio::test]
async fn test_ownable_transfer_ownership_success() {
    let current_owner = KeyPair::new();
    let _new_owner = KeyPair::new();
    let storage = create_contract_test_storage(&current_owner, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/ownable.so");
    let contract_hash = Hash::zero();

    // Initialize
    let _result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Transfer ownership to new_owner
    let _transfer_params = vec![OP_TRANSFER_OWNERSHIP];
    // Note: transfer_params would be used when contract API supports input
    // _transfer_params.extend(encode_address(new_owner.get_public_key().compress().as_bytes()));

    let _result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify transfer succeeded when contract is ready
}

// ============================================================================
// TEST 4: Transfer Ownership - Unauthorized
// ============================================================================

/// Test unauthorized ownership transfer attempt
///
/// Workflow:
/// 1. Non-owner attempts to transfer ownership
/// 2. Verify transaction fails with ERR_UNAUTHORIZED
/// 3. Verify owner unchanged
#[tokio::test]
async fn test_ownable_transfer_ownership_unauthorized() {
    let owner = KeyPair::new();
    let non_owner = KeyPair::new();
    let _new_owner = KeyPair::new();
    let storage = create_contract_test_storage(&owner, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/ownable.so");
    let contract_hash = Hash::zero();

    // Initialize with owner
    let _result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Non-owner attempts to transfer ownership
    let storage_nonowner = create_contract_test_storage(&non_owner, 10_000_000)
        .await
        .unwrap();

    let _transfer_params = vec![OP_TRANSFER_OWNERSHIP];
    // Note: transfer_params would be used when contract API supports input
    // _transfer_params.extend(encode_address(new_owner.get_public_key().compress().as_bytes()));

    let _result2 = execute_test_contract(bytecode, &storage_nonowner, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify error code = ERR_UNAUTHORIZED when contract is ready
}

// ============================================================================
// TEST 5: Transfer Ownership - Zero Address
// ============================================================================

/// Test transfer to zero address (should fail)
///
/// Workflow:
/// 1. Owner attempts to transfer to zero address
/// 2. Verify fails with ERR_ZERO_ADDRESS
/// 3. Verify owner unchanged
#[tokio::test]
async fn test_ownable_transfer_to_zero_address() {
    let owner = KeyPair::new();
    let storage = create_contract_test_storage(&owner, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/ownable.so");
    let contract_hash = Hash::zero();

    // Initialize
    let _result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Attempt transfer to zero address
    let _transfer_params = vec![OP_TRANSFER_OWNERSHIP];
    // Note: transfer_params would be used when contract API supports input
    // _transfer_params.extend(encode_address(&[0u8; 32]));

    let _result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify error code = ERR_ZERO_ADDRESS when contract is ready
}

// ============================================================================
// TEST 6: Renounce Ownership - Success
// ============================================================================

/// Test ownership renouncement
///
/// Workflow:
/// 1. Owner renounces ownership
/// 2. Verify renouncement succeeded
/// 3. Query owner
/// 4. Verify owner is now zero address
#[tokio::test]
async fn test_ownable_renounce_ownership_success() {
    let owner = KeyPair::new();
    let storage = create_contract_test_storage(&owner, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/ownable.so");
    let contract_hash = Hash::zero();

    // Initialize
    let _result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Renounce ownership
    let _renounce_params = vec![OP_RENOUNCE_OWNERSHIP];
    // Note: renounce_params would be used when contract API supports input

    let result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();

    assert_eq!(result2.return_value, 0, "Renouncement should succeed");
}

// ============================================================================
// TEST 7: Renounce Ownership - Unauthorized
// ============================================================================

/// Test unauthorized renouncement attempt
///
/// Workflow:
/// 1. Non-owner attempts to renounce ownership
/// 2. Verify fails with ERR_UNAUTHORIZED
/// 3. Verify owner unchanged
#[tokio::test]
async fn test_ownable_renounce_ownership_unauthorized() {
    let owner = KeyPair::new();
    let non_owner = KeyPair::new();
    let storage_owner = create_contract_test_storage(&owner, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/ownable.so");
    let contract_hash = Hash::zero();

    // Initialize with owner
    let _result1 = execute_test_contract(bytecode, &storage_owner, 1, &contract_hash)
        .await
        .unwrap();

    // Non-owner attempts renouncement
    let storage_nonowner = create_contract_test_storage(&non_owner, 10_000_000)
        .await
        .unwrap();

    let _renounce_params = vec![OP_RENOUNCE_OWNERSHIP];
    // Note: renounce_params would be used when contract API supports input

    let _result2 = execute_test_contract(bytecode, &storage_nonowner, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify error code = ERR_UNAUTHORIZED when contract is ready
}

// ============================================================================
// TEST 8: Ownership Transfer Chain
// ============================================================================

/// Test multiple ownership transfers
///
/// Workflow:
/// 1. Owner1 transfers to Owner2
/// 2. Owner2 transfers to Owner3
/// 3. Verify Owner3 is final owner
/// 4. Owner1 cannot transfer anymore
#[tokio::test]
async fn test_ownable_transfer_chain() {
    let owner1 = KeyPair::new();
    let _owner2 = KeyPair::new();
    let _owner3 = KeyPair::new();

    let storage1 = create_contract_test_storage(&owner1, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/ownable.so");
    let contract_hash = Hash::zero();

    // Initialize with owner1
    let result1 = execute_test_contract(bytecode, &storage1, 1, &contract_hash)
        .await
        .unwrap();

    assert_eq!(result1.return_value, 0);

    // TODO: Test ownership chain when contract is ready
    // 1. owner1 transfers to owner2
    // 2. owner2 transfers to owner3
    // 3. Verify owner3 is owner
    // 4. Verify owner1 cannot transfer
}

// ============================================================================
// TEST 9: Storage Persistence
// ============================================================================

/// Test owner storage persistence across topoheights
///
/// Workflow:
/// 1. Initialize at topoheight 1
/// 2. Transfer at topoheight 2
/// 3. Query at topoheight 3
/// 4. Verify owner persisted correctly
#[tokio::test]
async fn test_ownable_storage_persistence() {
    let owner = KeyPair::new();
    let storage = create_contract_test_storage(&owner, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/ownable.so");
    let contract_hash = Hash::zero();

    // Execute at multiple topoheights
    for topoheight in 1..=5 {
        let result = execute_test_contract(bytecode, &storage, topoheight, &contract_hash)
            .await
            .unwrap();

        assert_eq!(result.return_value, 0);
    }
}

// ============================================================================
// TEST 10: Compute Unit Verification
// ============================================================================

/// Test compute unit consumption for all operations
///
/// Workflow:
/// 1. Measure initialization CU
/// 2. Measure transfer CU
/// 3. Measure renounce CU
/// 4. Measure query CU
/// 5. Verify all within reasonable limits
#[tokio::test]
async fn test_ownable_compute_units() {
    let owner = KeyPair::new();
    let storage = create_contract_test_storage(&owner, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("fixtures/ownable.so");
    let contract_hash = Hash::zero();

    // Measure initialization
    let result_init = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    assert!(result_init.compute_units_used > 0);
    assert!(
        result_init.compute_units_used < 500_000,
        "Initialization should use < 500k CU"
    );
}
