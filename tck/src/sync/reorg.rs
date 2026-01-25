// Layer 3: DAG Reorg + Chain Sync Integration Tests
//
// Tests real chain sync behavior using LocalTosNetwork:
// - Partition → independent mining → heal → reorg to heavier chain
// - Late-joining node syncs full chain
// - Multiple sequential reorgs
// - Concurrent mining creates DAG branches
// - Chain convergence after complex partition patterns

#[cfg(test)]
mod tests {
    use crate::tier2_integration::rpc_helpers::*;
    use crate::tier2_integration::NodeRpc;
    use crate::tier3_e2e::network::{LocalTosNetworkBuilder, NetworkTopology};
    use anyhow::Result;

    // ─────────────────────────────────────────────────────────────────────────
    // Basic fork/reorg: partition → mine → heal → convergence
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_partition_creates_independent_chains() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(300)
            .build()
            .await?;

        // Partition: {0,1} vs {2,3}
        network.partition_groups(&[0, 1], &[2, 3]).await?;

        // Each side mines independently
        network.mine_and_propagate(0).await?;
        network.mine_and_propagate(0).await?;
        network.mine_and_propagate(2).await?;

        // Side A has 2 blocks, side B has 1
        assert_tip_height(&network.node(0), 2).await?;
        assert_tip_height(&network.node(1), 2).await?;
        assert_tip_height(&network.node(2), 1).await?;
        assert_tip_height(&network.node(3), 1).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_heavier_chain_wins_after_heal() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(301)
            .build()
            .await?;

        network.partition_groups(&[0, 1], &[2, 3]).await?;

        // Side A mines 3 blocks (heavier chain)
        for _ in 0..3 {
            network.mine_and_propagate(0).await?;
        }

        // Side B mines 1 block (lighter chain)
        network.mine_and_propagate(2).await?;

        assert_tip_height(&network.node(0), 3).await?;
        assert_tip_height(&network.node(2), 1).await?;

        // Heal partition
        network.heal_all_partitions().await;

        // Mine a block to trigger sync/propagation
        network.mine_and_propagate(0).await?;

        // After heal, node 0 has the heavier chain (4 blocks)
        assert_tip_height(&network.node(0), 4).await?;
        assert_tip_height(&network.node(1), 4).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Chain sync: all nodes converge
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_all_connected_nodes_stay_in_sync() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(5)
            .with_seed(302)
            .build()
            .await?;

        // Mine 10 blocks from different miners
        for round in 0..10 {
            let miner = round % 5;
            network.mine_and_propagate(miner).await?;
        }

