// File: testing-framework/examples/waiters_example.rs
//
// Comprehensive example demonstrating waiter primitives for deterministic testing.
//
// This example shows how to use wait_for_block, wait_for_tx, and wait_all_tips_equal
// instead of sleep-based timing in blockchain tests.
//
// Run this example with:
//   cargo run --example waiters_example

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
#![allow(unused)]
#![allow(clippy::useless_vec)]

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

// Import the testing framework waiter primitives
use tos_testing_framework::tier2_integration::waiters::wait_for_block;
use tos_testing_framework::tier2_integration::{Hash, NodeRpc};
use tos_testing_framework::tier3_e2e::waiters::{wait_all_heights_equal, wait_all_tips_equal};

// ============================================================================
// Mock Node Implementation for Demonstration
// ============================================================================

/// A mock node that simulates blockchain progression for demonstration purposes.
///
/// In real tests, this would be replaced with TestDaemon or actual TOS node RPC client.
struct MockNode {
    id: usize,
    height: Arc<Mutex<u64>>,
    tips: Arc<Mutex<Vec<Hash>>>,
}

impl MockNode {
    fn new(id: usize, initial_height: u64) -> Self {
        Self {
            id,
            height: Arc::new(Mutex::new(initial_height)),
            tips: Arc::new(Mutex::new(vec![Hash::new([id as u8; 32])])),
        }
    }

    /// Simulate mining a block (advances height)
    async fn mine_block(&self) {
        let mut height = self.height.lock().await;
        *height += 1;
        println!("  Node {} mined block at height {}", self.id, *height);
    }

    /// Set tips explicitly
    async fn set_tips(&self, new_tips: Vec<Hash>) {
        *self.tips.lock().await = new_tips;
    }
}

#[async_trait]
impl NodeRpc for MockNode {
    async fn get_tip_height(&self) -> Result<u64> {
        Ok(*self.height.lock().await)
    }

    async fn get_tips(&self) -> Result<Vec<Hash>> {
        Ok(self.tips.lock().await.clone())
    }

    async fn get_balance(&self, _address: &Hash) -> Result<u64> {
        Ok(1_000_000)
    }

    async fn get_nonce(&self, _address: &Hash) -> Result<u64> {
        Ok(0)
    }
}

// Implement NodeRpc for &MockNode to allow passing &[&MockNode]
#[async_trait]
impl NodeRpc for &MockNode {
    async fn get_tip_height(&self) -> Result<u64> {
        (*self).get_tip_height().await
    }

    async fn get_tips(&self) -> Result<Vec<Hash>> {
        (*self).get_tips().await
    }

    async fn get_balance(&self, address: &Hash) -> Result<u64> {
        (*self).get_balance(address).await
    }

    async fn get_nonce(&self, address: &Hash) -> Result<u64> {
        (*self).get_nonce(address).await
    }
}

// ============================================================================
// Example 1: wait_for_block - Deterministic Height Waiting
// ============================================================================

async fn example_wait_for_block() -> Result<()> {
    println!("\n=== Example 1: wait_for_block (Tier 2) ===\n");

    let node = Arc::new(MockNode::new(0, 50));

    println!("Initial state:");
    println!("  Node 0 height: {}", node.get_tip_height().await?);

    // Spawn a background task to mine blocks after a delay
    let node_clone = node.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        println!("\nBackground mining started...");
        for _ in 0..60 {
            node_clone.mine_block().await;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });

    println!("\n❌ OLD WAY (non-deterministic):");
    println!("  tokio::time::sleep(Duration::from_secs(5)).await;  // How long is enough?");
    println!("  assert!(node.get_tip_height().await? >= 100);");

    println!("\n✅ NEW WAY (deterministic with wait_for_block):");
    println!("  wait_for_block(&node, 100, Duration::from_secs(10)).await?;");

    // Use wait_for_block instead of sleep
    wait_for_block(&*node, 100, Duration::from_secs(10)).await?;

    let final_height = node.get_tip_height().await?;
    println!(
        "\n✓ Success! Node reached height {} (target was 100)",
        final_height
    );
    println!("  Test waited exactly as long as needed, no more, no less.");

    Ok(())
}

// ============================================================================
// Example 2: wait_all_tips_equal - Multi-Node Consensus
// ============================================================================

async fn example_wait_all_tips_equal() -> Result<()> {
    println!("\n=== Example 2: wait_all_tips_equal (Tier 3) ===\n");

    // Create a 5-node network
    let node0 = Arc::new(MockNode::new(0, 100));
    let node1 = Arc::new(MockNode::new(1, 100));
    let node2 = Arc::new(MockNode::new(2, 100));
    let node3 = Arc::new(MockNode::new(3, 100));
    let node4 = Arc::new(MockNode::new(4, 100));

    println!("Initial state: 5 nodes with different tip sets");
    println!("  Node 0 tips: {:?}", &node0.get_tips().await?[0]);
    println!("  Node 1 tips: {:?}", &node1.get_tips().await?[0]);
    println!("  Node 2 tips: {:?}", &node2.get_tips().await?[0]);
    println!("  Node 3 tips: {:?}", &node3.get_tips().await?[0]);
    println!("  Node 4 tips: {:?}", &node4.get_tips().await?[0]);

    // Simulate network partition healing and consensus convergence
    let nodes = vec![
        node0.clone(),
        node1.clone(),
        node2.clone(),
        node3.clone(),
        node4.clone(),
    ];

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        println!("\nSimulating partition healing and consensus convergence...");

        // All nodes converge to the same tips
        let common_tips = vec![Hash::new([99u8; 32]), Hash::new([100u8; 32])];
        for (i, node) in nodes.iter().enumerate() {
            tokio::time::sleep(Duration::from_millis(100)).await;
            node.set_tips(common_tips.clone()).await;
            println!("  Node {} converged to common tips", i);
        }
    });

    println!("\n❌ OLD WAY (non-deterministic):");
    println!("  tokio::time::sleep(Duration::from_secs(10)).await;  // Hope consensus completes");
    println!("  assert_eq!(node0.get_tips().await?, node4.get_tips().await?);");

    println!("\n✅ NEW WAY (deterministic with wait_all_tips_equal):");
    println!("  wait_all_tips_equal(&nodes, Duration::from_secs(10)).await?;");

    // Use wait_all_tips_equal instead of sleep
    let nodes_ref: Vec<&MockNode> = vec![&*node0, &*node1, &*node2, &*node3, &*node4];
    wait_all_tips_equal(&nodes_ref[..], Duration::from_secs(10)).await?;

    println!("\n✓ Success! All nodes converged to the same tip set");
    println!("  Nodes can now be safely compared - consensus is guaranteed.");

    Ok(())
}

