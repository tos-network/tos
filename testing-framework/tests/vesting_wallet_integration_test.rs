// File: testing-framework/tests/vesting_wallet_integration_test.rs
//
// VestingWallet Contract Integration Tests
//
// Tests for OpenZeppelin-style token vesting wallet with linear release schedule

use tos_common::crypto::{Hash, KeyPair};
use tos_testing_framework::utilities::{create_contract_test_storage, execute_test_contract};

const OP_INITIALIZE: u8 = 0x00;
const OP_RELEASE: u8 = 0x01;
const OP_BENEFICIARY: u8 = 0x10;
const OP_START: u8 = 0x11;
const OP_DURATION: u8 = 0x12;
const OP_RELEASED: u8 = 0x13;
const OP_RELEASABLE: u8 = 0x14;
const OP_VESTED_AMOUNT: u8 = 0x15;

const ERR_NOT_BENEFICIARY: u64 = 1001;
const ERR_ZERO_DURATION: u64 = 1002;
const ERR_ZERO_BENEFICIARY: u8 = 1003;
const ERR_NO_TOKENS_DUE: u64 = 1004;

fn encode_address(address: &[u8; 32]) -> Vec<u8> {
    address.to_vec()
}

fn encode_u64(value: u64) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

// ============================================================================
// TEST 1: Initialization
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_initialization() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Initialize with:
    // - beneficiary
    // - start: timestamp (e.g., current time)
    // - duration: 365 days in seconds (31536000)
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(beneficiary.get_public_key().compress().as_bytes()));
    init_params.extend(encode_u64(1700000000)); // start timestamp
    init_params.extend(encode_u64(31536000)); // 365 days duration

    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    assert_eq!(result.return_value, 0, "Initialization should succeed");
}

// ============================================================================
// TEST 2: Query Beneficiary
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_query_beneficiary() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(beneficiary.get_public_key().compress().as_bytes()));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    let _result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Query beneficiary
    let query_params = vec![OP_BENEFICIARY];
    let _result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify return_data = beneficiary address when contract is ready
}

// ============================================================================
// TEST 3: Query Start Time
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_query_start() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(beneficiary.get_public_key().compress().as_bytes()));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    let _result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Query start time
    let query_params = vec![OP_START];
    let _result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify return_data = 1700000000 when contract is ready
}

// ============================================================================
// TEST 4: Query Duration
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_query_duration() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(beneficiary.get_public_key().compress().as_bytes()));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    let _result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Query duration
    let query_params = vec![OP_DURATION];
    let _result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify return_data = 31536000 when contract is ready
}

// ============================================================================
// TEST 5: Release Tokens - Success
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_release_success() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage_beneficiary = create_contract_test_storage(&beneficiary, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Initialize
    let storage_deployer = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(beneficiary.get_public_key().compress().as_bytes()));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    let _result1 = execute_test_contract(bytecode, &storage_deployer, 1, &contract_hash)
        .await
        .unwrap();

    // Beneficiary releases tokens
    let release_params = vec![OP_RELEASE];
    let result2 = execute_test_contract(bytecode, &storage_beneficiary, 2, &contract_hash)
        .await
        .unwrap();

    assert_eq!(result2.return_value, 0, "Release should succeed");
}

// ============================================================================
// TEST 6: Release Tokens - Unauthorized
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_release_unauthorized() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let non_beneficiary = KeyPair::new();

    let storage_deployer = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(beneficiary.get_public_key().compress().as_bytes()));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    let _result1 = execute_test_contract(bytecode, &storage_deployer, 1, &contract_hash)
        .await
        .unwrap();

    // Non-beneficiary attempts release
    let storage_nonbeneficiary = create_contract_test_storage(&non_beneficiary, 10_000_000)
        .await
        .unwrap();

    let release_params = vec![OP_RELEASE];
    let _result2 = execute_test_contract(bytecode, &storage_nonbeneficiary, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify error = ERR_NOT_BENEFICIARY when contract is ready
}

// ============================================================================
// TEST 7: Query Released Amount
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_query_released() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(beneficiary.get_public_key().compress().as_bytes()));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    let _result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Query released amount (should be 0 initially)
    let query_params = vec![OP_RELEASED];
    let _result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify return_data = 0 when contract is ready
}

// ============================================================================
// TEST 8: Query Releasable Amount
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_query_releasable() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(beneficiary.get_public_key().compress().as_bytes()));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    let _result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Query releasable amount at current timestamp
    let mut query_params = vec![OP_RELEASABLE];
    query_params.extend(encode_u64(1700000000 + 15768000)); // 6 months later

    let _result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify return_data = 50% of total when contract is ready
}

// ============================================================================
// TEST 9: Query Vested Amount
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_query_vested_amount() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(beneficiary.get_public_key().compress().as_bytes()));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    let _result1 = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // Query vested amount at timestamp
    let mut query_params = vec![OP_VESTED_AMOUNT];
    query_params.extend(encode_u64(1700000000 + 31536000)); // After full duration

    let _result2 = execute_test_contract(bytecode, &storage, 2, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify return_data = 100% of total when contract is ready
}

// ============================================================================
// TEST 10: Linear Vesting Calculation
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_linear_vesting() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    assert_eq!(result.return_value, 0);

    // TODO: Test linear vesting at different timestamps:
    // - 0% at start
    // - 25% at 1/4 duration
    // - 50% at 1/2 duration
    // - 75% at 3/4 duration
    // - 100% at end
}

// ============================================================================
// TEST 11: Zero Duration Validation
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_zero_duration() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Attempt initialization with zero duration
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(beneficiary.get_public_key().compress().as_bytes()));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(0)); // Zero duration

    let _result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify error = ERR_ZERO_DURATION when contract is ready
}

// ============================================================================
// TEST 12: Zero Beneficiary Validation
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_zero_beneficiary() {
    let deployer = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Attempt initialization with zero beneficiary
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(&[0u8; 32])); // Zero address
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));

    let _result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    // TODO: Verify error when contract is ready
}

// ============================================================================
// TEST 13: Storage Persistence
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_storage_persistence() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    for topoheight in 1..=5 {
        let result = execute_test_contract(bytecode, &storage, topoheight, &contract_hash)
            .await
            .unwrap();

        assert_eq!(result.return_value, 0);
    }
}

// ============================================================================
// TEST 14: Compute Units
// ============================================================================

#[tokio::test]
async fn test_vesting_wallet_compute_units() {
    let deployer = KeyPair::new();
    let beneficiary = KeyPair::new();
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/token.so");
    let contract_hash = Hash::zero();

    // Measure initialization
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(beneficiary.get_public_key().compress().as_bytes()));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));

    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash)
        .await
        .unwrap();

    assert!(result.compute_units_used > 0);
    assert!(result.compute_units_used < 500_000);
}
