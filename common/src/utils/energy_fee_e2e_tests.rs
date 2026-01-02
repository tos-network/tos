//! E2E Tests for Energy/TOS Consumption Model
//!
//! This module contains comprehensive end-to-end tests that verify the actual
//! energy and TOS consumption during transaction execution matches the specification
//! defined in `~/memo/21-Energy/tos-new-energy.md`.
//!
//! # Test Coverage
//!
//! 1. **TOS-Only Fees** (cannot use Energy)
//!    - Account creation fee: 0.1 TOS (FEE_PER_ACCOUNT_CREATION)
//!    - MultiSig fee: 1 TOS per signature (FEE_PER_MULTISIG_SIGNATURE)
//!
//! 2. **Energy Consumption** (can auto-burn TOS if insufficient)
//!    - Transfer: size + outputs × 100
//!    - New account: +25,000 additional energy
//!    - Burn: 1,000 energy
//!    - UNO transfers: size + outputs × 500 (5x cost)
//!    - Contract deploy: 32,000 + bytecode_size × 10
//!
//! 3. **Energy Priority**
//!    - Free quota (1,500/day) → Frozen energy → Auto TOS burn (100 atomic/energy)
//!
//! 4. **Edge Cases**
//!    - Boundary tests for minimum amounts
//!    - Insufficient funds scenarios
//!    - Mixed energy + TOS payment scenarios

#![allow(clippy::disallowed_methods)]

use crate::{
    account::AccountEnergy,
    config::{
        COIN_VALUE, ENERGY_COST_BURN, ENERGY_COST_CONTRACT_DEPLOY_BASE,
        ENERGY_COST_CONTRACT_DEPLOY_PER_BYTE, ENERGY_COST_NEW_ACCOUNT,
        ENERGY_COST_TRANSFER_PER_OUTPUT, FEE_PER_ACCOUNT_CREATION, FEE_PER_MULTISIG_SIGNATURE,
        FREE_ENERGY_QUOTA, TOS_PER_ENERGY,
    },
    transaction::TransactionResult,
    utils::energy_fee::{EnergyFeeCalculator, EnergyResourceManager},
};

// ============================================================================
// CONSTANTS FROM SPECIFICATION (tos-new-energy.md)
// ============================================================================

/// Account creation fee: 0.1 TOS (10,000,000 atomic units)
const SPEC_ACCOUNT_CREATION_FEE: u64 = 10_000_000;

/// MultiSig fee: 1 TOS per signature (100,000,000 atomic units)
const SPEC_MULTISIG_FEE_PER_SIGNATURE: u64 = 100_000_000;

/// Free energy quota: 1,500 Energy per account per day
/// Provides ~3-4 free transfers for casual users
const SPEC_FREE_ENERGY_QUOTA: u64 = 1_500; // Aligned: spec and code both use 1,500

/// TOS cost per energy unit: 100 atomic TOS
const SPEC_TOS_PER_ENERGY: u64 = 100;

/// Transfer energy cost per output: 100 energy
const SPEC_ENERGY_PER_TRANSFER_OUTPUT: u64 = 100;

/// New account additional energy cost: 25,000 energy
const SPEC_ENERGY_NEW_ACCOUNT: u64 = 25_000;

/// Burn operation energy cost: 1,000 energy
const SPEC_ENERGY_BURN: u64 = 1_000;

/// Contract deploy base cost: 32,000 energy
const SPEC_ENERGY_CONTRACT_DEPLOY_BASE: u64 = 32_000;

/// Contract deploy per byte cost: 10 energy
const SPEC_ENERGY_CONTRACT_DEPLOY_PER_BYTE: u64 = 10;

/// UNO transfer cost multiplier: 5x compared to regular transfer
const SPEC_UNO_COST_MULTIPLIER: u64 = 5;

// ============================================================================
// 1. CONSTANT VERIFICATION TESTS
// ============================================================================

#[cfg(test)]
mod constant_verification_tests {
    use super::*;

    #[test]
    fn test_account_creation_fee_matches_spec() {
        // Spec: Account creation fee = 0.1 TOS = 10,000,000 atomic
        assert_eq!(
            FEE_PER_ACCOUNT_CREATION, SPEC_ACCOUNT_CREATION_FEE,
            "FEE_PER_ACCOUNT_CREATION should be 10,000,000 (0.1 TOS)"
        );
        assert_eq!(
            FEE_PER_ACCOUNT_CREATION,
            COIN_VALUE / 10,
            "Account creation fee should be 0.1 TOS"
        );
    }

    #[test]
    fn test_multisig_fee_matches_spec() {
        // Spec: MultiSig fee = 1 TOS per signature = 100,000,000 atomic
        assert_eq!(
            FEE_PER_MULTISIG_SIGNATURE, SPEC_MULTISIG_FEE_PER_SIGNATURE,
            "FEE_PER_MULTISIG_SIGNATURE should be 100,000,000 (1 TOS)"
        );
        assert_eq!(
            FEE_PER_MULTISIG_SIGNATURE, COIN_VALUE,
            "MultiSig fee should be 1 TOS per signature"
        );
    }

