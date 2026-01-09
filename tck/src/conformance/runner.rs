//! Conformance test runner
//!
//! Executes YAML-based conformance specifications and generates reports.
//! Supports multiple output formats: JSON, JUnit XML, and human-readable.

use super::*;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

/// Default test timeout in seconds
pub const DEFAULT_TEST_TIMEOUT_SECS: u64 = 30;

/// Maximum test timeout in seconds
pub const MAX_TEST_TIMEOUT_SECS: u64 = 300;

/// Conformance test runner
pub struct ConformanceRunner {
    /// Loaded specifications
    specs: Vec<ConformanceSpec>,
    /// Test timeout duration
    test_timeout: Duration,
}

/// Test execution context - manages state during test execution
#[derive(Default)]
pub struct TestContext {
    /// Named accounts with their balances
    accounts: HashMap<String, AccountState>,
    /// Named contracts that have been deployed
    contracts: HashMap<String, ContractState>,
    /// Execution results from the last action
    last_result: Option<ExecutionResult>,
}

/// Account state for testing
#[derive(Debug, Clone, Default)]
pub struct AccountState {
    /// Account balance in base units
    pub balance: u64,
    /// Account nonce (transaction count)
    pub nonce: u64,
    /// Account storage (key-value pairs)
    pub storage: HashMap<String, Vec<u8>>,
}

/// Contract state for testing
#[derive(Debug, Clone)]
pub struct ContractState {
    /// Contract address
    pub address: String,
    /// Contract bytecode
    pub code: Vec<u8>,
    /// Contract storage (key-value pairs)
    pub storage: HashMap<String, Vec<u8>>,
}

/// Result of executing an action
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Whether the execution succeeded
    pub success: bool,
    /// Error code if execution failed
    pub error_code: Option<String>,
    /// Return value from execution
    pub return_value: Option<serde_yaml::Value>,
    /// Gas consumed during execution
    pub gas_used: u64,
    /// Whether the execution was reverted
    pub reverted: bool,
}

impl ConformanceRunner {
    /// Create a new runner with given specs
    pub fn new(specs: Vec<ConformanceSpec>) -> Self {
        Self {
            specs,
            test_timeout: Duration::from_secs(DEFAULT_TEST_TIMEOUT_SECS),
        }
    }

    /// Create a new runner with custom timeout
    pub fn with_timeout(specs: Vec<ConformanceSpec>, timeout_secs: u64) -> Self {
        let timeout_secs = timeout_secs.min(MAX_TEST_TIMEOUT_SECS);
        Self {
            specs,
            test_timeout: Duration::from_secs(timeout_secs),
        }
    }

    /// Set the test timeout duration
    pub fn set_timeout(&mut self, timeout: Duration) {
        // Cap at maximum timeout
        self.test_timeout = timeout.min(Duration::from_secs(MAX_TEST_TIMEOUT_SECS));
    }

    /// Load specs from a directory
    pub fn load_from_dir(path: &Path) -> Result<Self> {
        let specs = spec::load_specs_from_dir(path)?;
        Ok(Self::new(specs))
    }

    /// Load specs from a directory with custom timeout
    pub fn load_from_dir_with_timeout(path: &Path, timeout_secs: u64) -> Result<Self> {
        let specs = spec::load_specs_from_dir(path)?;
        Ok(Self::with_timeout(specs, timeout_secs))
    }

    /// Get the number of loaded specs
    pub fn spec_count(&self) -> usize {
        self.specs.len()
    }

    /// Run all conformance tests
    pub async fn run_all(&self) -> TestReport {
        let start = Instant::now();
        let mut report = TestReport::new();

        for spec in &self.specs {
            let result = self.run_spec(spec).await;
            report.add_result(&spec.spec.name, &spec.spec.category, result);
        }

        report.duration = start.elapsed();
        report
    }

    /// Run tests for a specific category
    pub async fn run_category(&self, category: Category) -> TestReport {
        let start = Instant::now();
        let mut report = TestReport::new();

        for spec in self.specs.iter().filter(|s| s.spec.category == category) {
            let result = self.run_spec(spec).await;
            report.add_result(&spec.spec.name, &spec.spec.category, result);
        }

        report.duration = start.elapsed();
        report
    }

