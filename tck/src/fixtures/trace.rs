//! Calculation trace output for fixture execution.
//!
//! Provides detailed step-by-step trace of balance calculations,
//! state transitions, and overflow checks during fixture execution.
//! Useful for debugging failed fixtures and verifying correct behavior.
//!
//! # Example Output
//! ```text
//! trace:
//!   step_1:
//!     operation: "transfer(alice -> bob, 1000 TOS)"
//!     pre_state:
//!       alice_balance: 10000
//!       bob_balance: 1000
//!     computation:
//!       debit: "alice_balance - amount - fee = 10000 - 1000 - 10 = 8990"
//!       credit: "bob_balance + amount = 1000 + 1000 = 2000"
//!     post_state:
//!       alice_balance: 8990
//!       bob_balance: 2000
//!     overflow_check: "8990 >= 0"
//! ```

use std::collections::HashMap;
use std::fmt;

use super::types::{StepResult, TransactionStep, TransactionType};

/// A single state value tracked in the trace.
#[derive(Debug, Clone)]
pub struct TracedValue {
    /// Account name
    pub account: String,
    /// Field name (balance, nonce, frozen_balance, etc.)
    pub field: String,
    /// The value
    pub value: u64,
}

/// A computation step showing the formula used.
#[derive(Debug, Clone)]
pub struct Computation {
    /// Label for this computation (e.g., "debit", "credit", "freeze")
    pub label: String,
    /// Human-readable formula (e.g., "alice_balance - amount - fee = 10000 - 1000 - 10 = 8990")
    pub formula: String,
}

/// Overflow/underflow check result.
#[derive(Debug, Clone)]
pub struct OverflowCheck {
    /// Description of what was checked
    pub description: String,
    /// Whether the check passed
    pub passed: bool,
}

/// A single step trace entry.
#[derive(Debug, Clone)]
pub struct StepTrace {
    /// Step number
    pub step: u32,
    /// Human-readable operation description
    pub operation: String,
    /// Pre-state values before the operation
    pub pre_state: Vec<TracedValue>,
    /// Computations performed
    pub computations: Vec<Computation>,
    /// Post-state values after the operation
    pub post_state: Vec<TracedValue>,
    /// Overflow/underflow checks
    pub overflow_checks: Vec<OverflowCheck>,
    /// Whether the step succeeded
    pub success: bool,
    /// Error message if the step failed
    pub error: Option<String>,
}

/// Complete execution trace for a fixture.
#[derive(Debug, Clone)]
pub struct ExecutionTrace {
    /// Fixture name
    pub fixture_name: String,
    /// Per-step traces
    pub steps: Vec<StepTrace>,
}

impl ExecutionTrace {
    /// Create a new empty execution trace.
    pub fn new(fixture_name: &str) -> Self {
        Self {
            fixture_name: fixture_name.to_string(),
            steps: Vec::new(),
        }
    }

    /// Add a step trace entry.
    pub fn add_step(&mut self, step: StepTrace) {
        self.steps.push(step);
    }

    /// Format as YAML-like trace output.
    pub fn to_yaml(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("# Trace: {}\n", self.fixture_name));
        output.push_str("trace:\n");

        for step in &self.steps {
            output.push_str(&format!("  step_{}:\n", step.step));
            output.push_str(&format!("    operation: \"{}\"\n", step.operation));
            output.push_str(&format!("    success: {}\n", step.success));

            if let Some(ref err) = step.error {
                output.push_str(&format!("    error: \"{}\"\n", err));
            }

            if !step.pre_state.is_empty() {
                output.push_str("    pre_state:\n");
                for val in &step.pre_state {
                    output.push_str(&format!(
                        "      {}_{}: {}\n",
                        val.account, val.field, val.value
                    ));
                }
            }

            if !step.computations.is_empty() {
                output.push_str("    computation:\n");
                for comp in &step.computations {
                    output.push_str(&format!("      {}: \"{}\"\n", comp.label, comp.formula));
                }
            }

            if !step.post_state.is_empty() {
                output.push_str("    post_state:\n");
                for val in &step.post_state {
                    output.push_str(&format!(
                        "      {}_{}: {}\n",
                        val.account, val.field, val.value
                    ));
                }
            }

            if !step.overflow_checks.is_empty() {
                for check in &step.overflow_checks {
                    let status = if check.passed { "pass" } else { "FAIL" };
                    output.push_str(&format!(
                        "    overflow_check: \"{}\" [{}]\n",
                        check.description, status
                    ));
                }
            }
        }

