//! Advanced Multi-Node E2E Scenarios
//!
//! This module contains complex multi-node scenarios that demonstrate
//! the framework's capabilities for testing:
//! - Network partitions and healing
//! - Competing miners and fork resolution
//! - Cascading block propagation
//! - Chain reorganization
//! - Byzantine behavior detection

use crate::tier1_component::TestTransaction;
use crate::tier2_integration::rpc_helpers::*;
use crate::tier2_integration::NodeRpc;
use crate::tier3_e2e::network::{LocalTosNetworkBuilder, NetworkTopology};
use anyhow::Result;
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

/// Scenario 1: Network Partition and Isolated Mining
///
/// This scenario tests the framework's ability to simulate a network partition
/// where different sides of the network build independent chains.
///
/// Note: Full chain reorganization is not yet implemented, so this test
/// demonstrates partition behavior without automatic convergence.
///
/// Timeline:
/// 1. Start with 4-node network
/// 2. Partition into [0,1] vs [2,3]
/// 3. Both sides mine blocks independently
/// 4. Verify isolation is maintained
#[tokio::test]
async fn test_partition_with_competing_chains() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(4)
        .with_genesis_account("alice", 10_000_000_000_000)
        .with_genesis_account("bob", 10_000_000_000_000)
        .with_seed(2000)
        .build()
        .await?;

    let alice = network.get_genesis_account("alice").unwrap().0.clone();
    let bob = network.get_genesis_account("bob").unwrap().0.clone();

    // All nodes start at height 0
    for i in 0..4 {
        assert_tip_height(&network.node(i), 0).await?;
    }

    // Create network partition: [0,1] vs [2,3]
    network.partition_groups(&[0, 1], &[2, 3]).await?;

    // Side A (nodes 0,1): Alice sends 3 transactions
    for nonce in 1..=3 {
        let tx = create_test_tx(
            alice.clone(),
            create_test_address(50 + nonce as u8),
            1_000_000_000,
            100,
            nonce,
        );
        network.submit_and_propagate(0, tx).await?;
    }

    // Side B (nodes 2,3): Bob sends 2 transactions
    for nonce in 1..=2 {
        let tx = create_test_tx(
            bob.clone(),
            create_test_address(60 + nonce as u8),
            2_000_000_000,
            100,
            nonce,
        );
        network.submit_and_propagate(2, tx).await?;
    }

    // Both sides mine blocks independently
    network.mine_and_propagate(0).await?; // Side A mines
    network.mine_and_propagate(2).await?; // Side B mines

    // Verify partition: each side has height 1, but isolated
    assert_tip_height(&network.node(0), 1).await?;
    assert_tip_height(&network.node(1), 1).await?;
    assert_tip_height(&network.node(2), 1).await?;
    assert_tip_height(&network.node(3), 1).await?;

    // Verify side A has Alice's transactions
    assert_nonce(&network.node(0), &alice, 3).await?;
    assert_nonce(&network.node(1), &alice, 3).await?;

    // Verify side B has Bob's transactions
    assert_nonce(&network.node(2), &bob, 2).await?;
    assert_nonce(&network.node(3), &bob, 2).await?;

    // Verify side A doesn't have Bob's state
    assert_nonce(&network.node(0), &bob, 0).await?;
    assert_nonce(&network.node(1), &bob, 0).await?;

    // Verify side B doesn't have Alice's state
    assert_nonce(&network.node(2), &alice, 0).await?;
    assert_nonce(&network.node(3), &alice, 0).await?;

    // Partition successfully isolated both sides!
    Ok(())
}

