/// Transaction Cost Estimation with Precompile Support
///
/// This module provides transaction-level cost estimation that includes:
/// - Base transaction cost
/// - Contract execution cost
/// - Precompile verification cost
///
/// # Architecture
///
/// ```text
/// Transaction Validation
///     ↓
/// estimate_transaction_cost()
///     ↓
/// ├─> Base cost (fixed)
/// ├─> Contract execution cost (estimated from bytecode size)
/// └─> Precompile cost (from precompile_cost module)
///     ↓
/// Total compute budget required
/// ```

use super::precompile_cost;
use std::error::Error;

/// Base transaction cost (fixed overhead for all transactions)
///
/// Covers basic validation, signature verification, and state updates.
/// This is a flat fee applied to all transactions.
pub const BASE_TRANSACTION_COST: u64 = 5_000;

/// Default contract execution cost per KB of bytecode
///
/// Used for estimating contract execution cost when no better estimate is available.
/// Actual cost depends on contract complexity, but this provides a reasonable baseline.
pub const CONTRACT_COST_PER_KB: u64 = 10_000;

/// Maximum transaction compute budget
///
/// This should match or be derived from TakoExecutor::MAX_COMPUTE_BUDGET
pub const MAX_TRANSACTION_COMPUTE_BUDGET: u64 = 10_000_000;

/// Transaction cost components breakdown
#[derive(Debug, Clone)]
pub struct TransactionCost {
    /// Base transaction cost (fixed)
    pub base_cost: u64,

    /// Contract execution cost (estimated from bytecode)
    pub contract_cost: u64,

    /// Precompile verification cost (sum of all precompile instructions)
    pub precompile_cost: u64,

    /// Total cost (sum of all components)
    pub total_cost: u64,
}

/// Estimate total transaction cost including precompiles
///
/// This function provides a comprehensive cost estimate for a transaction,
/// accounting for:
/// - Base transaction overhead
/// - Contract bytecode execution
/// - Precompile signature verifications
///
/// # Arguments
///
/// * `contract_bytecode` - Contract bytecode (if deploying/calling a contract)
/// * `precompile_instructions` - List of (program_id, instruction_data) for precompiles
///
/// # Returns
///
/// `TransactionCost` with detailed cost breakdown
///
/// # Example
///
/// ```ignore
/// use tos_daemon::tako_integration::transaction_cost::estimate_transaction_cost;
///
/// // Transaction with contract and precompile
/// let contract_bytecode = include_bytes!("contract.so");
/// let ed25519_id = [3, 0, 0, ...]; // Ed25519 program ID
/// let precompiles = vec![(&ed25519_id, &[1u8, 0])]; // 1 signature
///
/// let cost = estimate_transaction_cost(
///     Some(contract_bytecode),
///     &precompiles
/// )?;
///
/// println!("Total cost: {} CU", cost.total_cost);
/// println!("  Base: {} CU", cost.base_cost);
/// println!("  Contract: {} CU", cost.contract_cost);
/// println!("  Precompile: {} CU", cost.precompile_cost);
/// ```
pub fn estimate_transaction_cost(
    contract_bytecode: Option<&[u8]>,
    precompile_instructions: &[(&[u8; 32], &[u8])],
) -> Result<TransactionCost, Box<dyn Error>> {
    // 1. Base cost (always applied)
    let base_cost = BASE_TRANSACTION_COST;

    // 2. Contract execution cost (if contract is present)
    let contract_cost = if let Some(bytecode) = contract_bytecode {
        estimate_contract_cost(bytecode)
    } else {
        0
    };

    // 3. Precompile cost (sum of all precompile instructions)
    let precompile_cost =
        precompile_cost::estimate_transaction_precompile_cost(precompile_instructions)?;

    // 4. Total cost
    let total_cost = base_cost
        .saturating_add(contract_cost)
        .saturating_add(precompile_cost);

    Ok(TransactionCost {
        base_cost,
        contract_cost,
        precompile_cost,
        total_cost,
    })
}

/// Estimate contract execution cost from bytecode size
///
/// This is a simple heuristic: larger contracts generally consume more compute.
/// For more accurate estimation, static analysis of the bytecode would be needed.
///
/// # Arguments
///
/// * `bytecode` - Contract bytecode
///
/// # Returns
///
/// Estimated compute units for contract execution
fn estimate_contract_cost(bytecode: &[u8]) -> u64 {
    let size_kb = (bytecode.len() as u64 + 1023) / 1024; // Round up to KB
    size_kb.saturating_mul(CONTRACT_COST_PER_KB)
}

