//! YAML scenario parser with V2.2 format corrections
//!
//! Implements parser following TOS Testing Framework V3.0 spec:
//! - No underscores in numbers (use strings: "1000000000000")
//! - No tilde approximation (use `within` structure)
//! - Support `within`, `compare`, and `eq` assertions
//!
//! ## Example Scenario
//!
//! ```yaml
//! name: "Simple Transfer"
//! genesis:
//!   accounts:
//!     - name: "alice"
//!       balance: "1000000000000"  # String, no underscores
//! steps:
//!   - action: "transfer"
//!     from: "alice"
//!     to: "bob"
//!     amount: "100000000000"
//!     fee: "50"
//!   - action: "assert_balance"
//!     account: "alice"
//!     within:
//!       target: "899999999950"
//!       tolerance: "100"
//! ```

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Complete test scenario loaded from YAML
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TestScenario {
    /// Scenario name
    pub name: String,

    /// Optional description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Genesis configuration
    pub genesis: GenesisConfig,

    /// Execution steps
    pub steps: Vec<Step>,

    /// Invariants to check after execution
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invariants: Option<Vec<String>>,
}

/// Genesis blockchain configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GenesisConfig {
    /// Network name (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// Genesis accounts with initial balances
    pub accounts: Vec<GenesisAccount>,
}

/// Genesis account with initial funding
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GenesisAccount {
    /// Account name (e.g., "alice", "bob")
    pub name: String,

    /// Initial balance (supports string or number)
    #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
    pub balance: u64,
}

/// Test execution step
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Step {
    /// Transfer funds between accounts
    Transfer {
        /// Sender account name
        from: String,
        /// Recipient account name
        to: String,
        /// Transfer amount in nanoTOS
        #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
        amount: u64,
        /// Transaction fee in nanoTOS
        #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
        fee: u64,
        /// Expected transfer result
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<TransferExpect>,
    },

    /// Mine a new block
    MineBlock {
        /// Expected mine block result
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<MineBlockExpect>,
    },

    /// Assert account balance (supports eq/within/compare)
    AssertBalance {
        /// Account name to check
        account: String,
        /// Balance expectation
        #[serde(flatten)]
        expect: BalanceExpect,
    },

    /// Assert account nonce
    AssertNonce {
        /// Account name to check
        account: String,
        /// Expected nonce value
        eq: u64,
    },

    /// Advance test time
    AdvanceTime {
        /// Number of seconds to advance
        seconds: u64,
    },
}

/// Expected transfer result
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TransferExpect {
    /// Expected status ("success" or "failure")
    pub status: String,
}

/// Expected mine block result
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MineBlockExpect {
    /// Expected block count
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_count: Option<u64>,
}

/// Balance assertion modes (V2.2 P1-8)
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum BalanceExpect {
    /// Exact equality
    Eq {
        /// Expected exact balance value
        #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
        eq: u64,
    },

    /// Within tolerance range
    Within {
        /// Tolerance specification
        within: Tolerance,
    },

    /// Comparison operator
    Compare {
        /// Comparison operation
        compare: CompareOp,
    },
}

/// Tolerance specification for approximate assertions
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Tolerance {
    /// Target value
    #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
    pub target: u64,

    /// Tolerance delta (± tolerance)
    #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
    pub tolerance: u64,
}

/// Comparison operators for balance assertions
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum CompareOp {
    /// Greater than or equal
    Gte {
        /// Minimum balance value (inclusive)
        #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
        gte: u64,
    },

    /// Less than or equal
    Lte {
        /// Maximum balance value (inclusive)
        #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
        lte: u64,
    },

    /// Greater than
    Gt {
        /// Minimum balance value (exclusive)
        #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
        gt: u64,
    },

    /// Less than
    Lt {
        /// Maximum balance value (exclusive)
        #[serde(deserialize_with = "deserialize_u64_from_string_or_number")]
        lt: u64,
    },
}

/// Custom deserializer: accepts u64 as string or number
///
/// This allows YAML to use either format:
/// - `balance: 1000000000000` (number)
/// - `balance: "1000000000000"` (string, no underscores)
fn deserialize_u64_from_string_or_number<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct U64Visitor;

    impl<'de> Visitor<'de> for U64Visitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a u64 as number or string")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value < 0 {
                return Err(de::Error::custom(format!(
                    "negative value not allowed: {}",
                    value
                )));
            }
            Ok(value as u64)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.parse::<u64>().map_err(de::Error::custom)
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            value.parse::<u64>().map_err(de::Error::custom)
        }
    }

    deserializer.deserialize_any(U64Visitor)
}

