//! YAML scenario execution engine
//!
//! This module executes parsed YAML test scenarios against TestDaemon,
//! providing a high-level DSL for integration testing.
//!
//! # Example
//!
//! ```rust,ignore
//! use tos_testing_framework::scenarios::{ScenarioExecutor, parse_scenario};
//!
//! let yaml = r#"
//! name: "Simple Transfer"
//! genesis:
//!   accounts:
//!     - name: "alice"
//!       balance: "1000000000000"
//! steps:
//!   - action: "transfer"
//!     from: "alice"
//!     to: "bob"
//!     amount: "100000000000"
//!     fee: "50"
//!   - action: "mine_block"
//!   - action: "assert_balance"
//!     account: "alice"
//!     eq: "899999999950"
//! "#;
//!
//! let scenario = parse_scenario(yaml)?;
//! let mut executor = ScenarioExecutor::new();
//! let report = executor.execute(scenario).await?;
//!
//! assert!(report.success);
//! ```

use super::parser::{BalanceExpect, CompareOp, Step, TestScenario, TransferExpect};
use crate::orchestrator::{Clock, PausedClock};
use crate::tier1_component::{TestBlockchainBuilder, TestTransaction};
use crate::tier2_integration::TestDaemon;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tos_common::crypto::Hash;

/// Scenario executor that runs parsed YAML scenarios
pub struct ScenarioExecutor {
    /// TestDaemon for blockchain operations
    daemon: Option<TestDaemon>,

    /// Clock for deterministic time control
    clock: Option<Arc<PausedClock>>,

    /// Named accounts (name → address)
    accounts: HashMap<String, Hash>,

    /// Execution log
    log: Vec<String>,

    /// Current step number (1-indexed)
    current_step: usize,
}

impl ScenarioExecutor {
    /// Create new executor
    pub fn new() -> Self {
        Self {
            daemon: None,
            clock: None,
            accounts: HashMap::new(),
            log: Vec::new(),
            current_step: 0,
        }
    }

    /// Execute a complete scenario
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Genesis setup fails
    /// - Any step execution fails
    /// - Any assertion fails
    pub async fn execute(&mut self, scenario: TestScenario) -> Result<ExecutionReport> {
        self.log.clear();
        self.current_step = 0;

        self.log(format!("Starting scenario: {}", scenario.name));
        if let Some(desc) = &scenario.description {
            self.log(format!("Description: {}", desc));
        }

        // Setup genesis
        self.setup_genesis(&scenario).await?;

        // Execute steps
        for (idx, step) in scenario.steps.iter().enumerate() {
            self.current_step = idx + 1;
            self.log(format!("\n--- Step {}: {:?} ---", self.current_step, step));

            self.execute_step(step)
                .await
                .with_context(|| format!("Failed at step {} ({:?})", self.current_step, step))?;
        }

        // Check invariants if specified
        if let Some(invariants) = &scenario.invariants {
            self.log("\n--- Checking Invariants ---".to_string());
            for inv in invariants {
                self.check_invariant(inv).await?;
            }
        }

        self.log("\n=== Scenario completed successfully ===".to_string());

        Ok(ExecutionReport {
            scenario_name: scenario.name,
            steps_executed: self.current_step,
            success: true,
            log: self.log.clone(),
        })
    }

    /// Setup genesis blockchain state
    async fn setup_genesis(&mut self, scenario: &TestScenario) -> Result<()> {
        self.log("Setting up genesis...".to_string());

        // Create paused clock for deterministic testing
        let clock = Arc::new(PausedClock::new());
        self.clock = Some(clock.clone());

        // Build blockchain with genesis accounts
        let mut builder = TestBlockchainBuilder::new()
            .with_clock(clock.clone() as Arc<dyn Clock>)
            .with_default_balance(0);

        // Create accounts
        for (idx, genesis_account) in scenario.genesis.accounts.iter().enumerate() {
            let addr = Self::create_account_address(&genesis_account.name, idx);
            self.accounts
                .insert(genesis_account.name.clone(), addr.clone());

            builder = builder.with_funded_account(addr.clone(), genesis_account.balance);

            self.log(format!(
                "  Account '{}': balance={} ({})",
                genesis_account.name, genesis_account.balance, addr
            ));
        }

        let blockchain = builder.build().await?;
        self.daemon = Some(TestDaemon::new(blockchain, clock as Arc<dyn Clock>));

        self.log(format!(
            "Genesis complete: {} accounts",
            self.accounts.len()
        ));
        Ok(())
    }