        output
    }
}

impl fmt::Display for ExecutionTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_yaml())
    }
}

/// Builder for constructing step traces from transaction execution.
pub struct TraceBuilder {
    /// Current step number counter
    step_counter: u32,
    /// Accumulated step traces
    steps: Vec<StepTrace>,
}

#[allow(clippy::too_many_arguments)]
impl TraceBuilder {
    /// Create a new trace builder.
    pub fn new() -> Self {
        Self {
            step_counter: 0,
            steps: Vec::new(),
        }
    }

    /// Record a transfer step trace.
    pub fn trace_transfer(
        &mut self,
        step: &TransactionStep,
        from_name: &str,
        to_name: &str,
        amount: u64,
        fee: u64,
        pre_from_balance: u64,
        pre_to_balance: u64,
        result: &StepResult,
    ) {
        self.step_counter = self.step_counter.saturating_add(1);
        let step_num = step.step.unwrap_or(self.step_counter);

        let operation = format!(
            "transfer({} -> {}, {} TOS, fee {} TOS)",
            from_name, to_name, amount, fee
        );

        let mut computations = Vec::new();
        let mut post_state = Vec::new();
        let mut overflow_checks = Vec::new();

        if result.success {
            let total_debit = amount.saturating_add(fee);
            let post_from = pre_from_balance.saturating_sub(total_debit);
            let post_to = pre_to_balance.saturating_add(amount);

            computations.push(Computation {
                label: "debit".to_string(),
                formula: format!(
                    "{}_balance - amount - fee = {} - {} - {} = {}",
                    from_name, pre_from_balance, amount, fee, post_from
                ),
            });
            computations.push(Computation {
                label: "credit".to_string(),
                formula: format!(
                    "{}_balance + amount = {} + {} = {}",
                    to_name, pre_to_balance, amount, post_to
                ),
            });

            post_state.push(TracedValue {
                account: from_name.to_string(),
                field: "balance".to_string(),
                value: post_from,
            });
            post_state.push(TracedValue {
                account: to_name.to_string(),
                field: "balance".to_string(),
                value: post_to,
            });

            overflow_checks.push(OverflowCheck {
                description: format!("{} >= 0", post_from),
                passed: true,
            });
        }

        self.steps.push(StepTrace {
            step: step_num,
            operation,
            pre_state: vec![
                TracedValue {
                    account: from_name.to_string(),
                    field: "balance".to_string(),
                    value: pre_from_balance,
                },
                TracedValue {
                    account: to_name.to_string(),
                    field: "balance".to_string(),
                    value: pre_to_balance,
                },
            ],
            computations,
            post_state,
            overflow_checks,
            success: result.success,
            error: result.error.clone(),
        });
    }

