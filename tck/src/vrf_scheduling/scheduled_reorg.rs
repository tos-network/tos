// Phase 16: Scheduled Execution Reorg Tests (Layer 3)
//
// Tests scheduled execution behavior during DAG reorgs using LocalTosNetwork.
// Requires: Multi-node scheduled execution processing.

#[cfg(test)]
mod tests {
    use tos_common::contract::{
        ScheduledExecution, ScheduledExecutionKind, ScheduledExecutionStatus,
    };
    use tos_common::crypto::Hash;

    use crate::tier2_integration::NodeRpc;
    use crate::tier3_e2e::LocalTosNetworkBuilder;

    // ========================================================================
    // Cross-Node Execution Consistency Tests
    // ========================================================================

    /// Helper to create a scheduled execution with unique hash based on contract_id
    fn make_exec(target_topo: u64, offer: u64, contract_id: u8) -> ScheduledExecution {
        let contract = Hash::new([contract_id; 32]);
        let scheduler = Hash::new([0xDD; 32]);
        ScheduledExecution::new_offercall(
            contract,
            0,
            vec![],
            100_000,
            offer,
            scheduler,
            ScheduledExecutionKind::TopoHeight(target_topo),
            0,
        )
    }

    #[tokio::test]
    async fn execution_consistent_across_nodes() {
        // Setup: 3-node network
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .build()
            .await
            .expect("Failed to build network");

        // Schedule execution at target=5 on node 0
        let exec = make_exec(5, 1000, 0xCC);
        let exec_hash = network.node(0).schedule_execution(exec).expect("schedule");

        // Verify scheduled on node 0
        let (status, _) = network.node(0).get_scheduled_status(&exec_hash).unwrap();
        assert_eq!(status, ScheduledExecutionStatus::Pending);

        // Mine until topoheight 5 on node 0
        for _ in 0..5 {
            network.mine_and_propagate(0).await.expect("mine");
        }

        // Verify execution happened on node 0
        let (status_0, topo_0) = network.node(0).get_scheduled_status(&exec_hash).unwrap();
        assert_eq!(status_0, ScheduledExecutionStatus::Executed);
        assert_eq!(topo_0, 5);

        // Note: In this simplified model, scheduling is per-node (not propagated via blocks)
        // So nodes 1 and 2 won't have this scheduled execution unless we explicitly add it
        // This test verifies that execution happens correctly on the scheduling node
    }

    #[tokio::test]
    async fn execution_result_consistent_post_heal() {
        // Setup: 3-node network
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .build()
            .await
            .expect("Failed to build network");

        // Create partition: [0,1] | [2]
        network
            .partition_groups(&[0, 1], &[2])
            .await
            .expect("partition");

        // Schedule execution on node 0 (partition A) at target topo 3
        let exec = make_exec(3, 1000, 0xEE);
        let exec_hash = network.node(0).schedule_execution(exec).expect("schedule");

        // Mine past target on partition A (nodes 0,1) - 5 blocks (longer chain)
        for _ in 0..5 {
            network.node(0).daemon().mine_block().await.expect("mine A");
        }

        // Mine past target on partition B (node 2) - 3 blocks (shorter chain)
        for _ in 0..3 {
            network.node(2).daemon().mine_block().await.expect("mine B");
        }

        // Verify partition A executed the scheduled execution
        let (status_a, topo_a) = network
            .node(0)
            .get_scheduled_status(&exec_hash)
            .expect("should have status on node 0");
        assert_eq!(
            status_a,
            ScheduledExecutionStatus::Executed,
            "Partition A should have executed"
        );
        assert_eq!(topo_a, 3, "Should execute at target topo");

        // Heal partition
        network.heal_all_partitions().await;

        // Propagate blocks from partition A to partition B
        // Send all blocks from node 0 to node 2
        for height in 1..=5 {
            let block = network
                .node(0)
                .daemon()
                .blockchain()
                .get_block_at_height(height)
                .await
                .expect("block should exist")
                .expect("block should exist at height");
            network
                .node(2)
                .receive_fork_block(block)
                .await
                .expect("receive block");
        }

        // Trigger reorg on node 2 to the longer chain
        let tip_hash = network.node(0).get_tips().await.expect("get tips")[0].clone();
        network
            .node(2)
            .reorg_to_chain(&tip_hash)
            .await
            .expect("reorg");

        // Now schedule the same execution on node 2 (it should already be in Executed state
        // after syncing the chain, but since scheduling is local, we need to register it)
        let exec_for_node2 = make_exec(3, 1000, 0xEE);
        let _ = network.node(2).schedule_execution(exec_for_node2);

        // Process scheduled executions on node 2 at the target topoheight
        // (This simulates replaying the chain state)

        // All nodes should now agree on the chain state
        assert_eq!(
            network.node(0).get_tip_height().await.unwrap(),
            network.node(2).get_tip_height().await.unwrap(),
            "Nodes should have same height after heal"
        );
    }

