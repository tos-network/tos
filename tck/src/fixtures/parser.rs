//! YAML fixture parsing.
//!
//! Parses YAML fixture definition files into the typed Fixture structure.
//! Handles template resolution, parameter interpolation, and validation.

use std::path::Path;

use anyhow::{anyhow, Context, Result};

use super::types::{Fixture, FixtureSetup, Step, TransactionStep, TransactionType};

/// Parse a fixture from a YAML string.
pub fn parse_fixture(yaml: &str) -> Result<Fixture> {
    let fixture: Fixture = serde_yaml::from_str(yaml).context("Failed to parse fixture YAML")?;

    validate_fixture(&fixture)?;
    Ok(fixture)
}

/// Parse a fixture from a file path.
pub fn parse_fixture_file(path: &Path) -> Result<Fixture> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read fixture file: {}", path.display()))?;
    parse_fixture(&content)
}

/// Validate a parsed fixture for consistency.
fn validate_fixture(fixture: &Fixture) -> Result<()> {
    validate_meta(fixture)?;
    validate_setup(&fixture.setup)?;
    validate_transactions(&fixture.transactions, &fixture.setup)?;
    Ok(())
}

/// Validate fixture metadata.
fn validate_meta(fixture: &Fixture) -> Result<()> {
    if fixture.fixture.name.is_empty() {
        return Err(anyhow!("Fixture name cannot be empty"));
    }
    for tier in &fixture.fixture.tier {
        if *tier == 0 || *tier > 3 {
            return Err(anyhow!("Invalid tier {}: must be 1, 2, or 3", tier));
        }
    }
    Ok(())
}

/// Validate fixture setup.
fn validate_setup(setup: &FixtureSetup) -> Result<()> {
    if setup.accounts.is_empty() {
        return Err(anyhow!("Fixture must define at least one account"));
    }
    for (name, account) in &setup.accounts {
        // Validate balance is parseable
        super::types::parse_amount(&account.balance)
            .map_err(|e| anyhow!("Invalid balance for account '{}': {}", name, e))?;

        // Validate frozen balance if specified
        if let Some(frozen) = &account.frozen_balance {
            super::types::parse_amount(frozen)
                .map_err(|e| anyhow!("Invalid frozen_balance for account '{}': {}", name, e))?;
        }
    }
    Ok(())
}

/// Validate transaction steps.
fn validate_transactions(steps: &[Step], setup: &FixtureSetup) -> Result<()> {
    for step in steps {
        if let Step::Transaction(tx) = step {
            validate_transaction_step(tx, setup)?;
        }
    }
    Ok(())
}