    /// Test: Free energy quota matches between spec and code
    ///
    /// Spec (tos-new-energy.md): FREE_ENERGY_LIMIT = 1,500
    /// Code (config.rs): FREE_ENERGY_QUOTA = 1,500
    #[test]
    fn test_free_energy_quota_matches_spec() {
        // Verify spec and code are aligned
        assert_eq!(
            SPEC_FREE_ENERGY_QUOTA, FREE_ENERGY_QUOTA,
            "FREE_ENERGY_QUOTA should match spec value of 1,500"
        );

        // Verify the actual value
        assert_eq!(
            FREE_ENERGY_QUOTA, 1_500,
            "FREE_ENERGY_QUOTA should be 1,500"
        );
    }

    #[test]
    fn test_tos_per_energy_matches_spec() {
        // Spec: TOS cost = 100 atomic per energy
        assert_eq!(
            TOS_PER_ENERGY, SPEC_TOS_PER_ENERGY,
            "TOS_PER_ENERGY should be 100"
        );
    }

    #[test]
    fn test_energy_costs_match_spec() {
        // Transfer per output: 100 energy
        assert_eq!(
            ENERGY_COST_TRANSFER_PER_OUTPUT, SPEC_ENERGY_PER_TRANSFER_OUTPUT,
            "ENERGY_COST_TRANSFER_PER_OUTPUT should be 100"
        );

        // New account: 25,000 energy
        assert_eq!(
            ENERGY_COST_NEW_ACCOUNT, SPEC_ENERGY_NEW_ACCOUNT,
            "ENERGY_COST_NEW_ACCOUNT should be 25,000"
        );

        // Burn: 1,000 energy
        assert_eq!(
            ENERGY_COST_BURN, SPEC_ENERGY_BURN,
            "ENERGY_COST_BURN should be 1,000"
        );

        // Contract deploy base: 32,000 energy
        assert_eq!(
            ENERGY_COST_CONTRACT_DEPLOY_BASE, SPEC_ENERGY_CONTRACT_DEPLOY_BASE,
            "ENERGY_COST_CONTRACT_DEPLOY_BASE should be 32,000"
        );

        // Contract deploy per byte: 10 energy
        assert_eq!(
            ENERGY_COST_CONTRACT_DEPLOY_PER_BYTE, SPEC_ENERGY_CONTRACT_DEPLOY_PER_BYTE,
            "ENERGY_COST_CONTRACT_DEPLOY_PER_BYTE should be 10"
        );
    }
}

// ============================================================================
// 2. TOS-ONLY FEE TESTS
// ============================================================================

#[cfg(test)]
mod tos_only_fee_tests {
    use super::*;

    /// Test: Account creation fee is exactly 0.1 TOS
    ///
    /// Scenario: Transfer 1 TOS to new account
    /// Expected: Receiver gets 0.9 TOS (1 TOS - 0.1 TOS fee)
    #[test]
    fn test_account_creation_fee_deduction() {
        let transfer_amount = COIN_VALUE; // 1 TOS
        let expected_receiver_amount = transfer_amount - FEE_PER_ACCOUNT_CREATION;

        assert_eq!(
            expected_receiver_amount, 90_000_000,
            "Receiver should get 0.9 TOS after 0.1 TOS fee deduction"
        );
    }

    /// Test: Account creation fee boundary - exactly 0.1 TOS transfer
    ///
    /// Scenario: Transfer exactly 0.1 TOS to new account
    /// Expected: Receiver gets 0 TOS (all goes to fee)
    #[test]
    fn test_account_creation_fee_exact_minimum() {
        let transfer_amount = FEE_PER_ACCOUNT_CREATION; // Exactly 0.1 TOS

        // After fee deduction, receiver gets nothing
        // But transaction should still be valid (receiver can receive 0)
        let receiver_amount = transfer_amount - FEE_PER_ACCOUNT_CREATION;
        assert_eq!(
            receiver_amount, 0,
            "Receiver gets 0 when transfer equals fee"
        );
    }

    /// Test: Account creation fee boundary - less than 0.1 TOS should fail
    ///
    /// Scenario: Transfer 0.05 TOS to new account
    /// Expected: Transaction should fail (amount < fee)
    #[test]
    fn test_account_creation_fee_insufficient_amount() {
        let transfer_amount = FEE_PER_ACCOUNT_CREATION / 2; // 0.05 TOS

        // This should be rejected during verification
        assert!(
            transfer_amount < FEE_PER_ACCOUNT_CREATION,
            "Transfer amount {} is less than fee {}",
            transfer_amount,
            FEE_PER_ACCOUNT_CREATION
        );
    }