    /// Run a single spec with timeout enforcement
    async fn run_spec(&self, spec: &ConformanceSpec) -> TestResult {
        let start = Instant::now();
        let timeout = self.test_timeout;

        // Execute with timeout
        let result = tokio::time::timeout(timeout, self.execute_spec(spec)).await;

        match result {
            Ok(inner_result) => TestResult {
                status: match &inner_result {
                    Ok(_) => TestStatus::Pass,
                    Err(_) => TestStatus::Fail,
                },
                duration: start.elapsed(),
                error: inner_result.err().map(|e| e.to_string()),
            },
            Err(_elapsed) => TestResult {
                status: TestStatus::Fail,
                duration: start.elapsed(),
                error: Some(format!(
                    "Test timed out after {} seconds",
                    timeout.as_secs()
                )),
            },
        }
    }

    /// Execute a spec - the core test execution logic
    async fn execute_spec(&self, spec: &ConformanceSpec) -> Result<()> {
        if log::log_enabled!(log::Level::Debug) {
            log::debug!("Executing spec: {}", spec.spec.name);
        }

        // Create fresh context for this test
        let mut ctx = TestContext::default();

        // Step 1: Setup preconditions
        self.setup_preconditions(&mut ctx, &spec.preconditions)
            .context("Failed to setup preconditions")?;

        // Step 2: Execute action(s)
        if let Some(test_cases) = &spec.test_cases {
            // Multiple test cases
            for test_case in test_cases {
                self.execute_test_case(&mut ctx, test_case)
                    .with_context(|| format!("Test case '{}' failed", test_case.name))?;
            }
        } else if let Some(action) = &spec.action {
            // Single action
            let result = self
                .execute_action(&mut ctx, action)
                .context("Failed to execute action")?;
            ctx.last_result = Some(result);

            // Step 3: Verify expected outcome
            self.verify_expected(&ctx, &spec.expected)
                .context("Expected outcome verification failed")?;
        }

        // Step 4: Verify postconditions
        self.verify_postconditions(&ctx, &spec.postconditions)
            .context("Postcondition verification failed")?;

        Ok(())
    }

    /// Setup preconditions from spec
    fn setup_preconditions(&self, ctx: &mut TestContext, conditions: &[Condition]) -> Result<()> {
        for condition in conditions {
            if let Some(account_name) = &condition.account {
                let account = ctx.accounts.entry(account_name.clone()).or_default();

                if let Some(balance) = condition.balance {
                    account.balance = balance;
                }
                if let Some(nonce) = condition.nonce {
                    account.nonce = nonce;
                }
                if let Some(storage) = &condition.storage {
                    // Maximum hex string length to prevent DoS via memory exhaustion
                    const MAX_HEX_LENGTH: usize = 4096;

                    for (key, value) in storage {
                        // Validate lengths before decoding
                        if key.len() > MAX_HEX_LENGTH {
                            anyhow::bail!(
                                "Storage key hex too long: {} chars (max {})",
                                key.len(),
                                MAX_HEX_LENGTH
                            );
                        }
                        if value.len() > MAX_HEX_LENGTH {
                            anyhow::bail!(
                                "Storage value hex too long: {} chars (max {})",
                                value.len(),
                                MAX_HEX_LENGTH
                            );
                        }

                        let key_bytes = hex::decode(key.trim_start_matches("0x"))
                            .with_context(|| format!("Invalid hex key in storage: '{}'", key))?;
                        let value_bytes = hex::decode(value.trim_start_matches("0x"))
                            .with_context(|| {
                                format!("Invalid hex value in storage: '{}'", value)
                            })?;
                        account.storage.insert(hex::encode(&key_bytes), value_bytes);
                    }
                }
            }

            // Handle assertion preconditions
            if let Some(assertion) = &condition.assertion {
                self.evaluate_assertion(ctx, assertion)
                    .with_context(|| format!("Precondition assertion failed: {}", assertion))?;
            }
        }
        Ok(())
    }

    /// Execute a single action
    fn execute_action(&self, ctx: &mut TestContext, action: &Action) -> Result<ExecutionResult> {
        match action {
            Action::Transfer { from, to, amount } => self.execute_transfer(ctx, from, to, *amount),
            Action::Deploy { code, args } => self.execute_deploy(ctx, code, args),
            Action::Call {
                contract,
                function,
                args,
            } => self.execute_call(ctx, contract, function, args),
            Action::Syscall { name, args } => self.execute_syscall(ctx, name, args),
        }
    }

