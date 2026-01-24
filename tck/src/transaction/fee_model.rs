// Fee model tests - pure function testing for fee calculation.
//
// Tests the calculate_tx_fee, calculate_uno_tx_fee, and calculate_energy_fee
// functions which are pure and deterministic, making them ideal for direct unit testing.
//
// Fee formula (TOS and UNO):
//   fee = ceil(tx_size / BYTES_PER_KB) * FEE_PER_KB
//       + output_count * FEE_PER_TRANSFER
//       + new_addresses * FEE_PER_ACCOUNT_CREATION
//       + multisig * FEE_PER_TRANSFER
//
// Energy formula:
//   energy_cost = if output_count > 0 { ENERGY_PER_TRANSFER } else { 0 }
//   (i.e., 1 energy per transaction, regardless of size or output count)

#[cfg(test)]
mod tests {
    use tos_common::config::{
        BYTES_PER_KB, FEE_PER_ACCOUNT_CREATION, FEE_PER_KB, FEE_PER_TRANSFER, MAX_BLOCK_SIZE,
        MAX_TRANSACTION_SIZE, MAX_TXS_PER_BLOCK,
    };
    use tos_common::utils::{calculate_energy_fee, calculate_tx_fee};

    // =========================================================================
    // TOS Fee Tests
    // =========================================================================

    #[test]
    fn test_fee_zero_size_zero_outputs() {
        // Edge case: size=0 is a multiple of BYTES_PER_KB (0 % 1024 == 0)
        // so size_in_kb stays 0. With zero outputs, fee is 0.
        let fee = calculate_tx_fee(0, 0, 0, 0);
        assert_eq!(fee, 0);
    }

    #[test]
    fn test_fee_exact_one_kb() {
        // size=1024 is exactly 1KB, 1024 % 1024 == 0 so size_in_kb = 1
        // fee = 1 * 10000 = 10000
        let fee = calculate_tx_fee(1024, 0, 0, 0);
        assert_eq!(fee, FEE_PER_KB);
        assert_eq!(fee, 10000);
    }

    #[test]
    fn test_fee_less_than_one_kb() {
        // size=500 is less than 1KB. 500/1024=0, but 500%1024!=0 so rounds up.
        // size_in_kb = 0 + 1 = 1. fee = 1 * 10000 = 10000
        let fee = calculate_tx_fee(500, 0, 0, 0);
        assert_eq!(fee, FEE_PER_KB);
        assert_eq!(fee, 10000);
    }

    #[test]
    fn test_fee_one_byte_over_kb() {
        // size=1025. 1025/1024=1, but 1025%1024=1 != 0, so rounds up.
        // size_in_kb = 1 + 1 = 2. fee = 2 * 10000 = 20000
        let fee = calculate_tx_fee(1025, 0, 0, 0);
        assert_eq!(fee, 2 * FEE_PER_KB);
        assert_eq!(fee, 20000);
    }

    #[test]
    fn test_fee_exact_two_kb() {
        // size=2048 is exactly 2KB, 2048 % 1024 == 0 so size_in_kb = 2
        // fee = 2 * 10000 = 20000
        let fee = calculate_tx_fee(2048, 0, 0, 0);
        assert_eq!(fee, 2 * FEE_PER_KB);
        assert_eq!(fee, 20000);
    }

    #[test]
    fn test_fee_with_one_output() {
        // size=1024 (1KB) + 1 output
        // fee = 1 * 10000 + 1 * 5000 = 15000
        let fee = calculate_tx_fee(1024, 1, 0, 0);
        assert_eq!(fee, FEE_PER_KB + FEE_PER_TRANSFER);
        assert_eq!(fee, 15000);
    }

    #[test]
    fn test_fee_with_multiple_outputs() {
        // size=1024 (1KB) + 5 outputs
        // fee = 1 * 10000 + 5 * 5000 = 10000 + 25000 = 35000
        let fee = calculate_tx_fee(1024, 5, 0, 0);
        assert_eq!(fee, FEE_PER_KB + 5 * FEE_PER_TRANSFER);
        assert_eq!(fee, 35000);
    }

    #[test]
    fn test_fee_with_new_addresses() {
        // FEE_PER_ACCOUNT_CREATION is 0, so new addresses don't add cost
        // size=1024 (1KB) + 1 output + 1 new address
        // fee = 10000 + 5000 + 0 = 15000
        let fee = calculate_tx_fee(1024, 1, 1, 0);
        assert_eq!(
            fee,
            FEE_PER_KB + FEE_PER_TRANSFER + FEE_PER_ACCOUNT_CREATION
        );
        assert_eq!(fee, 15000);

        // With multiple new addresses, still no additional cost
        let fee_multi = calculate_tx_fee(1024, 1, 5, 0);
        assert_eq!(fee_multi, fee);
    }

    #[test]
    fn test_fee_with_multisig() {
        // multisig signatures each add FEE_PER_TRANSFER (5000)
        // size=1024 (1KB) + 0 outputs + 2 multisig signatures
        // fee = 10000 + 0 + 0 + 2 * 5000 = 20000
        let fee = calculate_tx_fee(1024, 0, 0, 2);
        assert_eq!(fee, FEE_PER_KB + 2 * FEE_PER_TRANSFER);
        assert_eq!(fee, 20000);
    }

