//! Integration tests for precompile cost estimation
//!
//! These tests verify that the cost estimation functions correctly calculate
//! compute units for precompile instructions in various scenarios.

use tos_daemon::tako_integration::precompile_cost::{
    costs, estimate_single_precompile_cost, estimate_transaction_precompile_cost,
    TransactionCostEstimator,
};

/// Test single Ed25519 precompile cost estimation
#[test]
fn test_ed25519_single_signature_cost() {
    // Ed25519 program ID: [3, 0, 0, ...]
    let program_id = [
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];

    // Instruction data: [num_signatures=1, padding=0, ...]
    let instruction_data = vec![1u8, 0u8];

    let cost = estimate_single_precompile_cost(&program_id, &instruction_data)
        .expect("Failed to estimate cost");

    // 1 signature × 2,280 CU = 2,280 CU
    assert_eq!(cost, costs::ED25519_COST);
}

/// Test batch Ed25519 cost estimation
#[test]
fn test_ed25519_batch_cost() {
    let program_id = [
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];

    // Test various batch sizes
    for num_signatures in [1, 5, 10, 20, 50] {
        let instruction_data = vec![num_signatures as u8, 0u8];

        let cost = estimate_single_precompile_cost(&program_id, &instruction_data)
            .expect("Failed to estimate cost");

        let expected = num_signatures as u64 * costs::ED25519_COST;
        assert_eq!(
            cost, expected,
            "Batch size {} should cost {} CU",
            num_signatures, expected
        );
    }
}

/// Test single secp256k1 precompile cost estimation
#[test]
fn test_secp256k1_single_signature_cost() {
    // secp256k1 program ID: [2, 0, 0, ...]
    let program_id = [
        2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];

    // Instruction data: [num_signatures=1, ...]
    let instruction_data = vec![1u8];

    let cost = estimate_single_precompile_cost(&program_id, &instruction_data)
        .expect("Failed to estimate cost");

    // 1 signature × 6,690 CU = 6,690 CU
    assert_eq!(cost, costs::SECP256K1_COST);
}

/// Test single secp256r1 precompile cost estimation
#[test]
fn test_secp256r1_single_signature_cost() {
    // secp256r1 program ID: [113, 0, 0, ...]
    let program_id = [
        113, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0,
    ];

    // Instruction data: [num_signatures=1, padding=0, ...]
    let instruction_data = vec![1u8, 0u8];

    let cost = estimate_single_precompile_cost(&program_id, &instruction_data)
        .expect("Failed to estimate cost");

    // 1 signature × 4,800 CU = 4,800 CU
    assert_eq!(cost, costs::SECP256R1_COST);
}

/// Test transaction with mixed precompile types
#[test]
fn test_mixed_precompile_transaction_cost() {
    // Ed25519 program ID
    let ed25519_program_id = [
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];

    // secp256k1 program ID
    let secp256k1_program_id = [
        2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];

    // secp256r1 program ID
    let secp256r1_program_id = [
        113, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0,
    ];

    // Regular contract program ID (not a precompile)
    let regular_program_id = [
        1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];

    // Instruction data
    let ed25519_data = vec![2u8, 0u8]; // 2 signatures
    let secp256k1_data = vec![1u8]; // 1 signature
    let secp256r1_data = vec![3u8, 0u8]; // 3 signatures
    let regular_data = vec![0u8; 100]; // Regular contract data

    // Build transaction instructions
    let instructions = vec![
        (&ed25519_program_id, ed25519_data.as_slice()),
        (&secp256k1_program_id, secp256k1_data.as_slice()),
        (&secp256r1_program_id, secp256r1_data.as_slice()),
        (&regular_program_id, regular_data.as_slice()),
    ];

    let cost =
        estimate_transaction_precompile_cost(&instructions).expect("Failed to estimate cost");

    // Expected: 2×Ed25519 + 1×secp256k1 + 3×secp256r1
    let expected = 2 * costs::ED25519_COST + costs::SECP256K1_COST + 3 * costs::SECP256R1_COST;
    assert_eq!(cost, expected);
}