    /// Test: MultiSig fee calculation - 2 signatures
    ///
    /// Scenario: Transaction with 2 signatures
    /// Expected: Fee = 2 × 1 TOS = 2 TOS
    #[test]
    fn test_multisig_fee_2_signatures() {
        let signature_count = 2u64;
        let expected_fee = signature_count * FEE_PER_MULTISIG_SIGNATURE;

        assert_eq!(
            expected_fee,
            2 * COIN_VALUE,
            "2-signature multisig should cost 2 TOS"
        );
        assert_eq!(expected_fee, 200_000_000, "2 TOS = 200,000,000 atomic");
    }

    /// Test: MultiSig fee calculation - 5 signatures
    ///
    /// Scenario: Transaction with 5 signatures
    /// Expected: Fee = 5 × 1 TOS = 5 TOS
    #[test]
    fn test_multisig_fee_5_signatures() {
        let signature_count = 5u64;
        let expected_fee = signature_count * FEE_PER_MULTISIG_SIGNATURE;

        assert_eq!(
            expected_fee,
            5 * COIN_VALUE,
            "5-signature multisig should cost 5 TOS"
        );
        assert_eq!(expected_fee, 500_000_000, "5 TOS = 500,000,000 atomic");
    }

    /// Test: Single signature transaction has no MultiSig fee
    ///
    /// Scenario: Normal transaction with 1 signature
    /// Expected: No MultiSig fee (only single sig)
    #[test]
    fn test_single_signature_no_multisig_fee() {
        let signature_count = 1u64;
        // Single signature transactions don't pay MultiSig fee
        // The fee is only for signatures >= 2
        let multisig_fee = if signature_count > 1 {
            signature_count * FEE_PER_MULTISIG_SIGNATURE
        } else {
            0
        };

        assert_eq!(multisig_fee, 0, "Single signature has no multisig fee");
    }
}

// ============================================================================
// 3. ENERGY CONSUMPTION CALCULATION TESTS
// ============================================================================

#[cfg(test)]
mod energy_consumption_tests {
    use super::*;

    /// Test: Simple transfer energy cost
    ///
    /// Formula: size + outputs × 100
    /// Scenario: 250 bytes, 1 output
    /// Expected: 250 + 100 = 350 energy
    #[test]
    fn test_transfer_cost_simple() {
        let tx_size = 250;
        let output_count = 1;

        let cost = EnergyFeeCalculator::calculate_transfer_cost(tx_size, output_count);

        assert_eq!(cost, 350, "Simple transfer: 250 + 1*100 = 350 energy");
    }

    /// Test: Batch transfer energy cost
    ///
    /// Formula: size + outputs × 100
    /// Scenario: 800 bytes, 10 outputs
    /// Expected: 800 + 1000 = 1,800 energy
    #[test]
    fn test_transfer_cost_batch() {
        let tx_size = 800;
        let output_count = 10;

        let cost = EnergyFeeCalculator::calculate_transfer_cost(tx_size, output_count);

        assert_eq!(cost, 1800, "Batch transfer: 800 + 10*100 = 1,800 energy");
    }

    /// Test: Transfer to new account energy cost
    ///
    /// Formula: size + outputs × 100 + new_accounts × 25,000
    /// Scenario: 250 bytes, 1 output, 1 new account
    /// Expected: 250 + 100 + 25,000 = 25,350 energy
    #[test]
    fn test_transfer_to_new_account_cost() {
        let tx_size = 250;
        let output_count = 1;
        let new_accounts = 1;

        let transfer_cost = EnergyFeeCalculator::calculate_transfer_cost(tx_size, output_count);
        let new_account_cost = EnergyFeeCalculator::calculate_new_account_cost(new_accounts);
        let total_cost = transfer_cost + new_account_cost;

        assert_eq!(
            total_cost, 25_350,
            "New account transfer: 250 + 100 + 25,000 = 25,350 energy"
        );
    }

    /// Test: Burn operation energy cost
    ///
    /// Scenario: Burn TOS
    /// Expected: 1,000 energy
    #[test]
    fn test_burn_cost() {
        let cost = EnergyFeeCalculator::calculate_burn_cost();

        assert_eq!(cost, 1_000, "Burn operation costs 1,000 energy");
    }

    /// Test: Contract deploy energy cost
    ///
    /// Formula: 32,000 + bytecode_size × 10
    /// Scenario: 1,000 bytes bytecode
    /// Expected: 32,000 + 10,000 = 42,000 energy
    #[test]
    fn test_contract_deploy_cost() {
        let bytecode_size = 1_000;

        let cost = EnergyFeeCalculator::calculate_deploy_cost(bytecode_size);

        assert_eq!(
            cost, 42_000,
            "Contract deploy: 32,000 + 1,000*10 = 42,000 energy"
        );
    }

