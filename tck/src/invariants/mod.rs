//! Core invariant checkers
//!
//! Implements blockchain invariants:
//! - Balance conservation (total supply must be constant)
//! - Nonce monotonicity (nonce must equal confirmed tx count)
//! - State equivalence (parallel execution must match sequential)
//! - BlockDAG properties
//! - Fee deduction

use anyhow::{bail, Result};

/// Check balance conservation invariant
///
/// Verifies that the total balance before and after an operation are equal.
/// This ensures no tokens are created or destroyed during execution.
///
/// # Arguments
/// * `total_before` - Sum of all balances before the operation
/// * `total_after` - Sum of all balances after the operation
///
/// # Errors
/// Returns error if balances don't match (conservation violated)
pub fn check_balance_conservation(total_before: u64, total_after: u64) -> Result<()> {
    if total_before != total_after {
        bail!(
            "Balance conservation violated: total before ({}) != total after ({}), difference: {}",
            total_before,
            total_after,
            if total_before > total_after {
                format!("-{}", total_before - total_after)
            } else {
                format!("+{}", total_after - total_before)
            }
        );
    }
    Ok(())
}

/// Check balance conservation with fee consideration
///
/// For transactions with fees, the total after should be less by exactly the fee amount
/// (fees are burned or sent to miners, which should be accounted for separately).
///
/// # Arguments
/// * `total_before` - Sum of all balances before the operation
/// * `total_after` - Sum of all balances after the operation
/// * `fees_collected` - Total fees deducted during operation
pub fn check_balance_conservation_with_fees(
    total_before: u64,
    total_after: u64,
    fees_collected: u64,
) -> Result<()> {
    let expected_after = total_before.checked_sub(fees_collected).ok_or_else(|| {
        anyhow::anyhow!(
            "Fee calculation underflow: total {} < fees {}",
            total_before,
            fees_collected
        )
    })?;

    if total_after != expected_after {
        bail!(
            "Balance conservation with fees violated: expected {} (before {} - fees {}), got {}",
            expected_after,
            total_before,
            fees_collected,
            total_after
        );
    }
    Ok(())
}

/// Check nonce monotonicity invariant
///
/// Verifies that the account nonce equals the number of confirmed transactions.
/// This ensures transaction ordering is correct and no transactions are skipped.
///
/// # Arguments
/// * `nonce` - Current account nonce
/// * `confirmed_count` - Number of confirmed transactions for this account
///
/// # Errors
/// Returns error if nonce doesn't match confirmed transaction count
pub fn check_nonce_monotonicity(nonce: u64, confirmed_count: u64) -> Result<()> {
    if nonce != confirmed_count {
        bail!(
            "Nonce monotonicity violated: nonce ({}) != confirmed tx count ({})",
            nonce,
            confirmed_count
        );
    }
    Ok(())
}

/// Check nonce is strictly increasing
///
/// Verifies that a new nonce is exactly one more than the current nonce.
///
/// # Arguments
/// * `current_nonce` - Current account nonce
/// * `new_nonce` - Proposed new nonce value
pub fn check_nonce_increment(current_nonce: u64, new_nonce: u64) -> Result<()> {
    let expected = current_nonce.checked_add(1).ok_or_else(|| {
        anyhow::anyhow!(
            "Nonce overflow: current nonce {} is at maximum",
            current_nonce
        )
    })?;

    if new_nonce != expected {
        bail!(
            "Nonce increment violated: expected {} (current {} + 1), got {}",
            expected,
            current_nonce,
            new_nonce
        );
    }
    Ok(())
}