    /// Record a freeze step trace.
    pub fn trace_freeze(
        &mut self,
        step: &TransactionStep,
        from_name: &str,
        amount: u64,
        fee: u64,
        pre_balance: u64,
        pre_frozen: u64,
        result: &StepResult,
    ) {
        self.step_counter = self.step_counter.saturating_add(1);
        let step_num = step.step.unwrap_or(self.step_counter);

        let operation = format!("freeze({}, {} TOS, fee {} TOS)", from_name, amount, fee);

        let mut computations = Vec::new();
        let mut post_state = Vec::new();
        let mut overflow_checks = Vec::new();

        if result.success {
            let total_cost = amount.saturating_add(fee);
            let post_balance = pre_balance.saturating_sub(total_cost);
            let post_frozen = pre_frozen.saturating_add(amount);

            computations.push(Computation {
                label: "debit".to_string(),
                formula: format!(
                    "{}_balance - amount - fee = {} - {} - {} = {}",
                    from_name, pre_balance, amount, fee, post_balance
                ),
            });
            computations.push(Computation {
                label: "freeze".to_string(),
                formula: format!(
                    "{}_frozen + amount = {} + {} = {}",
                    from_name, pre_frozen, amount, post_frozen
                ),
            });

            post_state.push(TracedValue {
                account: from_name.to_string(),
                field: "balance".to_string(),
                value: post_balance,
            });
            post_state.push(TracedValue {
                account: from_name.to_string(),
                field: "frozen_balance".to_string(),
                value: post_frozen,
            });

            overflow_checks.push(OverflowCheck {
                description: format!("{} >= 0", post_balance),
                passed: true,
            });
        }

        self.steps.push(StepTrace {
            step: step_num,
            operation,
            pre_state: vec![
                TracedValue {
                    account: from_name.to_string(),
                    field: "balance".to_string(),
                    value: pre_balance,
                },
                TracedValue {
                    account: from_name.to_string(),
                    field: "frozen_balance".to_string(),
                    value: pre_frozen,
                },
            ],
            computations,
            post_state,
            overflow_checks,
            success: result.success,
            error: result.error.clone(),
        });
    }

    /// Record a delegate step trace.
    pub fn trace_delegate(
        &mut self,
        step: &TransactionStep,
        from_name: &str,
        to_name: &str,
        amount: u64,
        fee: u64,
        pre_balance: u64,
        pre_frozen: u64,
        pre_delegated_out: u64,
        result: &StepResult,
    ) {
        self.step_counter = self.step_counter.saturating_add(1);
        let step_num = step.step.unwrap_or(self.step_counter);

        let operation = format!(
            "delegate({} -> {}, {} TOS, fee {} TOS)",
            from_name, to_name, amount, fee
        );

        let mut computations = Vec::new();
        let mut post_state = Vec::new();
        let mut overflow_checks = Vec::new();

        if result.success {
            let post_balance = pre_balance.saturating_sub(fee);
            let post_delegated = pre_delegated_out.saturating_add(amount);

            computations.push(Computation {
                label: "fee_debit".to_string(),
                formula: format!(
                    "{}_balance - fee = {} - {} = {}",
                    from_name, pre_balance, fee, post_balance
                ),
            });
            computations.push(Computation {
                label: "delegate".to_string(),
                formula: format!(
                    "{}_delegated_to_{} + amount = {} + {} = {}",
                    from_name, to_name, pre_delegated_out, amount, post_delegated
                ),
            });

            post_state.push(TracedValue {
                account: from_name.to_string(),
                field: "balance".to_string(),
                value: post_balance,
            });
            post_state.push(TracedValue {
                account: from_name.to_string(),
                field: format!("delegated_to_{}", to_name),
                value: post_delegated,
            });

            // Delegation must not exceed frozen balance
            overflow_checks.push(OverflowCheck {
                description: format!("{} <= {}", post_delegated, pre_frozen),
                passed: post_delegated <= pre_frozen,
            });
        }

        self.steps.push(StepTrace {
            step: step_num,
            operation,
            pre_state: vec![
                TracedValue {
                    account: from_name.to_string(),
                    field: "balance".to_string(),
                    value: pre_balance,
                },
                TracedValue {
                    account: from_name.to_string(),
                    field: "frozen_balance".to_string(),
                    value: pre_frozen,
                },
                TracedValue {
                    account: from_name.to_string(),
                    field: format!("delegated_to_{}", to_name),
                    value: pre_delegated_out,
                },
            ],
            computations,
            post_state,
            overflow_checks,
            success: result.success,
            error: result.error.clone(),
        });
    }

