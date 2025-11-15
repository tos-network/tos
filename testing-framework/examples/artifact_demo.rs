// Example: Artifact Collection Demo
//
// This example demonstrates how to use the artifact collection system
// to capture test failure state for debugging and reproduction.

use anyhow::Result;
use std::collections::HashMap;
use tos_testing_framework::utilities::artifacts::{
    BlockchainStateSnapshot, TopologySnapshot, TransactionRecord,
};
use tos_testing_framework::utilities::{load_artifact, print_artifact_summary, ArtifactCollector};

#[tokio::main]
async fn main() -> Result<()> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║            Artifact Collection System Demo                     ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    // Step 1: Create artifact collector
    println!("1. Creating artifact collector...");
    let mut collector = ArtifactCollector::new("example_consensus_failure");
    collector.set_rng_seed(0xa3f5c8e1b2d94706);

    // Step 2: Simulate test failure scenario
    println!("2. Simulating test failure scenario...");

    // Capture network topology
    let topology = TopologySnapshot {
        node_count: 3,
        connections: HashMap::from([(0, vec![1, 2]), (1, vec![0, 2]), (2, vec![0, 1])]),
        partitions: vec![],
    };
    collector.save_topology(topology);

    // Capture blockchain states
    for node_id in 0..3 {
        let state = BlockchainStateSnapshot {
            node_id,
            tip_height: 5 + node_id as u64,
            tip_hash: format!("0xabcd{:04x}", node_id),
            balances: HashMap::from([
                ("0xalice".to_string(), 1_000_000),
                ("0xbob".to_string(), 500_000),
            ]),
            nonces: HashMap::from([("0xalice".to_string(), 3), ("0xbob".to_string(), 1)]),
            total_supply: 1_500_000,
            fees_burned: 0,
        };
        collector.add_blockchain_state(state);
    }

    // Capture transaction history
    for i in 0..5 {
        let tx = TransactionRecord {
            hash: format!("0xtx{:04x}", i),
            sender: "0xalice".to_string(),
            recipient: "0xbob".to_string(),
            amount: 100_000,
            fee: 100,
            nonce: i + 1,
            block_height: Some(i as u64 + 1),
        };
        collector.add_transaction(tx);
    }

    // Capture logs
    collector.capture_log("INFO", "Network started with 3 nodes");
    collector.capture_log("INFO", "Mining block at height 5");
    collector.capture_log(
        "ERROR",
        "Height mismatch detected: node0=5, node1=6, node2=7",
    );
    collector.capture_log("ERROR", "Consensus failure: nodes diverged");

    // Set failure reason
    collector.set_failure_reason("Consensus failure: Nodes reached different heights after partition healing. Expected all nodes at height 5, but got [5, 6, 7].".to_string());

    // Step 3: Save artifact
    println!("3. Saving artifact...");
    let temp_dir = std::env::temp_dir().join("tos_artifacts");
    std::fs::create_dir_all(&temp_dir)?;
    let artifact_path = collector.save(&temp_dir).await?;
    println!("   ✅ Artifact saved to: {}\n", artifact_path.display());

    // Step 4: Load and display artifact
    println!("4. Loading and displaying artifact...\n");
    let loaded_artifact = load_artifact(&artifact_path).await?;
    print_artifact_summary(&loaded_artifact);

    println!("\n5. Artifact file contents:");
    println!(
        "   File size: {} bytes",
        std::fs::metadata(&artifact_path)?.len()
    );
    println!("   Format: JSON (human-readable)");
    println!("   Contains:");
    println!("     - Test metadata (name, seed, timestamp, duration)");
    println!("     - Network topology (3 nodes, full mesh)");
    println!("     - Blockchain states (3 node snapshots)");
    println!("     - Transaction history (5 transactions)");
    println!("     - Logs (4 entries)");

    println!("\n6. Cleanup:");
    std::fs::remove_file(&artifact_path)?;
    println!("   ✅ Artifact file removed\n");

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║                  Demo Complete                                 ║");
    println!("╠════════════════════════════════════════════════════════════════╣");
    println!("║ To use in your tests:                                          ║");
    println!("║   1. Create ArtifactCollector at test start                   ║");
    println!("║   2. Capture state on failure                                  ║");
    println!("║   3. Save artifact with collector.save()                       ║");
    println!("║   4. Use RNG seed to replay exact failure                      ║");
    println!("╚════════════════════════════════════════════════════════════════╝");

    Ok(())
}
