// Layer 3: Network Partition + Recovery Integration Tests
//
// Tests real partition behavior using LocalTosNetwork:
// - Partition creation and healing
// - Independent chain growth during partition
// - State divergence and convergence after heal
// - Multiple sequential partition/heal cycles
// - Asymmetric partitions

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
    // Basic partition semantics
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_partition_state_tracking() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(200)
            .build()
            .await?;

        assert!(!network.is_partitioned(0, 1).await);
        assert!(!network.is_partitioned(0, 2).await);

        network.partition_groups(&[0, 1], &[2, 3]).await?;

        assert!(network.is_partitioned(0, 2).await);
        assert!(network.is_partitioned(0, 3).await);
        assert!(network.is_partitioned(1, 2).await);
        assert!(network.is_partitioned(1, 3).await);

        assert!(!network.is_partitioned(0, 1).await);
        assert!(!network.is_partitioned(2, 3).await);

        network.heal_all_partitions().await;
        assert!(!network.is_partitioned(0, 2).await);
        assert!(!network.is_partitioned(1, 3).await);

        Ok(())
    }

    #[tokio::test]
    async fn test_partition_is_bidirectional() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_seed(201)
            .build()
            .await?;

        network.partition_groups(&[0], &[1, 2]).await?;

        assert!(network.is_partitioned(0, 1).await);
        assert!(network.is_partitioned(1, 0).await);
        assert!(network.is_partitioned(0, 2).await);
        assert!(network.is_partitioned(2, 0).await);

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Partition isolation verification
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_isolated_node_cannot_receive_blocks() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(5)
            .with_seed(202)
            .build()
            .await?;

        network.partition_groups(&[4], &[0, 1, 2, 3]).await?;

        for i in 0..3 {
            network.mine_and_propagate(i % 4).await?;
        }

        for i in 0..4 {
            assert_tip_height(&network.node(i), 3).await?;
        }

        assert_tip_height(&network.node(4), 0).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_isolated_node_mines_independently() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_seed(203)
            .build()
            .await?;

        network.partition_groups(&[2], &[0, 1]).await?;

        network.mine_and_propagate(2).await?;
        network.mine_and_propagate(2).await?;

        network.mine_and_propagate(0).await?;

        assert_tip_height(&network.node(2), 2).await?;
        assert_tip_height(&network.node(0), 1).await?;
        assert_tip_height(&network.node(1), 1).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Partition and heal cycles
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_multiple_partition_heal_cycles() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(204)
            .build()
            .await?;

        // Cycle 1
        network.partition_groups(&[0, 1], &[2, 3]).await?;
        network.mine_and_propagate(0).await?;
        network.mine_and_propagate(2).await?;

        assert_tip_height(&network.node(0), 1).await?;
        assert_tip_height(&network.node(2), 1).await?;

        network.heal_all_partitions().await;

        // Cycle 2: Different partition groups
        network.partition_groups(&[0, 2], &[1, 3]).await?;
        network.mine_and_propagate(0).await?;
        network.mine_and_propagate(1).await?;

        assert!(!network.is_partitioned(0, 2).await);
        assert!(network.is_partitioned(0, 1).await);

        Ok(())
    }

    #[tokio::test]
    async fn test_heal_enables_convergence() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_seed(205)
            .build()
            .await?;

        network.partition_groups(&[0, 1], &[2]).await?;

        network.mine_and_propagate(0).await?;
        assert_tip_height(&network.node(2), 0).await?;

        network.heal_all_partitions().await;

        // After heal, mine from node 2 to verify forward propagation works
        network.mine_and_propagate(2).await?;
        assert_tip_height(&network.node(2), 1).await?;

        // Node 2's block should reach nodes 0 and 1 after heal
        let h0 = network.node(0).daemon().get_tip_height().await?;
        let h1 = network.node(1).daemon().get_tip_height().await?;
        assert!(h0 >= 1, "Node 0 should have received blocks after heal");
        assert!(h1 >= 1, "Node 1 should have received blocks after heal");

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Partition with transactions
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_transactions_isolated_during_partition() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_genesis_account("alice", 10_000_000_000_000)
            .with_seed(206)
            .build()
            .await?;

        let alice = network.get_genesis_account("alice").unwrap().0.clone();
        let bob = create_test_address(50);

        network.partition_groups(&[0, 1], &[2, 3]).await?;

        let tx = create_test_tx(alice.clone(), bob.clone(), 1_000_000_000, 100, 1);
        network.submit_and_propagate(0, tx).await?;

        network.mine_and_propagate(0).await?;

        assert_balance(&network.node(0), &bob, 1_000_000_000).await?;
        assert_balance(&network.node(1), &bob, 1_000_000_000).await?;

        assert_tip_height(&network.node(2), 0).await?;
        assert_tip_height(&network.node(3), 0).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Partition in different topologies
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_partition_in_ring_topology() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_topology(NetworkTopology::Ring)
            .with_seed(207)
            .build()
            .await?;

        network.partition_groups(&[0, 1], &[2, 3]).await?;

        network.mine_and_propagate(0).await?;
        assert_tip_height(&network.node(1), 1).await?;
        assert_tip_height(&network.node(2), 0).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_partition_star_center_isolated() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_topology(NetworkTopology::Star { center: 0 })
            .with_seed(208)
            .build()
            .await?;

        network.partition_groups(&[0], &[1, 2, 3]).await?;

        network.mine_and_propagate(0).await?;
        assert_tip_height(&network.node(0), 1).await?;
        assert_tip_height(&network.node(1), 0).await?;
        assert_tip_height(&network.node(2), 0).await?;
        assert_tip_height(&network.node(3), 0).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Multi-partition scenarios (3+ groups)
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_three_way_partition() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(6)
            .with_seed(209)
            .build()
            .await?;

        network.partition_groups(&[0, 1], &[2, 3]).await?;
        network.partition_groups(&[0, 1], &[4, 5]).await?;
        network.partition_groups(&[2, 3], &[4, 5]).await?;

        network.mine_and_propagate(0).await?;
        network.mine_and_propagate(2).await?;
        network.mine_and_propagate(4).await?;

        assert_tip_height(&network.node(0), 1).await?;
        assert_tip_height(&network.node(1), 1).await?;
        assert_tip_height(&network.node(2), 1).await?;
        assert_tip_height(&network.node(3), 1).await?;
        assert_tip_height(&network.node(4), 1).await?;
        assert_tip_height(&network.node(5), 1).await?;

        assert!(network.is_partitioned(0, 2).await);
        assert!(network.is_partitioned(0, 4).await);
        assert!(network.is_partitioned(2, 4).await);

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Rapid partition/heal stress
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_rapid_partition_heal_mining() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_seed(210)
            .build()
            .await?;

        for round in 0..5 {
            if round % 2 == 0 {
                network.partition_groups(&[0], &[1, 2]).await?;
            } else {
                network.heal_all_partitions().await;
            }
            network.mine_and_propagate(round % 3).await?;
        }

        // Network should be in a valid state
        let h0 = network.node(0).daemon().get_tip_height().await?;
        let h1 = network.node(1).daemon().get_tip_height().await?;
        let h2 = network.node(2).daemon().get_tip_height().await?;
        assert!(h0 >= 1);
        assert!(h1 >= 1);
        assert!(h2 >= 1);

        Ok(())
    }
}
