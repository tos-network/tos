//! Fixture execution engine.
//!
//! Orchestrates the execution of fixture tests across the setup, execution,
//! verification, and invariant checking phases. Supports single-tier and
//! cross-tier execution modes.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Result};

use super::backend::{verify_step_result, FixtureBackend};
use super::invariants::check_invariants;
use super::parser::{parse_fixture, parse_fixture_file};
use super::types::{AccountState, CrossTierResult, Fixture, FixtureResult, Step, StepResult, Tier};
use super::verification::verify_expected;

/// Run a fixture from a YAML string on a specific backend.
pub async fn run_fixture_on_backend(
    yaml: &str,
    backend: &mut dyn FixtureBackend,
) -> Result<FixtureResult> {
    let fixture = parse_fixture(yaml)?;
    execute_fixture(&fixture, backend).await
}

/// Run a fixture from a file path on a specific backend.
pub async fn run_fixture_file_on_backend(
    path: &Path,
    backend: &mut dyn FixtureBackend,
) -> Result<FixtureResult> {
    let fixture = parse_fixture_file(path)?;
    execute_fixture(&fixture, backend).await
}

/// Execute a parsed fixture on a backend.
pub async fn execute_fixture(
    fixture: &Fixture,
    backend: &mut dyn FixtureBackend,
) -> Result<FixtureResult> {
    let fixture_name = fixture.fixture.name.clone();

    // Phase 1: Setup initial state
    backend
        .setup(&fixture.setup)
        .await
        .map_err(|e| anyhow!("Fixture '{}' setup failed: {}", fixture_name, e))?;

    // Phase 2: Execute transaction sequence
    let mut step_results: Vec<StepResult> = Vec::new();
    for step in &fixture.transactions {
        match step {
            Step::Transaction(tx) => {
                let result = backend.execute_step(tx).await.map_err(|e| {
                    anyhow!(
                        "Fixture '{}' step {} execution error: {}",
                        fixture_name,
                        tx.step.unwrap_or(0),
                        e
                    )
                })?;

                // Verify step result matches expectations
                verify_step_result(&result, tx).map_err(|e| {
                    anyhow!(
                        "Fixture '{}' step {} verification: {}",
                        fixture_name,
                        tx.step.unwrap_or(0),
                        e
                    )
                })?;

                step_results.push(result);

                // If this is a mine_block type, also mine
                if tx.tx_type == super::types::TransactionType::MineBlock {
                    backend.mine_block().await?;
                }
            }
            Step::Checkpoint(cp) => {
                // Mine block at checkpoint if requested
                if cp.mine_block.unwrap_or(false) {
                    backend.mine_block().await?;
                }

                // Verify checkpoint state if specified
                if let Some(verify) = &cp.verify {
                    super::verification::verify_checkpoint(backend, &cp.checkpoint, verify).await?;
                }
            }
        }
    }

    // Phase 3: Verify expected final state
    let mut verification_errors = Vec::new();
    if let Some(expected) = &fixture.expected {
        verification_errors = verify_expected(backend, expected).await;
    }

    // Phase 4: Check invariants
    let invariant_errors = check_invariants(backend, &fixture.invariants, &fixture.setup).await;

    // Collect final state
    let final_state = collect_final_state(backend).await;

    let success = verification_errors.is_empty() && invariant_errors.is_empty();
    Ok(FixtureResult {
        fixture_name,
        success,
        step_results,
        verification_errors,
        invariant_errors,
        final_state,
    })
}

/// Collect the final account states from a backend.
async fn collect_final_state(backend: &dyn FixtureBackend) -> HashMap<String, AccountState> {
    let mut states = HashMap::new();

    for account_name in backend.account_names() {
        let balance = backend.get_balance(&account_name).await.unwrap_or(0);
        let nonce = backend.get_nonce(&account_name).await.unwrap_or(0);
        let frozen_balance = backend.get_frozen_balance(&account_name).await.unwrap_or(0);
        let energy = backend.get_energy(&account_name).await.unwrap_or_default();
        let delegations_out = backend
            .get_delegations_out(&account_name)
            .await
            .unwrap_or_default();
        let delegations_in = backend
            .get_delegations_in(&account_name)
            .await
            .unwrap_or_default();

        // Collect UNO balances (we don't have a list of assets,
        // so this is empty unless the backend tracks them)
        let uno_balances = HashMap::new();

        states.insert(
            account_name,
            AccountState {
                balance,
                nonce,
                uno_balances,
                frozen_balance,
                energy_limit: energy.limit,
                energy_usage: energy.usage,
                delegations_out,
                delegations_in,
            },
        );
    }

    states
}