    /// Record a generic step (mine_block, advance_time, etc.).
    pub fn trace_generic(&mut self, step: &TransactionStep, result: &StepResult) {
        self.step_counter = self.step_counter.saturating_add(1);
        let step_num = step.step.unwrap_or(self.step_counter);

        let operation = match &step.tx_type {
            TransactionType::MineBlock => "mine_block()".to_string(),
            TransactionType::AdvanceTime => {
                let duration = step.duration.as_deref().unwrap_or("0s");
                format!("advance_time({})", duration)
            }
            TransactionType::Register => {
                let from = step.from.as_deref().unwrap_or("unknown");
                format!("register({})", from)
            }
            TransactionType::DeployContract => {
                let from = step.from.as_deref().unwrap_or("unknown");
                format!("deploy_contract(from={})", from)
            }
            TransactionType::CallContract => {
                let from = step.from.as_deref().unwrap_or("unknown");
                let contract = step.contract.as_deref().unwrap_or("unknown");
                let function = step.function.as_deref().unwrap_or("unknown");
                format!("call_contract({}, {}.{})", from, contract, function)
            }
            other => format!("{:?}", other),
        };

        self.steps.push(StepTrace {
            step: step_num,
            operation,
            pre_state: Vec::new(),
            computations: Vec::new(),
            post_state: Vec::new(),
            overflow_checks: Vec::new(),
            success: result.success,
            error: result.error.clone(),
        });
    }

    /// Record an unfreeze step trace.
    pub fn trace_unfreeze(
        &mut self,
        step: &TransactionStep,
        from_name: &str,
        amount: u64,
        fee: u64,
        pre_balance: u64,
        pre_frozen: u64,
        result: &StepResult,
    ) {
        self.step_counter = self.step_counter.saturating_add(1);
        let step_num = step.step.unwrap_or(self.step_counter);

        let operation = format!("unfreeze({}, {} TOS, fee {} TOS)", from_name, amount, fee);

        let mut computations = Vec::new();
        let mut post_state = Vec::new();
        let mut overflow_checks = Vec::new();

        if result.success {
            let post_balance = pre_balance.saturating_sub(fee).saturating_add(amount);
            let post_frozen = pre_frozen.saturating_sub(amount);

            computations.push(Computation {
                label: "credit".to_string(),
                formula: format!(
                    "{}_balance - fee + amount = {} - {} + {} = {}",
                    from_name, pre_balance, fee, amount, post_balance
                ),
            });
            computations.push(Computation {
                label: "unfreeze".to_string(),
                formula: format!(
                    "{}_frozen - amount = {} - {} = {}",
                    from_name, pre_frozen, amount, post_frozen
                ),
            });

            post_state.push(TracedValue {
                account: from_name.to_string(),
                field: "balance".to_string(),
                value: post_balance,
            });
            post_state.push(TracedValue {
                account: from_name.to_string(),
                field: "frozen_balance".to_string(),
                value: post_frozen,
            });

            overflow_checks.push(OverflowCheck {
                description: format!("{}_frozen {} >= 0", from_name, post_frozen),
                passed: true,
            });
        }

        self.steps.push(StepTrace {
            step: step_num,
            operation,
            pre_state: vec![
                TracedValue {
                    account: from_name.to_string(),
                    field: "balance".to_string(),
                    value: pre_balance,
                },
                TracedValue {
                    account: from_name.to_string(),
                    field: "frozen_balance".to_string(),
                    value: pre_frozen,
                },
            ],
            computations,
            post_state,
            overflow_checks,
            success: result.success,
            error: result.error.clone(),
        });
    }

