//! Precompile Cost Estimation Integration
//!
//! This module provides utilities for estimating and accounting for precompile
//! compute costs during transaction validation and execution.

// Note: Hash and InvokeContext not currently needed but available for future use

/// Precompile compute costs
pub mod costs {
    // SVM-aligned costs (cross-chain compatibility)

    /// Ed25519 signature verification cost per signature (SVM-aligned)
    pub const ED25519_COST: u64 = 2_280;

    /// secp256k1 signature verification cost per signature (SVM-aligned)
    pub const SECP256K1_COST: u64 = 6_690;

    /// secp256r1 (P-256) signature verification cost per signature (SVM-aligned)
    pub const SECP256R1_COST: u64 = 4_800;

    // TOS native precompile costs (hardware calibrated)

    /// Schnorr Ristretto signature verification cost per signature (hardware calibrated)
    /// Based on actual hardware benchmarks: 68.509 µs median on Apple Silicon
    pub const SCHNORR_RISTRETTO_COST: u64 = 69_000;

    /// Threshold multisig base cost (overhead for threshold verification logic)
    pub const THRESHOLD_MULTISIG_BASE_COST: u64 = 20_000;

    /// Threshold multisig cost per verified signature (hardware calibrated)
    /// Formula: BASE_COST + (threshold × PER_SIGNATURE_COST)
    pub const THRESHOLD_MULTISIG_PER_SIGNATURE_COST: u64 = 86_000;

    /// BLS Fast Aggregate base cost (pairing computation)
    /// Based on actual hardware benchmarks: 843.42 µs median on Apple Silicon
    pub const BLS_FAST_AGGREGATE_BASE_COST: u64 = 843_000;

    /// BLS Fast Aggregate cost per signer (public key deserialization + aggregation)
    /// Formula: BASE_COST + (num_signers × PER_SIGNER_COST)
    pub const BLS_FAST_AGGREGATE_PER_SIGNER_COST: u64 = 70_000;
}

/// Estimate total compute units required for all precompile instructions in a transaction
///
/// This function should be called during transaction validation/scheduling to
/// reserve compute budget for precompile verification.
///
/// # Arguments
/// * `instructions` - List of (program_id, instruction_data) tuples from the transaction
///
/// # Returns
/// Total compute units required for all precompile instructions
///
/// # Note
/// Precompiles are FREE at runtime (0 CU consumed during execution),
/// but cost is accounted during transaction validation/scheduling.
pub fn estimate_transaction_precompile_cost(
    instructions: &[(&[u8; 32], &[u8])],
) -> Result<u64, Box<dyn std::error::Error>> {
    let mut total_cost = 0u64;

    for (program_id, instruction_data) in instructions {
        // Check if this is a precompile
        if is_precompile_program_id(program_id) {
            let cost = estimate_single_precompile_cost(program_id, instruction_data)?;
            total_cost = total_cost.saturating_add(cost);
        }
    }

    Ok(total_cost)
}

/// Estimate compute cost for a single precompile instruction
///
/// # Arguments
/// * `program_id` - The precompile program ID
/// * `instruction_data` - The instruction data containing signature count
///
/// # Returns
/// Compute units required for this precompile instruction
pub fn estimate_single_precompile_cost(
    program_id: &[u8; 32],
    instruction_data: &[u8],
) -> Result<u64, Box<dyn std::error::Error>> {
    if instruction_data.is_empty() {
        return Ok(0);
    }

    // Check precompile type and calculate cost accordingly
    match program_id[0] {
        // Ed25519: [num_signatures: u8, padding: u8, ...]
        3 if program_id[1..].iter().all(|&b| b == 0) => {
            let num_sigs = instruction_data[0] as u64;
            Ok(num_sigs.saturating_mul(costs::ED25519_COST))
        }
        // secp256k1: [num_signatures: u8, ...]
        2 if program_id[1..].iter().all(|&b| b == 0) => {
            let num_sigs = instruction_data[0] as u64;
            Ok(num_sigs.saturating_mul(costs::SECP256K1_COST))
        }
        // secp256r1: [num_signatures: u8, padding: u8, ...]
        113 if program_id[1..].iter().all(|&b| b == 0) => {
            let num_sigs = instruction_data[0] as u64;
            Ok(num_sigs.saturating_mul(costs::SECP256R1_COST))
        }
        // Schnorr Ristretto: [num_signatures: u8, ...]
        4 if program_id[1..].iter().all(|&b| b == 0) => {
            let num_sigs = instruction_data[0] as u64;
            Ok(num_sigs.saturating_mul(costs::SCHNORR_RISTRETTO_COST))
        }
        // Threshold Multisig: [threshold: u8, num_keys: u8, ...]
        // Cost = BASE_COST + (threshold × PER_SIGNATURE_COST)
        5 if program_id[1..].iter().all(|&b| b == 0) => {
            let threshold = instruction_data[0] as u64;
            Ok(costs::THRESHOLD_MULTISIG_BASE_COST
                .saturating_add(threshold.saturating_mul(costs::THRESHOLD_MULTISIG_PER_SIGNATURE_COST)))
        }
        // BLS Fast Aggregate: [num_signers: u8, ...]
        // Cost = BASE_COST + (num_signers × PER_SIGNER_COST)
        6 if program_id[1..].iter().all(|&b| b == 0) => {
            let num_signers = instruction_data[0] as u64;
            Ok(costs::BLS_FAST_AGGREGATE_BASE_COST
                .saturating_add(num_signers.saturating_mul(costs::BLS_FAST_AGGREGATE_PER_SIGNER_COST)))
        }
        _ => Err("Unknown precompile program ID".into()),
    }
}