/// Test transaction with only regular contracts (no precompiles)
#[test]
fn test_non_precompile_transaction_cost() {
    let regular_program_id_1 = [1u8; 32];
    let regular_program_id_2 = [2u8; 32];

    let data1 = vec![0u8; 50];
    let data2 = vec![0u8; 100];

    let instructions = vec![
        (&regular_program_id_1, data1.as_slice()),
        (&regular_program_id_2, data2.as_slice()),
    ];

    let cost =
        estimate_transaction_precompile_cost(&instructions).expect("Failed to estimate cost");

    // No precompiles, so cost should be 0
    assert_eq!(cost, 0);
}

/// Test TransactionCostEstimator with base cost and contract execution cost
#[test]
fn test_transaction_cost_estimator() {
    let estimator = TransactionCostEstimator::with_base_cost(5_000);

    let program_id = [
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];
    let instruction_data = vec![5u8, 0u8]; // 5 signatures

    let instructions = vec![(&program_id, instruction_data.as_slice())];

    let contract_execution_cost = 10_000u64;
    let total = estimator
        .estimate_total_cost(&instructions, contract_execution_cost)
        .expect("Failed to estimate total cost");

    // Expected: base (5,000) + contract (10,000) + precompile (5 × 2,280)
    let expected = 5_000 + 10_000 + 5 * costs::ED25519_COST;
    assert_eq!(total, expected);
}

/// Test zero signatures edge case
#[test]
fn test_zero_signatures() {
    let program_id = [
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];
    let instruction_data = vec![0u8, 0u8]; // 0 signatures

    let cost = estimate_single_precompile_cost(&program_id, &instruction_data)
        .expect("Failed to estimate cost");

    assert_eq!(cost, 0);
}

/// Test empty instruction data edge case
#[test]
fn test_empty_instruction_data() {
    let program_id = [
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];
    let instruction_data = vec![];

    let cost = estimate_single_precompile_cost(&program_id, &instruction_data)
        .expect("Failed to estimate cost");

    assert_eq!(cost, 0);
}

/// Test large batch cost estimation (stress test)
#[test]
fn test_large_batch_cost() {
    let program_id = [
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];

    // Test with 255 signatures (max u8 value)
    let instruction_data = vec![255u8, 0u8];

    let cost = estimate_single_precompile_cost(&program_id, &instruction_data)
        .expect("Failed to estimate cost");

    let expected = 255 * costs::ED25519_COST;
    assert_eq!(cost, expected);
}

/// Test cost estimation with all three precompile types in one transaction
#[test]
fn test_all_precompile_types_transaction() {
    let ed25519_id = [3u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let secp256k1_id = [2u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let secp256r1_id = [113u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    let ed25519_data = vec![10u8, 0u8]; // 10 signatures
    let secp256k1_data = vec![5u8]; // 5 signatures
    let secp256r1_data = vec![7u8, 0u8]; // 7 signatures

    let instructions = vec![
        (&ed25519_id, ed25519_data.as_slice()),
        (&secp256k1_id, secp256k1_data.as_slice()),
        (&secp256r1_id, secp256r1_data.as_slice()),
    ];

    let cost =
        estimate_transaction_precompile_cost(&instructions).expect("Failed to estimate cost");

    // Expected: 10×2,280 + 5×6,690 + 7×4,800
    let expected = 10 * costs::ED25519_COST + 5 * costs::SECP256K1_COST + 7 * costs::SECP256R1_COST;
    assert_eq!(cost, expected);
    assert_eq!(cost, 22_800 + 33_450 + 33_600); // 89,850 CU
}

/// Test invalid program ID (should return error)
#[test]
fn test_invalid_program_id() {
    // Invalid program ID (not a precompile)
    let program_id = [
        5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];

    let instruction_data = vec![1u8, 0u8];

    let result = estimate_single_precompile_cost(&program_id, &instruction_data);

    assert!(
        result.is_err(),
        "Should return error for invalid program ID"
    );
}

/// Test cost saturation (verify no overflow)
#[test]
fn test_cost_saturation() {
    let program_id = [
        2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];

    // Test with max u8 value for secp256k1 (highest cost precompile)
    let instruction_data = vec![255u8];

    let cost = estimate_single_precompile_cost(&program_id, &instruction_data)
        .expect("Failed to estimate cost");

    let expected = 255 * costs::SECP256K1_COST;
    assert_eq!(cost, expected);
    assert_eq!(cost, 1_705_950); // Should not overflow
}
