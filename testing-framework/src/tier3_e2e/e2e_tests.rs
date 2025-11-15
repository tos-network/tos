//! End-to-End Multi-Node Consensus Tests
//!
//! This module contains comprehensive E2E tests for multi-node TOS blockchain
//! scenarios, including consensus convergence, network partitions, fork resolution,
//! and transaction propagation.

use crate::orchestrator::Clock;
use crate::tier1_component::TestTransaction;
use crate::tier2_integration::rpc_helpers::*;
use crate::tier3_e2e::network::{LocalTosNetworkBuilder, NetworkTopology};
use anyhow::Result;
use tokio::time::Duration;
use tos_common::crypto::Hash;

/// Helper to create deterministic test address from seed
fn create_test_address(seed: u8) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    Hash::new(bytes)
}

/// Helper to create a test transaction with proper hashing
fn create_test_tx(
    sender: Hash,
    recipient: Hash,
    amount: u64,
    fee: u64,
    nonce: u64,
) -> TestTransaction {
    // Create a deterministic hash based on tx parameters
    let mut hash_bytes = [0u8; 32];
    hash_bytes[0..8].copy_from_slice(&amount.to_le_bytes());
    hash_bytes[8..16].copy_from_slice(&fee.to_le_bytes());
    hash_bytes[16..24].copy_from_slice(&nonce.to_le_bytes());
    let hash = Hash::new(hash_bytes);

    TestTransaction {
        hash,
        sender,
        recipient,
        amount,
        fee,
        nonce,
    }
}

/// Test 1: Multi-Node Consensus Convergence
///
/// Scenario:
/// - Create 3-node network
/// - Submit transaction on node 0
/// - Mine block on node 1
/// - Verify all nodes converge to same height and tips
#[tokio::test]
async fn test_multi_node_consensus_convergence() -> Result<()> {
    // Create 3-node network with alice account
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(3)
        .with_genesis_account("alice", 1_000_000_000_000) // 1000 TOS
        .with_seed(42)
        .build()
        .await?;

    let alice = network.get_genesis_account("alice").unwrap().0.clone();
    let bob = create_test_address(10);

    // All nodes should start at height 0
    assert_tip_height(&network.node(0), 0).await?;
    assert_tip_height(&network.node(1), 0).await?;
    assert_tip_height(&network.node(2), 0).await?;

    // Submit transaction on node 0 and propagate to all connected peers
    let tx = create_test_tx(alice.clone(), bob.clone(), 100_000_000_000, 100, 1);
    network.submit_and_propagate(0, tx).await?;

    // Mine block on node 1 and propagate
    // Node 1 received the transaction via propagation, mines it, and shares the block
    network.mine_and_propagate(1).await?;

    // ✅ Full consensus convergence! All nodes now at height 1
    assert_tip_height(&network.node(0), 1).await?;
    assert_tip_height(&network.node(1), 1).await?;
    assert_tip_height(&network.node(2), 1).await?;

    // Verify bob received funds on all nodes
    assert_balance(&network.node(0), &bob, 100_000_000_000).await?;
    assert_balance(&network.node(1), &bob, 100_000_000_000).await?;
    assert_balance(&network.node(2), &bob, 100_000_000_000).await?;

    Ok(())
}

/// Test 2: Network Partition and Healing
///
/// Scenario:
/// - Create 4-node network
/// - Partition into two groups: [0,1] and [2,3]
/// - Mine blocks on both sides of partition
/// - Heal network and verify convergence
#[tokio::test]
async fn test_network_partition_and_healing() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(4)
        .with_genesis_account("miner_a", 1_000_000)
        .with_genesis_account("miner_b", 1_000_000)
        .with_seed(100)
        .build()
        .await?;

    // Initially all nodes at height 0
    for i in 0..4 {
        assert_tip_height(&network.node(i), 0).await?;
    }

    // Partition network: [0,1] vs [2,3]
    network.partition_groups(&[0, 1], &[2, 3]).await?;

    // Mine block on partition A (node 0)
    network.node(0).daemon().mine_block().await?;
    assert_tip_height(&network.node(0), 1).await?;

    // Mine block on partition B (node 2)
    network.node(2).daemon().mine_block().await?;
    assert_tip_height(&network.node(2), 1).await?;

    // Verify partition is active: nodes 0 and 2 should have different tips
    // (This will work once block propagation is implemented)

    // Heal network
    network.heal_all_partitions().await;

    // TODO: Once block propagation is implemented, verify convergence:
    // network.wait_for_convergence(Duration::from_secs(10)).await?;
    // All nodes should converge to same height (likely 2 - both chains merged)

    Ok(())
}