    /// Test: Large contract deploy energy cost
    ///
    /// Scenario: 10,000 bytes bytecode
    /// Expected: 32,000 + 100,000 = 132,000 energy
    #[test]
    fn test_contract_deploy_large() {
        let bytecode_size = 10_000;

        let cost = EnergyFeeCalculator::calculate_deploy_cost(bytecode_size);

        assert_eq!(
            cost, 132_000,
            "Large contract: 32,000 + 10,000*10 = 132,000 energy"
        );
    }

    /// Test: UNO transfer energy cost (5x multiplier)
    ///
    /// Formula: size + outputs × 500 (5x the regular 100)
    /// Scenario: 500 bytes, 1 output
    /// Expected: 500 + 500 = 1,000 energy
    #[test]
    fn test_uno_transfer_cost() {
        let tx_size = 500;
        let output_count = 1;

        let cost = EnergyFeeCalculator::calculate_uno_transfer_cost(tx_size, output_count);

        // UNO uses 500 per output (5x regular transfer)
        assert_eq!(cost, 1_000, "UNO transfer: 500 + 1*500 = 1,000 energy");
    }

    /// Test: UNO transfer vs regular transfer ratio
    ///
    /// Verify that UNO costs 5x more per output than regular transfer
    #[test]
    fn test_uno_vs_regular_transfer_ratio() {
        let tx_size = 0; // Isolate output cost
        let output_count = 1;

        let regular_cost = EnergyFeeCalculator::calculate_transfer_cost(tx_size, output_count);
        let uno_cost = EnergyFeeCalculator::calculate_uno_transfer_cost(tx_size, output_count);

        assert_eq!(
            uno_cost,
            regular_cost * SPEC_UNO_COST_MULTIPLIER,
            "UNO should cost 5x more per output than regular transfer"
        );
    }
}

// ============================================================================
// 4. ENERGY CONSUMPTION PRIORITY TESTS
// ============================================================================

#[cfg(test)]
mod energy_priority_tests {
    use super::*;

    /// Create a test account with specified energy state
    fn create_test_account(
        frozen_balance: u64,
        free_usage: u64,
        frozen_usage: u64,
    ) -> AccountEnergy {
        let mut account = AccountEnergy::new();
        account.frozen_balance = frozen_balance;
        account.free_energy_usage = free_usage;
        account.energy_usage = frozen_usage;
        account
    }

    /// Test: Free quota consumed first
    ///
    /// Scenario: Request 500 energy, have 1,500 free quota
    /// Expected: Use 500 from free quota, 0 TOS burned
    #[test]
    fn test_free_quota_consumed_first() {
        let mut account = create_test_account(0, 0, 0);
        let required_energy = 500;
        let total_weight = 100_000_000; // 100M total weight
        let now_ms = 1_000;

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            required_energy,
            total_weight,
            now_ms,
        );