/// Check if a program ID is a precompile
fn is_precompile_program_id(program_id: &[u8; 32]) -> bool {
    // Check if first byte matches any precompile ID, and rest are zeros
    matches!(program_id[0], 2 | 3 | 4 | 5 | 6 | 113) && program_id[1..].iter().all(|&b| b == 0)
}

/// Transaction cost estimator that includes precompile costs
///
/// Use this to calculate total transaction cost including precompiles:
///
/// ```rust,ignore
/// let base_cost = 5_000; // Base transaction cost
/// let contract_cost = estimate_contract_execution_cost(&transaction);
/// let precompile_cost = estimate_transaction_precompile_cost(&instructions)?;
/// let total_cost = base_cost + contract_cost + precompile_cost;
/// ```
pub struct TransactionCostEstimator {
    base_cost: u64,
}

impl TransactionCostEstimator {
    /// Create a new cost estimator with default base cost
    pub fn new() -> Self {
        Self {
            base_cost: 5_000, // Default base transaction cost
        }
    }

    /// Create a new cost estimator with custom base cost
    pub fn with_base_cost(base_cost: u64) -> Self {
        Self { base_cost }
    }

    /// Estimate total transaction cost including precompiles
    ///
    /// # Arguments
    /// * `instructions` - Transaction instructions (program_id, data)
    /// * `contract_execution_cost` - Estimated cost for regular contract execution
    ///
    /// # Returns
    /// Total compute units required for the transaction
    pub fn estimate_total_cost(
        &self,
        instructions: &[(&[u8; 32], &[u8])],
        contract_execution_cost: u64,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let precompile_cost = estimate_transaction_precompile_cost(instructions)?;

        Ok(self
            .base_cost
            .saturating_add(contract_execution_cost)
            .saturating_add(precompile_cost))
    }
}

impl Default for TransactionCostEstimator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_ed25519_cost() {
        let program_id = [
            3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        // 2 signatures
        let instruction_data = vec![2u8, 0]; // num_signatures=2, padding

        let cost = estimate_single_precompile_cost(&program_id, &instruction_data).unwrap();
        assert_eq!(cost, 2 * costs::ED25519_COST);
    }

