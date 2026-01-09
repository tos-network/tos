#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::useless_vec)]
// File: testing-framework/tests/artifact_collection_example.rs
//
// Failure Artifact Collection Examples
//
// This test demonstrates how to use the ArtifactCollector to capture
// comprehensive debugging information when tests fail, enabling easy
// reproduction and diagnosis of issues.

use anyhow::Result;
use std::collections::HashMap;
use tos_tck::orchestrator::rng::TestRng;
use tos_tck::utilities::artifacts::{
    ArtifactCollector, BlockchainStateSnapshot, Partition, TopologySnapshot, TransactionRecord,
};
use tos_tck::utilities::{load_artifact, print_artifact_summary, validate_artifact};

/// Example: Collect artifacts from a multi-node network failure
///
/// This demonstrates the recommended pattern for capturing failure state
/// in complex multi-node tests.
#[tokio::test]
async fn example_artifact_collection_multi_node() -> Result<()> {
    // Setup artifact collector with test name
    let mut collector = ArtifactCollector::new("test_network_partition_convergence");

    // Initialize test with seeded RNG for reproducibility
    let rng = TestRng::new_from_env_or_random();
    collector.set_rng_seed(rng.seed());

    // Simulate network topology (3 nodes in full mesh)
    let topology = TopologySnapshot {
        node_count: 3,
        connections: HashMap::from([(0, vec![1, 2]), (1, vec![0, 2]), (2, vec![0, 1])]),
        partitions: vec![Partition {
            group_a: vec![0, 1],
            group_b: vec![2],
        }],
    };
    collector.save_topology(topology);

    // Capture logs during test execution
    collector.capture_log("INFO", "Starting network partition test");
    collector.capture_log("INFO", "Creating partition: [0,1] vs [2]");

    // Simulate blockchain state for each node
    for node_id in 0..3 {
        let state = BlockchainStateSnapshot {
            node_id,
            tip_height: if node_id == 2 { 3 } else { 5 }, // Node 2 fell behind
            tip_hash: if node_id == 2 {
                "0xabcd1234".to_string()
            } else {
                "0xef567890".to_string()
            },
            balances: HashMap::from([
                ("alice".to_string(), 1_000_000),
                ("bob".to_string(), 500_000),
            ]),
            nonces: HashMap::from([("alice".to_string(), 3), ("bob".to_string(), 1)]),
            total_supply: 1_500_000,
            fees_burned: 150,
        };
        collector.add_blockchain_state(state);
    }

    // Capture transaction history
    for i in 1..=3 {
        let tx = TransactionRecord {
            hash: format!("0x{:064x}", i),
            sender: "alice".to_string(),
            recipient: "bob".to_string(),
            amount: 100_000,
            fee: 50,
            nonce: i,
            block_height: Some(i + 1),
        };
        collector.add_transaction(tx);
    }

    // Simulate test failure condition
    let failure_detected = true; // In real test, this would be actual failure detection

    if failure_detected {
        collector.capture_log(
            "ERROR",
            "Test failed: Node 2 height (3) differs from nodes 0,1 (5)",
        );
        collector.set_failure_reason(
            "Network partition healing failed: height mismatch after 10s timeout".to_string(),
        );

        // Save artifact to disk
        let artifact_path = collector.save("./target/test-artifacts/").await?;

        println!("\n=== Test Failed - Artifact Saved ===");
        println!("Artifact location: {}", artifact_path.display());
        println!("Reproduce with: TOS_TEST_SEED=0x{:016x} cargo test example_artifact_collection_multi_node", rng.seed());
        println!("=====================================\n");
    }

    Ok(())
}