    /// Execute transfer action
    fn execute_transfer(
        &self,
        ctx: &mut TestContext,
        from: &str,
        to: &str,
        amount: u64,
    ) -> Result<ExecutionResult> {
        // Get sender account
        let sender = ctx
            .accounts
            .get_mut(from)
            .ok_or_else(|| anyhow::anyhow!("Sender account '{}' not found", from))?;

        // Check sufficient balance
        if sender.balance < amount {
            return Ok(ExecutionResult {
                success: false,
                error_code: Some("INSUFFICIENT_BALANCE".to_string()),
                return_value: None,
                gas_used: 21000, // Base transfer gas
                reverted: false,
            });
        }

        // Deduct from sender
        sender.balance = sender
            .balance
            .checked_sub(amount)
            .ok_or_else(|| anyhow::anyhow!("Balance underflow"))?;
        sender.nonce = sender
            .nonce
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Nonce overflow"))?;

        // Add to receiver
        let receiver = ctx.accounts.entry(to.to_string()).or_default();
        receiver.balance = receiver
            .balance
            .checked_add(amount)
            .ok_or_else(|| anyhow::anyhow!("Balance overflow"))?;

        Ok(ExecutionResult {
            success: true,
            error_code: None,
            return_value: None,
            gas_used: 21000,
            reverted: false,
        })
    }

    /// Execute contract deployment
    fn execute_deploy(
        &self,
        ctx: &mut TestContext,
        code: &str,
        args: &[String],
    ) -> Result<ExecutionResult> {
        let code_bytes =
            hex::decode(code.trim_start_matches("0x")).context("Invalid contract code hex")?;

        // Generate contract address using hash of code + args to avoid collisions
        let mut hasher_input = code_bytes.clone();
        for arg in args {
            hasher_input.extend(arg.as_bytes());
        }
        // Hash the entire input to generate a unique address
        let hash = tos_common::crypto::hash(&hasher_input);
        let hash_bytes = hash.to_bytes();
        // Validate hash length before slicing (should always be 32 bytes, but defensive check)
        if hash_bytes.len() < 32 {
            anyhow::bail!("Invalid hash length: expected 32, got {}", hash_bytes.len());
        }
        // Use last 20 bytes of hash as address (similar to Ethereum CREATE)
        let address = format!("0x{}", hex::encode(&hash_bytes[12..32]));

        // Store contract
        ctx.contracts.insert(
            address.clone(),
            ContractState {
                address: address.clone(),
                code: code_bytes,
                storage: HashMap::new(),
            },
        );

        Ok(ExecutionResult {
            success: true,
            error_code: None,
            return_value: Some(serde_yaml::Value::String(address)),
            gas_used: 100000, // Approximate deployment gas
            reverted: false,
        })
    }

    /// Execute contract call
    fn execute_call(
        &self,
        ctx: &mut TestContext,
        contract: &str,
        function: &str,
        _args: &[String],
    ) -> Result<ExecutionResult> {
        // Check contract exists
        if !ctx.contracts.contains_key(contract) {
            return Ok(ExecutionResult {
                success: false,
                error_code: Some("CONTRACT_NOT_FOUND".to_string()),
                return_value: None,
                gas_used: 0,
                reverted: true,
            });
        }

        // Simulate contract execution (in real implementation, would use VM)
        // For now, return success for known test functions
        let gas_used = match function {
            "balanceOf" => 2100,
            "transfer" => 21000,
            "approve" => 21000,
            _ => 50000,
        };

        Ok(ExecutionResult {
            success: true,
            error_code: None,
            return_value: Some(serde_yaml::Value::Number(serde_yaml::Number::from(0))),
            gas_used,
            reverted: false,
        })
    }