    /// Execute a single step
    async fn execute_step(&mut self, step: &Step) -> Result<()> {
        match step {
            Step::Transfer {
                from,
                to,
                amount,
                fee,
                expect,
            } => self.execute_transfer(from, to, *amount, *fee, expect).await,
            Step::MineBlock { expect } => self.execute_mine_block(expect).await,
            Step::AssertBalance { account, expect } => {
                self.execute_assert_balance(account, expect).await
            }
            Step::AssertNonce { account, eq } => self.execute_assert_nonce(account, *eq).await,
            Step::AdvanceTime { seconds } => self.execute_advance_time(*seconds).await,
        }
    }

    /// Execute transfer action
    async fn execute_transfer(
        &mut self,
        from: &str,
        to: &str,
        amount: u64,
        fee: u64,
        expect: &Option<TransferExpect>,
    ) -> Result<()> {
        // Get addresses first (may mutate self.accounts)
        let from_addr = self.get_account(from)?.clone();
        let to_addr = self.get_or_create_account(to).clone();

        self.log(format!(
            "Transfer: {} → {} (amount={}, fee={})",
            from, to, amount, fee
        ));

        // Now get daemon reference (immutable borrow)
        let daemon = self.daemon.as_ref().context("Daemon not initialized")?;

        // Get nonce
        let nonce = daemon.get_nonce(&from_addr).await? + 1;

        // Create transaction
        let tx = TestTransaction {
            hash: Hash::zero(),
            sender: from_addr.clone(),
            recipient: to_addr.clone(),
            amount,
            fee,
            nonce,
        };

        // Submit transaction
        let result = daemon.submit_transaction(tx).await;

        // Check expectations
        if let Some(exp) = expect {
            match exp.status.as_str() {
                "success" => {
                    result.context("Expected transfer to succeed")?;
                    self.log(format!("  ✓ Transfer succeeded (txid={})", Hash::zero()));
                }
                "failure" => {
                    if result.is_ok() {
                        anyhow::bail!("Expected transfer to fail, but it succeeded");
                    }
                    self.log("  ✓ Transfer failed as expected".to_string());
                }
                status => anyhow::bail!("Unknown expect status: {}", status),
            }
        } else {
            // No expectation, default to success
            result?;
            self.log("  Transfer submitted successfully".to_string());
        }

        Ok(())
    }

    /// Execute mine_block action
    async fn execute_mine_block(
        &mut self,
        _expect: &Option<super::parser::MineBlockExpect>,
    ) -> Result<()> {
        self.log("Mining block...".to_string());

        let daemon = self.daemon.as_ref().context("Daemon not initialized")?;
        let block_hash = daemon.mine_block().await?;

        self.log(format!("  ✓ Block mined: hash={}", block_hash));

        Ok(())
    }

    /// Execute assert_balance action
    async fn execute_assert_balance(
        &mut self,
        account: &str,
        expect: &BalanceExpect,
    ) -> Result<()> {
        let daemon = self.daemon.as_ref().context("Daemon not initialized")?;
        let addr = self.get_account(account)?;

        let actual_balance = daemon.get_balance(addr).await?;

        self.log(format!(
            "Assert balance for '{}': actual={}",
            account, actual_balance
        ));

        match expect {
            BalanceExpect::Eq { eq } => {
                if actual_balance != *eq {
                    anyhow::bail!(
                        "Balance assertion failed: expected {}, got {}",
                        eq,
                        actual_balance
                    );
                }
                self.log(format!("  ✓ Balance equals {}", eq));
            }
            BalanceExpect::Within { within } => {
                let min = within.target.saturating_sub(within.tolerance);
                let max = within.target.saturating_add(within.tolerance);

                if actual_balance < min || actual_balance > max {
                    anyhow::bail!(
                        "Balance out of tolerance: expected {} ± {}, got {} (range: {}-{})",
                        within.target,
                        within.tolerance,
                        actual_balance,
                        min,
                        max
                    );
                }
                self.log(format!(
                    "  ✓ Balance within {} ± {} (range: {}-{})",
                    within.target, within.tolerance, min, max
                ));
            }
            BalanceExpect::Compare { compare } => match compare {
                CompareOp::Gte { gte } => {
                    if actual_balance < *gte {
                        anyhow::bail!("Balance {} < {}", actual_balance, gte);
                    }
                    self.log(format!("  ✓ Balance >= {}", gte));
                }
                CompareOp::Lte { lte } => {
                    if actual_balance > *lte {
                        anyhow::bail!("Balance {} > {}", actual_balance, lte);
                    }
                    self.log(format!("  ✓ Balance <= {}", lte));
                }
                CompareOp::Gt { gt } => {
                    if actual_balance <= *gt {
                        anyhow::bail!("Balance {} <= {}", actual_balance, gt);
                    }
                    self.log(format!("  ✓ Balance > {}", gt));
                }
                CompareOp::Lt { lt } => {
                    if actual_balance >= *lt {
                        anyhow::bail!("Balance {} >= {}", actual_balance, lt);
                    }
                    self.log(format!("  ✓ Balance < {}", lt));
                }
            },
        }

        Ok(())
    }