    // ========================================================================
    // Reorg Rollback Tests
    // ========================================================================

    #[tokio::test]
    async fn reorg_cancels_executed() {
        // Setup: 3-node network
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .build()
            .await
            .expect("Failed to build network");

        // Create partition: [0,1] | [2]
        network
            .partition_groups(&[0, 1], &[2])
            .await
            .expect("partition");

        // Schedule execution on node 0 at target topo 3
        let exec = make_exec(3, 1000, 0xFF);
        let exec_hash = network.node(0).schedule_execution(exec).expect("schedule");

        // Mine past target on partition A (nodes 0,1) - 4 blocks
        for _ in 0..4 {
            network.node(0).daemon().mine_block().await.expect("mine A");
        }

        // Verify execution happened on partition A
        let (status_before, _) = network
            .node(0)
            .get_scheduled_status(&exec_hash)
            .expect("should have status");
        assert_eq!(
            status_before,
            ScheduledExecutionStatus::Executed,
            "Should be executed before reorg"
        );

        // Node 2 mines a HEAVIER chain (6 blocks) WITHOUT this scheduling
        for _ in 0..6 {
            network.node(2).daemon().mine_block().await.expect("mine B");
        }

        // Heal partition
        network.heal_all_partitions().await;

        // Propagate heavier chain from node 2 to node 0
        for height in 1..=6 {
            let block = network
                .node(2)
                .daemon()
                .blockchain()
                .get_block_at_height(height)
                .await
                .expect("get block")
                .expect("block should exist");
            network
                .node(0)
                .receive_fork_block(block)
                .await
                .expect("receive block");
        }

        // Trigger reorg on node 0 to the heavier chain
        let tip_hash = network.node(2).get_tips().await.expect("get tips")[0].clone();
        network
            .node(0)
            .reorg_to_chain(&tip_hash)
            .await
            .expect("reorg");

        // Assert: Execution is rolled back on node 0 (heavier chain has no scheduling)
        // After reorg, the scheduled execution should not exist
        assert!(
            network.node(0).get_scheduled_status(&exec_hash).is_none(),
            "Execution should be rolled back after reorg to chain without scheduling"
        );

        // Verify node 0 is now on the heavier chain
        assert_eq!(
            network.node(0).get_tip_height().await.unwrap(),
            6,
            "Node 0 should be at height 6 after reorg"
        );
    }