    /// Record a UNO transfer step trace.
    pub fn trace_uno_transfer(
        &mut self,
        step: &TransactionStep,
        from_name: &str,
        to_name: &str,
        asset: &str,
        amount: u64,
        fee: u64,
        pre_from_balance: u64,
        pre_from_uno: u64,
        pre_to_uno: u64,
        result: &StepResult,
    ) {
        self.step_counter = self.step_counter.saturating_add(1);
        let step_num = step.step.unwrap_or(self.step_counter);

        let operation = format!(
            "uno_transfer({} -> {}, {} {}, fee {} TOS)",
            from_name, to_name, amount, asset, fee
        );

        let mut computations = Vec::new();
        let mut post_state = Vec::new();
        let mut overflow_checks = Vec::new();

        if result.success {
            let post_from_balance = pre_from_balance.saturating_sub(fee);
            let post_from_uno = pre_from_uno.saturating_sub(amount);
            let post_to_uno = pre_to_uno.saturating_add(amount);

            computations.push(Computation {
                label: "fee_debit".to_string(),
                formula: format!(
                    "{}_balance - fee = {} - {} = {}",
                    from_name, pre_from_balance, fee, post_from_balance
                ),
            });
            computations.push(Computation {
                label: "uno_debit".to_string(),
                formula: format!(
                    "{}_{}  - amount = {} - {} = {}",
                    from_name, asset, pre_from_uno, amount, post_from_uno
                ),
            });
            computations.push(Computation {
                label: "uno_credit".to_string(),
                formula: format!(
                    "{}_{} + amount = {} + {} = {}",
                    to_name, asset, pre_to_uno, amount, post_to_uno
                ),
            });

            post_state.push(TracedValue {
                account: from_name.to_string(),
                field: "balance".to_string(),
                value: post_from_balance,
            });
            post_state.push(TracedValue {
                account: from_name.to_string(),
                field: format!("uno_{}", asset),
                value: post_from_uno,
            });
            post_state.push(TracedValue {
                account: to_name.to_string(),
                field: format!("uno_{}", asset),
                value: post_to_uno,
            });

            overflow_checks.push(OverflowCheck {
                description: format!("{}_balance {} >= 0", from_name, post_from_balance),
                passed: true,
            });
            overflow_checks.push(OverflowCheck {
                description: format!("{}_{} {} >= 0", from_name, asset, post_from_uno),
                passed: true,
            });
        }

        self.steps.push(StepTrace {
            step: step_num,
            operation,
            pre_state: vec![
                TracedValue {
                    account: from_name.to_string(),
                    field: "balance".to_string(),
                    value: pre_from_balance,
                },
                TracedValue {
                    account: from_name.to_string(),
                    field: format!("uno_{}", asset),
                    value: pre_from_uno,
                },
                TracedValue {
                    account: to_name.to_string(),
                    field: format!("uno_{}", asset),
                    value: pre_to_uno,
                },
            ],
            computations,
            post_state,
            overflow_checks,
            success: result.success,
            error: result.error.clone(),
        });
    }

    /// Build the final execution trace.
    pub fn build(self, fixture_name: &str) -> ExecutionTrace {
        ExecutionTrace {
            fixture_name: fixture_name.to_string(),
            steps: self.steps,
        }
    }

    /// Get the current list of traces (borrow).
    pub fn traces(&self) -> &[StepTrace] {
        &self.steps
    }
}