    /// Execute assert_nonce action
    async fn execute_assert_nonce(&mut self, account: &str, expected: u64) -> Result<()> {
        let daemon = self.daemon.as_ref().context("Daemon not initialized")?;
        let addr = self.get_account(account)?;

        let actual_nonce = daemon.get_nonce(addr).await?;

        self.log(format!(
            "Assert nonce for '{}': actual={}",
            account, actual_nonce
        ));

        if actual_nonce != expected {
            anyhow::bail!(
                "Nonce assertion failed: expected {}, got {}",
                expected,
                actual_nonce
            );
        }

        self.log(format!("  ✓ Nonce equals {}", expected));
        Ok(())
    }

    /// Execute advance_time action
    async fn execute_advance_time(&mut self, seconds: u64) -> Result<()> {
        self.log(format!("Advancing time by {} seconds...", seconds));

        let clock = self.clock.as_ref().context("Clock not initialized")?;
        clock
            .advance(tokio::time::Duration::from_secs(seconds))
            .await;

        self.log(format!("  ✓ Time advanced by {}s", seconds));
        Ok(())
    }

    /// Check invariant
    async fn check_invariant(&mut self, _invariant: &str) -> Result<()> {
        // TODO: Implement invariant checking
        // For now, just log
        self.log(format!("  Invariant check: (not yet implemented)"));
        Ok(())
    }

    /// Get account address by name
    fn get_account(&self, name: &str) -> Result<&Hash> {
        self.accounts
            .get(name)
            .with_context(|| format!("Account '{}' not found", name))
    }

    /// Get or create account address
    fn get_or_create_account(&mut self, name: &str) -> &Hash {
        let idx = self.accounts.len();
        self.accounts
            .entry(name.to_string())
            .or_insert_with(|| Self::create_account_address(name, idx))
    }

    /// Create deterministic account address from name
    fn create_account_address(name: &str, idx: usize) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = (idx + 1) as u8; // Offset by 1 to avoid zero address

        // Mix in name hash for readability
        let name_bytes = name.as_bytes();
        for (i, b) in name_bytes.iter().take(31).enumerate() {
            bytes[i + 1] = *b;
        }

        Hash::new(bytes)
    }

    /// Add log entry
    fn log(&mut self, message: String) {
        self.log.push(message);
    }

    /// Get execution log
    pub fn get_log(&self) -> &[String] {
        &self.log
    }
}

impl Default for ScenarioExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Execution report
#[derive(Debug, Clone)]
pub struct ExecutionReport {
    /// Scenario name
    pub scenario_name: String,

    /// Number of steps executed
    pub steps_executed: usize,

    /// Whether execution succeeded
    pub success: bool,

    /// Execution log
    pub log: Vec<String>,
}