/// Example: Load and inspect a saved artifact
///
/// This demonstrates how to load a previously saved artifact and
/// extract information for debugging or analysis.
#[tokio::test]
async fn example_load_and_inspect_artifact() -> Result<()> {
    // First, create an artifact
    let mut collector = ArtifactCollector::new("test_height_mismatch");
    collector.set_rng_seed(0x1234567890abcdef);

    // Add some test data
    collector.add_blockchain_state(BlockchainStateSnapshot {
        node_id: 0,
        tip_height: 100,
        tip_hash: "0xdeadbeef".to_string(),
        balances: HashMap::from([("test_account".to_string(), 999_500)]),
        nonces: HashMap::from([("test_account".to_string(), 10)]),
        total_supply: 1_000_000,
        fees_burned: 500,
    });

    collector.capture_log("ERROR", "Height validation failed");
    collector.set_failure_reason("Expected height 100, got 99".to_string());

    // Save artifact
    let artifact_path = collector.save("./target/test-artifacts/").await?;

    // Load it back
    let artifact = load_artifact(&artifact_path).await?;

    // Validate artifact structure
    validate_artifact(&artifact)?;

    // Print human-readable summary
    println!("\n=== Artifact Summary ===");
    print_artifact_summary(&artifact);
    println!("========================\n");

    // Access specific data
    assert_eq!(artifact.metadata.test_name, "test_height_mismatch");
    assert_eq!(artifact.metadata.rng_seed, Some(0x1234567890abcdef));
    assert_eq!(artifact.blockchain_states.len(), 1);
    assert_eq!(artifact.blockchain_states[0].tip_height, 100);

    println!("✅ Artifact loaded and validated successfully");

    Ok(())
}

/// Example: Minimal artifact collection for simple tests
///
/// This shows a lightweight pattern for simpler tests that don't need
/// full network topology tracking.
#[tokio::test]
async fn example_minimal_artifact_collection() -> Result<()> {
    let mut collector = ArtifactCollector::new("test_balance_calculation");

    let rng = TestRng::new_from_env_or_random();
    collector.set_rng_seed(rng.seed());

    // Capture just the essential state
    collector.add_blockchain_state(BlockchainStateSnapshot {
        node_id: 0,
        tip_height: 50,
        tip_hash: "0xcafebabe".to_string(),
        balances: HashMap::from([
            ("user1".to_string(), 500_000),
            ("user2".to_string(), 300_000),
        ]),
        nonces: HashMap::new(),
        total_supply: 800_000,
        fees_burned: 200,
    });

    // Capture key log entries
    collector.capture_log("INFO", "Testing balance transfer");
    collector.capture_log("ERROR", "Balance mismatch detected");

    // Set failure reason
    collector.set_failure_reason("Total supply mismatch: expected 1M, got 800K".to_string());

    // Save
    let path = collector.save("./target/test-artifacts/").await?;
    println!("Minimal artifact saved to: {}", path.display());

    Ok(())
}

/// Example: Capture transaction history for debugging
///
/// This demonstrates collecting detailed transaction data to debug
/// issues with transaction ordering or execution.
#[tokio::test]
async fn example_transaction_history_capture() -> Result<()> {
    let mut collector = ArtifactCollector::new("test_transaction_ordering");

    let rng = TestRng::new_from_env_or_random();
    collector.set_rng_seed(rng.seed());

    // Simulate a series of transactions
    let transactions = vec![
        ("0xa111", "alice", "bob", 1000, 1),
        ("0xa222", "alice", "charlie", 2000, 2),
        ("0xa333", "bob", "alice", 500, 1),
        ("0xa444", "charlie", "bob", 1500, 1),
    ];

    for (i, (hash, sender, recipient, amount, nonce)) in transactions.iter().enumerate() {
        collector.add_transaction(TransactionRecord {
            hash: hash.to_string(),
            sender: sender.to_string(),
            recipient: recipient.to_string(),
            amount: *amount,
            fee: 10,
            nonce: *nonce,
            block_height: Some((i / 2 + 1) as u64), // 2 txs per block
        });

        collector.capture_log(
            "DEBUG",
            format!(
                "Transaction {} from {} to {} for {} (nonce: {})",
                hash, sender, recipient, amount, nonce
            ),
        );
    }

    // Capture final state
    collector.add_blockchain_state(BlockchainStateSnapshot {
        node_id: 0,
        tip_height: 2,
        tip_hash: "0xtxhash".to_string(),
        balances: HashMap::from([
            ("alice".to_string(), 500),   // Started with ~3000, sent 3000
            ("bob".to_string(), 2000),    // Received 1000+1500, sent 500
            ("charlie".to_string(), 500), // Received 2000, sent 1500
        ]),
        nonces: HashMap::from([
            ("alice".to_string(), 2),
            ("bob".to_string(), 1),
            ("charlie".to_string(), 1),
        ]),
        total_supply: 3_000,
        fees_burned: 40, // 4 transactions × 10 fee
    });

    collector.set_failure_reason("Transaction ordering issue: nonce gap detected".to_string());

    let path = collector.save("./target/test-artifacts/").await?;
    println!("Transaction history artifact saved to: {}", path.display());

    Ok(())
}