    #[test]
    fn test_fee_combined_all_components() {
        // size=3000, outputs=3, new_addresses=1, multisig=2
        // size_in_kb: 3000/1024=2, 3000%1024=952!=0, so rounds up to 3
        // fee = 3 * 10000 + 3 * 5000 + 1 * 0 + 2 * 5000
        //     = 30000 + 15000 + 0 + 10000 = 55000
        let fee = calculate_tx_fee(3000, 3, 1, 2);
        assert_eq!(
            fee,
            3 * FEE_PER_KB + 3 * FEE_PER_TRANSFER + FEE_PER_ACCOUNT_CREATION + 2 * FEE_PER_TRANSFER
        );
        assert_eq!(fee, 55000);
    }

    #[test]
    fn test_fee_max_transaction_size() {
        // MAX_TRANSACTION_SIZE = 1024 * 1024 = 1048576 bytes = 1024 KB
        // 1048576 % 1024 == 0, so size_in_kb = 1024
        // fee = 1024 * 10000 = 10240000
        let fee = calculate_tx_fee(MAX_TRANSACTION_SIZE, 0, 0, 0);
        let expected_kb = MAX_TRANSACTION_SIZE as u64 / BYTES_PER_KB as u64;
        assert_eq!(fee, expected_kb * FEE_PER_KB);
        assert_eq!(fee, 1024 * 10000);
    }

    #[test]
    fn test_fee_large_output_count() {
        // outputs=100 with size=1024
        // fee = 1 * 10000 + 100 * 5000 = 10000 + 500000 = 510000
        // Verify no overflow occurs
        let fee = calculate_tx_fee(1024, 100, 0, 0);
        assert_eq!(fee, FEE_PER_KB + 100 * FEE_PER_TRANSFER);
        assert_eq!(fee, 510000);
    }

    // =========================================================================
    // Fee Constants Verification
    // =========================================================================

    #[test]
    fn test_constants_fee_per_kb() {
        assert_eq!(FEE_PER_KB, 10000);
    }

    #[test]
    fn test_constants_fee_per_transfer() {
        assert_eq!(FEE_PER_TRANSFER, 5000);
    }

    #[test]
    fn test_constants_bytes_per_kb() {
        assert_eq!(BYTES_PER_KB, 1024);
    }

    #[test]
    fn test_constants_max_block_size() {
        // MAX_BLOCK_SIZE = 1024 * 1024 + 256 * 1024 = 1310720
        assert_eq!(MAX_BLOCK_SIZE, 1_310_720);
    }

    #[test]
    fn test_constants_max_transaction_size() {
        // MAX_TRANSACTION_SIZE = 1024 * 1024 = 1048576
        assert_eq!(MAX_TRANSACTION_SIZE, 1_048_576);
    }

    #[test]
    fn test_constants_max_txs_per_block() {
        assert_eq!(MAX_TXS_PER_BLOCK, 10_000);
    }

    #[test]
    fn test_constants_fee_per_account_creation() {
        // Account creation is free
        assert_eq!(FEE_PER_ACCOUNT_CREATION, 0);
    }

    // =========================================================================
    // UNO Fee Tests
    // =========================================================================

    #[test]
    fn test_uno_fee_matches_tos_fee() {
        // UNO fee schedule uses the same constants as TOS:
        // UNO_FEE_PER_KB=10000, UNO_FEE_PER_TRANSFER=5000, UNO_FEE_PER_ACCOUNT_CREATION=0
        use tos_common::utils::calculate_uno_tx_fee;

        let tos_fee = calculate_tx_fee(1024, 1, 0, 0);
        let uno_fee = calculate_uno_tx_fee(1024, 1, 0, 0);
        assert_eq!(tos_fee, uno_fee);
    }

    #[test]
    fn test_uno_fee_with_outputs() {
        use tos_common::utils::calculate_uno_tx_fee;

        let fee = calculate_uno_tx_fee(1024, 3, 0, 0);
        // 1 * 10000 + 3 * 5000 = 25000
        assert_eq!(fee, 25000);
    }

    // =========================================================================
    // Energy Fee Tests
    // =========================================================================

    #[test]
    fn test_energy_fee_single_transfer() {
        // Energy model: 1 energy per transaction if outputs > 0
        // Size and new_addresses are ignored
        let fee = calculate_energy_fee(500, 1, 0);
        assert_eq!(fee, 1);
    }

    #[test]
    fn test_energy_fee_multiple_transfers() {
        // Multiple outputs still cost only 1 energy (per-transaction, not per-output)
        let fee = calculate_energy_fee(1024, 5, 0);
        assert_eq!(fee, 1);
    }

    #[test]
    fn test_energy_fee_with_new_addresses() {
        // New addresses don't add energy cost
        let fee = calculate_energy_fee(500, 1, 1);
        assert_eq!(fee, 1);

        let fee_multi = calculate_energy_fee(500, 1, 5);
        assert_eq!(fee_multi, 1);
    }

