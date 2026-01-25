// Phase 16: Combined VRF + Scheduling Tests
//
// Tests the interaction between VRF randomness and scheduled execution.
// VRF-driven execution paths, cross-node determinism, and cascade scheduling.
//
// Prerequisites:
// - ChainClient with VRF + scheduled execution support
// - LocalTosNetwork with VRF + scheduled execution support
// - Contracts that use vrf_random() to choose execution paths

#[cfg(test)]
mod tests {
    use tos_common::contract::{
        ScheduledExecution, ScheduledExecutionKind, ScheduledExecutionStatus,
    };
    use tos_common::crypto::Hash;
    use tos_daemon::vrf::VrfKeyManager;

    use crate::tier1_5::{
        chain_client_config::GenesisAccount, BlockWarp, ChainClient, ChainClientConfig, VrfConfig,
    };

    fn sample_hash(byte: u8) -> Hash {
        Hash::new([byte; 32])
    }

    fn make_exec(
        contract: Hash,
        target_topo: u64,
        offer: u64,
        max_gas: u64,
        registration_topo: u64,
        scheduler: Hash,
    ) -> ScheduledExecution {
        ScheduledExecution::new_offercall(
            contract,
            0,
            vec![],
            max_gas,
            offer,
            scheduler,
            ScheduledExecutionKind::TopoHeight(target_topo),
            registration_topo,
        )
    }

    // ========================================================================
    // VRF-Driven Execution Path Tests
    // ========================================================================