/// Validate that transaction cost is within budget
///
/// # Arguments
///
/// * `cost` - Transaction cost estimate
/// * `available_budget` - Available compute budget (optional, uses max if None)
///
/// # Returns
///
/// `Ok(())` if within budget, `Err` if exceeds budget
pub fn validate_transaction_cost(
    cost: &TransactionCost,
    available_budget: Option<u64>,
) -> Result<(), Box<dyn Error>> {
    let budget = available_budget.unwrap_or(MAX_TRANSACTION_COMPUTE_BUDGET);

    if cost.total_cost > budget {
        return Err(format!(
            "Transaction cost {} CU exceeds available budget {} CU",
            cost.total_cost, budget
        )
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_transaction_cost_no_contract_no_precompile() {
        let cost = estimate_transaction_cost(None, &[]).unwrap();

        assert_eq!(cost.base_cost, BASE_TRANSACTION_COST);
        assert_eq!(cost.contract_cost, 0);
        assert_eq!(cost.precompile_cost, 0);
        assert_eq!(cost.total_cost, BASE_TRANSACTION_COST);
    }

    #[test]
    fn test_estimate_transaction_cost_with_contract() {
        let bytecode = vec![0u8; 10_000]; // 10 KB contract

        let cost = estimate_transaction_cost(Some(&bytecode), &[]).unwrap();

        assert_eq!(cost.base_cost, BASE_TRANSACTION_COST);
        assert_eq!(cost.contract_cost, 10 * CONTRACT_COST_PER_KB);
        assert_eq!(cost.precompile_cost, 0);
        assert_eq!(cost.total_cost, BASE_TRANSACTION_COST + 100_000);
    }

    #[test]
    fn test_estimate_transaction_cost_with_precompile() {
        let ed25519_id = [
            3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let instruction_data = vec![2u8, 0]; // 2 signatures

        let precompiles = vec![(&ed25519_id, instruction_data.as_slice())];

        let cost = estimate_transaction_cost(None, &precompiles).unwrap();

        assert_eq!(cost.base_cost, BASE_TRANSACTION_COST);
        assert_eq!(cost.contract_cost, 0);
        assert_eq!(
            cost.precompile_cost,
            2 * precompile_cost::costs::ED25519_COST
        );
        assert_eq!(cost.total_cost, BASE_TRANSACTION_COST + 4_560);
    }

    #[test]
    fn test_estimate_transaction_cost_with_all_components() {
        // 5KB contract
        let bytecode = vec![0u8; 5_000];

        // Ed25519 + Schnorr + BLS
        let ed25519_id = [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let schnorr_id = [4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let bls_id = [6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

        let ed25519_data = vec![1u8, 0]; // 1 signature
        let schnorr_data = vec![1u8]; // 1 signature
        let bls_data = vec![10u8]; // 10 signers

        let precompiles = vec![
            (&ed25519_id, ed25519_data.as_slice()),
            (&schnorr_id, schnorr_data.as_slice()),
            (&bls_id, bls_data.as_slice()),
        ];

        let cost = estimate_transaction_cost(Some(&bytecode), &precompiles).unwrap();

        // Expected costs:
        // Base: 5,000
        // Contract: 5 KB × 10,000 = 50,000
        // Precompile: 2,280 + 69,000 + 1,543,000 = 1,614,280
        // Total: 1,669,280

        assert_eq!(cost.base_cost, 5_000);
        assert_eq!(cost.contract_cost, 50_000);
        assert_eq!(cost.precompile_cost, 1_614_280);
        assert_eq!(cost.total_cost, 1_669_280);
    }

    #[test]
    fn test_validate_transaction_cost_within_budget() {
        let cost = TransactionCost {
            base_cost: 5_000,
            contract_cost: 10_000,
            precompile_cost: 5_000,
            total_cost: 20_000,
        };

        // Should pass with default budget
        assert!(validate_transaction_cost(&cost, None).is_ok());

        // Should pass with custom budget
        assert!(validate_transaction_cost(&cost, Some(50_000)).is_ok());
    }

    #[test]
    fn test_validate_transaction_cost_exceeds_budget() {
        let cost = TransactionCost {
            base_cost: 5_000,
            contract_cost: 10_000,
            precompile_cost: 5_000,
            total_cost: 20_000,
        };

        // Should fail with insufficient budget
        let result = validate_transaction_cost(&cost, Some(10_000));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("exceeds available budget"));
    }

    #[test]
    fn test_estimate_contract_cost_rounding() {
        // Test that partial KB rounds up
        let bytecode1 = vec![0u8; 1]; // 1 byte = 1 KB (rounded up)
        let bytecode2 = vec![0u8; 1024]; // 1024 bytes = 1 KB
        let bytecode3 = vec![0u8; 1025]; // 1025 bytes = 2 KB (rounded up)

        assert_eq!(estimate_contract_cost(&bytecode1), CONTRACT_COST_PER_KB);
        assert_eq!(estimate_contract_cost(&bytecode2), CONTRACT_COST_PER_KB);
        assert_eq!(
            estimate_contract_cost(&bytecode3),
            2 * CONTRACT_COST_PER_KB
        );
    }
}