/// Validate a single transaction step.
fn validate_transaction_step(step: &TransactionStep, setup: &FixtureSetup) -> Result<()> {
    let step_name = step.name.as_deref().unwrap_or("unnamed");

    match &step.tx_type {
        TransactionType::Transfer | TransactionType::UnoTransfer => {
            // Must have from, to, amount
            if step.from.is_none() {
                return Err(anyhow!(
                    "Step '{}': transfer requires 'from' field",
                    step_name
                ));
            }
            if step.to.is_none() {
                return Err(anyhow!(
                    "Step '{}': transfer requires 'to' field",
                    step_name
                ));
            }
            if step.amount.is_none() {
                return Err(anyhow!(
                    "Step '{}': transfer requires 'amount' field",
                    step_name
                ));
            }
            // Validate accounts exist in setup
            if let Some(from) = &step.from {
                if !setup.accounts.contains_key(from) {
                    return Err(anyhow!(
                        "Step '{}': sender '{}' not defined in setup",
                        step_name,
                        from
                    ));
                }
            }
            if let Some(to) = &step.to {
                if !setup.accounts.contains_key(to) {
                    return Err(anyhow!(
                        "Step '{}': receiver '{}' not defined in setup",
                        step_name,
                        to
                    ));
                }
            }
            // UNO transfer must specify asset
            if step.tx_type == TransactionType::UnoTransfer && step.asset.is_none() {
                return Err(anyhow!(
                    "Step '{}': uno_transfer requires 'asset' field",
                    step_name
                ));
            }
        }
        TransactionType::Freeze | TransactionType::Unfreeze => {
            if step.from.is_none() {
                return Err(anyhow!(
                    "Step '{}': freeze/unfreeze requires 'from' field",
                    step_name
                ));
            }
            if step.amount.is_none() {
                return Err(anyhow!(
                    "Step '{}': freeze/unfreeze requires 'amount' field",
                    step_name
                ));
            }
        }
        TransactionType::Delegate | TransactionType::Undelegate => {
            if step.from.is_none() {
                return Err(anyhow!(
                    "Step '{}': delegate/undelegate requires 'from' field",
                    step_name
                ));
            }
            if step.to.is_none() {
                return Err(anyhow!(
                    "Step '{}': delegate/undelegate requires 'to' field",
                    step_name
                ));
            }
            if step.amount.is_none() {
                return Err(anyhow!(
                    "Step '{}': delegate/undelegate requires 'amount' field",
                    step_name
                ));
            }
        }
        TransactionType::AdvanceTime => {
            if step.duration.is_none() {
                return Err(anyhow!(
                    "Step '{}': advance_time requires 'duration' field",
                    step_name
                ));
            }
        }
        TransactionType::DeployContract => {
            if step.from.is_none() {
                return Err(anyhow!(
                    "Step '{}': deploy_contract requires 'from' field",
                    step_name
                ));
            }
            if step.code.is_none() {
                return Err(anyhow!(
                    "Step '{}': deploy_contract requires 'code' field",
                    step_name
                ));
            }
        }
        TransactionType::CallContract => {
            if step.from.is_none() {
                return Err(anyhow!(
                    "Step '{}': call_contract requires 'from' field",
                    step_name
                ));
            }
            if step.contract.is_none() {
                return Err(anyhow!(
                    "Step '{}': call_contract requires 'contract' field",
                    step_name
                ));
            }
        }
        // MineBlock and Register have no required fields beyond type
        TransactionType::MineBlock | TransactionType::Register => {}
    }

    // Validate amount is parseable if specified
    if let Some(amount) = &step.amount {
        super::types::parse_amount(amount)
            .map_err(|e| anyhow!("Step '{}': invalid amount: {}", step_name, e))?;
    }

    // Validate fee is parseable if specified
    if let Some(fee) = &step.fee {
        super::types::parse_amount(fee)
            .map_err(|e| anyhow!("Step '{}': invalid fee: {}", step_name, e))?;
    }

    // Validate duration is parseable for advance_time
    if let Some(duration) = &step.duration {
        if step.tx_type == TransactionType::AdvanceTime {
            super::types::parse_duration(duration)
                .map_err(|e| anyhow!("Step '{}': invalid duration: {}", step_name, e))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC_FIXTURE: &str = r#"
fixture:
  name: "basic_transfer"
  version: "1.0"
  description: "Simple TOS transfer"
  tier: [1, 2, 3]

setup:
  accounts:
    alice:
      balance: "10000 TOS"
      nonce: 0
    bob:
      balance: "1000 TOS"
      nonce: 0

transactions:
  - step: 1
    name: "Alice sends TOS to Bob"
    type: transfer
    from: alice
    to: bob
    amount: "2000 TOS"
    fee: "10 TOS"
    expect_status: success
  - step: 2
    name: "Mine block"
    type: mine_block
    expect_status: success

expected:
  accounts:
    alice:
      balance: "7990 TOS"
      nonce: 1
    bob:
      balance: "3000 TOS"
      nonce: 0

invariants:
  - balance_conservation:
      total_supply_change: "-10 TOS"
  - nonce_monotonicity: true
"#;

    #[test]
    fn test_parse_basic_fixture() {
        let fixture = parse_fixture(BASIC_FIXTURE).unwrap();
        assert_eq!(fixture.fixture.name, "basic_transfer");
        assert_eq!(fixture.fixture.tier, vec![1, 2, 3]);
        assert_eq!(fixture.setup.accounts.len(), 2);
        assert_eq!(fixture.transactions.len(), 2);
        assert!(fixture.expected.is_some());
        assert_eq!(fixture.invariants.len(), 2);
    }

    #[test]
    fn test_parse_empty_name_error() {
        let yaml = r#"
fixture:
  name: ""
setup:
  accounts:
    alice:
      balance: "1000 TOS"
transactions: []
"#;
        assert!(parse_fixture(yaml).is_err());
    }

    #[test]
    fn test_parse_invalid_tier() {
        let yaml = r#"
fixture:
  name: "test"
  tier: [0]
setup:
  accounts:
    alice:
      balance: "1000 TOS"
transactions: []
"#;
        assert!(parse_fixture(yaml).is_err());
    }

    #[test]
    fn test_parse_no_accounts_error() {
        let yaml = r#"
fixture:
  name: "test"
setup:
  accounts: {}
transactions: []
"#;
        assert!(parse_fixture(yaml).is_err());
    }

    #[test]
    fn test_parse_invalid_balance() {
        let yaml = r#"
fixture:
  name: "test"
setup:
  accounts:
    alice:
      balance: "not_a_number"
transactions: []
"#;
        assert!(parse_fixture(yaml).is_err());
    }

    #[test]
    fn test_parse_transfer_missing_to() {
        let yaml = r#"
fixture:
  name: "test"
setup:
  accounts:
    alice:
      balance: "1000 TOS"
transactions:
  - step: 1
    type: transfer
    from: alice
    amount: "100 TOS"
    expect_status: success
"#;
        assert!(parse_fixture(yaml).is_err());
    }

    #[test]
    fn test_parse_transfer_unknown_account() {
        let yaml = r#"
fixture:
  name: "test"
setup:
  accounts:
    alice:
      balance: "1000 TOS"
transactions:
  - step: 1
    type: transfer
    from: alice
    to: bob
    amount: "100 TOS"
    expect_status: success
"#;
        assert!(parse_fixture(yaml).is_err());
    }

    #[test]
    fn test_parse_error_case_fixture() {
        let yaml = r#"
fixture:
  name: "error_test"
setup:
  accounts:
    alice:
      balance: "100 TOS"
    bob:
      balance: "0 TOS"
transactions:
  - step: 1
    name: "Overdraft"
    type: transfer
    from: alice
    to: bob
    amount: "999999 TOS"
    expect_status: error
    expect_error: INSUFFICIENT_BALANCE
"#;
        let fixture = parse_fixture(yaml).unwrap();
        if let Step::Transaction(tx) = &fixture.transactions[0] {
            assert_eq!(tx.expect_status, super::super::types::ExpectStatus::Error);
            assert_eq!(tx.expect_error.as_deref(), Some("INSUFFICIENT_BALANCE"));
        } else {
            panic!("Expected transaction step");
        }
    }

    #[test]
    fn test_parse_advance_time_requires_duration() {
        let yaml = r#"
fixture:
  name: "test"
setup:
  accounts:
    alice:
      balance: "1000 TOS"
transactions:
  - step: 1
    type: advance_time
    expect_status: success
"#;
        assert!(parse_fixture(yaml).is_err());
    }

    #[test]
    fn test_parse_freeze_step() {
        let yaml = r#"
fixture:
  name: "freeze_test"
setup:
  accounts:
    alice:
      balance: "10000 TOS"
transactions:
  - step: 1
    type: freeze
    from: alice
    amount: "1000 TOS"
    expect_status: success
"#;
        let fixture = parse_fixture(yaml).unwrap();
        if let Step::Transaction(tx) = &fixture.transactions[0] {
            assert_eq!(tx.tx_type, TransactionType::Freeze);
        } else {
            panic!("Expected transaction step");
        }
    }
}
