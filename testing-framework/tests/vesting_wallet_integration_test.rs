// File: testing-framework/tests/vesting_wallet_integration_test.rs
//
// VestingWallet Contract Integration Tests
//
// Tests for OpenZeppelin-style token vesting wallet with linear release schedule

use tos_common::crypto::{Hash, KeyPair};
use tos_common::serializer::Serializer;
use tos_testing_framework::utilities::{
    create_contract_test_storage, execute_test_contract_with_input,
};

#[allow(dead_code)]
const OP_INITIALIZE: u8 = 0x00;
#[allow(dead_code)]
const OP_RELEASE: u8 = 0x01;
#[allow(dead_code)]
const OP_VESTED_AMOUNT: u8 = 0x10;
#[allow(dead_code)]
const OP_RELEASABLE: u8 = 0x11;
#[allow(dead_code)]
const OP_RELEASED: u8 = 0x12;
#[allow(dead_code)]
const OP_BENEFICIARY: u8 = 0x13;
#[allow(dead_code)]
const OP_START: u8 = 0x14;
#[allow(dead_code)]
const OP_DURATION: u8 = 0x15;
#[allow(dead_code)]
const OP_END: u8 = 0x16;
#[allow(dead_code)]
const OP_TOTAL_ALLOCATION: u8 = 0x17;