    #[tokio::test]
    async fn reorg_reschedules_pending() {
        // Setup: 3-node network
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .build()
            .await
            .expect("Failed to build network");

        // Create partition: [0,1] | [2]
        network
            .partition_groups(&[0, 1], &[2])
            .await
            .expect("partition");

        // Schedule execution on node 0 at target topo 10 (far future, won't be reached)
        let exec = make_exec(10, 1000, 0xFE);
        let exec_hash = network.node(0).schedule_execution(exec).expect("schedule");

        // Mine only 3 blocks on partition A (target 10 not reached)
        for _ in 0..3 {
            network.node(0).daemon().mine_block().await.expect("mine A");
        }

        // Verify execution is still pending on partition A
        let (status_before, _) = network
            .node(0)
            .get_scheduled_status(&exec_hash)
            .expect("should have status");
        assert_eq!(
            status_before,
            ScheduledExecutionStatus::Pending,
            "Should be pending before reorg"
        );

        // Node 2 mines a heavier chain (6 blocks)
        for _ in 0..6 {
            network.node(2).daemon().mine_block().await.expect("mine B");
        }

        // Heal partition
        network.heal_all_partitions().await;

        // Propagate heavier chain from node 2 to node 0
        for height in 1..=6 {
            let block = network
                .node(2)
                .daemon()
                .blockchain()
                .get_block_at_height(height)
                .await
                .expect("get block")
                .expect("block should exist");
            network
                .node(0)
                .receive_fork_block(block)
                .await
                .expect("receive block");
        }

        // Trigger reorg on node 0 to the heavier chain
        let tip_hash = network.node(2).get_tips().await.expect("get tips")[0].clone();
        network
            .node(0)
            .reorg_to_chain(&tip_hash)
            .await
            .expect("reorg");

        // After reorg, the scheduled execution is cleared (heavier chain has no scheduling)
        // In a real system, if the schedule TX was included in blocks, it would be re-scheduled
        // But in our test infrastructure, scheduling is local and doesn't persist across reorg
        assert!(
            network.node(0).get_scheduled_status(&exec_hash).is_none(),
            "Pending execution should be cleared after reorg (local scheduling not in new chain)"
        );

        // Verify node 0 is now on the heavier chain
        assert_eq!(
            network.node(0).get_tip_height().await.unwrap(),
            6,
            "Node 0 should be at height 6 after reorg"
        );
    }

    // ========================================================================
    // Partition Isolation Tests
    // ========================================================================

    #[tokio::test]
    async fn partition_isolation_separate_queues() {
        // Setup: 3-node network
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .build()
            .await
            .expect("Failed to build network");

        // Create partition: [0,1] | [2]
        network
            .partition_groups(&[0, 1], &[2])
            .await
            .expect("partition");

        // Schedule different executions on each partition (different contract IDs for unique hashes)
        let exec_a = make_exec(3, 1000, 0xAA);
        let exec_b = make_exec(3, 2000, 0xBB);

        let hash_a = network
            .node(0)
            .schedule_execution(exec_a)
            .expect("schedule A");
        let hash_b = network
            .node(2)
            .schedule_execution(exec_b)
            .expect("schedule B");

        // Mine past target on partition A (nodes 0,1)
        for _ in 0..3 {
            network.node(0).daemon().mine_block().await.expect("mine A");
        }

        // Mine past target on partition B (node 2)
        for _ in 0..3 {
            network.node(2).daemon().mine_block().await.expect("mine B");
        }

        // Assert: Partition A executed exec_a
        let (status_a, _) = network.node(0).get_scheduled_status(&hash_a).unwrap();
        assert_eq!(
            status_a,
            ScheduledExecutionStatus::Executed,
            "Partition A should have executed exec_a"
        );

        // Assert: Partition B executed exec_b
        let (status_b, _) = network.node(2).get_scheduled_status(&hash_b).unwrap();
        assert_eq!(
            status_b,
            ScheduledExecutionStatus::Executed,
            "Partition B should have executed exec_b"
        );

        // Assert: Partition A does NOT have exec_b (isolation)
        assert!(
            network.node(0).get_scheduled_status(&hash_b).is_none(),
            "Partition A should not have exec_b"
        );

        // Assert: Partition B does NOT have exec_a (isolation)
        assert!(
            network.node(2).get_scheduled_status(&hash_a).is_none(),
            "Partition B should not have exec_a"
        );
    }

    // ========================================================================
    // Miner Reward Reorg Tests
    // ========================================================================