impl Default for TraceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Collect pre-state balances from a backend for trace recording.
pub async fn collect_pre_state<B: super::backend::FixtureBackend + ?Sized>(
    backend: &B,
    accounts: &[&str],
) -> HashMap<String, u64> {
    let mut state = HashMap::new();
    for account in accounts {
        if let Ok(balance) = backend.get_balance(account).await {
            state.insert(account.to_string(), balance);
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::types::{StepResult, TransactionStep, TransactionType};

    fn make_step(step_num: u32, tx_type: TransactionType) -> TransactionStep {
        TransactionStep {
            step: Some(step_num),
            name: None,
            tx_type,
            from: Some("alice".to_string()),
            to: Some("bob".to_string()),
            amount: Some("1000 TOS".to_string()),
            fee: Some("10 TOS".to_string()),
            asset: None,
            nonce: None,
            duration: None,
            code: None,
            contract: None,
            function: None,
            args: None,
            expect_status: crate::fixtures::types::ExpectStatus::Success,
            expect_error: None,
        }
    }

    fn make_success_result(step_num: u32) -> StepResult {
        StepResult {
            step: Some(step_num),
            success: true,
            error: None,
            error_code: None,
            state_changes: vec![],
        }
    }

    fn make_failure_result(step_num: u32, error: &str) -> StepResult {
        StepResult {
            step: Some(step_num),
            success: false,
            error: Some(error.to_string()),
            error_code: Some("INSUFFICIENT_BALANCE".to_string()),
            state_changes: vec![],
        }
    }

    #[test]
    fn test_trace_transfer_success() {
        let mut builder = TraceBuilder::new();
        let step = make_step(1, TransactionType::Transfer);
        let result = make_success_result(1);

        builder.trace_transfer(&step, "alice", "bob", 1000, 10, 10000, 500, &result);

        let trace = builder.build("test_fixture");
        assert_eq!(trace.steps.len(), 1);
        assert!(trace.steps[0].success);
        assert_eq!(trace.steps[0].pre_state.len(), 2);
        assert_eq!(trace.steps[0].computations.len(), 2);
        assert_eq!(trace.steps[0].post_state.len(), 2);

        // Verify post-state values
        assert_eq!(trace.steps[0].post_state[0].value, 8990); // 10000 - 1000 - 10
        assert_eq!(trace.steps[0].post_state[1].value, 1500); // 500 + 1000
    }

    #[test]
    fn test_trace_transfer_failure() {
        let mut builder = TraceBuilder::new();
        let step = make_step(1, TransactionType::Transfer);
        let result = make_failure_result(1, "Insufficient balance");

        builder.trace_transfer(&step, "alice", "bob", 1000, 10, 500, 0, &result);

        let trace = builder.build("test_fixture");
        assert_eq!(trace.steps.len(), 1);
        assert!(!trace.steps[0].success);
        assert_eq!(
            trace.steps[0].error.as_deref(),
            Some("Insufficient balance")
        );
        // No computations or post-state for failed step
        assert!(trace.steps[0].computations.is_empty());
        assert!(trace.steps[0].post_state.is_empty());
    }

    #[test]
    fn test_trace_freeze() {
        let mut builder = TraceBuilder::new();
        let step = make_step(1, TransactionType::Freeze);
        let result = make_success_result(1);

        builder.trace_freeze(&step, "alice", 5000, 10, 10000, 0, &result);

        let trace = builder.build("freeze_test");
        assert_eq!(trace.steps.len(), 1);
        assert!(trace.steps[0].success);
        assert_eq!(trace.steps[0].post_state[0].value, 4990); // 10000 - 5000 - 10
        assert_eq!(trace.steps[0].post_state[1].value, 5000); // 0 + 5000
    }

    #[test]
    fn test_trace_delegate() {
        let mut builder = TraceBuilder::new();
        let step = make_step(1, TransactionType::Delegate);
        let result = make_success_result(1);

        builder.trace_delegate(&step, "alice", "charlie", 500, 10, 10000, 5000, 0, &result);

        let trace = builder.build("delegate_test");
        assert_eq!(trace.steps.len(), 1);
        assert!(trace.steps[0].success);
        // Balance after fee
        assert_eq!(trace.steps[0].post_state[0].value, 9990); // 10000 - 10
                                                              // Delegated amount
        assert_eq!(trace.steps[0].post_state[1].value, 500); // 0 + 500
                                                             // Overflow check: 500 <= 5000
        assert!(trace.steps[0].overflow_checks[0].passed);
    }

    #[test]
    fn test_trace_delegate_exceeds_frozen() {
        let mut builder = TraceBuilder::new();
        let step = make_step(1, TransactionType::Delegate);
        let result = make_success_result(1);

        // Delegating 6000 when only 5000 frozen
        builder.trace_delegate(&step, "alice", "charlie", 6000, 10, 10000, 5000, 0, &result);

        let trace = builder.build("delegate_overflow_test");
        // Overflow check: 6000 <= 5000 should fail
        assert!(!trace.steps[0].overflow_checks[0].passed);
    }

    #[test]
    fn test_trace_unfreeze() {
        let mut builder = TraceBuilder::new();
        let step = make_step(1, TransactionType::Unfreeze);
        let result = make_success_result(1);

        builder.trace_unfreeze(&step, "alice", 2000, 10, 5000, 5000, &result);

        let trace = builder.build("unfreeze_test");
        assert_eq!(trace.steps.len(), 1);
        assert!(trace.steps[0].success);
        // Balance: 5000 - 10 + 2000 = 6990
        assert_eq!(trace.steps[0].post_state[0].value, 6990);
        // Frozen: 5000 - 2000 = 3000
        assert_eq!(trace.steps[0].post_state[1].value, 3000);
    }

    #[test]
    fn test_trace_uno_transfer() {
        let mut builder = TraceBuilder::new();
        let mut step = make_step(1, TransactionType::UnoTransfer);
        step.asset = Some("GOLD".to_string());
        let result = make_success_result(1);

        builder.trace_uno_transfer(
            &step, "alice", "bob", "GOLD", 200, 10, 10000, 500, 100, &result,
        );

        let trace = builder.build("uno_transfer_test");
        assert_eq!(trace.steps.len(), 1);
        assert!(trace.steps[0].success);
        // TOS balance: 10000 - 10 = 9990
        assert_eq!(trace.steps[0].post_state[0].value, 9990);
        // Alice GOLD: 500 - 200 = 300
        assert_eq!(trace.steps[0].post_state[1].value, 300);
        // Bob GOLD: 100 + 200 = 300
        assert_eq!(trace.steps[0].post_state[2].value, 300);
    }

    #[test]
    fn test_trace_generic_mine_block() {
        let mut builder = TraceBuilder::new();
        let step = make_step(1, TransactionType::MineBlock);
        let result = make_success_result(1);

        builder.trace_generic(&step, &result);

        let trace = builder.build("mine_test");
        assert_eq!(trace.steps.len(), 1);
        assert_eq!(trace.steps[0].operation, "mine_block()");
    }

    #[test]
    fn test_trace_to_yaml() {
        let mut builder = TraceBuilder::new();
        let step = make_step(1, TransactionType::Transfer);
        let result = make_success_result(1);

        builder.trace_transfer(&step, "alice", "bob", 1000, 10, 10000, 500, &result);

        let trace = builder.build("yaml_test");
        let yaml = trace.to_yaml();

        assert!(yaml.contains("# Trace: yaml_test"));
        assert!(yaml.contains("step_1:"));
        assert!(yaml.contains("operation: \"transfer(alice -> bob, 1000 TOS, fee 10 TOS)\""));
        assert!(yaml.contains("alice_balance: 10000"));
        assert!(yaml.contains("bob_balance: 500"));
        assert!(yaml.contains("debit:"));
        assert!(yaml.contains("credit:"));
        assert!(yaml.contains("alice_balance: 8990"));
        assert!(yaml.contains("bob_balance: 1500"));
    }

    #[test]
    fn test_execution_trace_display() {
        let trace = ExecutionTrace::new("display_test");
        let output = format!("{}", trace);
        assert!(output.contains("# Trace: display_test"));
        assert!(output.contains("trace:"));
    }

    #[test]
    fn test_trace_multiple_steps() {
        let mut builder = TraceBuilder::new();

        let step1 = make_step(1, TransactionType::Transfer);
        let result1 = make_success_result(1);
        builder.trace_transfer(&step1, "alice", "bob", 1000, 10, 10000, 0, &result1);

        let step2 = make_step(2, TransactionType::Freeze);
        let result2 = make_success_result(2);
        builder.trace_freeze(&step2, "alice", 5000, 10, 8990, 0, &result2);

        let step3 = make_step(3, TransactionType::MineBlock);
        let result3 = make_success_result(3);
        builder.trace_generic(&step3, &result3);

        let trace = builder.build("multi_step");
        assert_eq!(trace.steps.len(), 3);
        assert_eq!(trace.steps[0].step, 1);
        assert_eq!(trace.steps[1].step, 2);
        assert_eq!(trace.steps[2].step, 3);
    }

    #[test]
    fn test_trace_builder_default() {
        let builder = TraceBuilder::default();
        assert_eq!(builder.step_counter, 0);
        assert!(builder.steps.is_empty());
    }

    #[test]
    fn test_trace_builder_auto_step_numbering() {
        let mut builder = TraceBuilder::new();

        // Step without explicit step number
        let mut step = make_step(1, TransactionType::MineBlock);
        step.step = None;
        let result = make_success_result(1);

        builder.trace_generic(&step, &result);
        builder.trace_generic(&step, &result);

        let trace = builder.build("auto_number");
        assert_eq!(trace.steps[0].step, 1);
        assert_eq!(trace.steps[1].step, 2);
    }
}
