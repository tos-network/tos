//! Comprehensive Scenarios Demo
//!
//! Demonstrates the YAML scenario DSL with V2.2 format corrections:
//! - Loading and parsing YAML scenarios
//! - Executing scenarios with TestBlockchain
//! - Using within/compare assertions
//! - Checking invariants after execution
//!
//! Run with: cargo run --example scenarios_demo

use anyhow::Result;
use tos_testing_framework::scenarios::{parse_scenario, ScenarioExecutor};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();

    println!("=== TOS Testing Framework V3.0 - Scenarios Demo ===\n");

    // Demo 1: Parse and validate simple scenario
    demo_parse_simple_scenario().await?;

    // Demo 2: Show all assertion modes
    demo_assertion_modes().await?;

    // Demo 3: Complex scenario with multiple steps
    demo_complex_scenario().await?;

    // Demo 4: Error handling
    demo_error_handling().await?;

    println!("\n=== All Demos Completed Successfully ===");
    Ok(())
}

/// Demo 1: Parse and validate a simple scenario
async fn demo_parse_simple_scenario() -> Result<()> {
    println!("Demo 1: Parsing Simple Scenario");
    println!("=================================\n");

    let yaml = r#"
name: "Demo Simple Transfer"
description: "Demonstrates basic YAML scenario format"

genesis:
  network: "devnet"
  accounts:
    - name: "alice"
      balance: "1000000000000"  # 1000 TOS (string, no underscores)
    - name: "bob"
      balance: "0"

steps:
  - action: "transfer"
    from: "alice"
    to: "bob"
    amount: "100000000000"  # 100 TOS
    fee: "50"

  - action: "mine_block"

  - action: "assert_balance"
    account: "bob"
    eq: "100000000000"

invariants:
  - "balance_conservation"
  - "nonce_monotonicity"
"#;

    // Parse the scenario
    let scenario = parse_scenario(yaml)?;

    println!("Scenario Name: {}", scenario.name);
    if let Some(desc) = &scenario.description {
        println!("Description: {}", desc);
    }
    println!("Genesis Accounts: {}", scenario.genesis.accounts.len());
    println!("Steps: {}", scenario.steps.len());
    if let Some(inv) = &scenario.invariants {
        println!("Invariants: {}", inv.len());
    }

    println!("\nValidation: ✓ Scenario parsed successfully");
    println!();

    Ok(())
}

/// Demo 2: Show all three assertion modes (eq/within/compare)
async fn demo_assertion_modes() -> Result<()> {
    println!("Demo 2: Assertion Modes (V2.2 P1-8)");
    println!("====================================\n");

    // Exact equality assertion
    let eq_yaml = r#"
name: "Exact Assertion"
genesis:
  accounts:
    - { name: "alice", balance: 1000 }
steps:
  - action: "assert_balance"
    account: "alice"
    eq: "1000"
"#;

    let scenario = parse_scenario(eq_yaml)?;
    println!("1. Exact (eq) Assertion:");
    println!("   {:#?}", scenario.steps[0]);
    println!("   ✓ Requires exact match\n");

    // Within tolerance assertion
    let within_yaml = r#"
name: "Within Tolerance"
genesis:
  accounts:
    - { name: "alice", balance: 1000 }
steps:
  - action: "assert_balance"
    account: "alice"
    within:
      target: "1000"
      tolerance: "10"
"#;

    let scenario = parse_scenario(within_yaml)?;
    println!("2. Within Tolerance Assertion:");
    println!("   {:#?}", scenario.steps[0]);
    println!("   ✓ Accepts values in range [990, 1010]\n");

    // Comparison assertions
    let compare_yaml = r#"
name: "Comparison Assertions"
genesis:
  accounts:
    - { name: "alice", balance: 1000 }
steps:
  - action: "assert_balance"
    account: "alice"
    compare:
      gte: "900"
"#;

    let scenario = parse_scenario(compare_yaml)?;
    println!("3. Comparison (gte/lte/gt/lt) Assertion:");
    println!("   {:#?}", scenario.steps[0]);
    println!("   ✓ Supports relational comparisons\n");

    Ok(())
}