/// Test 3: Concurrent Block Mining
///
/// Scenario:
/// - Create 5-node network
/// - Multiple nodes mine blocks simultaneously
/// - Verify network handles concurrent block production
#[tokio::test]
async fn test_concurrent_block_mining() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(3)
        .with_genesis_account("miner_a", 10_000_000_000_000)
        .with_genesis_account("miner_b", 10_000_000_000_000)
        .with_genesis_account("miner_c", 10_000_000_000_000)
        .with_seed(200)
        .build()
        .await?;

    let miner_a = network.get_genesis_account("miner_a").unwrap().0.clone();
    let miner_b = network.get_genesis_account("miner_b").unwrap().0.clone();
    let miner_c = network.get_genesis_account("miner_c").unwrap().0.clone();

    // Each miner submits a transaction on their respective node
    let tx_a = create_test_tx(miner_a, create_test_address(20), 1_000_000_000, 100, 1);
    let tx_b = create_test_tx(miner_b, create_test_address(21), 1_000_000_000, 100, 1);
    let tx_c = create_test_tx(miner_c, create_test_address(22), 1_000_000_000, 100, 1);

    network.node(0).daemon().submit_transaction(tx_a).await?;
    network.node(1).daemon().submit_transaction(tx_b).await?;
    network.node(2).daemon().submit_transaction(tx_c).await?;

    // Mine blocks concurrently on all 3 nodes
    network.node(0).daemon().mine_block().await?;
    network.node(1).daemon().mine_block().await?;
    network.node(2).daemon().mine_block().await?;

    // Each node should have mined a block
    assert_tip_height(&network.node(0), 1).await?;
    assert_tip_height(&network.node(1), 1).await?;
    assert_tip_height(&network.node(2), 1).await?;

    // TODO: Once block propagation is implemented:
    // - Verify all nodes converge to consistent DAG state
    // - Check that blocks form proper parent-child relationships
    // - Verify total supply conservation across all blocks

    Ok(())
}

/// Test 4: Fork Resolution
///
/// Scenario:
/// - Create 3-node network
/// - Create competing chains by mining on different nodes
/// - Verify longest/heaviest chain wins
#[tokio::test]
async fn test_fork_resolution() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(3)
        .with_genesis_account("alice", 5_000_000_000_000) // 5,000 TOS
        .with_seed(300)
        .build()
        .await?;

    let alice = network.get_genesis_account("alice").unwrap().0.clone();
    let bob = create_test_address(30);

    // Create shorter chain on node 0: mine 1 block
    let tx1 = create_test_tx(alice.clone(), bob.clone(), 100_000_000_000, 100, 1);
    network.node(0).daemon().submit_transaction(tx1).await?;
    network.node(0).daemon().mine_block().await?;

    // Advance time slightly
    network.advance_time(Duration::from_secs(1)).await;

    // Create longer chain on node 1: mine 2 blocks
    let tx2 = create_test_tx(alice.clone(), bob.clone(), 200_000_000_000, 100, 1);
    network.node(1).daemon().submit_transaction(tx2).await?;
    network.node(1).daemon().mine_block().await?;

    // Advance time
    network.advance_time(Duration::from_secs(1)).await;

    // Mine second block on node 1
    network.node(1).daemon().mine_block().await?;

    // Node 1 should be at height 2
    assert_tip_height(&network.node(1), 2).await?;

    // TODO: Once block propagation is implemented:
    // - Propagate blocks between nodes
    // - Verify all nodes converge to longer chain (height 2)
    // - Verify bob's balance matches the winning transaction (200 TOS)
    // - Verify node 0's chain was reorganized

    Ok(())
}

