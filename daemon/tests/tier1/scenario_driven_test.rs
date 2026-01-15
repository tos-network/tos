//! Scenario-Driven Tests - Tier 1 Component Test
//!
//! Tests using YAML scenario files

use anyhow::Result;
use tos_testing_framework::scenarios::{parse_scenario, ScenarioExecutor};

/// Load and execute simple_transfer.yaml scenario
#[tokio::test]
async fn test_scenario_simple_transfer() -> Result<()> {
    let yaml = include_str!("../scenarios/simple_transfer.yaml");
    let scenario = parse_scenario(yaml)?;

    let mut executor = ScenarioExecutor::new(&scenario).await?;
    executor.execute(&scenario).await?;

    Ok(())
}

/// Load and execute receive_then_spend.yaml scenario
#[tokio::test]
async fn test_scenario_receive_then_spend() -> Result<()> {
    let yaml = include_str!("../scenarios/receive_then_spend.yaml");
    let scenario = parse_scenario(yaml)?;

    let mut executor = ScenarioExecutor::new(&scenario).await?;
    executor.execute(&scenario).await?;

    Ok(())
}

/// Load and execute parallel_transfers.yaml scenario
#[tokio::test]
async fn test_scenario_parallel_transfers() -> Result<()> {
    let yaml = include_str!("../scenarios/parallel_transfers.yaml");
    let scenario = parse_scenario(yaml)?;

    let mut executor = ScenarioExecutor::new(&scenario).await?;
    executor.execute(&scenario).await?;

    Ok(())
}

/// Test parser handles malformed YAML gracefully
#[tokio::test]
async fn test_scenario_parser_error_handling() -> Result<()> {
    let invalid_yaml = r#"
name: "Bad Scenario"
genesis:
  accounts: []  # Empty accounts should fail validation
steps: []  # Empty steps should fail validation
"#;

    let result = parse_scenario(invalid_yaml);
    assert!(result.is_err());

    Ok(())
}

/// Test parser validates account references
#[tokio::test]
async fn test_scenario_unknown_account_error() -> Result<()> {
    let yaml = r#"
name: "Unknown Account"
genesis:
  accounts:
    - name: "alice"
      balance: 1000
steps:
  - action: "transfer"
    from: "alice"
    to: "bob"  # Bob doesn't exist!
    amount: 100
    fee: 1
"#;

    let result = parse_scenario(yaml);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("unknown account"));

    Ok(())
}