    /// Execute syscall
    fn execute_syscall(
        &self,
        ctx: &mut TestContext,
        name: &str,
        args: &[String],
    ) -> Result<ExecutionResult> {
        match name {
            "balance_get" => {
                let account = args
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("balance_get requires account argument"))?;
                let balance = ctx.accounts.get(account).map(|a| a.balance).unwrap_or(0);
                Ok(ExecutionResult {
                    success: true,
                    error_code: None,
                    return_value: Some(serde_yaml::Value::Number(serde_yaml::Number::from(
                        balance,
                    ))),
                    gas_used: 100,
                    reverted: false,
                })
            }
            "storage_read" => {
                let account = args
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("storage_read requires account argument"))?;
                let key = args
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("storage_read requires key argument"))?;
                let value = ctx
                    .accounts
                    .get(account)
                    .and_then(|a| a.storage.get(key))
                    .map(hex::encode)
                    .unwrap_or_else(|| "0x".to_string());
                Ok(ExecutionResult {
                    success: true,
                    error_code: None,
                    return_value: Some(serde_yaml::Value::String(value)),
                    gas_used: 200,
                    reverted: false,
                })
            }
            "storage_write" => {
                let account = args
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("storage_write requires account argument"))?;
                let key = args
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("storage_write requires key argument"))?;
                let value = args
                    .get(2)
                    .ok_or_else(|| anyhow::anyhow!("storage_write requires value argument"))?;

                let account_state = ctx.accounts.entry(account.clone()).or_default();
                let value_bytes = hex::decode(value.trim_start_matches("0x"))
                    .with_context(|| format!("Invalid hex value in storage_write: '{}'", value))?;
                account_state.storage.insert(key.clone(), value_bytes);

                Ok(ExecutionResult {
                    success: true,
                    error_code: None,
                    return_value: None,
                    gas_used: 20000,
                    reverted: false,
                })
            }
            _ => {
                // Unknown syscall - return success with default gas
                Ok(ExecutionResult {
                    success: true,
                    error_code: None,
                    return_value: None,
                    gas_used: 100,
                    reverted: false,
                })
            }
        }
    }

    /// Execute a test case with multiple steps
    fn execute_test_case(&self, ctx: &mut TestContext, test_case: &TestCase) -> Result<()> {
        for (i, step) in test_case.steps.iter().enumerate() {
            if let Some(action) = &step.call {
                let result = self
                    .execute_action(ctx, action)
                    .with_context(|| format!("Step {} failed", i + 1))?;
                ctx.last_result = Some(result);

                if let Some(expected) = &step.expected {
                    self.verify_expected(ctx, expected)
                        .with_context(|| format!("Step {} expected verification failed", i + 1))?;
                }
            }
        }
        Ok(())
    }

    /// Verify expected outcome matches actual result
    fn verify_expected(&self, ctx: &TestContext, expected: &Expected) -> Result<()> {
        let result = ctx
            .last_result
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No execution result to verify"))?;

        // Verify status
        match expected.status {
            ExpectedStatus::Success => {
                if !result.success {
                    anyhow::bail!("Expected success but got error: {:?}", result.error_code);
                }
            }
            ExpectedStatus::Error => {
                if result.success {
                    anyhow::bail!("Expected error but operation succeeded");
                }
                // Verify error code if specified
                if let Some(expected_code) = &expected.error_code {
                    let actual_code = result.error_code.as_deref().unwrap_or("");
                    if actual_code != expected_code {
                        anyhow::bail!(
                            "Expected error code '{}' but got '{}'",
                            expected_code,
                            actual_code
                        );
                    }
                }
            }
            ExpectedStatus::Revert => {
                if !result.reverted {
                    anyhow::bail!("Expected revert but operation did not revert");
                }
            }
        }

        // Verify return value if specified
        if let Some(expected_value) = &expected.return_value {
            if let Some(actual_value) = &result.return_value {
                if expected_value != actual_value {
                    anyhow::bail!(
                        "Expected return value {:?} but got {:?}",
                        expected_value,
                        actual_value
                    );
                }
            } else {
                anyhow::bail!("Expected return value but got none");
            }
        }

        // Verify gas usage if specified
        if let Some(gas_spec) = &expected.gas_used {
            self.verify_gas_usage(result.gas_used, gas_spec)?;
        }

        Ok(())
    }

    /// Verify gas usage against specification (supports comparisons like "<= 20000")
    fn verify_gas_usage(&self, actual: u64, spec: &str) -> Result<()> {
        let spec = spec.trim();

        if let Some(max) = spec.strip_prefix("<=") {
            let max_gas: u64 = max.trim().parse().context("Invalid gas specification")?;
            if actual > max_gas {
                anyhow::bail!("Gas usage {} exceeds maximum {}", actual, max_gas);
            }
        } else if let Some(min) = spec.strip_prefix(">=") {
            let min_gas: u64 = min.trim().parse().context("Invalid gas specification")?;
            if actual < min_gas {
                anyhow::bail!("Gas usage {} below minimum {}", actual, min_gas);
            }
        } else if let Some(exact) = spec.strip_prefix("==") {
            let exact_gas: u64 = exact.trim().parse().context("Invalid gas specification")?;
            if actual != exact_gas {
                anyhow::bail!("Gas usage {} != expected {}", actual, exact_gas);
            }
        } else {
            // Assume exact match
            let expected: u64 = spec.parse().context("Invalid gas specification")?;
            if actual != expected {
                anyhow::bail!("Gas usage {} != expected {}", actual, expected);
            }
        }

        Ok(())
    }

    /// Verify postconditions
    fn verify_postconditions(&self, ctx: &TestContext, conditions: &[Condition]) -> Result<()> {
        for condition in conditions {
            if let Some(account_name) = &condition.account {
                let account = ctx.accounts.get(account_name).ok_or_else(|| {
                    anyhow::anyhow!("Account '{}' not found for postcondition", account_name)
                })?;

                if let Some(expected_balance) = condition.balance {
                    if account.balance != expected_balance {
                        anyhow::bail!(
                            "Account '{}' balance: expected {} but got {}",
                            account_name,
                            expected_balance,
                            account.balance
                        );
                    }
                }

                if let Some(expected_nonce) = condition.nonce {
                    if account.nonce != expected_nonce {
                        anyhow::bail!(
                            "Account '{}' nonce: expected {} but got {}",
                            account_name,
                            expected_nonce,
                            account.nonce
                        );
                    }
                }

                if let Some(expected_storage) = &condition.storage {
                    for (key, expected_value) in expected_storage {
                        let actual_value = account
                            .storage
                            .get(key)
                            .map(|v| format!("0x{}", hex::encode(v)))
                            .unwrap_or_else(|| "0x".to_string());
                        if actual_value != *expected_value {
                            anyhow::bail!(
                                "Account '{}' storage[{}]: expected {} but got {}",
                                account_name,
                                key,
                                expected_value,
                                actual_value
                            );
                        }
                    }
                }
            }

            // Handle assertion postconditions
            if let Some(assertion) = &condition.assertion {
                self.evaluate_assertion(ctx, assertion)
                    .with_context(|| format!("Postcondition assertion failed: {}", assertion))?;
            }
        }
        Ok(())
    }

    /// Evaluate an assertion expression
    fn evaluate_assertion(&self, ctx: &TestContext, assertion: &str) -> Result<()> {
        // Simple assertion parser for common expressions
        let assertion = assertion.trim();

        // Handle balance comparisons: "balance(account) == 100"
        if assertion.contains("balance(") {
            return self.evaluate_balance_assertion(ctx, assertion);
        }

        // Handle nonce comparisons: "nonce(account) == 1"
        if assertion.contains("nonce(") {
            return self.evaluate_nonce_assertion(ctx, assertion);
        }

        // Handle storage comparisons: "storage(account, key) == value"
        if assertion.contains("storage(") {
            return self.evaluate_storage_assertion(ctx, assertion);
        }

        // Unknown assertion type - fail the test
        anyhow::bail!(
            "Unknown assertion type: '{}'. Supported: balance(account), nonce(account), storage(account, key)",
            assertion
        )
    }

    fn evaluate_balance_assertion(&self, ctx: &TestContext, assertion: &str) -> Result<()> {
        // Parse: balance(account) == 100
        // Use anchors to ensure we match the entire assertion
        let re_pattern = r"^balance\((\w+)\)\s*(==|>=|<=|>|<)\s*(\d+)$";
        let re = regex_lite::Regex::new(re_pattern).context("Invalid regex pattern")?;

        if let Some(captures) = re.captures(assertion) {
            let account = captures.get(1).map(|m| m.as_str()).unwrap_or("");
            let op = captures.get(2).map(|m| m.as_str()).unwrap_or("==");
            let value: u64 = captures
                .get(3)
                .map(|m| m.as_str())
                .unwrap_or("0")
                .parse()
                .context("Invalid balance value")?;

            let actual = ctx.accounts.get(account).map(|a| a.balance).unwrap_or(0);

            let result = match op {
                "==" => actual == value,
                ">=" => actual >= value,
                "<=" => actual <= value,
                ">" => actual > value,
                "<" => actual < value,
                _ => false,
            };

            if !result {
                anyhow::bail!(
                    "Assertion failed: balance({}) {} {} (actual: {})",
                    account,
                    op,
                    value,
                    actual
                );
            }
            return Ok(());
        }

        // Regex didn't match - invalid assertion format
        anyhow::bail!(
            "Invalid balance assertion format: '{}'. Expected: balance(account) <op> <value>",
            assertion
        )
    }

    fn evaluate_nonce_assertion(&self, ctx: &TestContext, assertion: &str) -> Result<()> {
        // Use anchors to ensure we match the entire assertion
        let re_pattern = r"^nonce\((\w+)\)\s*(==|>=|<=|>|<)\s*(\d+)$";
        let re = regex_lite::Regex::new(re_pattern).context("Invalid regex pattern")?;

        if let Some(captures) = re.captures(assertion) {
            let account = captures.get(1).map(|m| m.as_str()).unwrap_or("");
            let op = captures.get(2).map(|m| m.as_str()).unwrap_or("==");
            let value: u64 = captures
                .get(3)
                .map(|m| m.as_str())
                .unwrap_or("0")
                .parse()
                .context("Invalid nonce value")?;

            let actual = ctx.accounts.get(account).map(|a| a.nonce).unwrap_or(0);

            let result = match op {
                "==" => actual == value,
                ">=" => actual >= value,
                "<=" => actual <= value,
                ">" => actual > value,
                "<" => actual < value,
                _ => false,
            };

            if !result {
                anyhow::bail!(
                    "Assertion failed: nonce({}) {} {} (actual: {})",
                    account,
                    op,
                    value,
                    actual
                );
            }
            return Ok(());
        }

        // Regex didn't match - invalid assertion format
        anyhow::bail!(
            "Invalid nonce assertion format: '{}'. Expected: nonce(account) <op> <value>",
            assertion
        )
    }

    fn evaluate_storage_assertion(&self, ctx: &TestContext, assertion: &str) -> Result<()> {
        // Parse: storage(account, key) == value
        // Use anchors to ensure we match the entire assertion
        let re_pattern = r"^storage\((\w+),\s*([^)]+)\)\s*(==)\s*(.+)$";
        let re = regex_lite::Regex::new(re_pattern).context("Invalid regex pattern")?;

        if let Some(captures) = re.captures(assertion) {
            let account = captures.get(1).map(|m| m.as_str()).unwrap_or("");
            let key = captures.get(2).map(|m| m.as_str()).unwrap_or("").trim();
            let expected_value = captures.get(4).map(|m| m.as_str()).unwrap_or("").trim();

            let actual = ctx
                .accounts
                .get(account)
                .and_then(|a| a.storage.get(key))
                .map(|v| format!("0x{}", hex::encode(v)))
                .unwrap_or_else(|| "0x".to_string());

            if actual != expected_value {
                anyhow::bail!(
                    "Assertion failed: storage({}, {}) == {} (actual: {})",
                    account,
                    key,
                    expected_value,
                    actual
                );
            }
            return Ok(());
        }

        // Regex didn't match - invalid assertion format
        anyhow::bail!(
            "Invalid storage assertion format: '{}'. Expected: storage(account, key) == value",
            assertion
        )
    }
}