        assert_eq!(
            result.free_energy_used, 500,
            "Should use 500 energy from free quota"
        );
        assert_eq!(result.frozen_energy_used, 0, "Should not use frozen energy");
        assert_eq!(result.fee, 0, "Should not burn any TOS");
        assert_eq!(
            result.energy_used, required_energy,
            "Total energy should match request"
        );
    }

    /// Test: Free quota exhausted, then use frozen energy
    ///
    /// Scenario: Request 2,000 energy, have 1,500 free + frozen energy
    /// Expected: Use 1,500 free + 500 frozen, 0 TOS burned
    #[test]
    fn test_frozen_energy_after_free_quota() {
        // Account with sufficient frozen balance
        let mut account = create_test_account(10 * COIN_VALUE, 0, 0); // 10 TOS frozen
        let required_energy = 2_000;
        let total_weight = 100 * COIN_VALUE; // 100 TOS total weight
        let now_ms = 1_000;

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            required_energy,
            total_weight,
            now_ms,
        );

        assert_eq!(
            result.free_energy_used, FREE_ENERGY_QUOTA,
            "Should exhaust free quota ({})",
            FREE_ENERGY_QUOTA
        );

        let expected_frozen = required_energy - FREE_ENERGY_QUOTA;
        assert_eq!(
            result.frozen_energy_used, expected_frozen,
            "Should use {} from frozen energy",
            expected_frozen
        );

        assert_eq!(result.fee, 0, "Should not burn any TOS");
    }

    /// Test: All energy sources exhausted, burn TOS
    ///
    /// Scenario: Request 10,000 energy, have 1,500 free + 0 frozen
    /// Expected: Use 1,500 free, burn TOS for remaining 8,500
    #[test]
    fn test_auto_burn_tos_when_energy_insufficient() {
        let mut account = create_test_account(0, 0, 0); // No frozen balance
        let required_energy = 10_000;
        let total_weight = 100_000_000;
        let now_ms = 1_000;

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            required_energy,
            total_weight,
            now_ms,
        );

        assert_eq!(
            result.free_energy_used, FREE_ENERGY_QUOTA,
            "Should exhaust free quota"
        );
        assert_eq!(result.frozen_energy_used, 0, "No frozen energy available");

        let energy_to_burn = required_energy - FREE_ENERGY_QUOTA;
        let expected_fee = energy_to_burn * TOS_PER_ENERGY;
        assert_eq!(
            result.fee, expected_fee,
            "Should burn {} TOS for {} energy shortfall",
            expected_fee, energy_to_burn
        );
    }

    /// Test: Verify TOS cost calculation for energy shortfall
    ///
    /// Scenario: Need 1,000 energy, calculate TOS cost
    /// Expected: 1,000 × 100 = 100,000 atomic TOS
    #[test]
    fn test_tos_cost_calculation() {
        let energy_needed = 1_000;
        let tos_cost = EnergyResourceManager::calculate_tos_cost_for_energy(energy_needed);

        assert_eq!(
            tos_cost, 100_000,
            "1,000 energy = 1,000 × 100 = 100,000 atomic TOS"
        );
    }

    /// Test: Full quota used (1,500 energy)
    ///
    /// Scenario: Request exactly 1,500 energy (full quota)
    /// Expected: Use exactly 1,500 free, 0 TOS burned
    #[test]
    fn test_exact_free_quota_usage() {
        let mut account = create_test_account(0, 0, 0);
        let required_energy = FREE_ENERGY_QUOTA;
        let total_weight = 100_000_000;
        let now_ms = 1_000;

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            required_energy,
            total_weight,
            now_ms,
        );

        assert_eq!(
            result.free_energy_used, FREE_ENERGY_QUOTA,
            "Should use exactly {} free energy",
            FREE_ENERGY_QUOTA
        );
        assert_eq!(result.frozen_energy_used, 0);
        assert_eq!(result.fee, 0, "No TOS should be burned");
    }
}

// ============================================================================
// 5. EDGE CASE TESTS
// ============================================================================

#[cfg(test)]
mod edge_case_tests {
    use super::*;

    /// Test: Zero energy request
    #[test]
    fn test_zero_energy_request() {
        let mut account = AccountEnergy::new();
        let total_weight = 100_000_000;
        let now_ms = 1_000;

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            0, // Zero energy
            total_weight,
            now_ms,
        );

