//! Transaction-level Precompile Verification
//!
//! This module provides transaction-level verification of precompiled programs
//! (signature verification for Ed25519, secp256k1, and secp256r1).
//!
//! # Architecture
//!
//! Following SVM design:
//! - Precompiles are verified BEFORE transaction execution begins
//! - Verification is FREE at runtime (0 compute units consumed)
//! - Compute cost is charged during transaction cost estimation only
//! - Precompiles cannot be invoked via CPI (checked separately)
//!
//! # Usage
//!
//! ```rust,ignore
//! use tako_integration::precompile_verifier::verify_transaction_precompiles;
//!
//! // In transaction processor, before execution:
//! for instruction in &transaction.instructions {
//!     verify_transaction_precompiles(
//!         &instruction.program_id,
//!         &instruction.data,
//!         &all_instruction_datas,
//!     )?;
//! }
//!
//! // Then execute transaction normally
//! execute_transaction(transaction)?;
//! ```

use tos_program_runtime::InvokeContext;

/// Verify precompile instructions at transaction level
///
/// This function should be called for each instruction in a transaction
/// BEFORE execution begins. It checks if the instruction is for a precompile
/// and verifies it if so.
///
/// # Arguments
/// * `invoke_context` - The execution context (needed for `is_precompile` check)
/// * `program_id` - The program ID of the instruction
/// * `instruction_data` - The instruction data
/// * `all_instruction_datas` - All instruction data in the transaction
///
/// # Returns
/// * `Ok(())` if:
///   - The instruction is not a precompile, OR
///   - The instruction is a precompile and verification succeeds
/// * `Err(TakoExecutionError)` if precompile verification fails
///
/// # Cost Model
/// This verification is FREE at runtime (does not consume compute units).
/// The cost is charged during transaction cost estimation for scheduling.
///
/// # Example
/// ```rust,ignore
/// // In TOS transaction processor
/// let all_datas: Vec<&[u8]> = transaction.instructions
///     .iter()
///     .map(|ix| ix.data.as_slice())
///     .collect();
///
/// for (idx, instruction) in transaction.instructions.iter().enumerate() {
///     verify_precompile_instruction(
///         &invoke_context,
///         &instruction.program_id,
///         &instruction.data,
///         &all_datas,
///     )?;
/// }
/// ```
pub fn verify_precompile_instruction(
    invoke_context: &InvokeContext,
    program_id: &[u8; 32],
    instruction_data: &[u8],
    all_instruction_datas: &[&[u8]],
) -> Result<(), crate::tako_integration::error::TakoExecutionError> {
    // Check if this is a precompile
    if !invoke_context.is_precompile(program_id) {
        // Not a precompile, nothing to verify
        return Ok(());
    }

    // This is a precompile - verify it
    invoke_context
        .process_precompile(program_id, instruction_data, all_instruction_datas)
        .map_err(
            |e| crate::tako_integration::error::TakoExecutionError::ExecutionFailed {
                reason: format!("Precompile verification failed: {}", e),
                instruction_count: 0,
                compute_units_used: 0,
                error_code: None,
            },
        )
}

/// Verify all precompile instructions in a transaction
///
/// Convenience function that verifies all precompile instructions in a transaction.
/// This should be called BEFORE executing the transaction.
///
/// # Arguments
/// * `invoke_context` - The execution context
/// * `instructions` - List of (program_id, instruction_data) tuples
///
/// # Returns
/// * `Ok(())` if all precompile verifications succeed
/// * `Err(TakoExecutionError)` if any verification fails
///
/// # Example
/// ```rust,ignore
/// let instructions: Vec<([u8; 32], Vec<u8>)> = transaction.instructions
///     .iter()
///     .map(|ix| (ix.program_id, ix.data.clone()))
///     .collect();
///
/// verify_all_precompiles(&invoke_context, &instructions)?;
/// ```
pub fn verify_all_precompiles(
    invoke_context: &InvokeContext,
    instructions: &[([u8; 32], Vec<u8>)],
) -> Result<(), crate::tako_integration::error::TakoExecutionError> {
    // Collect all instruction data slices
    let all_datas: Vec<&[u8]> = instructions
        .iter()
        .map(|(_, data)| data.as_slice())
        .collect();

    // Verify each instruction
    for (program_id, instruction_data) in instructions {
        verify_precompile_instruction(invoke_context, program_id, instruction_data, &all_datas)?;
    }

    Ok(())
}