/// Scenario 2: Multi-Miner Competition
///
/// Multiple miners compete to produce blocks simultaneously.
/// Tests concurrent mining and block propagation.
#[tokio::test]
async fn test_multi_miner_competition() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(5)
        .with_topology(NetworkTopology::FullMesh)
        .with_genesis_account("whale", 100_000_000_000_000)
        .with_seed(2100)
        .build()
        .await?;

    let whale = network.get_genesis_account("whale").unwrap().0.clone();

    // Submit 10 transactions from whale
    for nonce in 1..=10 {
        let tx = create_test_tx(
            whale.clone(),
            create_test_address(70 + nonce as u8),
            500_000_000,
            100,
            nonce,
        );
        network.submit_and_propagate(0, tx).await?;
    }

    // All nodes mine and propagate simultaneously (simulating competition)
    // In FullMesh, the first to propagate will win for each round
    for round in 1..=3 {
        let miner = round % 5; // Rotate miners
        network.mine_and_propagate(miner).await?;

        // Verify all nodes converge after each round
        for i in 0..5 {
            assert_tip_height(&network.node(i), round as u64).await?;
        }
    }

    // After 3 blocks, verify consistent state
    for i in 0..5 {
        // Check whale's nonce (should be 10 - all transactions included)
        assert_nonce(&network.node(i), &whale, 10).await?;

        // Check first recipient got their transfer
        let recipient1_balance = network
            .node(i)
            .get_balance(&create_test_address(71))
            .await?;
        assert_eq!(recipient1_balance, 500_000_000);
    }

    Ok(())
}

/// Scenario 3: Cascading Block Propagation Through Ring
///
/// Tests how blocks propagate through a ring topology,
/// demonstrating multi-hop propagation.
#[tokio::test]
async fn test_cascading_propagation_ring() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(5)
        .with_topology(NetworkTopology::Ring) // 0→1→2→3→4→0
        .with_genesis_account("sender", 5_000_000_000_000)
        .with_seed(2200)
        .build()
        .await?;

    let sender = network.get_genesis_account("sender").unwrap().0.clone();
    let receiver = create_test_address(80);

    // Submit transaction on node 0
    let tx = create_test_tx(sender.clone(), receiver.clone(), 1_000_000_000, 100, 1);
    network.submit_and_propagate(0, tx.clone()).await?;

    // In ring topology, tx propagates: 0→1 and 0→4
    // Manually propagate to node 2 so it can mine
    network.node(2).daemon().submit_transaction(tx).await?;

    // Mine on node 2 and propagate through ring
    network.mine_and_propagate(2).await?;

    // Block propagates to neighbors: 2→1 and 2→3
    // Manually propagate to complete the ring
    network.propagate_block_from(1, 1).await?; // 1→0
    network.propagate_block_from(3, 1).await?; // 3→4

    // Now all nodes should have the block
    for i in 0..5 {
        assert_tip_height(&network.node(i), 1).await?;
        assert_balance(&network.node(i), &receiver, 1_000_000_000).await?;
    }

    Ok(())
}

/// Scenario 4: Byzantine Node Detection (Invalid Block Rejection)
///
/// Tests that nodes reject invalid blocks (wrong height, duplicate, etc.)
#[tokio::test]
async fn test_byzantine_block_rejection() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(3)
        .with_genesis_account("alice", 5_000_000_000_000)
        .with_seed(2300)
        .build()
        .await?;

    let alice = network.get_genesis_account("alice").unwrap().0.clone();

    // Submit and mine valid block on node 0
    let tx = create_test_tx(
        alice.clone(),
        create_test_address(90),
        1_000_000_000,
        100,
        1,
    );
    network.submit_and_propagate(0, tx).await?;
    network.mine_and_propagate(0).await?;

    // All nodes at height 1
    assert_tip_height(&network.node(0), 1).await?;
    assert_tip_height(&network.node(1), 1).await?;
    assert_tip_height(&network.node(2), 1).await?;

    // Test 1: Try to send block with wrong height (height check happens first)
    let block = network
        .node(0)
        .daemon()
        .get_block_at_height(1)
        .await?
        .unwrap();

    // Try to send the same block again to node 1 (which is already at height 1)
    // This should fail because node 1 expects height 2, not height 1
    let result = network.node(1).daemon().receive_block(block.clone()).await;
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Invalid block height"),
        "Expected height error, got: {}",
        error_msg
    );

    // Test 2: Try to skip a height (send height 3 when at height 1)
    // First mine blocks 2 and 3 on node 0
    let tx2 = create_test_tx(alice.clone(), create_test_address(91), 500_000_000, 100, 2);
    network.node(0).daemon().submit_transaction(tx2).await?;
    network.node(0).daemon().mine_block().await?;

    let tx3 = create_test_tx(alice.clone(), create_test_address(92), 300_000_000, 100, 3);
    network.node(0).daemon().submit_transaction(tx3).await?;
    network.node(0).daemon().mine_block().await?;

    // Node 0 now at height 3
    assert_tip_height(&network.node(0), 3).await?;

    // Try to send block 3 directly to node 2 (which is at height 1)
    let block_h3 = network
        .node(0)
        .daemon()
        .get_block_at_height(3)
        .await?
        .unwrap();
    let result3 = network.node(2).daemon().receive_block(block_h3).await;
    assert!(result3.is_err());
    let error_msg3 = result3.unwrap_err().to_string();
    assert!(
        error_msg3.contains("Invalid block height: expected 2, got 3"),
        "Expected height skip error, got: {}",
        error_msg3
    );

    // Test 3: Valid sequential block should work
    let block_h2 = network
        .node(0)
        .daemon()
        .get_block_at_height(2)
        .await?
        .unwrap();
    network.node(2).daemon().receive_block(block_h2).await?;
    assert_tip_height(&network.node(2), 2).await?;

    Ok(())
}