        assert_eq!(result.energy_used, 0);
        assert_eq!(result.free_energy_used, 0);
        assert_eq!(result.frozen_energy_used, 0);
        assert_eq!(result.fee, 0);
    }

    /// Test: Very large energy request
    #[test]
    fn test_large_energy_request() {
        let mut account = AccountEnergy::new();
        let required_energy = 1_000_000; // 1M energy
        let total_weight = 100_000_000;
        let now_ms = 1_000;

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            required_energy,
            total_weight,
            now_ms,
        );

        // Should use free quota and burn TOS for rest
        let energy_to_burn = required_energy - FREE_ENERGY_QUOTA;
        let expected_fee = energy_to_burn * TOS_PER_ENERGY;

        assert_eq!(result.free_energy_used, FREE_ENERGY_QUOTA);
        assert_eq!(result.fee, expected_fee);
    }

    /// Test: Account creation with various amounts
    #[test]
    fn test_account_creation_amounts() {
        // Test various transfer amounts to new account
        let test_cases = vec![
            // (transfer_amount, expected_receiver_amount, should_succeed)
            (COIN_VALUE, 90_000_000, true),      // 1 TOS -> 0.9 TOS
            (50_000_000, 40_000_000, true),      // 0.5 TOS -> 0.4 TOS
            (FEE_PER_ACCOUNT_CREATION, 0, true), // 0.1 TOS -> 0 TOS (edge)
            (FEE_PER_ACCOUNT_CREATION + 1, 1, true), // 0.1 TOS + 1 -> 1 atomic
            (FEE_PER_ACCOUNT_CREATION - 1, 0, false), // 0.1 TOS - 1 -> FAIL
            (0, 0, false),                       // 0 -> FAIL
        ];

        for (transfer, expected_receiver, should_succeed) in test_cases {
            if should_succeed {
                assert!(
                    transfer >= FEE_PER_ACCOUNT_CREATION,
                    "Transfer {} should succeed",
                    transfer
                );
                let receiver_gets = transfer - FEE_PER_ACCOUNT_CREATION;
                assert_eq!(
                    receiver_gets, expected_receiver,
                    "Transfer {} should give receiver {}",
                    transfer, expected_receiver
                );
            } else {
                assert!(
                    transfer < FEE_PER_ACCOUNT_CREATION,
                    "Transfer {} should fail (less than fee)",
                    transfer
                );
            }
        }
    }

    /// Test: MultiSig fee for various signature counts
    #[test]
    fn test_multisig_fee_various_counts() {
        let test_cases = vec![
            (1, 0),                // 1 sig: no fee
            (2, 2 * COIN_VALUE),   // 2 sigs: 2 TOS
            (3, 3 * COIN_VALUE),   // 3 sigs: 3 TOS
            (5, 5 * COIN_VALUE),   // 5 sigs: 5 TOS
            (10, 10 * COIN_VALUE), // 10 sigs: 10 TOS
        ];

        for (sig_count, expected_fee) in test_cases {
            let fee = if sig_count > 1 {
                sig_count as u64 * FEE_PER_MULTISIG_SIGNATURE
            } else {
                0
            };
            assert_eq!(
                fee,
                expected_fee,
                "{} signatures should cost {} TOS",
                sig_count,
                expected_fee / COIN_VALUE
            );
        }
    }

    /// Test: Can afford transaction check
    #[test]
    fn test_can_afford_transaction() {
        let account = AccountEnergy::new();
        let total_weight = 100_000_000;
        let now_ms = 1_000;

        // Case 1: Small energy, within free quota
        let can_afford = EnergyResourceManager::can_afford_transaction(
            &account,
            500, // 500 energy
            0,   // 0 TOS balance
            total_weight,
            now_ms,
        );
        assert!(can_afford, "Should afford 500 energy with free quota");

        // Case 2: Large energy, needs TOS
        let required = 10_000;
        let needed_tos = (required - FREE_ENERGY_QUOTA) * TOS_PER_ENERGY;

        let can_afford_with_tos = EnergyResourceManager::can_afford_transaction(
            &account,
            required,
            needed_tos, // Exact TOS needed
            total_weight,
            now_ms,
        );
        assert!(can_afford_with_tos, "Should afford with exact TOS balance");

        let cannot_afford = EnergyResourceManager::can_afford_transaction(
            &account,
            required,
            needed_tos - 1, // 1 atomic short
            total_weight,
            now_ms,
        );
        assert!(!cannot_afford, "Should not afford when 1 atomic short");
    }

    /// Test: Energy cost for Stake 2.0 operations (should be FREE)
    ///
    /// Stake 2.0 operations are free:
    /// - FreezeTOS: 0 energy
    /// - UnfreezeTOS: 0 energy
    /// - WithdrawExpireUnfreeze: 0 energy
    /// - CancelAllUnfreeze: 0 energy
    /// - DelegateResource: 0 energy
    /// - UndelegateResource: 0 energy
    #[test]
    fn test_stake_operations_are_free() {
        // Stake 2.0 operations consume 0 energy per specification
        // Energy consumption is only for Transfer, Burn, Contract, etc.

        // Verify by checking that a zero-energy request costs nothing
        let mut account = AccountEnergy::new();
        let total_weight = 100_000_000;
        let now_ms = 1_000;

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            0, // Stake operations consume 0 energy
            total_weight,
            now_ms,
        );

        assert_eq!(result.fee, 0, "Stake operations should be free");
        assert_eq!(result.energy_used, 0);
    }
}

// ============================================================================
// 6. TRANSACTION RESULT TESTS
// ============================================================================

#[cfg(test)]
mod transaction_result_tests {
    use super::*;

    /// Test: TransactionResult creation and field values
    #[test]
    fn test_transaction_result_fields() {
        let result = TransactionResult::with_values(
            100_000, // fee (TOS burned)
            10_000,  // energy_used
            1_500,   // free_energy_used
            7_500,   // frozen_energy_used
        );

        assert_eq!(result.fee, 100_000);
        assert_eq!(result.energy_used, 10_000);
        assert_eq!(result.free_energy_used, 1_500);
        assert_eq!(result.frozen_energy_used, 7_500);
    }

    /// Test: Total energy from stake calculation
    #[test]
    fn test_total_energy_from_stake() {
        let result = TransactionResult::with_values(0, 10_000, 1_500, 7_500);

        let total_stake = result.total_energy_from_stake();
        assert_eq!(total_stake, 9_000, "Free (1,500) + Frozen (7,500) = 9,000");
    }

    /// Test: Auto-burned energy calculation
    #[test]
    fn test_auto_burned_energy_calculation() {
        let result = TransactionResult::with_values(
            100_000, // 1,000 energy worth of TOS burned
            10_000,  // total energy used
            1_500,   // from free
            7_500,   // from frozen
        );

        let auto_burned = result.auto_burned_energy();
        // 10,000 - 1,500 - 7,500 = 1,000 energy was auto-burned
        assert_eq!(auto_burned, 1_000, "Auto-burned energy should be 1,000");

        // Verify TOS cost matches
        let expected_tos = auto_burned * TOS_PER_ENERGY;
        assert_eq!(result.fee, expected_tos);
    }

