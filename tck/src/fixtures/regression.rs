//! Regression capture utility for transaction fixtures.
//!
//! Provides tools to capture the current state of a transaction sequence
//! and generate YAML fixture files that can be used for regression testing.
//! This is useful for capturing production bugs as reproducible test cases.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use super::types::{
    AccountDef, AccountExpected, ExpectStatus, ExpectedState, Fixture, FixtureMeta, FixtureSetup,
    Step, TransactionStep, TransactionType,
};

/// A captured transaction for regression testing.
#[derive(Debug, Clone)]
pub struct CapturedTransaction {
    /// Sender account name
    pub from: String,
    /// Recipient account name
    pub to: String,
    /// Transfer amount
    pub amount: u64,
    /// Transaction fee
    pub fee: u64,
    /// Whether the transaction succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Transaction type (transfer, freeze, delegate, etc.)
    pub tx_type: TransactionType,
}

/// A captured account state.
#[derive(Debug, Clone)]
pub struct CapturedAccount {
    /// Account name/label
    pub name: String,
    /// Current balance
    pub balance: u64,
    /// Current nonce
    pub nonce: u64,
    /// Frozen balance
    pub frozen_balance: Option<u64>,
}

/// Builder for capturing regression test fixtures.
///
/// Captures observed transaction behavior and generates reproducible
/// YAML fixture files for regression testing.
///
/// # Example
/// ```ignore
/// let fixture = RegressionCapture::new("double_spend_regression")
///     .description("Reproduces double-spend scenario found in testing")
///     .add_initial_account("alice", 100_000, 0)
///     .add_initial_account("bob", 0, 0)
///     .add_transaction("alice", "bob", 50_000, 10, true, None)
///     .add_transaction("alice", "bob", 60_000, 10, false, Some("insufficient_balance"))
///     .add_final_account("alice", 49_990, 1)
///     .add_final_account("bob", 50_000, 0)
///     .build();
///
/// fixture.write_yaml(Path::new("tck/fixtures/scenarios/regression_test.yaml"))?;
/// ```
#[derive(Debug)]
pub struct RegressionCapture {
    /// Fixture name
    name: String,
    /// Description of the regression
    description: String,
    /// Initial account states
    initial_accounts: Vec<CapturedAccount>,
    /// Captured transactions
    transactions: Vec<CapturedTransaction>,
    /// Final account states
    final_accounts: Vec<CapturedAccount>,
    /// Tiers this fixture can run on
    tiers: Vec<u8>,
}