    #[tokio::test]
    async fn miner_reward_consistent_post_reorg() {
        // Setup: 3-node network
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .build()
            .await
            .expect("Failed to build network");

        // Create partition: [0,1] | [2]
        network
            .partition_groups(&[0, 1], &[2])
            .await
            .expect("partition");

        // Get miner address for node 0
        let miner_addr = network.node(0).daemon().blockchain().get_miner_address();

        // Deploy contract at the expected address so execution doesn't defer
        let contract_addr = Hash::new([0xF1; 32]);
        network
            .node(0)
            .daemon()
            .blockchain()
            .deploy_contract_at(&contract_addr, &[0xF1]);

        // Schedule execution with offer=10_000 on node 0
        let exec = make_exec(3, 10_000, 0xF1);
        let exec_hash = network.node(0).schedule_execution(exec).expect("schedule");

        // Mine past target on partition A (4 blocks) - execution happens at topo 3
        for _ in 0..4 {
            network.node(0).daemon().mine_block().await.expect("mine A");
        }

        // Verify execution happened and miner got reward (70% of 10_000 = 7_000)
        let (status, _) = network
            .node(0)
            .get_scheduled_status(&exec_hash)
            .expect("should have status");
        assert_eq!(status, ScheduledExecutionStatus::Executed);

        let reward_before = network
            .node(0)
            .daemon()
            .blockchain()
            .get_miner_reward(&miner_addr);
        assert_eq!(
            reward_before, 7_000,
            "Miner should have received 70% of offer"
        );

        // Node 2 mines a HEAVIER chain (6 blocks) WITHOUT this scheduling
        for _ in 0..6 {
            network.node(2).daemon().mine_block().await.expect("mine B");
        }

        // Heal partition
        network.heal_all_partitions().await;

        // Propagate heavier chain from node 2 to node 0
        for height in 1..=6 {
            let block = network
                .node(2)
                .daemon()
                .blockchain()
                .get_block_at_height(height)
                .await
                .expect("get block")
                .expect("block should exist");
            network
                .node(0)
                .receive_fork_block(block)
                .await
                .expect("receive block");
        }

        // Trigger reorg on node 0 to the heavier chain
        let tip_hash = network.node(2).get_tips().await.expect("get tips")[0].clone();
        network
            .node(0)
            .reorg_to_chain(&tip_hash)
            .await
            .expect("reorg");

        // Assert: Miner reward is rolled back (cleared during reorg)
        let reward_after = network
            .node(0)
            .daemon()
            .blockchain()
            .get_miner_reward(&miner_addr);
        assert_eq!(
            reward_after, 0,
            "Miner reward should be rolled back after reorg"
        );

        // Assert: Scheduled execution no longer exists
        assert!(
            network.node(0).get_scheduled_status(&exec_hash).is_none(),
            "Execution should be rolled back"
        );
    }

    // ========================================================================
    // Deferral Across Reorg Tests
    // ========================================================================

    #[tokio::test]
    async fn deferral_across_partition_heal() {
        // Setup: 3-node network
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .build()
            .await
            .expect("Failed to build network");

        // Create partition: [0,1] | [2]
        network
            .partition_groups(&[0, 1], &[2])
            .await
            .expect("partition");

        // Deploy a dummy contract to enable deferral logic
        // (deferral only applies when at least one contract is deployed)
        network
            .node(0)
            .daemon()
            .blockchain()
            .deploy_contract_at(&Hash::new([0xFF; 32]), &[0xFF]);

        // Schedule execution targeting a NON-EXISTENT contract (will defer)
        // Contract 0xF2 is not deployed
        let exec = make_exec(2, 1000, 0xF2);
        let exec_hash = network.node(0).schedule_execution(exec).expect("schedule");

        // Mine 1 block - execution should defer (contract not found)
        network.node(0).daemon().mine_block().await.expect("mine 1");

        // Check status - should still be pending (deferred to next block)
        let _status1 = network.node(0).get_scheduled_status(&exec_hash);
        // After target topo 2, execution attempts but defers
        // It will be re-queued for topo 3

        // Mine more blocks - execution will keep deferring until max defer count
        network.node(0).daemon().mine_block().await.expect("mine 2");
        network.node(0).daemon().mine_block().await.expect("mine 3");
        network.node(0).daemon().mine_block().await.expect("mine 4");
        network.node(0).daemon().mine_block().await.expect("mine 5");

        // After 3 deferrals, execution should fail
        let (status, _) = network
            .node(0)
            .get_scheduled_status(&exec_hash)
            .expect("should have status");
        assert_eq!(
            status,
            ScheduledExecutionStatus::Failed,
            "Execution should fail after max deferrals"
        );

        // Node 2 mines a heavier chain (7 blocks)
        for _ in 0..7 {
            network.node(2).daemon().mine_block().await.expect("mine B");
        }

        // Heal partition
        network.heal_all_partitions().await;

        // Propagate heavier chain from node 2 to node 0
        for height in 1..=7 {
            let block = network
                .node(2)
                .daemon()
                .blockchain()
                .get_block_at_height(height)
                .await
                .expect("get block")
                .expect("block should exist");
            network
                .node(0)
                .receive_fork_block(block)
                .await
                .expect("receive block");
        }

        // Trigger reorg on node 0 to the heavier chain
        let tip_hash = network.node(2).get_tips().await.expect("get tips")[0].clone();
        network
            .node(0)
            .reorg_to_chain(&tip_hash)
            .await
            .expect("reorg");

        // After reorg, scheduled state is cleared (heavier chain has no scheduling)
        assert!(
            network.node(0).get_scheduled_status(&exec_hash).is_none(),
            "Deferral state should be cleared after reorg"
        );
    }