/// Example: Capture network partition state
///
/// This demonstrates capturing topology information for debugging
/// partition and healing issues.
#[tokio::test]
async fn example_partition_state_capture() -> Result<()> {
    let mut collector = ArtifactCollector::new("test_partition_healing");

    let rng = TestRng::new_from_env_or_random();
    collector.set_rng_seed(rng.seed());

    // Capture topology with active partition
    let topology = TopologySnapshot {
        node_count: 5,
        connections: HashMap::from([
            (0, vec![1]),    // Group A
            (1, vec![0]),    // Group A
            (2, vec![3, 4]), // Group B
            (3, vec![2, 4]), // Group B
            (4, vec![2, 3]), // Group B
        ]),
        partitions: vec![Partition {
            group_a: vec![0, 1],
            group_b: vec![2, 3, 4],
        }],
    };
    collector.save_topology(topology);

    // Capture state for each partition
    collector.capture_log("INFO", "Partition created: [0,1] vs [2,3,4]");
    collector.capture_log("INFO", "Mining on both sides");

    // Group A state (2 nodes)
    for node_id in 0..2 {
        collector.add_blockchain_state(BlockchainStateSnapshot {
            node_id,
            tip_height: 10, // Group A mined to height 10
            tip_hash: format!("0xgroupA_{}", node_id),
            balances: HashMap::from([("alice".to_string(), 1_000_000)]),
            nonces: HashMap::from([("alice".to_string(), 5)]),
            total_supply: 1_000_000,
            fees_burned: 50,
        });
    }

    // Group B state (3 nodes)
    for node_id in 2..5 {
        collector.add_blockchain_state(BlockchainStateSnapshot {
            node_id,
            tip_height: 15, // Group B mined to height 15 (longer chain)
            tip_hash: format!("0xgroupB_{}", node_id),
            balances: HashMap::from([("bob".to_string(), 2_000_000)]),
            nonces: HashMap::from([("bob".to_string(), 8)]),
            total_supply: 2_000_000,
            fees_burned: 80,
        });
    }

    collector.capture_log("WARN", "Healing partition...");
    collector.capture_log(
        "ERROR",
        "Convergence failed: Group A did not reorg to Group B chain",
    );

    collector.set_failure_reason(
        "Expected all nodes to converge to height 15, but nodes 0,1 remained at height 10"
            .to_string(),
    );

    let path = collector.save("./target/test-artifacts/").await?;
    println!("Partition state artifact saved to: {}", path.display());

    Ok(())
}

#[cfg(test)]
mod replay_tests {
    use super::*;

    /// Example: Replay a test with the same RNG seed
    ///
    /// This demonstrates how to use the seed from an artifact to
    /// reproduce a test failure exactly.
    #[tokio::test]
    async fn example_replay_from_artifact() -> Result<()> {
        // In real usage, you would:
        // 1. Load artifact from disk: let artifact = load_artifact(path).await?;
        // 2. Extract seed: let seed = artifact.metadata.rng_seed.unwrap();
        // 3. Use seed: let rng = TestRng::with_seed(seed);

        // For this example, we'll use a known seed
        let seed = 0xdeadbeefcafebabe;
        let rng = TestRng::with_seed(seed);

        println!("Replaying test with seed: 0x{:016x}", seed);

        // Now run the same test logic with the same seed
        // The RNG will produce identical random values, making the test deterministic

        // Generate two random values with the same RNG instance
        let value1 = rng.gen::<u64>();
        let value2 = rng.gen::<u64>();

        println!("First random value: {}", value1);
        println!("Second random value: {}", value2);

        // With the same seed, these values will always be the same
        // To truly verify determinism, you would run the test twice with the same seed
        // and compare results

        // Create another RNG with the same seed
        let rng2 = TestRng::with_seed(seed);
        assert_eq!(
            value1,
            rng2.gen::<u64>(),
            "Same seed should produce same first value"
        );

        Ok(())
    }
}
