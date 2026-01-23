//! FixtureBackend trait for multi-tier fixture execution.
//!
//! Defines the interface that all execution backends must implement.
//! Each tier (TestBlockchain, ChainClient, TestDaemon, LocalCluster) provides
//! its own backend implementation with appropriate fidelity.

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use super::types::{DelegationMap, EnergyState, FixtureSetup, StepResult, TransactionStep};

/// Backend trait for fixture execution.
///
/// Each tier implements this trait to provide setup, execution, and query
/// capabilities for declarative fixture tests.
#[async_trait]
pub trait FixtureBackend: Send + Sync {
    /// Setup initial state from fixture definition.
    ///
    /// Creates accounts with specified balances, nonces, assets,
    /// and delegations as defined in the fixture's setup section.
    async fn setup(&mut self, setup: &FixtureSetup) -> Result<()>;

    /// Execute a single transaction step.
    ///
    /// Returns the step result including success/failure status
    /// and any state changes caused by the transaction.
    async fn execute_step(&mut self, step: &TransactionStep) -> Result<StepResult>;

    /// Mine a block to finalize pending transactions.
    async fn mine_block(&mut self) -> Result<()>;

    /// Query account TOS balance.
    async fn get_balance(&self, account: &str) -> Result<u64>;

    /// Query UNO asset balance for an account.
    async fn get_uno_balance(&self, account: &str, asset: &str) -> Result<u64>;

    /// Query account nonce.
    async fn get_nonce(&self, account: &str) -> Result<u64>;

    /// Query account energy state.
    async fn get_energy(&self, account: &str) -> Result<EnergyState>;

    /// Query frozen TOS balance.
    async fn get_frozen_balance(&self, account: &str) -> Result<u64>;

    /// Query outgoing delegations for an account.
    async fn get_delegations_out(&self, account: &str) -> Result<DelegationMap>;

    /// Query incoming delegations for an account.
    async fn get_delegations_in(&self, account: &str) -> Result<DelegationMap>;

    /// Advance time by the specified duration.
    ///
    /// Used for testing time-dependent behavior like energy recovery.
    async fn advance_time(&mut self, duration: Duration) -> Result<()>;

    /// Get the list of known account names in this backend.
    fn account_names(&self) -> Vec<String>;

    /// Get the backend tier identifier.
    fn tier_name(&self) -> &str;
}

/// Verify a step result matches expected status.
pub fn verify_step_result(result: &StepResult, step: &TransactionStep) -> Result<()> {
    use super::types::ExpectStatus;

    match &step.expect_status {
        ExpectStatus::Success => {
            if !result.success {
                return Err(anyhow::anyhow!(
                    "Step {} '{}' expected success but got error: {}",
                    step.step.unwrap_or(0),
                    step.name.as_deref().unwrap_or("unnamed"),
                    result.error.as_deref().unwrap_or("unknown error")
                ));
            }
        }
        ExpectStatus::Error => {
            if result.success {
                return Err(anyhow::anyhow!(
                    "Step {} '{}' expected error but succeeded",
                    step.step.unwrap_or(0),
                    step.name.as_deref().unwrap_or("unnamed"),
                ));
            }
            // If a specific error code is expected, verify it
            if let Some(expected_error) = &step.expect_error {
                let actual_error = result.error_code.as_deref().unwrap_or("");
                if actual_error != expected_error.as_str() {
                    return Err(anyhow::anyhow!(
                        "Step {} '{}' expected error '{}' but got '{}'",
                        step.step.unwrap_or(0),
                        step.name.as_deref().unwrap_or("unnamed"),
                        expected_error,
                        actual_error,
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::types::{ExpectStatus, TransactionType};

    #[test]
    fn test_verify_step_result_success() {
        let result = StepResult {
            step: Some(1),
            success: true,
            error: None,
            error_code: None,
            state_changes: vec![],
        };
        let step = TransactionStep {
            step: Some(1),
            name: Some("test".to_string()),
            tx_type: TransactionType::Transfer,
            from: Some("alice".to_string()),
            to: Some("bob".to_string()),
            amount: Some("100 TOS".to_string()),
            fee: None,
            asset: None,
            nonce: None,
            duration: None,
            code: None,
            contract: None,
            function: None,
            args: None,
            expect_status: ExpectStatus::Success,
            expect_error: None,
        };
        assert!(verify_step_result(&result, &step).is_ok());
    }

    #[test]
    fn test_verify_step_result_expected_error() {
        let result = StepResult {
            step: Some(1),
            success: false,
            error: Some("Insufficient balance".to_string()),
            error_code: Some("INSUFFICIENT_BALANCE".to_string()),
            state_changes: vec![],
        };
        let step = TransactionStep {
            step: Some(1),
            name: Some("overdraft".to_string()),
            tx_type: TransactionType::Transfer,
            from: Some("alice".to_string()),
            to: Some("bob".to_string()),
            amount: Some("999999 TOS".to_string()),
            fee: None,
            asset: None,
            nonce: None,
            duration: None,
            code: None,
            contract: None,
            function: None,
            args: None,
            expect_status: ExpectStatus::Error,
            expect_error: Some("INSUFFICIENT_BALANCE".to_string()),
        };
        assert!(verify_step_result(&result, &step).is_ok());
    }

    #[test]
    fn test_verify_step_result_unexpected_failure() {
        let result = StepResult {
            step: Some(1),
            success: false,
            error: Some("Nonce mismatch".to_string()),
            error_code: None,
            state_changes: vec![],
        };
        let step = TransactionStep {
            step: Some(1),
            name: Some("transfer".to_string()),
            tx_type: TransactionType::Transfer,
            from: Some("alice".to_string()),
            to: Some("bob".to_string()),
            amount: Some("100 TOS".to_string()),
            fee: None,
            asset: None,
            nonce: None,
            duration: None,
            code: None,
            contract: None,
            function: None,
            args: None,
            expect_status: ExpectStatus::Success,
            expect_error: None,
        };
        assert!(verify_step_result(&result, &step).is_err());
    }
}