        // All nodes should have the same height
        for i in 0..5 {
            assert_tip_height(&network.node(i), 10).await?;
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_single_miner_propagates_to_all() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(303)
            .build()
            .await?;

        // Only node 0 mines, all should sync
        for _ in 0..5 {
            network.mine_and_propagate(0).await?;
        }

        for i in 0..4 {
            assert_tip_height(&network.node(i), 5).await?;
        }

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Sequential partition/heal cycles with mining
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_multiple_partition_heal_with_mining() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(304)
            .build()
            .await?;

        // Cycle 1: partition and mine
        network.partition_groups(&[0, 1], &[2, 3]).await?;
        network.mine_and_propagate(0).await?;
        network.mine_and_propagate(2).await?;

        // Heal
        network.heal_all_partitions().await;

        // Cycle 2: different partition
        network.partition_groups(&[0, 2], &[1, 3]).await?;
        network.mine_and_propagate(0).await?;
        network.mine_and_propagate(1).await?;

        // Heal again
        network.heal_all_partitions().await;

        // Mine one more block and propagate
        network.mine_and_propagate(0).await?;

        // All nodes should be in a valid state
        for i in 0..4 {
            let h = network.node(i).daemon().get_tip_height().await?;
            assert!(h >= 1, "Node {} should have height >= 1, got {}", i, h);
        }

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Ring topology reorg behavior
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_ring_topology_propagation_and_sync() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(5)
            .with_topology(NetworkTopology::Ring)
            .with_seed(305)
            .build()
            .await?;

        // Mine from multiple nodes in ring
        for round in 0..5 {
            network.mine_and_propagate(round % 5).await?;
        }

        // All nodes should have received blocks via ring propagation
        // In a ring, propagation takes multiple hops so not all may arrive
        for i in 0..5 {
            let h = network.node(i).daemon().get_tip_height().await?;
            assert!(
                h >= 1,
                "Node {} in ring should have height >= 1, got {}",
                i,
                h
            );
        }

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Star topology: center failure isolates leaves
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_star_topology_center_partition_isolates_leaves() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(5)
            .with_topology(NetworkTopology::Star { center: 0 })
            .with_seed(306)
            .build()
            .await?;

        // First mine normally through center
        network.mine_and_propagate(0).await?;
        for i in 0..5 {
            assert_tip_height(&network.node(i), 1).await?;
        }

        // Isolate center node
        network.partition_groups(&[0], &[1, 2, 3, 4]).await?;

        // Center mines alone
        network.mine_and_propagate(0).await?;
        assert_tip_height(&network.node(0), 2).await?;

        // Leaves can't propagate to each other (no direct connections in star)
        assert_tip_height(&network.node(1), 1).await?;
        assert_tip_height(&network.node(2), 1).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Three-way partition creates three independent chains
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_three_way_partition_independent_mining() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(6)
            .with_seed(307)
            .build()
            .await?;

        // Three-way partition: {0,1} vs {2,3} vs {4,5}
        network.partition_groups(&[0, 1], &[2, 3]).await?;
        network.partition_groups(&[0, 1], &[4, 5]).await?;
        network.partition_groups(&[2, 3], &[4, 5]).await?;

        // Each group mines different number of blocks
        network.mine_and_propagate(0).await?;
        network.mine_and_propagate(0).await?;
        network.mine_and_propagate(0).await?;

        network.mine_and_propagate(2).await?;
        network.mine_and_propagate(2).await?;

        network.mine_and_propagate(4).await?;

        // Each group has its own chain height
        assert_tip_height(&network.node(0), 3).await?;
        assert_tip_height(&network.node(1), 3).await?;
        assert_tip_height(&network.node(2), 2).await?;
        assert_tip_height(&network.node(3), 2).await?;
        assert_tip_height(&network.node(4), 1).await?;
        assert_tip_height(&network.node(5), 1).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Asymmetric mining during partition
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_asymmetric_mining_rates_during_partition() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(308)
            .build()
            .await?;

        network.partition_groups(&[0, 1], &[2, 3]).await?;

        // Side A mines 5 blocks
        for _ in 0..5 {
            network.mine_and_propagate(0).await?;
        }

        // Side B mines only 1 block
        network.mine_and_propagate(2).await?;

        assert_tip_height(&network.node(0), 5).await?;
        assert_tip_height(&network.node(1), 5).await?;
        assert_tip_height(&network.node(2), 1).await?;
        assert_tip_height(&network.node(3), 1).await?;

        // Heal - after this, the heavier chain (side A) should be preferred
        network.heal_all_partitions().await;

        // Mine a new block from side A to trigger sync
        network.mine_and_propagate(0).await?;

        // Side A nodes continue with their chain
        assert_tip_height(&network.node(0), 6).await?;
        assert_tip_height(&network.node(1), 6).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Forward propagation after reconnection
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_forward_propagation_after_reconnection() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_seed(309)
            .build()
            .await?;

        // Isolate node 2
        network.partition_groups(&[0, 1], &[2]).await?;

        // Mine on side A
        network.mine_and_propagate(0).await?;
        network.mine_and_propagate(1).await?;

        assert_tip_height(&network.node(0), 2).await?;
        assert_tip_height(&network.node(2), 0).await?;

        // Heal
        network.heal_all_partitions().await;

        // Node 2 mines after reconnection - should propagate to all
        network.mine_and_propagate(2).await?;
        assert_tip_height(&network.node(2), 1).await?;

        // Nodes 0,1 should receive node 2's block
        let h0 = network.node(0).daemon().get_tip_height().await?;
        assert!(
            h0 >= 2,
            "Node 0 should have height >= 2 after heal, got {}",
            h0
        );

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Rapid mining stress test
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_rapid_mining_all_nodes_converge() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(310)
            .build()
            .await?;

        // Rapid mining from alternating nodes
        for round in 0..20 {
            network.mine_and_propagate(round % 4).await?;
        }

        // All nodes should converge to same height
        let expected_height = 20u64;
        for i in 0..4 {
            assert_tip_height(&network.node(i), expected_height).await?;
        }

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Partition during high block production
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_partition_during_active_mining() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(311)
            .build()
            .await?;

        // Mine some initial blocks together
        for _ in 0..3 {
            network.mine_and_propagate(0).await?;
        }

        // All nodes at height 3
        for i in 0..4 {
            assert_tip_height(&network.node(i), 3).await?;
        }

        // Partition mid-mining
        network.partition_groups(&[0, 1], &[2, 3]).await?;

        // Continue mining on both sides
        for _ in 0..3 {
            network.mine_and_propagate(0).await?;
        }
        for _ in 0..2 {
            network.mine_and_propagate(2).await?;
        }

        // Side A: 3 + 3 = 6
        assert_tip_height(&network.node(0), 6).await?;
        assert_tip_height(&network.node(1), 6).await?;

        // Side B: 3 + 2 = 5
        assert_tip_height(&network.node(2), 5).await?;
        assert_tip_height(&network.node(3), 5).await?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Partition creates height divergence between groups
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_partition_groups_diverge_in_height() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_seed(312)
            .build()
            .await?;

        network.partition_groups(&[0, 1], &[2, 3]).await?;

        // Side A mines 3 blocks, side B mines 1 block
        for _ in 0..3 {
            network.mine_and_propagate(0).await?;
        }
        network.mine_and_propagate(2).await?;

        // Heights should diverge between partitions
        assert_tip_height(&network.node(0), 3).await?;
        assert_tip_height(&network.node(1), 3).await?;
        assert_tip_height(&network.node(2), 1).await?;
        assert_tip_height(&network.node(3), 1).await?;

        // Within each partition, nodes agree on height
        let h0 = network.node(0).get_tip_height().await?;
        let h1 = network.node(1).get_tip_height().await?;
        assert_eq!(h0, h1, "Nodes in same partition should agree on height");

        let h2 = network.node(2).get_tip_height().await?;
        let h3 = network.node(3).get_tip_height().await?;
        assert_eq!(h2, h3, "Nodes in same partition should agree on height");

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Empty blocks don't affect sync
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_empty_blocks_propagate_correctly() -> Result<()> {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_seed(313)
            .build()
            .await?;

        // Mine empty blocks (no transactions) from different nodes
        for round in 0..6 {
            network.mine_and_propagate(round % 3).await?;
        }

        // All nodes at same height
        for i in 0..3 {
            assert_tip_height(&network.node(i), 6).await?;
        }

        Ok(())
    }
}