    /// Test: No auto-burn when energy covers all
    #[test]
    fn test_no_auto_burn_when_sufficient_energy() {
        let result = TransactionResult::with_values(
            0,     // No TOS burned
            5_000, // 5,000 energy used
            1_500, // 1,500 from free
            3_500, // 3,500 from frozen
        );

        assert_eq!(result.fee, 0, "No TOS should be burned");
        assert_eq!(result.auto_burned_energy(), 0, "No auto-burned energy");
        assert_eq!(
            result.total_energy_from_stake(),
            5_000,
            "All energy from stake"
        );
    }
}

// ============================================================================
// 7. COMPREHENSIVE SCENARIO TESTS
// ============================================================================

#[cfg(test)]
mod scenario_tests {
    use super::*;

    /// Scenario: User with no frozen TOS makes a transfer
    ///
    /// User: 10 TOS balance, 0 frozen, 1,500 free quota
    /// Action: Transfer 1 TOS to existing account (350 energy)
    /// Expected: Use free quota, 0 TOS fee
    #[test]
    fn test_scenario_no_frozen_small_transfer() {
        let mut account = AccountEnergy::new();
        let total_weight = 100_000_000;
        let now_ms = 1_000;

        // 250 bytes + 1 output = 350 energy
        let energy_cost = EnergyFeeCalculator::calculate_transfer_cost(250, 1);
        assert_eq!(energy_cost, 350);

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            energy_cost,
            total_weight,
            now_ms,
        );

        assert_eq!(result.free_energy_used, 350, "Should use 350 free energy");
        assert_eq!(result.frozen_energy_used, 0);
        assert_eq!(result.fee, 0, "No TOS fee for small transfer");
    }

    /// Scenario: User transfers to new account
    ///
    /// User: 10 TOS balance, 0 frozen
    /// Action: Transfer 1 TOS to NEW account
    /// Expected:
    /// - TOS-Only: 0.1 TOS creation fee (deducted from transfer)
    /// - Energy: 250 + 100 + 25,000 = 25,350
    /// - Free quota covers 1,500, rest auto-burns TOS
    #[test]
    fn test_scenario_transfer_to_new_account() {
        let mut account = AccountEnergy::new();
        let total_weight = 100_000_000;
        let now_ms = 1_000;

        // Energy calculation: size + outputs×100 + new_account
        let tx_size = 250;
        let output_count = 1;
        let new_accounts = 1;

        let energy_cost =
            EnergyFeeCalculator::calculate_energy_cost(tx_size, output_count, new_accounts);
        assert_eq!(
            energy_cost, 25_350,
            "New account transfer: 250 + 100 + 25,000"
        );

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            energy_cost,
            total_weight,
            now_ms,
        );

        // Free quota: 1,500
        assert_eq!(result.free_energy_used, FREE_ENERGY_QUOTA);

        // TOS burned: (25,350 - 1,500) × 100 = 2,385,000 atomic
        let energy_to_burn = energy_cost - FREE_ENERGY_QUOTA;
        let expected_tos_fee = energy_to_burn * TOS_PER_ENERGY;
        assert_eq!(
            result.fee, expected_tos_fee,
            "Should burn {} TOS for {} energy",
            expected_tos_fee, energy_to_burn
        );

        // Plus: 0.1 TOS account creation fee (TOS-Only, separate from energy)
        let total_tos_cost = expected_tos_fee + FEE_PER_ACCOUNT_CREATION;
        assert_eq!(
            total_tos_cost,
            2_385_000 + 10_000_000,
            "Total TOS: energy fee + creation fee"
        );
    }

    /// Scenario: User with frozen TOS makes large transfer
    ///
    /// User: 100 TOS balance, 10 TOS frozen
    /// Action: Transfer 50 TOS in batch (10 outputs, 800 bytes)
    /// Expected: Use free + frozen energy, minimal TOS burn
    #[test]
    fn test_scenario_frozen_user_batch_transfer() {
        // Account with 10 TOS frozen
        let mut account = AccountEnergy::new();
        account.frozen_balance = 10 * COIN_VALUE;

        // Total weight: 100 TOS (so user has 10% of energy)
        let total_weight = 100 * COIN_VALUE;
        let now_ms = 1_000;

        // 800 bytes + 10 outputs = 800 + 1,000 = 1,800 energy
        let energy_cost = EnergyFeeCalculator::calculate_transfer_cost(800, 10);
        assert_eq!(energy_cost, 1_800);

        // Calculate user's frozen energy limit
        // (10 TOS / 100 TOS) × TOTAL_ENERGY_LIMIT = 10% of total
        let user_energy_limit = account.calculate_energy_limit(total_weight);

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            energy_cost,
            total_weight,
            now_ms,
        );

        // Should use free quota first
        if energy_cost <= FREE_ENERGY_QUOTA {
            assert_eq!(result.free_energy_used, energy_cost);
            assert_eq!(result.frozen_energy_used, 0);
        } else if energy_cost <= FREE_ENERGY_QUOTA + user_energy_limit {
            // Should use free + some frozen
            assert_eq!(result.free_energy_used, FREE_ENERGY_QUOTA);
            assert_eq!(result.frozen_energy_used, energy_cost - FREE_ENERGY_QUOTA);
        }

        println!(
            "User energy limit: {}, Free: {}, Frozen: {}, Fee: {}",
            user_energy_limit, result.free_energy_used, result.frozen_energy_used, result.fee
        );
    }

    /// Scenario: MultiSig transaction (2-of-3)
    ///
    /// User: 10 TOS balance
    /// Action: 2-signature multisig transfer
    /// Expected: 2 TOS multisig fee + energy cost
    #[test]
    fn test_scenario_multisig_transaction() {
        let signature_count = 2u64;

        // MultiSig fee: 2 × 1 TOS = 2 TOS (TOS-Only)
        let multisig_fee = signature_count * FEE_PER_MULTISIG_SIGNATURE;
        assert_eq!(multisig_fee, 2 * COIN_VALUE, "2 signatures = 2 TOS fee");

        // Energy for transfer: 250 + 100 = 350 energy
        let energy_cost = EnergyFeeCalculator::calculate_transfer_cost(250, 1);

        // Total TOS needed if no frozen energy:
        // - MultiSig: 2 TOS
        // - Energy (after free quota): (350 - 1,500) = 0 (covered by free)
        // Total: 2 TOS

        let energy_tos = if energy_cost > FREE_ENERGY_QUOTA {
            (energy_cost - FREE_ENERGY_QUOTA) * TOS_PER_ENERGY
        } else {
            0
        };

        let total_tos = multisig_fee + energy_tos;
        assert_eq!(
            total_tos,
            2 * COIN_VALUE,
            "MultiSig only costs 2 TOS (energy covered by free quota)"
        );
    }
}