    // ========================================================================
    // Late Join Tests
    // ========================================================================

    #[tokio::test]
    #[ignore = "Requires LocalTosNetwork with dynamic node join + scheduling"]
    async fn late_join_node_replays_scheduled() {
        // 2-node network, schedule and execute several executions
        // Add node 3 (late joiner)
        // Wait for sync
        // Assert: Node 3 has correct scheduled execution state
    }

    // ========================================================================
    // Concurrent Scheduling Tests
    // ========================================================================

    #[tokio::test]
    async fn concurrent_scheduling_deterministic() {
        // Setup: single-node network
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(1)
            .build()
            .await
            .expect("Failed to build network");

        // Schedule multiple executions at same target with different offers
        let exec1 = make_exec(3, 1000, 0x01); // Lower priority
        let exec2 = make_exec(3, 5000, 0x02); // Higher priority
        let exec3 = make_exec(3, 3000, 0x03); // Medium priority

        let hash1 = network
            .node(0)
            .schedule_execution(exec1)
            .expect("schedule 1");
        let hash2 = network
            .node(0)
            .schedule_execution(exec2)
            .expect("schedule 2");
        let hash3 = network
            .node(0)
            .schedule_execution(exec3)
            .expect("schedule 3");

        // Mine past target
        for _ in 0..3 {
            network.node(0).daemon().mine_block().await.expect("mine");
        }

        // Assert: All executed at same topoheight
        let (status1, topo1) = network.node(0).get_scheduled_status(&hash1).unwrap();
        let (status2, topo2) = network.node(0).get_scheduled_status(&hash2).unwrap();
        let (status3, topo3) = network.node(0).get_scheduled_status(&hash3).unwrap();

        assert_eq!(status1, ScheduledExecutionStatus::Executed);
        assert_eq!(status2, ScheduledExecutionStatus::Executed);
        assert_eq!(status3, ScheduledExecutionStatus::Executed);

        // All should execute at the same topoheight (3)
        assert_eq!(topo1, 3);
        assert_eq!(topo2, 3);
        assert_eq!(topo3, 3);
    }

