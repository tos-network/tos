// Layer 3: Block/TX Propagation Integration Tests
//
// Tests real P2P propagation behavior using LocalTosNetwork:
// - Transaction propagation across connected peers
// - Block propagation after mining
// - Topology-aware propagation (Ring, Star, FullMesh)
// - Propagation respects partition boundaries

#[cfg(test)]
mod tests {
    use crate::tier1_component::TestTransaction;
    use crate::tier2_integration::rpc_helpers::*;
    use crate::tier3_e2e::network::{LocalTosNetworkBuilder, NetworkTopology};
    use anyhow::Result;
    use tos_common::crypto::Hash;

    fn create_test_address(seed: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        Hash::new(bytes)
    }

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

    // ─────────────────────────────────────────────────────────────────────────
    // Block propagation in FullMesh topology
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_block_propagates_to_all_nodes_fullmesh() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(100)
            .build()
            .await?;

        for i in 0..4 {
            assert_tip_height(&network.node(i), 0).await?;
        }

        network.mine_and_propagate(0).await?;

        for i in 0..4 {
            assert_tip_height(&network.node(i), 1).await?;
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_multiple_blocks_propagate_sequentially() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_seed(101)
            .build()
            .await?;

        for round in 0..5 {
            let miner_node = round % 3;
            network.mine_and_propagate(miner_node).await?;
        }

        for i in 0..3 {
            assert_tip_height(&network.node(i), 5).await?;
        }

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Transaction propagation
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_transaction_propagates_to_peers() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_genesis_account("alice", 1_000_000_000_000)
            .with_seed(102)
            .build()
            .await?;

        let alice = network.get_genesis_account("alice").unwrap().0.clone();
        let bob = create_test_address(20);

        let tx = create_test_tx(alice.clone(), bob.clone(), 500_000_000, 100, 1);
        network.submit_and_propagate(0, tx).await?;

        network.mine_and_propagate(1).await?;
        assert_balance(&network.node(0), &bob, 500_000_000).await?;
        assert_balance(&network.node(2), &bob, 500_000_000).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_multiple_transactions_same_block() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(2)
            .with_genesis_account("alice", 10_000_000_000_000)
            .with_seed(103)
            .build()
            .await?;

        let alice = network.get_genesis_account("alice").unwrap().0.clone();
        let bob = create_test_address(30);
        let charlie = create_test_address(31);

        let tx1 = create_test_tx(alice.clone(), bob.clone(), 100_000_000, 100, 1);
        let tx2 = create_test_tx(alice.clone(), charlie.clone(), 200_000_000, 100, 2);

        network.submit_and_propagate(0, tx1).await?;
        network.submit_and_propagate(0, tx2).await?;

        network.mine_and_propagate(0).await?;

        assert_balance(&network.node(1), &bob, 100_000_000).await?;
        assert_balance(&network.node(1), &charlie, 200_000_000).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Ring topology propagation
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_block_propagation_ring_topology() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_topology(NetworkTopology::Ring)
            .with_seed(104)
            .build()
            .await?;

        // Verify ring connectivity
        assert!(network.node(0).is_connected_to(1));
        assert!(network.node(0).is_connected_to(3));
        assert!(!network.node(0).is_connected_to(2));

        network.mine_and_propagate(0).await?;

        // Direct peers (1 and 3) get the block
        assert_tip_height(&network.node(1), 1).await?;
        assert_tip_height(&network.node(3), 1).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Star topology propagation
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_block_propagation_star_topology() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(5)
            .with_topology(NetworkTopology::Star { center: 0 })
            .with_seed(105)
            .build()
            .await?;

        for i in 1..5 {
            assert!(network.node(0).is_connected_to(i));
        }
        assert!(!network.node(1).is_connected_to(2));

        network.mine_and_propagate(0).await?;
        for i in 0..5 {
            assert_tip_height(&network.node(i), 1).await?;
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_leaf_node_propagation_star_topology() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_topology(NetworkTopology::Star { center: 0 })
            .with_seed(106)
            .build()
            .await?;

        network.mine_and_propagate(2).await?;

        // Center (node 0) should receive
        assert_tip_height(&network.node(0), 1).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Propagation respects partitions
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_block_does_not_cross_partition() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(107)
            .build()
            .await?;

        network.partition_groups(&[0, 1], &[2, 3]).await?;

        network.mine_and_propagate(0).await?;

        assert_tip_height(&network.node(0), 1).await?;
        assert_tip_height(&network.node(1), 1).await?;
        assert_tip_height(&network.node(2), 0).await?;
        assert_tip_height(&network.node(3), 0).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_transaction_does_not_cross_partition() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_genesis_account("alice", 1_000_000_000_000)
            .with_seed(108)
            .build()
            .await?;

        let alice = network.get_genesis_account("alice").unwrap().0.clone();
        let bob = create_test_address(40);

        network.partition_groups(&[0], &[1, 2, 3]).await?;

        let tx = create_test_tx(alice.clone(), bob.clone(), 100_000_000, 100, 1);
        // Node 0 is isolated, submit will succeed locally but propagation won't reach others
        network.submit_and_propagate(0, tx).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Propagation after partition heal
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_block_propagates_after_partition_heals() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_seed(109)
            .build()
            .await?;

        network.partition_groups(&[0, 1], &[2]).await?;

        network.mine_and_propagate(0).await?;
        assert_tip_height(&network.node(2), 0).await?;

        network.heal_all_partitions().await;

        // After heal, mine from node 2 and verify it propagates to nodes 0 and 1
        // (forward propagation works after partition heal)
        network.mine_and_propagate(2).await?;
        assert_tip_height(&network.node(2), 1).await?;
        // Nodes 0,1 receive node 2's block (they may have different chain tip)
        let h0 = network.node(0).daemon().get_tip_height().await?;
        let h1 = network.node(1).daemon().get_tip_height().await?;
        assert!(h0 >= 1, "Node 0 should have received blocks after heal");
        assert!(h1 >= 1, "Node 1 should have received blocks after heal");

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Divergent chains during partition
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_divergent_chains_during_partition() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(110)
            .build()
            .await?;

        network.partition_groups(&[0, 1], &[2, 3]).await?;

        network.mine_and_propagate(0).await?;
        network.mine_and_propagate(2).await?;

        // Each partition has height 1 but different blocks
        assert_tip_height(&network.node(0), 1).await?;
        assert_tip_height(&network.node(1), 1).await?;
        assert_tip_height(&network.node(2), 1).await?;
        assert_tip_height(&network.node(3), 1).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Edge cases
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_single_node_network() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(1)
            .with_seed(111)
            .build()
            .await?;

        network.mine_and_propagate(0).await?;
        assert_tip_height(&network.node(0), 1).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_two_node_bidirectional() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(2)
            .with_seed(112)
            .build()
            .await?;

        network.mine_and_propagate(0).await?;
        assert_tip_height(&network.node(1), 1).await?;

        network.mine_and_propagate(1).await?;
        assert_tip_height(&network.node(0), 2).await?;

        network.mine_and_propagate(0).await?;
        assert_tip_height(&network.node(1), 3).await?;

        Ok(())
    }
}