impl RegressionCapture {
    /// Create a new regression capture with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: String::new(),
            initial_accounts: Vec::new(),
            transactions: Vec::new(),
            final_accounts: Vec::new(),
            tiers: vec![1],
        }
    }

    /// Set the description for this regression fixture.
    pub fn description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Set the tiers this fixture can run on.
    pub fn tiers(mut self, tiers: Vec<u8>) -> Self {
        self.tiers = tiers;
        self
    }

    /// Add an initial account state.
    pub fn add_initial_account(mut self, name: &str, balance: u64, nonce: u64) -> Self {
        self.initial_accounts.push(CapturedAccount {
            name: name.to_string(),
            balance,
            nonce,
            frozen_balance: None,
        });
        self
    }

    /// Add an initial account with frozen balance.
    pub fn add_initial_account_with_frozen(
        mut self,
        name: &str,
        balance: u64,
        nonce: u64,
        frozen: u64,
    ) -> Self {
        self.initial_accounts.push(CapturedAccount {
            name: name.to_string(),
            balance,
            nonce,
            frozen_balance: Some(frozen),
        });
        self
    }

    /// Add a captured transfer transaction.
    pub fn add_transaction(
        mut self,
        from: &str,
        to: &str,
        amount: u64,
        fee: u64,
        success: bool,
        error: Option<&str>,
    ) -> Self {
        self.transactions.push(CapturedTransaction {
            from: from.to_string(),
            to: to.to_string(),
            amount,
            fee,
            success,
            error: error.map(|s| s.to_string()),
            tx_type: TransactionType::Transfer,
        });
        self
    }

    /// Add a captured transaction with a custom type.
    #[allow(clippy::too_many_arguments)]
    pub fn add_typed_transaction(
        mut self,
        tx_type: TransactionType,
        from: &str,
        to: &str,
        amount: u64,
        fee: u64,
        success: bool,
        error: Option<&str>,
    ) -> Self {
        self.transactions.push(CapturedTransaction {
            from: from.to_string(),
            to: to.to_string(),
            amount,
            fee,
            success,
            error: error.map(|s| s.to_string()),
            tx_type,
        });
        self
    }

    /// Add a final account state for verification.
    pub fn add_final_account(mut self, name: &str, balance: u64, nonce: u64) -> Self {
        self.final_accounts.push(CapturedAccount {
            name: name.to_string(),
            balance,
            nonce,
            frozen_balance: None,
        });
        self
    }

    /// Add a final account state with frozen balance for verification.
    pub fn add_final_account_with_frozen(
        mut self,
        name: &str,
        balance: u64,
        nonce: u64,
        frozen: u64,
    ) -> Self {
        self.final_accounts.push(CapturedAccount {
            name: name.to_string(),
            balance,
            nonce,
            frozen_balance: Some(frozen),
        });
        self
    }

    /// Build the internal fixture representation from captured data.
    pub fn build(&self) -> Fixture {
        let mut accounts = HashMap::new();
        for acc in &self.initial_accounts {
            accounts.insert(
                acc.name.clone(),
                AccountDef {
                    balance: format!("{} TOS", acc.balance),
                    nonce: Some(acc.nonce),
                    frozen_balance: acc.frozen_balance.map(|f| format!("{} TOS", f)),
                    energy: None,
                    uno_balances: None,
                    delegations_out: None,
                    delegations_in: None,
                    template: None,
                },
            );
        }

        let transactions: Vec<Step> = self
            .transactions
            .iter()
            .enumerate()
            .map(|(idx, tx)| {
                let step_num = idx.checked_add(1).unwrap_or(idx) as u32;
                Step::Transaction(Box::new(TransactionStep {
                    step: Some(step_num),
                    name: None,
                    tx_type: tx.tx_type.clone(),
                    from: Some(tx.from.clone()),
                    to: Some(tx.to.clone()),
                    amount: Some(format!("{} TOS", tx.amount)),
                    fee: Some(format!("{} TOS", tx.fee)),
                    asset: None,
                    nonce: None,
                    duration: None,
                    code: None,
                    contract: None,
                    function: None,
                    args: None,
                    expect_status: if tx.success {
                        ExpectStatus::Success
                    } else {
                        ExpectStatus::Error
                    },
                    expect_error: tx.error.clone(),
                }))
            })
            .collect();

        let mut expected_accounts = HashMap::new();
        for acc in &self.final_accounts {
            expected_accounts.insert(
                acc.name.clone(),
                AccountExpected {
                    balance: Some(format!("{} TOS", acc.balance)),
                    nonce: Some(acc.nonce),
                    frozen_balance: acc.frozen_balance.map(|f| format!("{} TOS", f)),
                    uno_balances: None,
                    energy: None,
                    delegations_out: None,
                    delegations_in: None,
                },
            );
        }

        // Calculate total supply change from fees of successful transactions
        let total_fees: u64 = self
            .transactions
            .iter()
            .filter(|tx| tx.success)
            .map(|tx| tx.fee)
            .fold(0u64, |acc, fee| acc.saturating_add(fee));

        let mut invariants = Vec::new();
        invariants.push(super::types::Invariant::BalanceConservation {
            balance_conservation: super::types::BalanceConservationDef {
                fee_recipient: None,
                total_supply_change: if total_fees > 0 {
                    Some(format!("-{} TOS", total_fees))
                } else {
                    Some("0 TOS".to_string())
                },
            },
        });

        Fixture {
            fixture: FixtureMeta {
                name: self.name.clone(),
                version: Some("1.0".to_string()),
                description: if self.description.is_empty() {
                    None
                } else {
                    Some(self.description.clone())
                },
                tier: self.tiers.clone(),
            },
            setup: FixtureSetup {
                accounts,
                assets: None,
                network: None,
            },
            transactions,
            expected: Some(ExpectedState {
                accounts: expected_accounts,
            }),
            invariants,
            fee_model: None,
        }
    }

    /// Generate YAML content for the fixture.
    ///
    /// Formats the captured data as a YAML fixture file string.
    /// Since the internal types only derive Deserialize, this method
    /// manually formats the YAML output.
    pub fn to_yaml(&self) -> Result<String> {
        let mut yaml = String::new();

        // Fixture metadata
        yaml.push_str("fixture:\n");
        yaml.push_str(&format!("  name: \"{}\"\n", self.name));
        yaml.push_str("  version: \"1.0\"\n");
        if !self.description.is_empty() {
            yaml.push_str(&format!("  description: \"{}\"\n", self.description));
        }
        yaml.push_str(&format!("  tier: {:?}\n", self.tiers));

        // Setup section
        yaml.push('\n');
        yaml.push_str("setup:\n");
        yaml.push_str("  accounts:\n");
        for acc in &self.initial_accounts {
            yaml.push_str(&format!("    {}:\n", acc.name));
            yaml.push_str(&format!("      balance: \"{} TOS\"\n", acc.balance));
            yaml.push_str(&format!("      nonce: {}\n", acc.nonce));
            if let Some(frozen) = acc.frozen_balance {
                yaml.push_str(&format!("      frozen_balance: \"{} TOS\"\n", frozen));
            }
        }

        // Transactions section
        yaml.push('\n');
        yaml.push_str("transactions:\n");
        for (idx, tx) in self.transactions.iter().enumerate() {
            let step_num = idx.checked_add(1).unwrap_or(idx);
            yaml.push_str(&format!("  - step: {}\n", step_num));
            yaml.push_str(&format!("    type: {}\n", tx_type_to_str(&tx.tx_type)));
            yaml.push_str(&format!("    from: {}\n", tx.from));
            yaml.push_str(&format!("    to: {}\n", tx.to));
            yaml.push_str(&format!("    amount: \"{} TOS\"\n", tx.amount));
            yaml.push_str(&format!("    fee: \"{} TOS\"\n", tx.fee));
            if tx.success {
                yaml.push_str("    expect_status: success\n");
            } else {
                yaml.push_str("    expect_status: error\n");
                if let Some(ref err) = tx.error {
                    yaml.push_str(&format!("    expect_error: \"{}\"\n", err));
                }
            }
        }

        // Expected state section
        if !self.final_accounts.is_empty() {
            yaml.push('\n');
            yaml.push_str("expected:\n");
            yaml.push_str("  accounts:\n");
            for acc in &self.final_accounts {
                yaml.push_str(&format!("    {}:\n", acc.name));
                yaml.push_str(&format!("      balance: \"{} TOS\"\n", acc.balance));
                yaml.push_str(&format!("      nonce: {}\n", acc.nonce));
                if let Some(frozen) = acc.frozen_balance {
                    yaml.push_str(&format!("      frozen_balance: \"{} TOS\"\n", frozen));
                }
            }
        }

        // Invariants section
        let total_fees: u64 = self
            .transactions
            .iter()
            .filter(|tx| tx.success)
            .map(|tx| tx.fee)
            .fold(0u64, |acc, fee| acc.saturating_add(fee));

        yaml.push('\n');
        yaml.push_str("invariants:\n");
        yaml.push_str("  - balance_conservation:\n");
        if total_fees > 0 {
            yaml.push_str(&format!(
                "      total_supply_change: \"-{} TOS\"\n",
                total_fees
            ));
        } else {
            yaml.push_str("      total_supply_change: \"0 TOS\"\n");
        }

        Ok(yaml)
    }

    /// Write the fixture to a YAML file at the given path.
    pub fn write_yaml(&self, path: &Path) -> Result<()> {
        let yaml = self.to_yaml()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("Failed to create directory: {}", e))?;
        }
        std::fs::write(path, yaml)
            .map_err(|e| anyhow::anyhow!("Failed to write fixture file: {}", e))?;
        Ok(())
    }
}