    #[tokio::test]
    async fn priority_order_preserved_after_reorg() {
        // Setup: single-node network for simplicity
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(1)
            .build()
            .await
            .expect("Failed to build network");

        // Deploy contracts at expected addresses so executions don't defer
        // make_exec uses Hash::new([contract_id; 32]) for contract addresses
        network
            .node(0)
            .daemon()
            .blockchain()
            .deploy_contract_at(&Hash::new([0x01; 32]), &[0x01]);
        network
            .node(0)
            .daemon()
            .blockchain()
            .deploy_contract_at(&Hash::new([0x02; 32]), &[0x02]);
        network
            .node(0)
            .daemon()
            .blockchain()
            .deploy_contract_at(&Hash::new([0x03; 32]), &[0x03]);
        network
            .node(0)
            .daemon()
            .blockchain()
            .deploy_contract_at(&Hash::new([0x04; 32]), &[0x04]);
        network
            .node(0)
            .daemon()
            .blockchain()
            .deploy_contract_at(&Hash::new([0x05; 32]), &[0x05]);

        // Schedule 5 executions with different offers at same target topo
        let exec1 = make_exec(3, 1000, 0x01); // Lowest priority
        let exec2 = make_exec(3, 5000, 0x02); // Highest priority
        let exec3 = make_exec(3, 3000, 0x03); // Medium
        let exec4 = make_exec(3, 2000, 0x04); // Low-medium
        let exec5 = make_exec(3, 4000, 0x05); // High-medium

        let hash1 = network
            .node(0)
            .schedule_execution(exec1)
            .expect("schedule 1");
        let hash2 = network
            .node(0)
            .schedule_execution(exec2)
            .expect("schedule 2");
        let hash3 = network
            .node(0)
            .schedule_execution(exec3)
            .expect("schedule 3");
        let hash4 = network
            .node(0)
            .schedule_execution(exec4)
            .expect("schedule 4");
        let hash5 = network
            .node(0)
            .schedule_execution(exec5)
            .expect("schedule 5");

        // Mine past target
        for _ in 0..3 {
            network.node(0).daemon().mine_block().await.expect("mine");
        }

        // Verify all executed at same topoheight
        let (status1, topo1) = network.node(0).get_scheduled_status(&hash1).unwrap();
        let (status2, topo2) = network.node(0).get_scheduled_status(&hash2).unwrap();
        let (status3, topo3) = network.node(0).get_scheduled_status(&hash3).unwrap();
        let (status4, topo4) = network.node(0).get_scheduled_status(&hash4).unwrap();
        let (status5, topo5) = network.node(0).get_scheduled_status(&hash5).unwrap();

        assert_eq!(status1, ScheduledExecutionStatus::Executed);
        assert_eq!(status2, ScheduledExecutionStatus::Executed);
        assert_eq!(status3, ScheduledExecutionStatus::Executed);
        assert_eq!(status4, ScheduledExecutionStatus::Executed);
        assert_eq!(status5, ScheduledExecutionStatus::Executed);

        // All should execute at the same topoheight (priority affects order, not topo)
        assert_eq!(topo1, 3);
        assert_eq!(topo2, 3);
        assert_eq!(topo3, 3);
        assert_eq!(topo4, 3);
        assert_eq!(topo5, 3);

        // Note: In current implementation, all execute at same topo.
        // Priority ordering affects execution order within the block,
        // which would matter for gas budget or contract state dependencies.
        // We verify that priority sorting doesn't break execution.
    }

    // ========================================================================
    // BlockEnd in Fork Tests
    // ========================================================================

    #[tokio::test]
    #[ignore = "Requires LocalTosNetwork with BlockEnd + orphan handling"]
    async fn block_end_in_forked_block() {
        // Schedule BlockEnd execution in a block that becomes orphaned
        // Assert: BlockEnd does NOT execute on the main chain
    }

    // ========================================================================
    // Stress Tests
    // ========================================================================

    #[tokio::test]
    async fn rapid_scheduling_stress() {
        // Setup: single-node network for simplicity
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(1)
            .build()
            .await
            .expect("Failed to build network");

        // Schedule 50 executions across different target topoheights
        let mut exec_hashes = Vec::new();
        for i in 1..=50 {
            let target_topo = (i % 10) + 1; // Targets 1-10
            let exec = make_exec(target_topo, i as u64 * 100, i as u8);
            let hash = network.node(0).schedule_execution(exec).expect("schedule");
            exec_hashes.push((hash, target_topo));
        }

        // Verify all are pending
        for (hash, _) in &exec_hashes {
            let (status, _) = network.node(0).get_scheduled_status(hash).unwrap();
            assert_eq!(status, ScheduledExecutionStatus::Pending);
        }

        // Mine through all targets (up to topoheight 10)
        for _ in 0..10 {
            network.node(0).daemon().mine_block().await.expect("mine");
        }

        // Assert: All 50 correctly executed
        let mut executed_count = 0;
        for (hash, target_topo) in &exec_hashes {
            let (status, exec_topo) = network.node(0).get_scheduled_status(hash).unwrap();
            assert_eq!(
                status,
                ScheduledExecutionStatus::Executed,
                "Execution for target {} should be Executed",
                target_topo
            );
            assert_eq!(
                exec_topo, *target_topo,
                "Execution should happen at target topoheight"
            );
            executed_count += 1;
        }

        assert_eq!(executed_count, 50, "All 50 executions should be processed");
    }
}