impl ExecutionReport {
    /// Print report to stdout
    pub fn print(&self) {
        println!("\n╔════════════════════════════════════════════════════════════╗");
        println!("║  Scenario Execution Report                                 ║");
        println!("╠════════════════════════════════════════════════════════════╣");
        println!("║  Name: {:<50} ║", self.scenario_name);
        println!("║  Steps: {:<48} ║", self.steps_executed);
        println!(
            "║  Status: {:<47} ║",
            if self.success {
                "SUCCESS ✓"
            } else {
                "FAILED ✗"
            }
        );
        println!("╚════════════════════════════════════════════════════════════╝\n");

        println!("Execution Log:");
        println!("═════════════");
        for entry in &self.log {
            println!("{}", entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::parser::parse_scenario;

    #[tokio::test]
    async fn test_simple_transfer_scenario() {
        let yaml = r#"
name: "Simple Transfer"
description: "Basic transfer between two accounts"
genesis:
  accounts:
    - name: "alice"
      balance: "1000000000000"
steps:
  - action: "transfer"
    from: "alice"
    to: "bob"
    amount: "100000000000"
    fee: "50"
  - action: "mine_block"
  - action: "assert_balance"
    account: "alice"
    eq: "899999999950"
  - action: "assert_balance"
    account: "bob"
    eq: "100000000000"
  - action: "assert_nonce"
    account: "alice"
    eq: 1
"#;

        let scenario = parse_scenario(yaml).unwrap();
        let mut executor = ScenarioExecutor::new();
        let report = executor.execute(scenario).await.unwrap();

        assert!(report.success);
        assert_eq!(report.steps_executed, 5);
    }

    #[tokio::test]
    async fn test_balance_within_tolerance() {
        let yaml = r#"
name: "Balance Tolerance"
genesis:
  accounts:
    - name: "alice"
      balance: "1000000"
steps:
  - action: "transfer"
    from: "alice"
    to: "bob"
    amount: "100"
    fee: "10"
  - action: "mine_block"
  - action: "assert_balance"
    account: "alice"
    within:
      target: "999890"
      tolerance: "10"
"#;

        let scenario = parse_scenario(yaml).unwrap();
        let mut executor = ScenarioExecutor::new();
        let report = executor.execute(scenario).await.unwrap();

        assert!(report.success);
    }

    #[tokio::test]
    async fn test_balance_comparison() {
        let yaml = r#"
name: "Balance Comparison"
genesis:
  accounts:
    - name: "alice"
      balance: "1000000"
steps:
  - action: "transfer"
    from: "alice"
    to: "bob"
    amount: "100"
    fee: "10"
  - action: "mine_block"
  - action: "assert_balance"
    account: "alice"
    compare:
      lte: "1000000"
  - action: "assert_balance"
    account: "alice"
    compare:
      gte: "999000"
"#;

        let scenario = parse_scenario(yaml).unwrap();
        let mut executor = ScenarioExecutor::new();
        let report = executor.execute(scenario).await.unwrap();

        assert!(report.success);
    }

    #[tokio::test]
    async fn test_advance_time() {
        let yaml = r#"
name: "Time Control"
genesis:
  accounts:
    - name: "alice"
      balance: "1000000"
steps:
  - action: "advance_time"
    seconds: 3600
  - action: "transfer"
    from: "alice"
    to: "bob"
    amount: "100"
    fee: "10"
  - action: "mine_block"
"#;

        let scenario = parse_scenario(yaml).unwrap();
        let mut executor = ScenarioExecutor::new();
        let report = executor.execute(scenario).await.unwrap();

        assert!(report.success);
    }

    #[tokio::test]
    async fn test_yaml_simple_transfer() {
        let yaml = std::fs::read_to_string("scenarios/simple_transfer.yaml")
            .expect("Failed to read simple_transfer.yaml");

        let scenario = parse_scenario(&yaml).unwrap();
        let mut executor = ScenarioExecutor::new();
        let report = executor.execute(scenario).await.unwrap();

        assert!(report.success);
        println!("\nSimple Transfer Scenario:");
        for line in &report.log {
            println!("{}", line);
        }
    }

    #[tokio::test]
    async fn test_yaml_multi_hop() {
        let yaml = std::fs::read_to_string("scenarios/multi_hop_transfer.yaml")
            .expect("Failed to read multi_hop_transfer.yaml");

        let scenario = parse_scenario(&yaml).unwrap();
        let mut executor = ScenarioExecutor::new();
        let report = executor.execute(scenario).await.unwrap();

        assert!(report.success);
    }

    #[tokio::test]
    async fn test_yaml_high_frequency() {
        let yaml = std::fs::read_to_string("scenarios/high_frequency_trading.yaml")
            .expect("Failed to read high_frequency_trading.yaml");

        let scenario = parse_scenario(&yaml).unwrap();
        let mut executor = ScenarioExecutor::new();
        let report = executor.execute(scenario).await.unwrap();

        assert!(report.success);
    }

    #[tokio::test]
    async fn test_yaml_time_based() {
        let yaml = std::fs::read_to_string("scenarios/time_based_operations.yaml")
            .expect("Failed to read time_based_operations.yaml");

        let scenario = parse_scenario(&yaml).unwrap();
        let mut executor = ScenarioExecutor::new();
        let report = executor.execute(scenario).await.unwrap();

        assert!(report.success);
    }

    #[tokio::test]
    async fn test_yaml_stress_test() {
        let yaml = std::fs::read_to_string("scenarios/stress_test_100tx.yaml")
            .expect("Failed to read stress_test_100tx.yaml");

        let scenario = parse_scenario(&yaml).unwrap();
        let mut executor = ScenarioExecutor::new();
        let report = executor.execute(scenario).await.unwrap();

        assert!(report.success);
    }
}