/// Scenario 5: High-Throughput Network Stress Test
///
/// Tests the network with high transaction volume and rapid block production
#[tokio::test]
async fn test_high_throughput_stress() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(3)
        .with_genesis_account("whale", 1_000_000_000_000_000)
        .with_seed(2400)
        .build()
        .await?;

    let whale = network.get_genesis_account("whale").unwrap().0.clone();

    // Submit 50 transactions
    for nonce in 1..=50 {
        let tx = create_test_tx(
            whale.clone(),
            create_test_address(100 + (nonce % 50) as u8),
            10_000_000,
            100,
            nonce,
        );
        network.submit_and_propagate(0, tx).await?;
    }

    // Mine 10 blocks rapidly
    for _ in 0..10 {
        let miner = 0; // All from node 0
        network.mine_and_propagate(miner).await?;
    }

    // Verify all nodes at height 10
    for i in 0..3 {
        assert_tip_height(&network.node(i), 10).await?;
    }

    // Verify whale's nonce advanced to 50
    for i in 0..3 {
        assert_nonce(&network.node(i), &whale, 50).await?;
    }

    Ok(())
}

/// Scenario 6: Network Healing After Full Partition
///
/// Tests that after healing a complete partition, nodes can sync new blocks
#[tokio::test]
async fn test_gradual_partition_healing() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(4)
        .with_genesis_account("alice", 10_000_000_000_000)
        .with_genesis_account("bob", 5_000_000_000_000)
        .with_seed(2500)
        .build()
        .await?;

    let alice = network.get_genesis_account("alice").unwrap().0.clone();
    let bob = network.get_genesis_account("bob").unwrap().0.clone();

    // Start with a partition: [0,1] vs [2,3]
    network.partition_groups(&[0, 1], &[2, 3]).await?;

    // Side A mines one block
    let tx_a = create_test_tx(alice.clone(), create_test_address(110), 1_000_000, 100, 1);
    network.submit_and_propagate(0, tx_a).await?;
    network.mine_and_propagate(0).await?;

    // Verify side A at height 1
    assert_tip_height(&network.node(0), 1).await?;
    assert_tip_height(&network.node(1), 1).await?;

    // Verify side B still at height 0 (partitioned)
    assert_tip_height(&network.node(2), 0).await?;
    assert_tip_height(&network.node(3), 0).await?;

    // Heal the partition
    network.heal_all_partitions().await;

    // After healing, propagate side A's block to side B
    network.propagate_block_from(0, 1).await?;

    // All nodes should now be at height 1
    for i in 0..4 {
        assert_tip_height(&network.node(i), 1).await?;
    }

    // After healing, mine a new block with bob on node 2
    let tx_b = create_test_tx(bob.clone(), create_test_address(120), 500_000, 100, 1);
    network.submit_and_propagate(2, tx_b).await?;
    network.mine_and_propagate(2).await?;

    // All nodes should now converge to height 2
    for i in 0..4 {
        assert_tip_height(&network.node(i), 2).await?;
    }

    // Verify both alice and bob's transactions were included
    for i in 0..4 {
        assert_nonce(&network.node(i), &alice, 1).await?;
        assert_nonce(&network.node(i), &bob, 1).await?;
    }

    Ok(())
}