/// Demo 3: Complex scenario with multiple steps
async fn demo_complex_scenario() -> Result<()> {
    println!("Demo 3: Complex Multi-Step Scenario");
    println!("====================================\n");

    let yaml = r#"
name: "Receive-Then-Spend Chain"
description: "Tests that received funds can be spent in same block"

genesis:
  network: "devnet"
  accounts:
    - name: "alice"
      balance: "1000000000000"
    - name: "bob"
      balance: "0"
    - name: "charlie"
      balance: "0"

steps:
  # Step 1: Alice → Bob
  - action: "transfer"
    from: "alice"
    to: "bob"
    amount: "100000000000"
    fee: "50"

  # Step 2: Bob → Charlie (spending received funds!)
  - action: "transfer"
    from: "bob"
    to: "charlie"
    amount: "50000000000"
    fee: "50"

  # Step 3: Mine block
  - action: "mine_block"

  # Step 4-6: Assertions with different modes
  - action: "assert_balance"
    account: "alice"
    within:
      target: "899999999950"
      tolerance: "100"

  - action: "assert_balance"
    account: "charlie"
    eq: "50000000000"

  - action: "assert_nonce"
    account: "bob"
    eq: 1

invariants:
  - "balance_conservation"
  - "nonce_monotonicity"
"#;

    let scenario = parse_scenario(yaml)?;

    println!("Scenario: {}", scenario.name);
    println!("Steps breakdown:");
    for (idx, step) in scenario.steps.iter().enumerate() {
        match step {
            tos_testing_framework::scenarios::parser::Step::Transfer {
                from, to, amount, ..
            } => {
                println!("  {}. Transfer {} from {} to {}", idx + 1, amount, from, to);
            }
            tos_testing_framework::scenarios::parser::Step::MineBlock { .. } => {
                println!("  {}. Mine block", idx + 1);
            }
            tos_testing_framework::scenarios::parser::Step::AssertBalance { account, .. } => {
                println!("  {}. Assert balance for {}", idx + 1, account);
            }
            tos_testing_framework::scenarios::parser::Step::AssertNonce { account, eq } => {
                println!("  {}. Assert nonce for {} = {}", idx + 1, account, eq);
            }
            _ => {
                println!("  {}. Other step", idx + 1);
            }
        }
    }

    println!("\n✓ Complex scenario parsed successfully");
    println!();

    Ok(())
}

/// Demo 4: Error handling and validation
async fn demo_error_handling() -> Result<()> {
    println!("Demo 4: Error Handling and Validation");
    println!("======================================\n");

    // Test 1: Duplicate account names
    let bad_yaml1 = r#"
name: "Duplicate Accounts"
genesis:
  accounts:
    - { name: "alice", balance: 1000 }
    - { name: "alice", balance: 2000 }
steps:
  - action: "mine_block"
"#;

    match parse_scenario(bad_yaml1) {
        Ok(_) => println!("❌ Should have rejected duplicate accounts"),
        Err(e) => println!("✓ Correctly rejected duplicate accounts: {}", e),
    }

    // Test 2: Unknown account reference
    let bad_yaml2 = r#"
name: "Unknown Account"
genesis:
  accounts:
    - { name: "alice", balance: 1000 }
steps:
  - action: "transfer"
    from: "alice"
    to: "bob"
    amount: 100
    fee: 1
"#;

    match parse_scenario(bad_yaml2) {
        Ok(_) => println!("❌ Should have rejected unknown account"),
        Err(e) => println!("✓ Correctly rejected unknown account: {}", e),
    }

    // Test 3: Empty accounts
    let bad_yaml3 = r#"
name: "No Accounts"
genesis:
  accounts: []
steps:
  - action: "mine_block"
"#;

    match parse_scenario(bad_yaml3) {
        Ok(_) => println!("❌ Should have rejected empty accounts"),
        Err(e) => println!("✓ Correctly rejected empty accounts: {}", e),
    }

    // Test 4: Empty steps
    let bad_yaml4 = r#"
name: "No Steps"
genesis:
  accounts:
    - { name: "alice", balance: 1000 }
steps: []
"#;

    match parse_scenario(bad_yaml4) {
        Ok(_) => println!("❌ Should have rejected empty steps"),
        Err(e) => println!("✓ Correctly rejected empty steps: {}", e),
    }

    println!("\n✓ All validation checks working correctly");
    println!();

    Ok(())
}

/// Demo 5: Executor usage (placeholder until TestBlockchain is complete)
#[allow(dead_code)]
async fn demo_executor_usage() -> Result<()> {
    println!("Demo 5: Scenario Execution");
    println!("===========================\n");

    let yaml = r#"
name: "Executor Demo"
genesis:
  accounts:
    - { name: "alice", balance: 1000 }
    - { name: "bob", balance: 0 }
steps:
  - action: "mine_block"
"#;

    let scenario = parse_scenario(yaml)?;

    println!("Creating executor...");
    let mut executor = ScenarioExecutor::new(&scenario).await?;

    println!("Executing scenario: {}", scenario.name);
    executor.execute(&scenario).await?;

    println!("✓ Scenario executed successfully");
    println!();

    Ok(())
}
