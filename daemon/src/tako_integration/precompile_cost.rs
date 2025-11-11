//! Precompile Cost Estimation Integration
//!
//! This module provides utilities for estimating and accounting for precompile
//! compute costs during transaction validation and execution.

// Note: Hash and InvokeContext not currently needed but available for future use

/// Precompile compute costs (SVM-aligned)
pub mod costs {
    /// Ed25519 signature verification cost per signature
    pub const ED25519_COST: u64 = 2_280;

    /// secp256k1 signature verification cost per signature
    pub const SECP256K1_COST: u64 = 6_690;

    /// secp256r1 (P-256) signature verification cost per signature
    pub const SECP256R1_COST: u64 = 4_800;
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

    // Get cost per signature based on precompile type
    let cost_per_signature = get_precompile_cost_per_signature(program_id)?;

    // Parse number of signatures from instruction data
    // All three precompiles have num_signatures as first byte (or first byte after padding)
    let num_signatures = if is_ed25519_or_secp256r1(program_id) {
        // Ed25519 and secp256r1: [num_signatures: u8, padding: u8, ...]
        instruction_data[0] as u64
    } else {
        // secp256k1: [num_signatures: u8, ...]
        instruction_data[0] as u64
    };

    Ok(num_signatures.saturating_mul(cost_per_signature))
}

/// Get cost per signature for a given precompile
fn get_precompile_cost_per_signature(
    program_id: &[u8; 32],
) -> Result<u64, Box<dyn std::error::Error>> {
    // Ed25519: [3, 0, 0, ...]
    if program_id[0] == 3 && program_id[1..].iter().all(|&b| b == 0) {
        return Ok(costs::ED25519_COST);
    }

    // secp256k1: [2, 0, 0, ...]
    if program_id[0] == 2 && program_id[1..].iter().all(|&b| b == 0) {
        return Ok(costs::SECP256K1_COST);
    }

    // secp256r1: [113, 0, 0, ...]
    if program_id[0] == 113 && program_id[1..].iter().all(|&b| b == 0) {
        return Ok(costs::SECP256R1_COST);
    }

    Err("Unknown precompile program ID".into())
}

/// Check if a program ID is a precompile
fn is_precompile_program_id(program_id: &[u8; 32]) -> bool {
    // Check if first byte is 2, 3, or 113, and rest are zeros
    matches!(program_id[0], 2 | 3 | 113) && program_id[1..].iter().all(|&b| b == 0)
}

/// Check if precompile is Ed25519 or secp256r1 (both have padding byte)
fn is_ed25519_or_secp256r1(program_id: &[u8; 32]) -> bool {
    matches!(program_id[0], 3 | 113) && program_id[1..].iter().all(|&b| b == 0)
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
}