#[allow(dead_code)]
const ERR_NOT_BENEFICIARY: u64 = 1001;
#[allow(dead_code)]
const ERR_NO_TOKENS: u64 = 1002;
#[allow(dead_code)]
const ERR_ALREADY_INITIALIZED: u64 = 1003;
#[allow(dead_code)]
const ERR_NOT_INITIALIZED: u64 = 1004;
#[allow(dead_code)]
const ERR_INVALID_INSTRUCTION: u64 = 1005;
#[allow(dead_code)]
const ERR_INVALID_PARAMS: u64 = 1006;

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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Initialize with:
    // - beneficiary
    // - start: timestamp (e.g., current time)
    // - duration: 365 days in seconds (31536000)
    // - total_allocation: 1,000,000 tokens
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000)); // start timestamp
    init_params.extend(encode_u64(31536000)); // 365 days duration
    init_params.extend(encode_u64(1_000_000)); // total allocation

    let result = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    init_params.extend(encode_u64(1_000_000));
    let _result1 = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
    .await
    .unwrap();

    // Query beneficiary
    let query_params = vec![OP_BENEFICIARY];
    let _result2 = execute_test_contract_with_input(
        bytecode,
        &storage,
        2,
        &contract_hash,
        &deployer_hash,
        &query_params,
    )
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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    init_params.extend(encode_u64(1_000_000));
    let _result1 = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
    .await
    .unwrap();

    // Query start time
    let query_params = vec![OP_START];
    let _result2 = execute_test_contract_with_input(
        bytecode,
        &storage,
        2,
        &contract_hash,
        &deployer_hash,
        &query_params,
    )
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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    init_params.extend(encode_u64(1_000_000));
    let _result1 = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
    .await
    .unwrap();

    // Query duration
    let query_params = vec![OP_DURATION];
    let _result2 = execute_test_contract_with_input(
        bytecode,
        &storage,
        2,
        &contract_hash,
        &deployer_hash,
        &query_params,
    )
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
    // Use shared storage so both transactions see the same state
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert keypairs to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();
    let beneficiary_hash =
        Hash::from_bytes(beneficiary.get_public_key().compress().as_bytes()).unwrap();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1)); // start at timestamp 1 (vesting started long ago)
    init_params.extend(encode_u64(1)); // duration 1 second (already fully vested)
    init_params.extend(encode_u64(1_000_000));
    let _result1 = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
    .await
    .unwrap();

    // Beneficiary releases tokens (should succeed since fully vested)
    let release_params = vec![OP_RELEASE];
    let result2 = execute_test_contract_with_input(
        bytecode,
        &storage,
        2,
        &contract_hash,
        &beneficiary_hash,
        &release_params,
    )
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
    let _non_beneficiary = KeyPair::new();

    // Use shared storage so state persists
    let storage = create_contract_test_storage(&deployer, 10_000_000)
        .await
        .unwrap();

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    init_params.extend(encode_u64(1_000_000));
    let _result1 = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
    .await
    .unwrap();

    // Non-beneficiary attempts release
    let release_params = vec![OP_RELEASE];
    let _result2 = execute_test_contract_with_input(
        bytecode,
        &storage,
        2,
        &contract_hash,
        &deployer_hash,
        &release_params,
    )
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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    init_params.extend(encode_u64(1_000_000));
    let _result1 = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
    .await
    .unwrap();

    // Query released amount (should be 0 initially)
    let query_params = vec![OP_RELEASED];
    let _result2 = execute_test_contract_with_input(
        bytecode,
        &storage,
        2,
        &contract_hash,
        &deployer_hash,
        &query_params,
    )
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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    init_params.extend(encode_u64(1_000_000));
    let _result1 = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
    .await
    .unwrap();

    // Query releasable amount at current timestamp
    let mut query_params = vec![OP_RELEASABLE];
    query_params.extend(encode_u64(1700000000 + 15768000)); // 6 months later

    let _result2 = execute_test_contract_with_input(
        bytecode,
        &storage,
        2,
        &contract_hash,
        &deployer_hash,
        &query_params,
    )
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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Initialize
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    init_params.extend(encode_u64(1_000_000));
    let _result1 = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
    .await
    .unwrap();

    // Query vested amount at timestamp
    let mut query_params = vec![OP_VESTED_AMOUNT];
    query_params.extend(encode_u64(1700000000 + 31536000)); // After full duration

    let _result2 = execute_test_contract_with_input(
        bytecode,
        &storage,
        2,
        &contract_hash,
        &deployer_hash,
        &query_params,
    )
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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Initialize vesting wallet
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000)); // start
    init_params.extend(encode_u64(1000)); // duration: 1000 seconds for easier testing
    init_params.extend(encode_u64(1_000_000)); // total allocation

    let result = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
    .await
    .unwrap();

    assert_eq!(result.return_value, 0, "Initialization should succeed");

    // Test vested amount at different timestamps:
    // - 0% at start (1700000000)
    let mut query_params = vec![OP_VESTED_AMOUNT];
    query_params.extend(encode_u64(1700000000)); // exactly at start
    let result = execute_test_contract_with_input(
        bytecode,
        &storage,
        2,
        &contract_hash,
        &deployer_hash,
        &query_params,
    )
    .await
    .unwrap();
    assert_eq!(result.return_value, 0);

    // - 25% at 1/4 duration (1700000000 + 250)
    let mut query_params = vec![OP_VESTED_AMOUNT];
    query_params.extend(encode_u64(1700000250));
    let _result = execute_test_contract_with_input(
        bytecode,
        &storage,
        3,
        &contract_hash,
        &deployer_hash,
        &query_params,
    )
    .await
    .unwrap();
    // TODO: Verify return_data = 250,000 (25%)

    // - 50% at 1/2 duration (1700000000 + 500)
    let mut query_params = vec![OP_VESTED_AMOUNT];
    query_params.extend(encode_u64(1700000500));
    let _result = execute_test_contract_with_input(
        bytecode,
        &storage,
        4,
        &contract_hash,
        &deployer_hash,
        &query_params,
    )
    .await
    .unwrap();
    // TODO: Verify return_data = 500,000 (50%)

    // - 100% at end (1700000000 + 1000)
    let mut query_params = vec![OP_VESTED_AMOUNT];
    query_params.extend(encode_u64(1700001000));
    let _result = execute_test_contract_with_input(
        bytecode,
        &storage,
        5,
        &contract_hash,
        &deployer_hash,
        &query_params,
    )
    .await
    .unwrap();
    // TODO: Verify return_data = 1,000,000 (100%)
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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Attempt initialization with zero duration
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(0)); // Zero duration
    init_params.extend(encode_u64(1_000_000));

    let _result = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Attempt initialization with zero beneficiary
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(&[0u8; 32])); // Zero address
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    init_params.extend(encode_u64(1_000_000));

    let _result = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Initialize once at topoheight 1
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    init_params.extend(encode_u64(1_000_000));

    let result = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
    .await
    .unwrap();
    assert_eq!(result.return_value, 0, "Initialization should succeed");

    // Query beneficiary at different topoheights to verify storage persistence
    for topoheight in 2..=5 {
        let query_params = vec![OP_BENEFICIARY];
        let result = execute_test_contract_with_input(
            bytecode,
            &storage,
            topoheight,
            &contract_hash,
            &deployer_hash,
            &query_params,
        )
        .await
        .unwrap();

        assert_eq!(
            result.return_value, 0,
            "Query should succeed at topoheight {}",
            topoheight
        );
        // TODO: Verify return_data matches beneficiary address
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

    let bytecode = include_bytes!("../../daemon/tests/fixtures/vesting_wallet.so");
    let contract_hash = Hash::zero();

    // Convert deployer public key to Hash for tx_sender
    let deployer_hash = Hash::from_bytes(deployer.get_public_key().compress().as_bytes()).unwrap();

    // Measure initialization
    let mut init_params = vec![OP_INITIALIZE];
    init_params.extend(encode_address(
        beneficiary.get_public_key().compress().as_bytes(),
    ));
    init_params.extend(encode_u64(1700000000));
    init_params.extend(encode_u64(31536000));
    init_params.extend(encode_u64(1_000_000));

    let result = execute_test_contract_with_input(
        bytecode,
        &storage,
        1,
        &contract_hash,
        &deployer_hash,
        &init_params,
    )
    .await
    .unwrap();

    assert!(result.compute_units_used > 0);
    assert!(result.compute_units_used < 500_000);
}
