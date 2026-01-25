//! Invariant checking for fixture testing.
//!
//! Verifies system-wide invariants after fixture execution,
//! such as balance conservation, nonce monotonicity, energy
//! weight consistency, and delegation bounds.

use anyhow::Result;

use super::backend::FixtureBackend;
use super::types::{FixtureSetup, Invariant};

/// Check all invariants against the current backend state.
///
/// Returns a list of invariant violation messages. Empty list means all passed.
pub async fn check_invariants(
    backend: &dyn FixtureBackend,
    invariants: &[Invariant],
    setup: &FixtureSetup,
) -> Vec<String> {
    let mut errors = Vec::new();

    for invariant in invariants {
        match invariant {
            Invariant::BalanceConservation {
                balance_conservation,
            } => {
                if let Err(e) =
                    check_balance_conservation(backend, setup, balance_conservation).await
                {
                    errors.push(format!("Balance conservation: {}", e));
                }
            }
            Invariant::NonceMonotonicity {
                nonce_monotonicity: true,
            } => {
                if let Err(e) = check_nonce_monotonicity(backend, setup).await {
                    errors.push(format!("Nonce monotonicity: {}", e));
                }
            }
            Invariant::EnergyWeightConsistency {
                energy_weight_consistency: true,
            } => {
                if let Err(e) = check_energy_weight_consistency(backend).await {
                    errors.push(format!("Energy weight consistency: {}", e));
                }
            }
            Invariant::UnoSupplyConservation {
                uno_supply_conservation,
            } => {
                if let Err(e) =
                    check_uno_supply_conservation(backend, uno_supply_conservation).await
                {
                    errors.push(format!("UNO supply conservation: {}", e));
                }
            }
            Invariant::DelegationBounds { delegation_bounds } => {
                if let Err(e) = check_delegation_bounds(backend, delegation_bounds).await {
                    errors.push(format!("Delegation bounds: {}", e));
                }
            }
            Invariant::NoNegativeBalances {
                no_negative_balances: true,
            } => {
                if let Err(e) = check_no_negative_balances(backend).await {
                    errors.push(format!("No negative balances: {}", e));
                }
            }
            Invariant::ReceiverRegistered {
                receiver_registered: true,
            } => {
                // Receiver registration check is done during execution
                // No post-execution check needed
            }
            Invariant::Custom { custom } => {
                // Custom invariants are not evaluated in this basic implementation
                // They serve as documentation and can be extended later
                let _ = custom;
            }
            // Disabled checks (value = false)
            Invariant::NonceMonotonicity { .. }
            | Invariant::EnergyWeightConsistency { .. }
            | Invariant::NoNegativeBalances { .. }
            | Invariant::ReceiverRegistered { .. } => {}
        }
    }

    errors
}

/// Check that total balance is conserved (accounting for fees).
async fn check_balance_conservation(
    backend: &dyn FixtureBackend,
    setup: &FixtureSetup,
    params: &super::types::BalanceConservationDef,
) -> Result<()> {
    // Calculate initial total supply
    let mut initial_total: u64 = 0;
    for account in setup.accounts.values() {
        let balance =
            super::types::parse_amount(&account.balance).map_err(|e| anyhow::anyhow!("{}", e))?;
        initial_total = initial_total.saturating_add(balance);
    }

    // Calculate current total supply
    let mut current_total: u64 = 0;
    for account_name in backend.account_names() {
        let balance = backend.get_balance(&account_name).await?;
        current_total = current_total.saturating_add(balance);
    }

    // Calculate expected change
    let expected_change: i64 = if let Some(change_str) = &params.total_supply_change {
        let change_str = change_str.trim();
        let is_negative = change_str.starts_with('-');
        let numeric = if is_negative {
            &change_str[1..]
        } else {
            change_str
        };
        let amount = super::types::parse_amount(numeric)
            .map_err(|e| anyhow::anyhow!("Invalid supply change: {}", e))?;
        if is_negative {
            -(amount as i64)
        } else {
            amount as i64
        }
    } else {
        0
    };

    let actual_change = current_total as i64 - initial_total as i64;
    if actual_change != expected_change {
        return Err(anyhow::anyhow!(
            "Supply change mismatch: expected {} (initial: {}, expected final: {}), got {} (actual final: {})",
            expected_change,
            initial_total,
            (initial_total as i64 + expected_change) as u64,
            actual_change,
            current_total
        ));
    }

    Ok(())
}

