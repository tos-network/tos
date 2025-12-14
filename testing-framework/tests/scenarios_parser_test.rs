#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::useless_vec)]
//! Standalone test for scenarios parser
//!
//! Tests parser functionality independently of TestBlockchain

use tos_testing_framework::scenarios::parse_scenario;

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
fn test_parse_all_scenarios() {
    // Test all 6 scenario files
    let scenarios = vec![
        include_str!("../../daemon/tests/scenarios/simple_transfer.yaml"),
        include_str!("../../daemon/tests/scenarios/receive_then_spend.yaml"),
        include_str!("../../daemon/tests/scenarios/miner_reward_spend.yaml"),
        include_str!("../../daemon/tests/scenarios/parallel_transfers.yaml"),
        include_str!("../../daemon/tests/scenarios/nonce_conflict.yaml"),
        include_str!("../../daemon/tests/scenarios/multi_block_sequence.yaml"),
    ];

    for (idx, yaml) in scenarios.iter().enumerate() {
        match parse_scenario(yaml) {
            Ok(scenario) => {
                println!(
                    "✓ Scenario {}: {} parsed successfully",
                    idx + 1,
                    scenario.name
                );
            }
            Err(e) => {
                panic!("Failed to parse scenario {}: {}", idx + 1, e);
            }
        }
    }
}

#[test]
fn test_validation_errors() {
    // Duplicate accounts
    let bad_yaml1 = r#"
name: "Bad"
genesis:
  accounts:
    - { name: "alice", balance: 1000 }
    - { name: "alice", balance: 2000 }
steps:
  - action: "mine_block"
"#;
    assert!(parse_scenario(bad_yaml1).is_err());

    // Unknown 'from' account (to accounts are auto-created)
    let bad_yaml2 = r#"
name: "Bad"
genesis:
  accounts:
    - { name: "alice", balance: 1000 }
steps:
  - action: "transfer"
    from: "charlie"
    to: "bob"
    amount: 100
    fee: 1
"#;
    assert!(parse_scenario(bad_yaml2).is_err());

    // Empty accounts
    let bad_yaml3 = r#"
name: "Bad"
genesis:
  accounts: []
steps:
  - action: "mine_block"
"#;
    assert!(parse_scenario(bad_yaml3).is_err());

    // Empty steps
    let bad_yaml4 = r#"
name: "Bad"
genesis:
  accounts:
    - { name: "alice", balance: 1000 }
steps: []
"#;
    assert!(parse_scenario(bad_yaml4).is_err());
}