/// Test execution result
#[derive(Debug, Clone)]
pub struct TestResult {
    /// Test status
    pub status: TestStatus,
    /// Execution duration
    pub duration: Duration,
    /// Error message if failed
    pub error: Option<String>,
}

/// Test status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestStatus {
    /// Test passed
    Pass,
    /// Test failed
    Fail,
    /// Test skipped
    Skip,
    /// Test errored (unexpected failure)
    Error,
}

/// Test report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReport {
    /// Total tests run
    pub total: usize,
    /// Tests passed
    pub passed: usize,
    /// Tests failed
    pub failed: usize,
    /// Tests skipped
    pub skipped: usize,
    /// Total duration
    #[serde(with = "duration_serde")]
    pub duration: Duration,
    /// Individual results
    pub results: Vec<TestResultEntry>,
}

/// Individual test result entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResultEntry {
    /// Test name
    pub name: String,
    /// Category
    pub category: String,
    /// Status
    pub status: TestStatus,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Error message if failed
    pub error: Option<String>,
}

impl TestReport {
    /// Create a new empty report
    pub fn new() -> Self {
        Self {
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            duration: Duration::ZERO,
            results: Vec::new(),
        }
    }

    /// Add a test result
    pub fn add_result(&mut self, name: &str, category: &Category, result: TestResult) {
        self.total += 1;
        match result.status {
            TestStatus::Pass => self.passed += 1,
            TestStatus::Fail => self.failed += 1,
            TestStatus::Skip => self.skipped += 1,
            TestStatus::Error => self.failed += 1,
        }

        self.results.push(TestResultEntry {
            name: name.to_string(),
            category: format!("{:?}", category),
            status: result.status,
            // Convert duration to u64 milliseconds with saturation on overflow
            duration_ms: u64::try_from(result.duration.as_millis()).unwrap_or(u64::MAX),
            error: result.error,
        });
    }