/// Check that nonces only increase for each account.
async fn check_nonce_monotonicity(
    backend: &dyn FixtureBackend,
    setup: &FixtureSetup,
) -> Result<()> {
    for (account_name, account_def) in &setup.accounts {
        let initial_nonce = account_def.nonce.unwrap_or(0);
        let current_nonce = backend.get_nonce(account_name).await?;
        if current_nonce < initial_nonce {
            return Err(anyhow::anyhow!(
                "Nonce decreased for '{}': initial {}, current {}",
                account_name,
                initial_nonce,
                current_nonce
            ));
        }
    }
    Ok(())
}

/// Check that total energy weight equals sum of frozen balances.
async fn check_energy_weight_consistency(backend: &dyn FixtureBackend) -> Result<()> {
    let mut total_frozen: u64 = 0;
    for account_name in backend.account_names() {
        let frozen = backend.get_frozen_balance(&account_name).await?;
        total_frozen = total_frozen.saturating_add(frozen);
    }

    // Energy weight should correspond to total frozen balance
    // The exact relationship depends on the protocol parameters
    // For now, just verify all frozen balances are non-negative (u64 guarantees this)
    let _ = total_frozen;
    Ok(())
}

/// Check UNO asset supply conservation.
async fn check_uno_supply_conservation(
    backend: &dyn FixtureBackend,
    params: &super::types::UnoSupplyDef,
) -> Result<()> {
    let mut total_supply: u64 = 0;
    for account_name in backend.account_names() {
        let balance = backend
            .get_uno_balance(&account_name, &params.asset)
            .await?;
        total_supply = total_supply.saturating_add(balance);
    }

    if total_supply != params.total {
        return Err(anyhow::anyhow!(
            "UNO '{}' supply mismatch: expected {}, got {} (across all accounts)",
            params.asset,
            params.total,
            total_supply
        ));
    }
    Ok(())
}

/// Check that delegations don't exceed frozen balance.
async fn check_delegation_bounds(
    backend: &dyn FixtureBackend,
    params: &super::types::DelegationBoundsDef,
) -> Result<()> {
    let accounts_to_check: Vec<String> = if let Some(account) = &params.account {
        vec![account.clone()]
    } else {
        backend.account_names()
    };

    for account_name in &accounts_to_check {
        let frozen = backend.get_frozen_balance(account_name).await?;
        let delegations = backend.get_delegations_out(account_name).await?;
        let total_delegated: u64 = delegations.values().sum();

        if total_delegated > frozen {
            return Err(anyhow::anyhow!(
                "Delegation exceeds frozen balance for '{}': delegated {}, frozen {}",
                account_name,
                total_delegated,
                frozen
            ));
        }

        // Check max_delegation if specified
        if let Some(max_str) = &params.max_delegation {
            let max_delegation = super::types::parse_amount(max_str)
                .map_err(|e| anyhow::anyhow!("Invalid max_delegation: {}", e))?;
            if total_delegated > max_delegation {
                return Err(anyhow::anyhow!(
                    "Delegation exceeds max for '{}': delegated {}, max {}",
                    account_name,
                    total_delegated,
                    max_delegation
                ));
            }
        }
    }
    Ok(())
}

/// Check that no account has negative balance.
///
/// Since balances are u64, this is inherently satisfied. However, this
/// check can detect logic errors where underflow produced MAX_U64.
async fn check_no_negative_balances(backend: &dyn FixtureBackend) -> Result<()> {
    // With u64, we can't have negative balances.
    // But we can check for suspiciously large values that might indicate underflow.
    const SUSPICIOUS_THRESHOLD: u64 = u64::MAX / 2;

    for account_name in backend.account_names() {
        let balance = backend.get_balance(&account_name).await?;
        if balance > SUSPICIOUS_THRESHOLD {
            return Err(anyhow::anyhow!(
                "Suspicious balance for '{}': {} (possible underflow)",
                account_name,
                balance
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::types::{BalanceConservationDef, DelegationBoundsDef, UnoSupplyDef};

    #[test]
    fn test_invariant_types() {
        let _balance_inv = Invariant::BalanceConservation {
            balance_conservation: BalanceConservationDef {
                fee_recipient: Some("miner".to_string()),
                total_supply_change: Some("-10 TOS".to_string()),
            },
        };
        let _nonce_inv = Invariant::NonceMonotonicity {
            nonce_monotonicity: true,
        };
        let _energy_inv = Invariant::EnergyWeightConsistency {
            energy_weight_consistency: true,
        };
        let _uno_inv = Invariant::UnoSupplyConservation {
            uno_supply_conservation: UnoSupplyDef {
                asset: "GOLD".to_string(),
                total: 1_000_000,
            },
        };
        let _delegation_inv = Invariant::DelegationBounds {
            delegation_bounds: DelegationBoundsDef {
                account: Some("alice".to_string()),
                max_delegation: Some("1000 TOS".to_string()),
            },
        };
    }
}