/// Parse YAML scenario file
pub fn parse_scenario(yaml: &str) -> Result<TestScenario> {
    let scenario: TestScenario = serde_yaml::from_str(yaml)
        .map_err(|e| anyhow::anyhow!("Failed to parse YAML scenario: {}", e))?;

    // Validate scenario
    validate_scenario(&scenario)?;

    Ok(scenario)
}

/// Validate scenario structure
fn validate_scenario(scenario: &TestScenario) -> Result<()> {
    // Check name is non-empty
    anyhow::ensure!(!scenario.name.is_empty(), "Scenario name cannot be empty");

    // Check at least one genesis account
    anyhow::ensure!(
        !scenario.genesis.accounts.is_empty(),
        "Scenario must have at least one genesis account"
    );

    // Validate account names are unique
    let mut account_names = std::collections::HashSet::new();
    for account in &scenario.genesis.accounts {
        anyhow::ensure!(!account.name.is_empty(), "Account name cannot be empty");
        anyhow::ensure!(
            account_names.insert(&account.name),
            "Duplicate account name: {}",
            account.name
        );
    }

    // Check at least one step
    anyhow::ensure!(
        !scenario.steps.is_empty(),
        "Scenario must have at least one step"
    );

    // Validate steps reference valid accounts
    for step in &scenario.steps {
        match step {
            Step::Transfer { from, .. } => {
                // Only validate 'from' account exists - 'to' accounts are auto-created
                anyhow::ensure!(
                    account_names.contains(from),
                    "Transfer 'from' account must exist in genesis: {}",
                    from
                );
            }
            Step::AssertBalance { account, .. } | Step::AssertNonce { account, .. } => {
                // Assertions can reference auto-created accounts, so don't validate
                // This allows asserting on recipients created during execution
                _ = account; // Suppress unused warning
            }
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::disallowed_methods)]

    use super::*;

    #[test]
    fn test_parse_simple_scenario() {
        let yaml = r#"
name: "Test Scenario"
description: "A simple test"
genesis:
  network: "devnet"
  accounts:
    - name: "alice"
      balance: "1000000000000"
    - name: "bob"
      balance: "0"
steps:
  - action: "transfer"
    from: "alice"
    to: "bob"
    amount: "100000000000"
    fee: "50"
  - action: "mine_block"
  - action: "assert_balance"
    account: "bob"
    eq: "100000000000"
invariants:
  - "balance_conservation"
"#;

        let scenario = parse_scenario(yaml).expect("Failed to parse");
        assert_eq!(scenario.name, "Test Scenario");
        assert_eq!(scenario.genesis.accounts.len(), 2);
        assert_eq!(scenario.steps.len(), 3);
    }

    #[test]
    fn test_parse_within_assertion() {
        let yaml = r#"
name: "Within Test"
genesis:
  accounts:
    - name: "alice"
      balance: 1000
steps:
  - action: "assert_balance"
    account: "alice"
    within:
      target: "1000"
      tolerance: "10"
"#;

        let scenario = parse_scenario(yaml).expect("Failed to parse");
        match &scenario.steps[0] {
            Step::AssertBalance {
                expect: BalanceExpect::Within { within },
                ..
            } => {
                assert_eq!(within.target, 1000);
                assert_eq!(within.tolerance, 10);
            }
            _ => panic!("Expected Within assertion"),
        }
    }

    #[test]
    fn test_parse_compare_assertion() {
        let yaml = r#"
name: "Compare Test"
genesis:
  accounts:
    - name: "alice"
      balance: 1000
steps:
  - action: "assert_balance"
    account: "alice"
    compare:
      gte: "900"
"#;

        let scenario = parse_scenario(yaml).expect("Failed to parse");
        match &scenario.steps[0] {
            Step::AssertBalance {
                expect:
                    BalanceExpect::Compare {
                        compare: CompareOp::Gte { gte },
                    },
                ..
            } => {
                assert_eq!(*gte, 900);
            }
            _ => panic!("Expected Compare assertion"),
        }
    }

    #[test]
    fn test_validation_duplicate_accounts() {
        let yaml = r#"
name: "Duplicate Test"
genesis:
  accounts:
    - name: "alice"
      balance: 1000
    - name: "alice"
      balance: 2000
steps:
  - action: "mine_block"
"#;

        let result = parse_scenario(yaml);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Duplicate account name"));
    }

    #[test]
    fn test_validation_unknown_sender() {
        // Test that unknown SENDER account fails (recipients can be auto-created)
        let yaml = r#"
name: "Unknown Sender Test"
genesis:
  accounts:
    - name: "alice"
      balance: 1000
steps:
  - action: "transfer"
    from: "charlie"
    to: "alice"
    amount: 100
    fee: 1
"#;

        let result = parse_scenario(yaml);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must exist in genesis"));
    }
}