/// Convert a TransactionType to its snake_case YAML string representation.
fn tx_type_to_str(tx_type: &TransactionType) -> &'static str {
    match tx_type {
        TransactionType::Transfer => "transfer",
        TransactionType::UnoTransfer => "uno_transfer",
        TransactionType::Freeze => "freeze",
        TransactionType::Unfreeze => "unfreeze",
        TransactionType::Delegate => "delegate",
        TransactionType::Undelegate => "undelegate",
        TransactionType::Register => "register",
        TransactionType::MineBlock => "mine_block",
        TransactionType::AdvanceTime => "advance_time",
        TransactionType::DeployContract => "deploy_contract",
        TransactionType::CallContract => "call_contract",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regression_capture_build() {
        let capture = RegressionCapture::new("test_bug")
            .description("Test regression")
            .add_initial_account("alice", 100_000, 0)
            .add_initial_account("bob", 0, 0)
            .add_transaction("alice", "bob", 50_000, 10, true, None)
            .add_final_account("alice", 49_990, 1)
            .add_final_account("bob", 50_000, 0);

        let fixture = capture.build();
        assert_eq!(fixture.fixture.name, "test_bug");
        assert_eq!(fixture.setup.accounts.len(), 2);
        assert_eq!(fixture.transactions.len(), 1);
    }

    #[test]
    fn test_regression_capture_with_failure() {
        let capture = RegressionCapture::new("failed_tx")
            .add_initial_account("alice", 100, 0)
            .add_initial_account("bob", 0, 0)
            .add_transaction("alice", "bob", 200, 10, false, Some("insufficient_balance"))
            .add_final_account("alice", 100, 0)
            .add_final_account("bob", 0, 0);

        let fixture = capture.build();
        assert_eq!(fixture.transactions.len(), 1);

        // No fees collected from failed transaction
        if let super::super::types::Invariant::BalanceConservation {
            balance_conservation,
        } = &fixture.invariants[0]
        {
            assert_eq!(
                balance_conservation.total_supply_change,
                Some("0 TOS".to_string())
            );
        } else {
            panic!("Expected BalanceConservation invariant");
        }
    }

    #[test]
    fn test_regression_capture_typed_transaction() {
        let capture = RegressionCapture::new("freeze_test")
            .add_initial_account("alice", 100_000, 0)
            .add_typed_transaction(
                TransactionType::Freeze,
                "alice",
                "alice",
                50_000,
                10,
                true,
                None,
            );

        let fixture = capture.build();
        assert_eq!(fixture.transactions.len(), 1);
        if let Step::Transaction(ref step) = fixture.transactions[0] {
            assert_eq!(step.tx_type, TransactionType::Freeze);
        } else {
            panic!("Expected Transaction step");
        }
    }

    #[test]
    fn test_regression_capture_to_yaml() {
        let capture = RegressionCapture::new("yaml_test")
            .description("Test YAML generation")
            .tiers(vec![1, 2])
            .add_initial_account("alice", 100_000, 0)
            .add_initial_account("bob", 0, 0)
            .add_transaction("alice", "bob", 50_000, 10, true, None)
            .add_final_account("alice", 49_990, 1)
            .add_final_account("bob", 50_000, 0);

        let yaml = capture.to_yaml().expect("YAML generation should succeed");
        assert!(yaml.contains("name: \"yaml_test\""));
        assert!(yaml.contains("description: \"Test YAML generation\""));
        assert!(yaml.contains("tier: [1, 2]"));
        assert!(yaml.contains("alice:"));
        assert!(yaml.contains("bob:"));
        assert!(yaml.contains("type: transfer"));
        assert!(yaml.contains("expect_status: success"));
        assert!(yaml.contains("total_supply_change: \"-10 TOS\""));
    }

    #[test]
    fn test_regression_capture_failed_tx_yaml() {
        let capture = RegressionCapture::new("fail_test")
            .add_initial_account("alice", 100, 0)
            .add_initial_account("bob", 0, 0)
            .add_transaction("alice", "bob", 200, 10, false, Some("insufficient_balance"));

        let yaml = capture.to_yaml().expect("YAML generation should succeed");
        assert!(yaml.contains("expect_status: error"));
        assert!(yaml.contains("expect_error: \"insufficient_balance\""));
        assert!(yaml.contains("total_supply_change: \"0 TOS\""));
    }

    #[test]
    fn test_regression_capture_with_frozen_balance() {
        let capture = RegressionCapture::new("frozen_test")
            .add_initial_account_with_frozen("staker", 100_000, 0, 50_000)
            .add_typed_transaction(
                TransactionType::Delegate,
                "staker",
                "validator",
                25_000,
                10,
                true,
                None,
            )
            .add_final_account_with_frozen("staker", 99_990, 1, 50_000);

        let fixture = capture.build();
        let staker_def = fixture
            .setup
            .accounts
            .get("staker")
            .expect("staker account should exist");
        assert_eq!(staker_def.frozen_balance, Some("50000 TOS".to_string()));

        let expected = fixture.expected.expect("expected state should exist");
        let staker_expected = expected
            .accounts
            .get("staker")
            .expect("staker expected should exist");
        assert_eq!(
            staker_expected.frozen_balance,
            Some("50000 TOS".to_string())
        );
    }

    #[test]
    fn test_regression_capture_multiple_successful_fees() {
        let capture = RegressionCapture::new("multi_fee_test")
            .add_initial_account("alice", 100_000, 0)
            .add_initial_account("bob", 0, 0)
            .add_transaction("alice", "bob", 10_000, 10, true, None)
            .add_transaction("alice", "bob", 20_000, 15, true, None)
            .add_transaction(
                "alice",
                "bob",
                80_000,
                10,
                false,
                Some("insufficient_balance"),
            );

        let fixture = capture.build();

        // Only successful transaction fees should be counted: 10 + 15 = 25
        if let super::super::types::Invariant::BalanceConservation {
            balance_conservation,
        } = &fixture.invariants[0]
        {
            assert_eq!(
                balance_conservation.total_supply_change,
                Some("-25 TOS".to_string())
            );
        } else {
            panic!("Expected BalanceConservation invariant");
        }
    }

    #[test]
    fn test_regression_capture_write_yaml_file() {
        let capture = RegressionCapture::new("file_write_test")
            .description("Test file writing")
            .add_initial_account("alice", 10_000, 0)
            .add_initial_account("bob", 0, 0)
            .add_transaction("alice", "bob", 5_000, 10, true, None)
            .add_final_account("alice", 4_990, 1)
            .add_final_account("bob", 5_000, 0);

        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let file_path = temp_dir.path().join("regression_test.yaml");

        capture
            .write_yaml(&file_path)
            .expect("write_yaml should succeed");

        let content =
            std::fs::read_to_string(&file_path).expect("Should be able to read written file");
        assert!(content.contains("name: \"file_write_test\""));
        assert!(content.contains("description: \"Test file writing\""));
    }

    #[test]
    fn test_tx_type_to_str() {
        assert_eq!(tx_type_to_str(&TransactionType::Transfer), "transfer");
        assert_eq!(
            tx_type_to_str(&TransactionType::UnoTransfer),
            "uno_transfer"
        );
        assert_eq!(tx_type_to_str(&TransactionType::Freeze), "freeze");
        assert_eq!(tx_type_to_str(&TransactionType::Unfreeze), "unfreeze");
        assert_eq!(tx_type_to_str(&TransactionType::Delegate), "delegate");
        assert_eq!(tx_type_to_str(&TransactionType::Undelegate), "undelegate");
        assert_eq!(tx_type_to_str(&TransactionType::Register), "register");
        assert_eq!(tx_type_to_str(&TransactionType::MineBlock), "mine_block");
        assert_eq!(
            tx_type_to_str(&TransactionType::AdvanceTime),
            "advance_time"
        );
        assert_eq!(
            tx_type_to_str(&TransactionType::DeployContract),
            "deploy_contract"
        );
        assert_eq!(
            tx_type_to_str(&TransactionType::CallContract),
            "call_contract"
        );
    }

    #[test]
    fn test_regression_capture_empty_description() {
        let capture = RegressionCapture::new("no_desc").add_initial_account("alice", 1_000, 0);

        let fixture = capture.build();
        assert_eq!(fixture.fixture.description, None);

        let yaml = capture.to_yaml().expect("YAML generation should succeed");
        assert!(!yaml.contains("description:"));
    }
}