/// Test 5: Cross-Node Transaction Propagation
///
/// Scenario:
/// - Create 3-node ring topology
/// - Submit transaction on node 0
/// - Mine on node 1
/// - Verify transaction state on node 2
#[tokio::test]
async fn test_cross_node_transaction_propagation() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(3)
        .with_topology(NetworkTopology::Ring) // 0→1→2→0
        .with_genesis_account("sender", 2_000_000_000_000)
        .with_seed(400)
        .build()
        .await?;

    let sender = network.get_genesis_account("sender").unwrap().0.clone();
    let receiver = create_test_address(40);

    // Submit transaction on node 0 and propagate via ring topology
    let tx = create_test_tx(sender.clone(), receiver.clone(), 500_000_000_000, 1_000, 1);
    network.submit_and_propagate(0, tx).await?;

    // Mine block on node 1 (adjacent in ring)
    // Node 1 received the transaction via ring propagation (0→1)
    network.node(1).daemon().mine_block().await?;

    // Verify node 1 has the block
    assert_tip_height(&network.node(1), 1).await?;
    assert_balance(&network.node(1), &receiver, 500_000_000_000).await?;

    // Verify ring topology configuration (node 0 connected to node 1)
    assert!(network.node(0).is_connected_to(1));
    assert!(network.node(1).is_connected_to(2));

    // Transaction propagation via ring topology works! ✓
    // Demonstrates that transactions propagate through topology constraints
    // TODO: Block propagation - blocks should propagate through ring:
    // - Verify block propagated from node 1 to node 2
    // - Verify receiver balance on node 2 matches (500 TOS)
    // - Verify sender balance decreased on all nodes
    // - Test ring topology constraint: node 0 and node 2 not directly connected

    Ok(())
}

/// Test 6: Time Synchronization Across Nodes
///
/// Scenario:
/// - Create 4-node network
/// - Advance time globally
/// - Verify all nodes see same time
/// - Mine blocks and verify timestamps
#[tokio::test]
async fn test_time_synchronization() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(4)
        .with_genesis_account("miner", 1_000_000)
        .with_seed(500)
        .build()
        .await?;

    // Get initial time from all nodes
    let t0_node0 = network.node(0).clock().now();
    let t0_node1 = network.node(1).clock().now();
    let t0_node2 = network.node(2).clock().now();
    let t0_node3 = network.node(3).clock().now();

    // All nodes should have same initial time
    assert_eq!(t0_node0, t0_node1);
    assert_eq!(t0_node1, t0_node2);
    assert_eq!(t0_node2, t0_node3);

    // Advance time by 10 seconds
    network.advance_time(Duration::from_secs(10)).await;

    // Get new time from all nodes
    let t1_node0 = network.node(0).clock().now();
    let t1_node1 = network.node(1).clock().now();

    // Verify time advanced by 10 seconds
    assert_eq!(t1_node0, t0_node0 + Duration::from_secs(10));
    assert_eq!(t1_node1, t0_node1 + Duration::from_secs(10));

    // All nodes still synchronized
    assert_eq!(t1_node0, t1_node1);

    // Mine block - verify time advancement worked
    network.node(0).daemon().mine_block().await?;
    assert_tip_height(&network.node(0), 1).await?;

    Ok(())
}

/// Test 7: Genesis State Consistency
///
/// Scenario:
/// - Create network with multiple genesis accounts
/// - Verify all nodes have identical genesis state
/// - Verify balances and account existence
#[tokio::test]
async fn test_genesis_state_consistency() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(5)
        .with_genesis_account("alice", 1_000_000_000_000)
        .with_genesis_account("bob", 2_000_000_000_000)
        .with_genesis_account("charlie", 500_000_000_000)
        .with_seed(600)
        .build()
        .await?;

    let alice = network.get_genesis_account("alice").unwrap().0.clone();
    let bob = network.get_genesis_account("bob").unwrap().0.clone();
    let charlie = network.get_genesis_account("charlie").unwrap().0.clone();

    // Verify all nodes have same genesis balances
    for i in 0..5 {
        assert_balance(&network.node(i), &alice, 1_000_000_000_000).await?;
        assert_balance(&network.node(i), &bob, 2_000_000_000_000).await?;
        assert_balance(&network.node(i), &charlie, 500_000_000_000).await?;

        // Verify nonces start at 0
        assert_nonce(&network.node(i), &alice, 0).await?;
        assert_nonce(&network.node(i), &bob, 0).await?;
        assert_nonce(&network.node(i), &charlie, 0).await?;
    }

    // Verify network-level genesis state
    assert_eq!(
        network.get_genesis_account("alice").unwrap().1,
        1_000_000_000_000
    );
    assert_eq!(
        network.get_genesis_account("bob").unwrap().1,
        2_000_000_000_000
    );
    assert_eq!(
        network.get_genesis_account("charlie").unwrap().1,
        500_000_000_000
    );

    Ok(())
}