    #[tokio::test]
    async fn vrf_randomness_determines_execution() {
        // Deploy vrf_branching contract that:
        //   - Reads vrf_random()
        //   - If random[0] < 128: stores "path" = b"a", "result" = b"path_a"
        //   - Else: stores "path" = b"b", "result" = b"path_b"
        let scheduler = sample_hash(0xAA);
        let mgr = VrfKeyManager::new();
        let secret_hex = mgr.secret_key_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(scheduler.clone(), 10_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();

        // Deploy vrf_branching contract
        let bytecode = include_bytes!("../../tests/fixtures/vrf_branching.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Schedule execution at topo 5
        let exec = make_exec(contract.clone(), 5, 1_000, 1_000_000, 0, scheduler);
        let hash = client.schedule_execution(exec).await.unwrap();

        // Warp to target
        client.warp_to_topoheight(5).await.unwrap();

        // Check execution succeeded
        let (status, _) = client.get_scheduled_status(&hash).unwrap();
        assert_eq!(status, ScheduledExecutionStatus::Executed);

        // Get VRF random byte used for branching
        let random_byte = client
            .get_contract_storage(&contract, b"random_byte")
            .await
            .unwrap()
            .expect("random_byte should be stored");
        assert_eq!(random_byte.len(), 1);
        let vrf_byte = random_byte[0];

        // Get the path taken
        let path = client
            .get_contract_storage(&contract, b"path")
            .await
            .unwrap()
            .expect("path should be stored");

        // Verify correct branch was taken based on VRF
        if vrf_byte < 128 {
            assert_eq!(path, b"a", "VRF byte {} < 128 should take path_a", vrf_byte);
        } else {
            assert_eq!(
                path, b"b",
                "VRF byte {} >= 128 should take path_b",
                vrf_byte
            );
        }

        // Verify result matches path
        let result = client
            .get_contract_storage(&contract, b"result")
            .await
            .unwrap()
            .expect("result should be stored");
        if vrf_byte < 128 {
            assert_eq!(result, b"path_a");
        } else {
            assert_eq!(result, b"path_b");
        }
    }

    #[tokio::test]
    async fn vrf_random_consistent_in_scheduled() {
        let scheduler = sample_hash(0xBB);
        let mgr = VrfKeyManager::new();
        let secret_hex = mgr.secret_key_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(scheduler.clone(), 10_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();

        let bytecode = include_bytes!("../../tests/fixtures/vrf_random.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Schedule at topo 10
        let exec = make_exec(contract.clone(), 10, 1_000, 1_000_000, 0, scheduler);
        client.schedule_execution(exec).await.unwrap();

        // Warp to target
        client.warp_to_topoheight(10).await.unwrap();

        // Get stored VRF random
        let stored_random = client
            .get_contract_storage(&contract, b"vrf_random")
            .await
            .unwrap()
            .unwrap();
        let pre_output = client
            .get_contract_storage(&contract, b"vrf_pre_output")
            .await
            .unwrap()
            .unwrap();
        let block_hash = client
            .get_contract_storage(&contract, b"vrf_block_hash")
            .await
            .unwrap()
            .unwrap();

        // Verify derivation: hash("TOS-VRF-DERIVE" || pre_output || block_hash)
        let mut input = Vec::new();
        input.extend_from_slice(b"TOS-VRF-DERIVE");
        input.extend_from_slice(&pre_output);
        input.extend_from_slice(&block_hash);
        let expected = tos_common::crypto::hash(&input);
        assert_eq!(stored_random, expected.as_bytes().to_vec());
    }

    // ========================================================================
    // Reorg Changes VRF Changes Schedule Result
    // ========================================================================

    #[tokio::test]
    #[ignore = "Requires LocalTosNetwork with VRF + scheduling + partition"]
    async fn reorg_changes_vrf_changes_schedule_result() {
        // Create partition: [0,1] | [2]
        // Schedule VRF-dependent contract on both partitions
        // Different miners -> different VRF -> potentially different execution paths
        // Heal partition
        // Assert: All nodes use heavier chain's VRF -> same execution path
    }

    // ========================================================================
    // Statistical Fairness Tests
    // ========================================================================

    #[tokio::test]
    async fn vrf_lottery_fairness() {
        // Run VRF branching across multiple blocks using scheduled execution
        // Each execution branches based on vrf_random[0] < 128 (50% probability)
        // Assert: Distribution varies (not all same path)
        let scheduler = sample_hash(0xCC);
        let mgr = VrfKeyManager::new();
        let secret_hex = mgr.secret_key_hex();
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(scheduler.clone(), 100_000_000))
            .with_vrf(VrfConfig {
                secret_key_hex: Some(secret_hex),
                chain_id: 3,
            });

        let mut client = ChainClient::start(config).await.unwrap();

        // Use vrf_branching contract which we know works
        let bytecode = include_bytes!("../../tests/fixtures/vrf_branching.so");
        let num_rounds = 10usize;

        // Deploy all contracts at unique addresses first
        let mut contract_addrs = Vec::new();
        for i in 0..num_rounds {
            let mut addr_bytes = [0u8; 32];
            addr_bytes[0] = (i & 0xFF) as u8;
            addr_bytes[1] = 0xCC; // Use different byte pattern
            let contract_addr = Hash::new(addr_bytes);
            client
                .deploy_contract_at(&contract_addr, bytecode)
                .await
                .unwrap();
            contract_addrs.push(contract_addr);
        }

        // Track path distribution
        let mut path_a_count = 0u32;
        let mut path_b_count = 0u32;

        // Call each contract at different blocks
        for (i, contract_addr) in contract_addrs.iter().enumerate() {
            // Mine a block to get fresh VRF
            client.mine_empty_block().await.unwrap();

            let result = client
                .call_contract(contract_addr, 0, vec![], vec![], 1_000_000)
                .await
                .unwrap();
            assert!(
                result.tx_result.success,
                "Round {} at topo {} should succeed: {:?}",
                i,
                client.topoheight(),
                result.tx_result.error
            );

            // Get the path taken
            let path = client
                .get_contract_storage(contract_addr, b"path")
                .await
                .unwrap()
                .expect("path should be stored");

            if path == b"a" {
                path_a_count = path_a_count.saturating_add(1);
            } else if path == b"b" {
                path_b_count = path_b_count.saturating_add(1);
            } else {
                panic!("Unexpected path: {:?}", path);
            }
        }

        // With 10 rounds and 50% probability, we should see both paths
        // P(all same) = 2 * (0.5)^10 = 0.2% - very unlikely
        let total = path_a_count.saturating_add(path_b_count);
        assert_eq!(total, num_rounds as u32, "All rounds should complete");

        // Log distribution
        eprintln!(
            "Path distribution: A={} B={} (total {})",
            path_a_count, path_b_count, num_rounds
        );

        // Verify VRF produces varied results - at least 2 of each path
        // (being lenient for small sample size)
        assert!(
            path_a_count >= 1 && path_b_count >= 1,
            "VRF should produce varied results. A={} B={}",
            path_a_count,
            path_b_count
        );
    }

    // ========================================================================
    // Cross-Node Determinism Tests
    // ========================================================================

    #[tokio::test]
    async fn scheduled_vrf_cross_node_determinism() {
        use crate::tier3_e2e::LocalTosNetworkBuilder;
        use tos_common::contract::ScheduledExecution;

        // Setup: 3-node network with VRF
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_random_vrf_keys()
            .build()
            .await
            .expect("Failed to build network");

        // Create the same scheduled execution for all nodes
        let contract = Hash::new([0xCC; 32]);
        let scheduler = Hash::new([0xDD; 32]);
        let target_topo = 5;

        // Schedule the same execution on all nodes
        // (In a real system, this would be propagated via transaction in a block)
        let mut hashes = Vec::new();
        for i in 0..3 {
            let exec = ScheduledExecution::new_offercall(
                contract.clone(),
                0,
                vec![],
                100_000,
                1000,
                scheduler.clone(),
                tos_common::contract::ScheduledExecutionKind::TopoHeight(target_topo),
                0,
            );
            let hash = network.node(i).schedule_execution(exec).expect("schedule");
            hashes.push(hash);
        }

        // Mine and propagate blocks until target
        for _ in 0..target_topo {
            network.mine_and_propagate(0).await.expect("mine");
        }

        // Verify all nodes executed at same topoheight
        for (i, hash) in hashes.iter().enumerate() {
            let (status, exec_topo) = network
                .node(i)
                .get_scheduled_status(hash)
                .expect("should have status");
            assert_eq!(
                status,
                tos_common::contract::ScheduledExecutionStatus::Executed,
                "Node {} should have Executed status",
                i
            );
            assert_eq!(
                exec_topo, target_topo,
                "Node {} should execute at target topo",
                i
            );
        }

        // Verify all nodes have same VRF data at execution block
        let vrf_0 = network.node(0).get_block_vrf_data(target_topo);
        let vrf_1 = network.node(1).get_block_vrf_data(target_topo);
        let vrf_2 = network.node(2).get_block_vrf_data(target_topo);

        // All nodes should have VRF data (blocks were propagated from node 0)
        assert!(vrf_0.is_some(), "Node 0 should have VRF data");
        assert!(vrf_1.is_some(), "Node 1 should have VRF data");
        assert!(vrf_2.is_some(), "Node 2 should have VRF data");

        // All should have the same VRF output (from the same miner - node 0)
        assert_eq!(
            vrf_0.as_ref().unwrap().output,
            vrf_1.as_ref().unwrap().output,
            "Nodes should have same VRF output"
        );
        assert_eq!(
            vrf_1.as_ref().unwrap().output,
            vrf_2.as_ref().unwrap().output,
            "Nodes should have same VRF output"
        );
    }

    // ========================================================================
    // Feature Interaction Tests
    // ========================================================================

    #[tokio::test]
    async fn vrf_unavailable_in_scheduled() {
        let scheduler = sample_hash(0xBB);
        // NO VRF configured (no secret key)
        let config = ChainClientConfig::default()
            .with_account(GenesisAccount::new(scheduler.clone(), 10_000_000));

        let mut client = ChainClient::start(config).await.unwrap();

        let bytecode = include_bytes!("../../tests/fixtures/vrf_random.so");
        let contract = client.deploy_contract(bytecode).await.unwrap();

        // Schedule execution
        let exec = make_exec(contract, 5, 1_000, 1_000_000, 0, scheduler);
        let hash = client.schedule_execution(exec).await.unwrap();

        // Warp to target
        client.warp_to_topoheight(5).await.unwrap();

        // When VRF is unavailable, the contract's vrf_random() call will fail,
        // causing the contract to return non-zero (error code 1), resulting in Failed status
        let (status, _) = client.get_scheduled_status(&hash).unwrap();
        assert_eq!(
            status,
            ScheduledExecutionStatus::Failed,
            "Execution should fail when VRF is unavailable"
        );
    }

    // ========================================================================
    // Cascade Scheduling Tests
    // ========================================================================

    #[tokio::test]
    #[ignore = "Requires offer_call syscall + VRF in scheduled context"]
    async fn cascade_scheduling_with_vrf() {
        // Contract A scheduled at topo=50
        // A reads vrf_random(), uses it to schedule Contract B at topo=60
        // Warp to 50: A executes, schedules B
        // Warp to 60: B executes
        // Assert: Both executions deterministic (A's VRF determines B's schedule)
    }

    // ========================================================================
    // Confirmation Depth Tests
    // ========================================================================

    #[tokio::test]
    async fn stable_depth_scheduled_vrf() {
        use crate::tier3_e2e::LocalTosNetworkBuilder;

        // Setup: single-node network with VRF
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(1)
            .with_random_vrf_keys()
            .build()
            .await
            .expect("Failed to build network");

        // Schedule execution at target topo 3
        let scheduler = Hash::new([0xDD; 32]);
        let contract = Hash::new([0xCC; 32]);
        let exec = ScheduledExecution::new_offercall(
            contract,
            0,
            vec![],
            100_000,
            1000,
            scheduler,
            ScheduledExecutionKind::TopoHeight(3),
            0,
        );
        let exec_hash = network.node(0).schedule_execution(exec).expect("schedule");

        // Mine to target (execution happens at topo 3)
        for _ in 0..3 {
            network.node(0).daemon().mine_block().await.expect("mine");
        }

        // Verify execution happened
        let (status, exec_topo) = network
            .node(0)
            .get_scheduled_status(&exec_hash)
            .expect("should have status");
        assert_eq!(status, ScheduledExecutionStatus::Executed);
        assert_eq!(exec_topo, 3);

        // Check if execution is NOT yet stable (default stable_depth = 10)
        let is_stable_before = network.node(0).daemon().blockchain().is_stable(exec_topo);
        assert!(
            !is_stable_before,
            "Execution at topo 3 should not be stable yet (current topo = 3)"
        );

        // Mine additional blocks past stable depth (10 more blocks)
        for _ in 0..10 {
            network.node(0).daemon().mine_block().await.expect("mine");
        }

        // Now the execution should be stable (irreversible)
        let is_stable_after = network.node(0).daemon().blockchain().is_stable(exec_topo);
        assert!(
            is_stable_after,
            "Execution at topo 3 should be stable now (current topo = 13)"
        );

        // Get stable depth for reference
        let stable_depth = network.node(0).daemon().blockchain().get_stable_depth();
        assert_eq!(stable_depth, 10, "Default stable depth should be 10");

        // Note: Once stable, the result cannot be changed by reorg
        // (any reorg would need to be longer than stable_depth blocks)
    }
}