    /// Convert to JSON string
    ///
    /// Returns an error if serialization fails rather than silently returning empty string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Convert to JSON string, returning empty string on error
    ///
    /// Use `to_json()` if you need error handling.
    pub fn to_json_lossy(&self) -> String {
        self.to_json().unwrap_or_else(|e| {
            if log::log_enabled!(log::Level::Error) {
                log::error!("Failed to serialize TestReport to JSON: {}", e);
            }
            String::new()
        })
    }

    /// Check if all tests passed
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }

    /// Convert to JUnit XML format
    pub fn to_junit_xml(&self) -> String {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str(&format!(
            "<testsuite name=\"TOS-TCK Conformance\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"{}\" time=\"{:.3}\">\n",
            self.total,
            self.failed,
            self.skipped,
            self.duration.as_secs_f64()
        ));

        for result in &self.results {
            xml.push_str(&format!(
                "  <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\"",
                result.category,
                result.name,
                result.duration_ms as f64 / 1000.0
            ));

            match result.status {
                TestStatus::Pass => {
                    xml.push_str(" />\n");
                }
                TestStatus::Fail | TestStatus::Error => {
                    xml.push_str(">\n");
                    if let Some(error) = &result.error {
                        xml.push_str(&format!(
                            "    <failure message=\"Test failed\">{}</failure>\n",
                            escape_xml(error)
                        ));
                    }
                    xml.push_str("  </testcase>\n");
                }
                TestStatus::Skip => {
                    xml.push_str(">\n");
                    xml.push_str("    <skipped />\n");
                    xml.push_str("  </testcase>\n");
                }
            }
        }

        xml.push_str("</testsuite>\n");
        xml
    }

    /// Get results by category
    pub fn by_category(&self) -> HashMap<String, Vec<&TestResultEntry>> {
        let mut map: HashMap<String, Vec<&TestResultEntry>> = HashMap::new();
        for result in &self.results {
            map.entry(result.category.clone()).or_default().push(result);
        }
        map
    }

    /// Print human-readable summary
    pub fn print_summary(&self) {
        println!("\n=== TOS-TCK Conformance Test Report ===\n");
        println!(
            "Total: {} | Passed: {} | Failed: {} | Skipped: {}",
            self.total, self.passed, self.failed, self.skipped
        );
        println!("Duration: {:.2}s\n", self.duration.as_secs_f64());

        if self.failed > 0 {
            println!("Failed tests:");
            for result in &self.results {
                if matches!(result.status, TestStatus::Fail | TestStatus::Error) {
                    println!("  - {} ({})", result.name, result.category);
                    if let Some(error) = &result.error {
                        println!("    Error: {}", error);
                    }
                }
            }
            println!();
        }

        println!(
            "Result: {}",
            if self.all_passed() { "PASS" } else { "FAIL" }
        );
    }
}

/// Escape XML special characters
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

impl Default for TestReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Custom serialization for Duration
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_millis().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}