    #[test]
    fn test_energy_fee_zero_outputs() {
        // Zero outputs means zero energy cost
        let fee = calculate_energy_fee(1024, 0, 0);
        assert_eq!(fee, 0);
    }

    #[test]
    fn test_energy_fee_size_independent() {
        // Energy cost should be the same regardless of transaction size
        let small = calculate_energy_fee(100, 1, 0);
        let medium = calculate_energy_fee(5000, 1, 0);
        let large = calculate_energy_fee(100_000, 1, 0);
        assert_eq!(small, medium);
        assert_eq!(medium, large);
        assert_eq!(small, 1);
    }

    // =========================================================================
    // Fee Proportionality and Determinism Tests
    // =========================================================================

    #[test]
    fn test_fee_increases_with_size() {
        // Larger transactions should have higher size-based fees
        let fee_small = calculate_tx_fee(500, 0, 0, 0);
        let fee_medium = calculate_tx_fee(2048, 0, 0, 0);
        let fee_large = calculate_tx_fee(5000, 0, 0, 0);

        assert!(
            fee_small < fee_medium,
            "small={} should be < medium={}",
            fee_small,
            fee_medium
        );
        assert!(
            fee_medium < fee_large,
            "medium={} should be < large={}",
            fee_medium,
            fee_large
        );
    }

    #[test]
    fn test_fee_increases_with_outputs() {
        // More outputs should increase the fee
        let fee_0 = calculate_tx_fee(1024, 0, 0, 0);
        let fee_1 = calculate_tx_fee(1024, 1, 0, 0);
        let fee_5 = calculate_tx_fee(1024, 5, 0, 0);
        let fee_10 = calculate_tx_fee(1024, 10, 0, 0);

        assert!(fee_0 < fee_1);
        assert!(fee_1 < fee_5);
        assert!(fee_5 < fee_10);

        // The difference between consecutive output counts should be FEE_PER_TRANSFER
        assert_eq!(fee_1 - fee_0, FEE_PER_TRANSFER);
        assert_eq!(fee_5 - fee_1, 4 * FEE_PER_TRANSFER);
    }

    #[test]
    fn test_fee_deterministic() {
        // Same inputs must always produce the same output
        for _ in 0..100 {
            let fee = calculate_tx_fee(1500, 3, 1, 2);
            // size_in_kb: 1500/1024=1, 1500%1024=476!=0, rounds up to 2
            // fee = 2*10000 + 3*5000 + 1*0 + 2*5000 = 20000 + 15000 + 10000 = 45000
            assert_eq!(fee, 45000);
        }
    }

    // =========================================================================
    // Additional Edge Cases
    // =========================================================================

    #[test]
    fn test_fee_size_exactly_at_boundary() {
        // Test all KB boundaries near the edge
        let fee_1023 = calculate_tx_fee(1023, 0, 0, 0);
        let fee_1024 = calculate_tx_fee(1024, 0, 0, 0);
        let fee_1025 = calculate_tx_fee(1025, 0, 0, 0);

        // 1023 rounds up to 1KB
        assert_eq!(fee_1023, FEE_PER_KB);
        // 1024 is exactly 1KB
        assert_eq!(fee_1024, FEE_PER_KB);
        // 1025 rounds up to 2KB
        assert_eq!(fee_1025, 2 * FEE_PER_KB);
    }

    #[test]
    fn test_fee_size_one_byte() {
        // Minimum non-zero size: 1 byte rounds up to 1KB
        let fee = calculate_tx_fee(1, 0, 0, 0);
        assert_eq!(fee, FEE_PER_KB);
    }

    #[test]
    fn test_fee_multisig_and_outputs_combined() {
        // Both multisig and outputs use FEE_PER_TRANSFER
        // size=1024, outputs=2, multisig=3
        // fee = 10000 + 2*5000 + 3*5000 = 10000 + 10000 + 15000 = 35000
        let fee = calculate_tx_fee(1024, 2, 0, 3);
        assert_eq!(fee, FEE_PER_KB + (2 + 3) * FEE_PER_TRANSFER);
        assert_eq!(fee, 35000);
    }

    #[test]
    fn test_fee_max_transaction_size_with_outputs() {
        // MAX_TRANSACTION_SIZE with maximum realistic output count
        // Verify no overflow with u64 arithmetic
        let fee = calculate_tx_fee(MAX_TRANSACTION_SIZE, 500, 0, 0);
        let expected = (MAX_TRANSACTION_SIZE as u64 / BYTES_PER_KB as u64) * FEE_PER_KB
            + 500 * FEE_PER_TRANSFER;
        assert_eq!(fee, expected);
    }

    #[test]
    fn test_energy_fee_deterministic() {
        // Energy fee should be deterministic
        for _ in 0..100 {
            let fee = calculate_energy_fee(1024, 3, 1);
            assert_eq!(fee, 1);
        }
    }

    #[test]
    fn test_energy_fee_large_values() {
        // Even with very large parameters, energy cost is always 0 or 1
        let fee = calculate_energy_fee(1_000_000, 1000, 500);
        assert_eq!(fee, 1);
    }
}
