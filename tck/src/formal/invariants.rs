//! Blockchain invariant definitions for formal verification

/// Check balance conservation invariant
///
/// Verifies that the sum of all balances equals the expected total supply.
pub fn check_balance_conservation(
    balances: &[u64],
    expected_total: u64,
) -> Result<(), InvariantViolation> {
    let actual_total: u64 = balances.iter().sum();

    if actual_total != expected_total {
        return Err(InvariantViolation::BalanceConservation {
            expected: expected_total,
            actual: actual_total,
        });
    }

    Ok(())
}

/// Check nonce monotonicity invariant
///
/// Verifies that a new nonce is strictly greater than the current nonce.
pub fn check_nonce_monotonicity(
    current_nonce: u64,
    new_nonce: u64,
) -> Result<(), InvariantViolation> {
    if new_nonce <= current_nonce {
        return Err(InvariantViolation::NonceMonotonicity {
            current: current_nonce,
            attempted: new_nonce,
        });
    }

    Ok(())
}

/// Check state equivalence invariant
///
/// Verifies that two state roots are equal (for parallel vs sequential execution).
pub fn check_state_equivalence(
    state_a: &[u8; 32],
    state_b: &[u8; 32],
) -> Result<(), InvariantViolation> {
    if state_a != state_b {
        return Err(InvariantViolation::StateEquivalence {
            state_a: hex::encode(state_a),
            state_b: hex::encode(state_b),
        });
    }

    Ok(())
}

/// Check no negative balance invariant
///
/// Verifies that a balance operation doesn't result in underflow.
pub fn check_no_negative_balance(
    current_balance: u64,
    debit_amount: u64,
) -> Result<u64, InvariantViolation> {
    current_balance
        .checked_sub(debit_amount)
        .ok_or(InvariantViolation::NegativeBalance {
            balance: current_balance,
            debit: debit_amount,
        })
}

/// Check no overflow invariant
///
/// Verifies that a balance operation doesn't result in overflow.
pub fn check_no_overflow(
    current_balance: u64,
    credit_amount: u64,
) -> Result<u64, InvariantViolation> {
    current_balance
        .checked_add(credit_amount)
        .ok_or(InvariantViolation::Overflow {
            balance: current_balance,
            credit: credit_amount,
        })
}

/// Invariant violation error
#[derive(Debug, Clone, thiserror::Error)]
#[allow(missing_docs)]
pub enum InvariantViolation {
    /// Balance conservation violated
    #[error("Balance conservation violated: expected {expected}, got {actual}")]
    BalanceConservation { expected: u64, actual: u64 },

    /// Nonce monotonicity violated
    #[error("Nonce monotonicity violated: current {current}, attempted {attempted}")]
    NonceMonotonicity { current: u64, attempted: u64 },

    /// State equivalence violated
    #[error("State equivalence violated: {state_a} != {state_b}")]
    StateEquivalence { state_a: String, state_b: String },

    /// Negative balance attempted
    #[error("Negative balance: {balance} - {debit} would underflow")]
    NegativeBalance { balance: u64, debit: u64 },

    /// Overflow attempted
    #[error("Overflow: {balance} + {credit} would overflow")]
    Overflow { balance: u64, credit: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balance_conservation_pass() {
        let balances = vec![100, 200, 300];
        assert!(check_balance_conservation(&balances, 600).is_ok());
    }

    #[test]
    fn test_balance_conservation_fail() {
        let balances = vec![100, 200, 300];
        assert!(check_balance_conservation(&balances, 500).is_err());
    }

    #[test]
    fn test_nonce_monotonicity_pass() {
        assert!(check_nonce_monotonicity(5, 6).is_ok());
    }

    #[test]
    fn test_nonce_monotonicity_fail() {
        assert!(check_nonce_monotonicity(5, 5).is_err());
        assert!(check_nonce_monotonicity(5, 4).is_err());
    }

    #[test]
    fn test_no_negative_balance() {
        assert!(check_no_negative_balance(100, 50).is_ok());
        assert!(check_no_negative_balance(100, 100).is_ok());
        assert!(check_no_negative_balance(100, 101).is_err());
    }

    #[test]
    fn test_no_overflow() {
        assert!(check_no_overflow(100, 50).is_ok());
        assert!(check_no_overflow(u64::MAX - 10, 10).is_ok());
        assert!(check_no_overflow(u64::MAX, 1).is_err());
    }
}