/// Run the same fixture across multiple tiers and compare results.
///
/// All tiers must produce the same final state for the fixture to pass.
pub async fn run_fixture_cross_tier(
    yaml: &str,
    backends: &mut [(&str, &mut dyn FixtureBackend)],
) -> Result<CrossTierResult> {
    let fixture = parse_fixture(yaml)?;

    let mut results: Vec<(String, FixtureResult)> = Vec::new();

    for (tier_name, backend) in backends.iter_mut() {
        // Check if fixture supports this tier
        let result = execute_fixture(&fixture, *backend).await?;
        results.push((tier_name.to_string(), result));
    }

    // Check consistency across tiers
    let mut consistent = true;
    let mut inconsistencies = Vec::new();

    if results.len() > 1 {
        let reference_state = &results[0].1.final_state;
        for (tier_name, result) in results.iter().skip(1) {
            for (account, ref_state) in reference_state {
                if let Some(other_state) = result.final_state.get(account) {
                    if ref_state != other_state {
                        consistent = false;
                        inconsistencies.push(format!(
                            "Account '{}' differs between {} and {}: {:?} vs {:?}",
                            account, results[0].0, tier_name, ref_state, other_state,
                        ));
                    }
                } else {
                    consistent = false;
                    inconsistencies.push(format!(
                        "Account '{}' missing in tier {}",
                        account, tier_name
                    ));
                }
            }
        }
    }

    // Map results to tier slots
    let mut tier1 = None;
    let mut tier1_5 = None;
    let mut tier2 = None;
    let mut tier3 = None;

    for (name, result) in results {
        match name.as_str() {
            "tier1" | "tier1_component" => tier1 = Some(result),
            "tier1_5" | "tier1_5_chain_client" => tier1_5 = Some(result),
            "tier2" | "tier2_integration" => tier2 = Some(result),
            "tier3" | "tier3_e2e" => tier3 = Some(result),
            _ => {}
        }
    }

    Ok(CrossTierResult {
        tier1,
        tier1_5,
        tier2,
        tier3,
        consistent,
        inconsistencies,
    })
}

/// Create a backend for the specified tier.
pub fn create_backend(tier: Tier) -> Box<dyn FixtureBackend> {
    match tier {
        Tier::Component => Box::new(super::backends::tier1::TestBlockchainBackend::new()),
        Tier::ChainClient => Box::new(super::backends::tier1_5::ChainClientBackend::new()),
        Tier::Integration => Box::new(super::backends::tier2::TestDaemonBackend::new()),
        Tier::E2E => Box::new(super::backends::tier3::LocalClusterBackend::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::backends::tier1::TestBlockchainBackend;

    const SIMPLE_FIXTURE: &str = r#"
fixture:
  name: "simple_transfer_test"
  tier: [1]

setup:
  accounts:
    alice:
      balance: "10000"
      nonce: 0
    bob:
      balance: "1000"
      nonce: 0

transactions:
  - step: 1
    name: "Alice sends to Bob"
    type: transfer
    from: alice
    to: bob
    amount: "2000"
    fee: "10"
    expect_status: success
  - step: 2
    name: "Mine block"
    type: mine_block
    expect_status: success

expected:
  accounts:
    alice:
      balance: "7990"
      nonce: 1
    bob:
      balance: "3000"
      nonce: 0
"#;

    #[tokio::test]
    async fn test_run_simple_fixture() {
        let mut backend = TestBlockchainBackend::new();
        let result = run_fixture_on_backend(SIMPLE_FIXTURE, &mut backend)
            .await
            .unwrap();

        assert!(
            result.success,
            "Fixture failed: verification_errors={:?}, invariant_errors={:?}",
            result.verification_errors, result.invariant_errors
        );
        assert_eq!(result.fixture_name, "simple_transfer_test");
    }

    #[tokio::test]
    async fn test_run_fixture_with_error_case() {
        let yaml = r#"
fixture:
  name: "overdraft_test"
  tier: [1]

setup:
  accounts:
    alice:
      balance: "100"
    bob:
      balance: "0"

transactions:
  - step: 1
    name: "Overdraft attempt"
    type: transfer
    from: alice
    to: bob
    amount: "999999"
    expect_status: error
    expect_error: INSUFFICIENT_BALANCE
"#;
        let mut backend = TestBlockchainBackend::new();
        let result = run_fixture_on_backend(yaml, &mut backend).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_create_backend_for_tiers() {
        let backend = create_backend(Tier::Component);
        assert_eq!(backend.tier_name(), "tier1_component");

        let backend = create_backend(Tier::ChainClient);
        assert_eq!(backend.tier_name(), "tier1_5_chain_client");

        let backend = create_backend(Tier::Integration);
        assert_eq!(backend.tier_name(), "tier2_integration");

        let backend = create_backend(Tier::E2E);
        assert_eq!(backend.tier_name(), "tier3_e2e");
    }

    #[tokio::test]
    async fn test_fixture_with_freeze() {
        let yaml = r#"
fixture:
  name: "freeze_test"
  tier: [1]

setup:
  accounts:
    alice:
      balance: "10000"

transactions:
  - step: 1
    name: "Freeze TOS"
    type: freeze
    from: alice
    amount: "1000"
    fee: "10"
    expect_status: success

expected:
  accounts:
    alice:
      balance: "8990"
      frozen_balance: "1000"
"#;
        let mut backend = TestBlockchainBackend::new();
        let result = run_fixture_on_backend(yaml, &mut backend).await.unwrap();
        assert!(
            result.success,
            "Fixture failed: {:?}",
            result.verification_errors
        );
    }
}