    #[test]
    fn test_estimate_secp256k1_cost() {
        let program_id = [
            2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        // 3 signatures
        let instruction_data = vec![3u8];

        let cost = estimate_single_precompile_cost(&program_id, &instruction_data).unwrap();
        assert_eq!(cost, 3 * costs::SECP256K1_COST);
    }

    #[test]
    fn test_estimate_secp256r1_cost() {
        let program_id = [
            113, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ];

        // 1 signature
        let instruction_data = vec![1u8, 0]; // num_signatures=1, padding

        let cost = estimate_single_precompile_cost(&program_id, &instruction_data).unwrap();
        assert_eq!(cost, costs::SECP256R1_COST);
    }

    #[test]
    fn test_estimate_transaction_cost_mixed() {
        let ed25519_program_id = [
            3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        let secp256k1_program_id = [
            2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        let regular_program_id = [
            1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        let ed25519_data = vec![2u8, 0]; // 2 signatures
        let secp256k1_data = vec![1u8]; // 1 signature
        let regular_data = vec![0u8; 100]; // Regular contract data

        let instructions = vec![
            (&ed25519_program_id, ed25519_data.as_slice()),
            (&secp256k1_program_id, secp256k1_data.as_slice()),
            (&regular_program_id, regular_data.as_slice()),
        ];

        let cost = estimate_transaction_precompile_cost(&instructions).unwrap();

        // Expected: 2 * ED25519 + 1 * secp256k1
        let expected = 2 * costs::ED25519_COST + costs::SECP256K1_COST;
        assert_eq!(cost, expected);
    }

    #[test]
    fn test_transaction_cost_estimator() {
        let estimator = TransactionCostEstimator::with_base_cost(5_000);

        let program_id = [
            3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let instruction_data = vec![1u8, 0]; // 1 signature

        let instructions = vec![(&program_id, instruction_data.as_slice())];

        let contract_cost = 10_000u64;
        let total = estimator
            .estimate_total_cost(&instructions, contract_cost)
            .unwrap();

        // Expected: base (5,000) + contract (10,000) + precompile (2,280)
        assert_eq!(total, 5_000 + 10_000 + costs::ED25519_COST);
    }

    #[test]
    fn test_empty_instruction() {
        let program_id = [
            3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let instruction_data = vec![];

        let cost = estimate_single_precompile_cost(&program_id, &instruction_data).unwrap();
        assert_eq!(cost, 0);
    }

    #[test]
    fn test_zero_signatures() {
        let program_id = [
            3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let instruction_data = vec![0u8, 0]; // num_signatures=0

        let cost = estimate_single_precompile_cost(&program_id, &instruction_data).unwrap();
        assert_eq!(cost, 0);
    }

    #[test]
    fn test_estimate_schnorr_ristretto_cost() {
        let program_id = [
            4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        // 3 signatures
        let instruction_data = vec![3u8];

        let cost = estimate_single_precompile_cost(&program_id, &instruction_data).unwrap();
        assert_eq!(cost, 3 * costs::SCHNORR_RISTRETTO_COST);
    }

    #[test]
    fn test_estimate_threshold_multisig_cost() {
        let program_id = [
            5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        // 2-of-3 threshold: threshold=2, num_keys=3
        let instruction_data = vec![2u8, 3];

        let cost = estimate_single_precompile_cost(&program_id, &instruction_data).unwrap();
        // Expected: BASE_COST + (threshold × PER_SIGNATURE_COST)
        let expected =
            costs::THRESHOLD_MULTISIG_BASE_COST + 2 * costs::THRESHOLD_MULTISIG_PER_SIGNATURE_COST;
        assert_eq!(cost, expected);
        assert_eq!(cost, 192_000); // 20K + 2 × 86K
    }

    #[test]
    fn test_estimate_bls_fast_aggregate_cost() {
        let program_id = [
            6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        // 10 signers
        let instruction_data = vec![10u8];

        let cost = estimate_single_precompile_cost(&program_id, &instruction_data).unwrap();
        // Expected: BASE_COST + (num_signers × PER_SIGNER_COST)
        let expected =
            costs::BLS_FAST_AGGREGATE_BASE_COST + 10 * costs::BLS_FAST_AGGREGATE_PER_SIGNER_COST;
        assert_eq!(cost, expected);
        assert_eq!(cost, 1_543_000); // 843K + 10 × 70K
    }

    #[test]
    fn test_estimate_transaction_cost_all_precompiles() {
        // Program IDs for all 6 precompiles
        let ed25519_program_id = [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let secp256k1_program_id = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let secp256r1_program_id = [113, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let schnorr_program_id = [4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let threshold_program_id = [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let bls_program_id = [6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

        // Instruction data
        let ed25519_data = vec![1u8, 0]; // 1 signature
        let secp256k1_data = vec![1u8]; // 1 signature
        let secp256r1_data = vec![1u8, 0]; // 1 signature
        let schnorr_data = vec![1u8]; // 1 signature
        let threshold_data = vec![2u8, 3]; // 2-of-3
        let bls_data = vec![2u8]; // 2 signers

        let instructions = vec![
            (&ed25519_program_id, ed25519_data.as_slice()),
            (&secp256k1_program_id, secp256k1_data.as_slice()),
            (&secp256r1_program_id, secp256r1_data.as_slice()),
            (&schnorr_program_id, schnorr_data.as_slice()),
            (&threshold_program_id, threshold_data.as_slice()),
            (&bls_program_id, bls_data.as_slice()),
        ];

        let cost = estimate_transaction_precompile_cost(&instructions).unwrap();

        // Expected costs:
        // Ed25519: 2,280
        // secp256k1: 6,690
        // secp256r1: 4,800
        // Schnorr: 69,000
        // Threshold (2-of-3): 20,000 + 2 × 86,000 = 192,000
        // BLS (2 signers): 843,000 + 2 × 70,000 = 983,000
        let expected = costs::ED25519_COST
            + costs::SECP256K1_COST
            + costs::SECP256R1_COST
            + costs::SCHNORR_RISTRETTO_COST
            + (costs::THRESHOLD_MULTISIG_BASE_COST + 2 * costs::THRESHOLD_MULTISIG_PER_SIGNATURE_COST)
            + (costs::BLS_FAST_AGGREGATE_BASE_COST + 2 * costs::BLS_FAST_AGGREGATE_PER_SIGNER_COST);

        assert_eq!(cost, expected);
        assert_eq!(cost, 1_257_770);
    }

    #[test]
    fn test_is_precompile_program_id() {
        // Valid precompiles
        let ed25519 = [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let secp256k1 = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let secp256r1 = [113, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let schnorr = [4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let threshold = [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let bls = [6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

        assert!(is_precompile_program_id(&ed25519));
        assert!(is_precompile_program_id(&secp256k1));
        assert!(is_precompile_program_id(&secp256r1));
        assert!(is_precompile_program_id(&schnorr));
        assert!(is_precompile_program_id(&threshold));
        assert!(is_precompile_program_id(&bls));

        // Invalid precompile (unknown ID)
        let invalid = [99, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(!is_precompile_program_id(&invalid));

        // Invalid precompile (non-zero tail)
        let invalid2 = [3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(!is_precompile_program_id(&invalid2));
    }
}