/// Check state equivalence invariant (parallel ≡ sequential)
///
/// Verifies that two state representations are identical.
/// This is used to ensure parallel execution produces the same result as sequential.
///
/// # Arguments
/// * `state1` - First state representation (e.g., from parallel execution)
/// * `state2` - Second state representation (e.g., from sequential execution)
///
/// # Errors
/// Returns error if states don't match
pub fn check_state_equivalence(state1: &[u8], state2: &[u8]) -> Result<()> {
    if state1.len() != state2.len() {
        bail!(
            "State equivalence violated: length mismatch ({} != {})",
            state1.len(),
            state2.len()
        );
    }

    if state1 != state2 {
        // Find first differing byte for debugging
        let first_diff = state1
            .iter()
            .zip(state2.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(0);

        bail!(
            "State equivalence violated: states differ at byte {} (0x{:02x} != 0x{:02x})",
            first_diff,
            state1.get(first_diff).copied().unwrap_or(0),
            state2.get(first_diff).copied().unwrap_or(0)
        );
    }
    Ok(())
}

/// Check state root equivalence
///
/// Verifies that two 32-byte state roots are identical.
pub fn check_state_root_equivalence(root1: &[u8; 32], root2: &[u8; 32]) -> Result<()> {
    if root1 != root2 {
        bail!(
            "State root equivalence violated: {} != {}",
            hex::encode(root1),
            hex::encode(root2)
        );
    }
    Ok(())
}

/// Calculate sum of balances with overflow checking
///
/// # Arguments
/// * `balances` - Slice of account balances
///
/// # Returns
/// Sum of all balances, or error if overflow would occur
pub fn sum_balances(balances: &[u64]) -> Result<u64> {
    balances.iter().try_fold(0u64, |acc, &balance| {
        acc.checked_add(balance).ok_or_else(|| {
            anyhow::anyhow!(
                "Balance sum overflow at accumulator {}, adding {}",
                acc,
                balance
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_conservation_pass() {
        assert!(check_balance_conservation(1000, 1000).is_ok());
        assert!(check_balance_conservation(0, 0).is_ok());
        assert!(check_balance_conservation(u64::MAX, u64::MAX).is_ok());
    }

    #[test]
    fn test_balance_conservation_fail() {
        let err = check_balance_conservation(1000, 999).unwrap_err();
        assert!(err.to_string().contains("conservation violated"));

        let err = check_balance_conservation(1000, 1001).unwrap_err();
        assert!(err.to_string().contains("conservation violated"));
    }

    #[test]
    fn test_balance_conservation_with_fees() {
        assert!(check_balance_conservation_with_fees(1000, 990, 10).is_ok());
        assert!(check_balance_conservation_with_fees(1000, 1000, 0).is_ok());

        let err = check_balance_conservation_with_fees(1000, 995, 10).unwrap_err();
        assert!(err.to_string().contains("fees violated"));
    }

    #[test]
    fn test_nonce_monotonicity_pass() {
        assert!(check_nonce_monotonicity(0, 0).is_ok());
        assert!(check_nonce_monotonicity(5, 5).is_ok());
        assert!(check_nonce_monotonicity(100, 100).is_ok());
    }

    #[test]
    fn test_nonce_monotonicity_fail() {
        let err = check_nonce_monotonicity(5, 4).unwrap_err();
        assert!(err.to_string().contains("monotonicity violated"));

        let err = check_nonce_monotonicity(5, 6).unwrap_err();
        assert!(err.to_string().contains("monotonicity violated"));
    }

    #[test]
    fn test_nonce_increment() {
        assert!(check_nonce_increment(0, 1).is_ok());
        assert!(check_nonce_increment(5, 6).is_ok());

        assert!(check_nonce_increment(5, 5).is_err());
        assert!(check_nonce_increment(5, 7).is_err());
        assert!(check_nonce_increment(u64::MAX, 0).is_err()); // Overflow case
    }

    #[test]
    fn test_state_equivalence_pass() {
        let state1 = [1u8, 2, 3, 4];
        let state2 = [1u8, 2, 3, 4];
        assert!(check_state_equivalence(&state1, &state2).is_ok());

        let empty1: [u8; 0] = [];
        let empty2: [u8; 0] = [];
        assert!(check_state_equivalence(&empty1, &empty2).is_ok());
    }

    #[test]
    fn test_state_equivalence_fail() {
        let state1 = [1u8, 2, 3, 4];
        let state2 = [1u8, 2, 3, 5];
        let err = check_state_equivalence(&state1, &state2).unwrap_err();
        assert!(err.to_string().contains("differ at byte 3"));

        let state3 = [1u8, 2, 3];
        let err = check_state_equivalence(&state1, &state3).unwrap_err();
        assert!(err.to_string().contains("length mismatch"));
    }

    #[test]
    fn test_sum_balances() {
        assert_eq!(sum_balances(&[100, 200, 300]).unwrap(), 600);
        assert_eq!(sum_balances(&[]).unwrap(), 0);
        assert_eq!(sum_balances(&[u64::MAX]).unwrap(), u64::MAX);

        // Overflow case
        assert!(sum_balances(&[u64::MAX, 1]).is_err());
    }

    #[test]
    fn test_state_root_equivalence() {
        let root1 = [0u8; 32];
        let root2 = [0u8; 32];
        assert!(check_state_root_equivalence(&root1, &root2).is_ok());

        let mut root3 = [0u8; 32];
        root3[31] = 1;
        assert!(check_state_root_equivalence(&root1, &root3).is_err());
    }
}