// ============================================================================
// Example 3: wait_all_heights_equal - Simpler Height Convergence
// ============================================================================

async fn example_wait_all_heights_equal() -> Result<()> {
    println!("\n=== Example 3: wait_all_heights_equal (Tier 3) ===\n");

    let node0 = Arc::new(MockNode::new(0, 100));
    let node1 = Arc::new(MockNode::new(1, 95));
    let node2 = Arc::new(MockNode::new(2, 90));

    println!("Initial state: Nodes at different heights");
    println!("  Node 0 height: {}", node0.get_tip_height().await?);
    println!("  Node 1 height: {}", node1.get_tip_height().await?);
    println!("  Node 2 height: {}", node2.get_tip_height().await?);

    // Simulate slower nodes catching up
    let nodes = vec![node0.clone(), node1.clone(), node2.clone()];

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        println!("\nNodes catching up...");

        // Node 1 catches up
        for _ in 95..100 {
            nodes[1].mine_block().await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // Node 2 catches up
        for _ in 90..100 {
            nodes[2].mine_block().await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    println!("\n✅ Using wait_all_heights_equal:");
    let nodes_ref: Vec<&MockNode> = vec![&*node0, &*node1, &*node2];
    wait_all_heights_equal(&nodes_ref[..], Duration::from_secs(5)).await?;

    println!("\n✓ Success! All nodes at height 100");
    println!(
        "  Node 0: {}, Node 1: {}, Node 2: {}",
        node0.get_tip_height().await?,
        node1.get_tip_height().await?,
        node2.get_tip_height().await?
    );

    Ok(())
}

// ============================================================================
// Example 4: Real-World Test Pattern - Network Partition & Recovery
// ============================================================================

async fn example_network_partition_recovery() -> Result<()> {
    println!("\n=== Example 4: Network Partition & Recovery Pattern ===\n");

    // Setup: 5-node network
    let nodes: Vec<Arc<MockNode>> = (0..5).map(|i| Arc::new(MockNode::new(i, 100))).collect();

    println!("Setup: 5-node network at height 100");

    // Simulate network partition: [0,1,2] vs [3,4]
    println!("\n1. Creating network partition: [0,1,2] vs [3,4]");

    let group_a = nodes[0..3].to_vec();
    let group_b = nodes[3..5].to_vec();

    // Each partition mines independently
    tokio::spawn({
        let group_a = group_a.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            for _ in 0..5 {
                for node in &group_a {
                    node.mine_block().await;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            println!("  Group A [0,1,2] mined 5 blocks independently");
        }
    });

    tokio::spawn({
        let group_b = group_b.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            for _ in 0..3 {
                for node in &group_b {
                    node.mine_block().await;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            println!("  Group B [3,4] mined 3 blocks independently");
        }
    });

    // Wait for both partitions to finish mining
    tokio::time::sleep(Duration::from_millis(500)).await;

    println!("\n2. Healing network partition...");

    // Simulate partition healing and consensus convergence
    tokio::spawn({
        let nodes = nodes.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(200)).await;

            // All nodes converge to group A's chain (higher blue work)
            let common_tips = vec![Hash::new([0xAA; 32])];
            for node in &nodes {
                node.set_tips(common_tips.clone()).await;
            }
            println!("  All nodes synchronized");
        }
    });

    println!("\n3. ✅ Waiting for consensus convergence (deterministic):");
    let nodes_ref: Vec<&MockNode> = nodes.iter().map(|n| &**n).collect();
    wait_all_tips_equal(&nodes_ref[..], Duration::from_secs(5)).await?;

    println!("\n✓ Network recovered! All nodes converged");
    println!("  Now safe to verify BlockDAG invariants and state consistency");

    Ok(())
}

// ============================================================================
// Main - Run all examples
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║  TOS Testing Framework V3.0 - Waiter Primitives Examples  ║");
    println!("╚════════════════════════════════════════════════════════════╝");

    example_wait_for_block().await?;
    example_wait_all_tips_equal().await?;
    example_wait_all_heights_equal().await?;
    example_network_partition_recovery().await?;

    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║  Key Takeaways                                             ║");
    println!("╠════════════════════════════════════════════════════════════╣");
    println!("║  1. NEVER use sleep() in tests - use wait_for_* instead   ║");
    println!("║  2. Tests run faster (wait exactly as long as needed)      ║");
    println!("║  3. Tests are deterministic (no flakiness from timing)     ║");
    println!("║  4. Tests are more readable (intent is clear)              ║");
    println!("╚════════════════════════════════════════════════════════════╝\n");

    Ok(())
}