/// Estimate compute cost for precompiles in a transaction
///
/// Returns the total compute units required for all precompile instructions.
/// This cost should be charged during transaction cost estimation (not runtime).
///
/// # Arguments
/// * `invoke_context` - The execution context
/// * `instructions` - List of (program_id, instruction_data) tuples
///
/// # Returns
/// * Total compute units for all precompiles (0 for non-precompiles)
///
/// # Cost Schedule (SVM-aligned)
/// - Ed25519: 2,280 CU per signature
/// - secp256k1: 6,690 CU per signature
/// - secp256r1: 4,800 CU per signature
///
/// # Example
/// ```rust,ignore
/// let instructions: Vec<([u8; 32], Vec<u8>)> = transaction.instructions
///     .iter()
///     .map(|ix| (ix.program_id, ix.data.clone()))
///     .collect();
///
/// let precompile_cost = estimate_precompile_cost(&invoke_context, &instructions);
/// let total_cost = base_cost + contract_cost + precompile_cost;
/// ```
pub fn estimate_precompile_cost(
    invoke_context: &InvokeContext,
    instructions: &[([u8; 32], Vec<u8>)],
) -> u64 {
    // Precompile program IDs (SVM-compatible)
    const ED25519_PROGRAM_ID: [u8; 32] = [
        3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];
    const SECP256K1_PROGRAM_ID: [u8; 32] = [
        2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];
    const SECP256R1_PROGRAM_ID: [u8; 32] = [
        113, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];

    // Compute costs per signature (SVM-aligned)
    const ED25519_COST_PER_SIG: u64 = 2_280;
    const SECP256K1_COST_PER_SIG: u64 = 6_690;
    const SECP256R1_COST_PER_SIG: u64 = 4_800;

    let mut total_cost = 0u64;

    for (program_id, instruction_data) in instructions {
        if !invoke_context.is_precompile(program_id) {
            continue;
        }

        // Parse number of signatures from instruction data
        if instruction_data.is_empty() {
            continue;
        }

        let num_signatures = instruction_data[0] as u64;

        // Calculate cost based on precompile type
        let cost_per_sig = if program_id == &ED25519_PROGRAM_ID {
            ED25519_COST_PER_SIG
        } else if program_id == &SECP256K1_PROGRAM_ID {
            SECP256K1_COST_PER_SIG
        } else if program_id == &SECP256R1_PROGRAM_ID {
            SECP256R1_COST_PER_SIG
        } else {
            continue; // Unknown precompile
        };

        total_cost = total_cost.saturating_add(num_signatures.saturating_mul(cost_per_sig));
    }

    total_cost
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tos_program_runtime::{
        storage::{NoOpAccounts, NoOpContractLoader, NoOpStorage},
        InvokeContext,
    };
    use tos_tbpf::{program::BuiltinProgram, vm::Config};

    // Helper to create test loader
    fn create_test_loader<'a>() -> Arc<BuiltinProgram<InvokeContext<'a>>> {
        let config = Config::default();
        unsafe {
            std::mem::transmute(Arc::new(BuiltinProgram::<InvokeContext>::new_loader(
                config,
            )))
        }
    }

    #[test]
    fn test_verify_non_precompile() {
        let mut storage = NoOpStorage;
        let mut accounts = NoOpAccounts;
        let contract_loader = NoOpContractLoader;
        let loader = create_test_loader();
        let context = InvokeContext::new(
            100_000,
            [1u8; 32],
            &mut storage,
            &mut accounts,
            &contract_loader,
            loader,
        );

        // Non-precompile program ID should succeed without verification
        let random_id = [0xFF; 32];
        let data = vec![0u8, 0u8];
        let result = verify_precompile_instruction(&context, &random_id, &data, &[data.as_slice()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_ed25519_precompile() {
        let mut storage = NoOpStorage;
        let mut accounts = NoOpAccounts;
        let contract_loader = NoOpContractLoader;
        let loader = create_test_loader();
        let context = InvokeContext::new(
            100_000,
            [1u8; 32],
            &mut storage,
            &mut accounts,
            &contract_loader,
            loader,
        );

        // Ed25519 precompile with 0 signatures should succeed
        let ed25519_id = [
            3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let data = vec![0u8, 0u8]; // num_signatures=0, padding=0
        let result =
            verify_precompile_instruction(&context, &ed25519_id, &data, &[data.as_slice()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_all_precompiles_empty() {
        let mut storage = NoOpStorage;
        let mut accounts = NoOpAccounts;
        let contract_loader = NoOpContractLoader;
        let loader = create_test_loader();
        let context = InvokeContext::new(
            100_000,
            [1u8; 32],
            &mut storage,
            &mut accounts,
            &contract_loader,
            loader,
        );

        // Empty instruction list should succeed
        let instructions: Vec<([u8; 32], Vec<u8>)> = vec![];
        let result = verify_all_precompiles(&context, &instructions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_all_precompiles_mixed() {
        let mut storage = NoOpStorage;
        let mut accounts = NoOpAccounts;
        let contract_loader = NoOpContractLoader;
        let loader = create_test_loader();
        let context = InvokeContext::new(
            100_000,
            [1u8; 32],
            &mut storage,
            &mut accounts,
            &contract_loader,
            loader,
        );

        // Mix of precompile and non-precompile instructions
        let ed25519_id = [
            3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let random_id = [0xFF; 32];

        let instructions = vec![
            (random_id, vec![1, 2, 3]),   // Non-precompile
            (ed25519_id, vec![0u8, 0u8]), // Ed25519 with 0 sigs
            (random_id, vec![4, 5, 6]),   // Non-precompile
        ];

        let result = verify_all_precompiles(&context, &instructions);
        assert!(result.is_ok());
    }

    #[test]
    fn test_estimate_precompile_cost_empty() {
        let mut storage = NoOpStorage;
        let mut accounts = NoOpAccounts;
        let contract_loader = NoOpContractLoader;
        let loader = create_test_loader();
        let context = InvokeContext::new(
            100_000,
            [1u8; 32],
            &mut storage,
            &mut accounts,
            &contract_loader,
            loader,
        );

        let instructions: Vec<([u8; 32], Vec<u8>)> = vec![];
        let cost = estimate_precompile_cost(&context, &instructions);
        assert_eq!(cost, 0);
    }

    #[test]
    fn test_estimate_precompile_cost_ed25519() {
        let mut storage = NoOpStorage;
        let mut accounts = NoOpAccounts;
        let contract_loader = NoOpContractLoader;
        let loader = create_test_loader();
        let context = InvokeContext::new(
            100_000,
            [1u8; 32],
            &mut storage,
            &mut accounts,
            &contract_loader,
            loader,
        );

        let ed25519_id = [
            3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        // 3 Ed25519 signatures = 3 * 2,280 = 6,840 CU
        let instructions = vec![(ed25519_id, vec![3u8, 0u8])];
        let cost = estimate_precompile_cost(&context, &instructions);
        assert_eq!(cost, 6_840);
    }

    #[test]
    fn test_estimate_precompile_cost_mixed() {
        let mut storage = NoOpStorage;
        let mut accounts = NoOpAccounts;
        let contract_loader = NoOpContractLoader;
        let loader = create_test_loader();
        let context = InvokeContext::new(
            100_000,
            [1u8; 32],
            &mut storage,
            &mut accounts,
            &contract_loader,
            loader,
        );

        let ed25519_id = [
            3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let secp256k1_id = [
            2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let random_id = [0xFF; 32];

        let instructions = vec![
            (ed25519_id, vec![2u8, 0u8]), // 2 * 2,280 = 4,560
            (secp256k1_id, vec![1u8]),    // 1 * 6,690 = 6,690
            (random_id, vec![10u8]),      // Non-precompile = 0
        ];

        let cost = estimate_precompile_cost(&context, &instructions);
        assert_eq!(cost, 4_560 + 6_690); // 11,250
    }
}