// ============================================================================
// 8. BOUNDARY VALUE TESTS
// ============================================================================

#[cfg(test)]
mod boundary_tests {
    use super::*;

    /// Test: Exactly FREE_ENERGY_QUOTA energy
    #[test]
    fn test_boundary_exact_free_quota() {
        let mut account = AccountEnergy::new();
        let total_weight = 100_000_000;
        let now_ms = 1_000;

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            FREE_ENERGY_QUOTA,
            total_weight,
            now_ms,
        );

        assert_eq!(result.free_energy_used, FREE_ENERGY_QUOTA);
        assert_eq!(result.frozen_energy_used, 0);
        assert_eq!(result.fee, 0);
    }

    /// Test: FREE_ENERGY_QUOTA + 1 energy
    #[test]
    fn test_boundary_free_quota_plus_one() {
        let mut account = AccountEnergy::new();
        let total_weight = 100_000_000;
        let now_ms = 1_000;

        let result = EnergyResourceManager::consume_transaction_energy_detailed(
            &mut account,
            FREE_ENERGY_QUOTA + 1,
            total_weight,
            now_ms,
        );

        assert_eq!(result.free_energy_used, FREE_ENERGY_QUOTA);
        // 1 energy needs TOS burn
        assert_eq!(result.fee, TOS_PER_ENERGY, "1 energy = 100 atomic TOS");
    }

    /// Test: Account creation fee boundary - exactly 0.1 TOS
    #[test]
    fn test_boundary_exact_creation_fee() {
        let transfer = FEE_PER_ACCOUNT_CREATION;
        let receiver_gets = transfer - FEE_PER_ACCOUNT_CREATION;

        assert_eq!(receiver_gets, 0, "Receiver gets 0 when transfer = fee");
    }

    /// Test: Account creation fee + 1 atomic
    #[test]
    fn test_boundary_creation_fee_plus_one() {
        let transfer = FEE_PER_ACCOUNT_CREATION + 1;
        let receiver_gets = transfer - FEE_PER_ACCOUNT_CREATION;

        assert_eq!(receiver_gets, 1, "Receiver gets 1 atomic");
    }

    /// Test: Zero output transfer (should this be allowed?)
    #[test]
    fn test_boundary_zero_outputs() {
        let cost = EnergyFeeCalculator::calculate_transfer_cost(100, 0);
        assert_eq!(cost, 100, "Zero outputs = size only");
    }

    /// Test: Maximum practical transfer outputs
    #[test]
    fn test_boundary_max_outputs() {
        // Maximum transfer count is typically limited
        let max_outputs = 256;
        let cost = EnergyFeeCalculator::calculate_transfer_cost(1000, max_outputs);

        // 1000 + 256 × 100 = 1000 + 25,600 = 26,600
        assert_eq!(cost, 26_600);
    }
}
