//! Expected state verification for fixture testing.
//!
//! Compares actual backend state against the expected state defined
//! in the fixture YAML, producing detailed error messages on mismatch.

use anyhow::Result;

use super::backend::FixtureBackend;
use super::types::{AccountExpected, ExpectedState};

/// Verify the expected state against a backend's actual state.
///
/// Returns a list of verification errors. Empty list means all passed.
pub async fn verify_expected(
    backend: &dyn FixtureBackend,
    expected: &ExpectedState,
) -> Vec<String> {
    let mut errors = Vec::new();

    for (account_name, expected_account) in &expected.accounts {
        verify_account(backend, account_name, expected_account, &mut errors).await;
    }

    errors
}

/// Verify a single account's state against expectations.
async fn verify_account(
    backend: &dyn FixtureBackend,
    account_name: &str,
    expected: &AccountExpected,
    errors: &mut Vec<String>,
) {
    // Verify balance
    if let Some(expected_balance_str) = &expected.balance {
        match super::types::parse_amount(expected_balance_str) {
            Ok(expected_balance) => match backend.get_balance(account_name).await {
                Ok(actual) => {
                    if actual != expected_balance {
                        errors.push(format!(
                            "Balance mismatch for '{}': expected {}, got {}",
                            account_name, expected_balance, actual
                        ));
                    }
                }
                Err(e) => {
                    errors.push(format!(
                        "Failed to query balance for '{}': {}",
                        account_name, e
                    ));
                }
            },
            Err(e) => {
                errors.push(format!(
                    "Invalid expected balance for '{}': {}",
                    account_name, e
                ));
            }
        }
    }

    // Verify nonce
    if let Some(expected_nonce) = expected.nonce {
        match backend.get_nonce(account_name).await {
            Ok(actual) => {
                if actual != expected_nonce {
                    errors.push(format!(
                        "Nonce mismatch for '{}': expected {}, got {}",
                        account_name, expected_nonce, actual
                    ));
                }
            }
            Err(e) => {
                errors.push(format!(
                    "Failed to query nonce for '{}': {}",
                    account_name, e
                ));
            }
        }
    }

    // Verify UNO balances
    if let Some(expected_uno) = &expected.uno_balances {
        for (asset, expected_amount) in expected_uno {
            match backend.get_uno_balance(account_name, asset).await {
                Ok(actual) => {
                    if actual != *expected_amount {
                        errors.push(format!(
                            "UNO balance mismatch for '{}' asset '{}': expected {}, got {}",
                            account_name, asset, expected_amount, actual
                        ));
                    }
                }
                Err(e) => {
                    errors.push(format!(
                        "Failed to query UNO balance for '{}' asset '{}': {}",
                        account_name, asset, e
                    ));
                }
            }
        }
    }

    // Verify frozen balance
    if let Some(frozen_str) = &expected.frozen_balance {
        match super::types::parse_amount(frozen_str) {
            Ok(expected_frozen) => match backend.get_frozen_balance(account_name).await {
                Ok(actual) => {
                    if actual != expected_frozen {
                        errors.push(format!(
                            "Frozen balance mismatch for '{}': expected {}, got {}",
                            account_name, expected_frozen, actual
                        ));
                    }
                }
                Err(e) => {
                    errors.push(format!(
                        "Failed to query frozen balance for '{}': {}",
                        account_name, e
                    ));
                }
            },
            Err(e) => {
                errors.push(format!(
                    "Invalid expected frozen balance for '{}': {}",
                    account_name, e
                ));
            }
        }
    }

    // Verify energy state
    if let Some(expected_energy) = &expected.energy {
        match backend.get_energy(account_name).await {
            Ok(actual) => {
                if let Some(expected_limit) = expected_energy.limit {
                    if actual.limit != expected_limit {
                        errors.push(format!(
                            "Energy limit mismatch for '{}': expected {}, got {}",
                            account_name, expected_limit, actual.limit
                        ));
                    }
                }
                if let Some(expected_usage) = expected_energy.usage {
                    if actual.usage != expected_usage {
                        errors.push(format!(
                            "Energy usage mismatch for '{}': expected {}, got {}",
                            account_name, expected_usage, actual.usage
                        ));
                    }
                }
                if let Some(expected_available) = expected_energy.available {
                    if actual.available != expected_available {
                        errors.push(format!(
                            "Energy available mismatch for '{}': expected {}, got {}",
                            account_name, expected_available, actual.available
                        ));
                    }
                }
            }
            Err(e) => {
                errors.push(format!(
                    "Failed to query energy for '{}': {}",
                    account_name, e
                ));
            }
        }
    }

    // Verify outgoing delegations
    if let Some(expected_delegations) = &expected.delegations_out {
        match backend.get_delegations_out(account_name).await {
            Ok(actual) => {
                for (delegate_to, expected_amount_str) in expected_delegations {
                    match super::types::parse_amount(expected_amount_str) {
                        Ok(expected_amount) => {
                            let actual_amount = actual.get(delegate_to).copied().unwrap_or(0);
                            if actual_amount != expected_amount {
                                errors.push(format!(
                                    "Delegation out mismatch for '{}' -> '{}': expected {}, got {}",
                                    account_name, delegate_to, expected_amount, actual_amount
                                ));
                            }
                        }
                        Err(e) => {
                            errors.push(format!(
                                "Invalid delegation amount for '{}' -> '{}': {}",
                                account_name, delegate_to, e
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                errors.push(format!(
                    "Failed to query delegations_out for '{}': {}",
                    account_name, e
                ));
            }
        }
    }

    // Verify incoming delegations
    if let Some(expected_delegations) = &expected.delegations_in {
        match backend.get_delegations_in(account_name).await {
            Ok(actual) => {
                for (delegator, expected_amount_str) in expected_delegations {
                    match super::types::parse_amount(expected_amount_str) {
                        Ok(expected_amount) => {
                            let actual_amount = actual.get(delegator).copied().unwrap_or(0);
                            if actual_amount != expected_amount {
                                errors.push(format!(
                                    "Delegation in mismatch for '{}' <- '{}': expected {}, got {}",
                                    account_name, delegator, expected_amount, actual_amount
                                ));
                            }
                        }
                        Err(e) => {
                            errors.push(format!(
                                "Invalid delegation amount for '{}' <- '{}': {}",
                                account_name, delegator, e
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                errors.push(format!(
                    "Failed to query delegations_in for '{}': {}",
                    account_name, e
                ));
            }
        }
    }
}

/// Verify a checkpoint's expected state.
pub async fn verify_checkpoint(
    backend: &dyn FixtureBackend,
    checkpoint_name: &str,
    verify: &std::collections::HashMap<String, AccountExpected>,
) -> Result<()> {
    let expected = ExpectedState {
        accounts: verify.clone(),
    };
    let errors = verify_expected(backend, &expected).await;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Checkpoint '{}' verification failed:\n  - {}",
            checkpoint_name,
            errors.join("\n  - ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::types::EnergyExpected;

    #[test]
    fn test_account_expected_with_energy() {
        let expected = AccountExpected {
            balance: Some("1000 TOS".to_string()),
            nonce: Some(5),
            uno_balances: None,
            frozen_balance: Some("500 TOS".to_string()),
            energy: Some(EnergyExpected {
                limit: Some(920_000),
                usage: Some(0),
                available: Some(920_000),
            }),
            delegations_out: None,
            delegations_in: None,
        };
        assert_eq!(expected.nonce, Some(5));
        assert!(expected.energy.is_some());
    }
}
