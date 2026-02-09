// Phase 16: VRF Multi-Node Consensus Tests (Layer 3)
//
// Tests VRF consistency across multiple nodes using LocalTosNetwork.
// Uses LocalTosNetwork with per-node VRF key configuration.

#[cfg(test)]
mod tests {
    use crate::tier2_integration::NodeRpc;
    use crate::tier3_e2e::LocalTosNetworkBuilder;

    // ========================================================================
    // Cross-Node VRF Agreement Tests
    // ========================================================================

    #[tokio::test]
    async fn all_nodes_agree_on_vrf_output() {
        // Setup: 3-node network with VRF keys
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_random_vrf_keys()
            .build()
            .await
            .expect("Failed to build network");

        // Verify all nodes have VRF configured
        for i in 0..3 {
            assert!(network.node(i).has_vrf(), "Node {} should have VRF", i);
        }

        // Node 0 mines a block
        network.node(0).daemon().mine_block().await.expect("mine");

        // Get VRF from node 0
        let vrf_node0 = network
            .node(0)
            .get_block_vrf_data(1)
            .expect("VRF data should be present on node 0");

        // Propagate to other nodes
        network.propagate_block_from(0, 1).await.expect("propagate");

        // All nodes should have the same VRF data
        let vrf_node1 = network
            .node(1)
            .get_block_vrf_data(1)
            .expect("VRF data should be present on node 1");
        let vrf_node2 = network
            .node(2)
            .get_block_vrf_data(1)
            .expect("VRF data should be present on node 2");

        assert_eq!(
            vrf_node0.output, vrf_node1.output,
            "Node 0 and 1 should agree on VRF output"
        );
        assert_eq!(
            vrf_node0.output, vrf_node2.output,
            "Node 0 and 2 should agree on VRF output"
        );
        assert_eq!(
            vrf_node0.public_key, vrf_node1.public_key,
            "Node 0 and 1 should agree on VRF public key"
        );
        assert_eq!(
            vrf_node0.proof, vrf_node1.proof,
            "Node 0 and 1 should agree on VRF proof"
        );
    }

    #[tokio::test]
    async fn vrf_consistent_after_propagation() {
        // Setup: 3-node network with VRF
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_random_vrf_keys()
            .build()
            .await
            .expect("Failed to build network");

        // Node 0 mines a block with VRF data
        network.node(0).daemon().mine_block().await.expect("mine");

        // Get original VRF data before propagation
        let original_vrf = network.node(0).get_block_vrf_data(1).expect("VRF data");

        // Propagate to nodes 1 and 2
        let propagated_count = network.propagate_block_from(0, 1).await.expect("propagate");
        assert_eq!(propagated_count, 2, "Should propagate to 2 peers");

        // Verify nodes 1 and 2 stored the VRF data correctly
        let vrf_node1 = network
            .node(1)
            .get_block_vrf_data(1)
            .expect("VRF on node 1");
        let vrf_node2 = network
            .node(2)
            .get_block_vrf_data(1)
            .expect("VRF on node 2");

        // VRF data should be identical to original
        assert_eq!(original_vrf.output, vrf_node1.output);
        assert_eq!(original_vrf.output, vrf_node2.output);
        assert_eq!(original_vrf.proof, vrf_node1.proof);
        assert_eq!(original_vrf.proof, vrf_node2.proof);
        assert_eq!(original_vrf.binding_signature, vrf_node1.binding_signature);
        assert_eq!(original_vrf.binding_signature, vrf_node2.binding_signature);
    }

    #[tokio::test]
    async fn vrf_output_survives_partition_heal() {
        // Setup: 3-node network with VRF keys
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_random_vrf_keys()
            .build()
            .await
            .expect("Failed to build network");

        // Create partition: [0,1] | [2]
        network
            .partition_groups(&[0, 1], &[2])
            .await
            .expect("partition");

        // Node 0 mines 3 blocks in partition A
        for _ in 0..3 {
            network.node(0).daemon().mine_block().await.expect("mine");
        }

        // Propagate within partition A (node 0 -> node 1)
        for height in 1..=3 {
            network
                .propagate_block_from(0, height)
                .await
                .expect("propagate within partition");
        }

        // Verify node 1 has VRF data but node 2 does not (partitioned)
        assert!(
            network.node(1).get_block_vrf_data(3).is_some(),
            "Node 1 should have VRF data"
        );
        assert!(
            network.node(2).get_block_vrf_data(1).is_none(),
            "Node 2 should NOT have VRF data (partitioned)"
        );

        // Get VRF data from partition A before healing
        let vrf_at_height_1 = network.node(0).get_block_vrf_data(1).expect("VRF 1");
        let vrf_at_height_2 = network.node(0).get_block_vrf_data(2).expect("VRF 2");
        let vrf_at_height_3 = network.node(0).get_block_vrf_data(3).expect("VRF 3");

        // Heal partition
        network.heal_all_partitions().await;

        // Now propagate blocks to node 2
        for height in 1..=3 {
            network
                .propagate_block_from(0, height)
                .await
                .expect("propagate after heal");
        }

        // Verify node 2 now has correct VRF data
        let vrf_node2_h1 = network
            .node(2)
            .get_block_vrf_data(1)
            .expect("VRF should be present on node 2 height 1");
        let vrf_node2_h2 = network
            .node(2)
            .get_block_vrf_data(2)
            .expect("VRF should be present on node 2 height 2");
        let vrf_node2_h3 = network
            .node(2)
            .get_block_vrf_data(3)
            .expect("VRF should be present on node 2 height 3");

        // VRF data should match original from partition A
        assert_eq!(vrf_at_height_1.output, vrf_node2_h1.output);
        assert_eq!(vrf_at_height_2.output, vrf_node2_h2.output);
        assert_eq!(vrf_at_height_3.output, vrf_node2_h3.output);
        assert_eq!(vrf_at_height_1.proof, vrf_node2_h1.proof);
        assert_eq!(vrf_at_height_2.proof, vrf_node2_h2.proof);
        assert_eq!(vrf_at_height_3.proof, vrf_node2_h3.proof);
    }

    #[tokio::test]
    async fn different_miners_different_vrf() {
        // Setup: 2-node network with different VRF keys (no propagation)
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(2)
            .with_random_vrf_keys()
            .build()
            .await
            .expect("Failed to build network");

        // Node 0 mines a block
        network
            .node(0)
            .daemon()
            .mine_block()
            .await
            .expect("mine node 0");

        // Node 1 mines a block (independently, not receiving from node 0)
        network
            .node(1)
            .daemon()
            .mine_block()
            .await
            .expect("mine node 1");

        // Get VRF outputs
        let vrf_node0 = network.node(0).get_block_vrf_data(1).expect("VRF node 0");
        let vrf_node1 = network.node(1).get_block_vrf_data(1).expect("VRF node 1");

        // VRF outputs should differ because:
        // 1. Different miner keys (different VRF secret keys)
        // 2. Different block hashes (inputs to VRF)
        assert_ne!(
            vrf_node0.output, vrf_node1.output,
            "Different miners should produce different VRF outputs"
        );
        assert_ne!(
            vrf_node0.public_key, vrf_node1.public_key,
            "Different VRF keys means different public keys"
        );
    }

    // ========================================================================
    // VRF Reorg Consistency Tests
    // ========================================================================

    #[tokio::test]
    async fn vrf_reorg_consistency() {
        // Setup: 3-node network with VRF keys
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_random_vrf_keys()
            .build()
            .await
            .expect("Failed to build network");

        // Create partition: [0,1] | [2]
        network
            .partition_groups(&[0, 1], &[2])
            .await
            .expect("partition");

        // Partition A (nodes 0,1) mines 5 blocks
        for _ in 0..5 {
            network.node(0).daemon().mine_block().await.expect("mine A");
        }
        // Propagate within partition A
        for height in 1..=5 {
            network
                .propagate_block_from(0, height)
                .await
                .expect("propagate A");
        }

        // Partition B (node 2) mines 3 blocks
        for _ in 0..3 {
            network.node(2).daemon().mine_block().await.expect("mine B");
        }

        // Capture VRF data from heavier chain (partition A) before heal
        let vrf_a1 = network.node(0).get_block_vrf_data(1).expect("VRF A1");
        let vrf_a3 = network.node(0).get_block_vrf_data(3).expect("VRF A3");
        let vrf_a5 = network.node(0).get_block_vrf_data(5).expect("VRF A5");

        // Get tip hash from partition A (heavier chain)
        let tips_a = network.node(0).daemon().get_tips().await.expect("tips A");
        let heavier_tip = tips_a[0].clone();

        // Verify partition B has different VRF (different miner)
        let vrf_b1 = network.node(2).get_block_vrf_data(1).expect("VRF B1");
        assert_ne!(
            vrf_a1.output, vrf_b1.output,
            "Different partitions should have different VRF"
        );

        // Heal partition
        network.heal_all_partitions().await;

        // Send heavier chain blocks to node 2
        for height in 1..=5 {
            let block = network
                .node(0)
                .daemon()
                .get_block_at_height(height)
                .await
                .expect("get block")
                .expect("block exists");
            network
                .node(2)
                .receive_fork_block(block)
                .await
                .expect("receive fork");
        }

        // Node 2 should reorg to heavier chain
        network
            .node(2)
            .reorg_to_chain(&heavier_tip)
            .await
            .expect("reorg");

        // Verify all nodes now have the same VRF data (from heavier chain)
        let vrf_node2_h1 = network.node(2).get_block_vrf_data(1).expect("VRF node2 h1");
        let vrf_node2_h3 = network.node(2).get_block_vrf_data(3).expect("VRF node2 h3");
        let vrf_node2_h5 = network.node(2).get_block_vrf_data(5).expect("VRF node2 h5");

        assert_eq!(
            vrf_a1.output, vrf_node2_h1.output,
            "Node 2 should have partition A's VRF at height 1 after reorg"
        );
        assert_eq!(
            vrf_a3.output, vrf_node2_h3.output,
            "Node 2 should have partition A's VRF at height 3 after reorg"
        );
        assert_eq!(
            vrf_a5.output, vrf_node2_h5.output,
            "Node 2 should have partition A's VRF at height 5 after reorg"
        );

        // Verify all nodes are at the same height
        assert_eq!(network.node(0).get_tip_height().await.unwrap(), 5);
        assert_eq!(network.node(1).get_tip_height().await.unwrap(), 5);
        assert_eq!(network.node(2).get_tip_height().await.unwrap(), 5);
    }

    #[tokio::test]
    async fn contract_vrf_consistent_cross_node() {
        use crate::tier3_e2e::LocalTosNetworkBuilder;
        use std::collections::HashMap;
        use tos_common::block::BlockVrfData;
        use tos_common::contract::{ContractCache, ContractProvider, ContractStorage};
        use tos_common::crypto::{hash, Hash, Signature};
        use tos_daemon::tako_integration::{SVMFeatureSet, TakoExecutor};
        use tos_daemon::vrf::{VrfData, VrfOutput, VrfProof, VrfPublicKey};
        use tos_kernel::ValueCell;

        #[allow(missing_docs)]
        struct InMemoryContractProvider {
            storage: HashMap<(Hash, Vec<u8>), Vec<u8>>,
            bytecodes: HashMap<Hash, Vec<u8>>,
            topoheight: u64,
        }

        impl ContractStorage for InMemoryContractProvider {
            fn load_data(
                &self,
                contract: &Hash,
                key: &ValueCell,
                _topoheight: tos_common::block::TopoHeight,
            ) -> Result<Option<(tos_common::block::TopoHeight, Option<ValueCell>)>, anyhow::Error>
            {
                let key_bytes = key
                    .as_bytes()
                    .map_err(|e| anyhow::anyhow!("{}", e))?
                    .clone();
                match self.storage.get(&(contract.clone(), key_bytes)) {
                    Some(value) => Ok(Some((
                        self.topoheight,
                        Some(ValueCell::Bytes(value.clone())),
                    ))),
                    None => Ok(None),
                }
            }

            fn load_data_latest_topoheight(
                &self,
                contract: &Hash,
                key: &ValueCell,
                _topoheight: tos_common::block::TopoHeight,
            ) -> Result<Option<tos_common::block::TopoHeight>, anyhow::Error> {
                let key_bytes = key
                    .as_bytes()
                    .map_err(|e| anyhow::anyhow!("{}", e))?
                    .clone();
                if self.storage.contains_key(&(contract.clone(), key_bytes)) {
                    Ok(Some(self.topoheight))
                } else {
                    Ok(None)
                }
            }

            fn has_data(
                &self,
                contract: &Hash,
                key: &ValueCell,
                _topoheight: tos_common::block::TopoHeight,
            ) -> Result<bool, anyhow::Error> {
                let key_bytes = key
                    .as_bytes()
                    .map_err(|e| anyhow::anyhow!("{}", e))?
                    .clone();
                Ok(self.storage.contains_key(&(contract.clone(), key_bytes)))
            }

            fn has_contract(
                &self,
                contract: &Hash,
                _topoheight: tos_common::block::TopoHeight,
            ) -> Result<bool, anyhow::Error> {
                Ok(self.bytecodes.contains_key(contract))
            }
        }

        impl ContractProvider for InMemoryContractProvider {
            fn get_contract_balance_for_asset(
                &self,
                _contract: &Hash,
                _asset: &Hash,
                _topoheight: tos_common::block::TopoHeight,
            ) -> Result<Option<(tos_common::block::TopoHeight, u64)>, anyhow::Error> {
                Ok(None)
            }

            fn get_account_balance_for_asset(
                &self,
                _key: &tos_common::crypto::PublicKey,
                _asset: &Hash,
                _topoheight: tos_common::block::TopoHeight,
            ) -> Result<Option<(tos_common::block::TopoHeight, u64)>, anyhow::Error> {
                Ok(None)
            }

            fn asset_exists(
                &self,
                asset: &Hash,
                _topoheight: tos_common::block::TopoHeight,
            ) -> Result<bool, anyhow::Error> {
                Ok(*asset == Hash::zero())
            }

            fn load_asset_data(
                &self,
                _asset: &Hash,
                _topoheight: tos_common::block::TopoHeight,
            ) -> Result<
                Option<(tos_common::block::TopoHeight, tos_common::asset::AssetData)>,
                anyhow::Error,
            > {
                Ok(None)
            }

            fn load_asset_supply(
                &self,
                _asset: &Hash,
                _topoheight: tos_common::block::TopoHeight,
            ) -> Result<Option<(tos_common::block::TopoHeight, u64)>, anyhow::Error> {
                Ok(None)
            }

            fn account_exists(
                &self,
                _key: &tos_common::crypto::PublicKey,
                _topoheight: tos_common::block::TopoHeight,
            ) -> Result<bool, anyhow::Error> {
                Ok(false)
            }

            fn load_contract_module(
                &self,
                contract: &Hash,
                _topoheight: tos_common::block::TopoHeight,
            ) -> Result<Option<Vec<u8>>, anyhow::Error> {
                Ok(self.bytecodes.get(contract).cloned())
            }
        }

        fn apply_contract_cache(
            storage: &mut HashMap<(Hash, Vec<u8>), Vec<u8>>,
            contract: &Hash,
            cache: &ContractCache,
        ) {
            for (key_cell, (_versioned, value_opt)) in &cache.storage {
                if let Ok(key_bytes) = key_cell.as_bytes() {
                    match value_opt {
                        Some(val_cell) => {
                            if let Ok(val_bytes) = val_cell.as_bytes() {
                                storage.insert(
                                    (contract.clone(), key_bytes.clone()),
                                    val_bytes.clone(),
                                );
                            }
                        }
                        None => {
                            storage.remove(&(contract.clone(), key_bytes.clone()));
                        }
                    }
                }
            }
        }

        fn block_vrf_to_executor_vrf(block_vrf: &BlockVrfData) -> Option<VrfData> {
            let public_key = VrfPublicKey::from_bytes(&block_vrf.public_key).ok()?;
            let output = VrfOutput::from_bytes(&block_vrf.output).ok()?;
            let proof = VrfProof::from_bytes(&block_vrf.proof).ok()?;
            let binding_signature = Signature::from_bytes(&block_vrf.binding_signature).ok()?;
            Some(VrfData {
                public_key,
                output,
                proof,
                binding_signature,
            })
        }

        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_random_vrf_keys()
            .build()
            .await
            .expect("Failed to build network");

        // Mine and propagate one block to all nodes.
        network.mine_and_propagate(0).await.expect("mine");

        let block = network
            .node(0)
            .daemon()
            .get_block_at_height(1)
            .await
            .expect("get block")
            .expect("block exists");

        let vrf_nodes: Vec<BlockVrfData> = (0..3)
            .map(|i| network.node(i).get_block_vrf_data(1).expect("vrf data"))
            .collect();

        assert_eq!(vrf_nodes[0].output, vrf_nodes[1].output);
        assert_eq!(vrf_nodes[1].output, vrf_nodes[2].output);

        let bytecode = include_bytes!("../../tests/fixtures/vrf_random.so");
        let contract = hash(bytecode);
        let entry_id: u16 = 0;
        let mut input_data = Vec::with_capacity(2);
        input_data.extend_from_slice(&entry_id.to_le_bytes());

        let miner_pk = block.miner;
        let miner_pk_bytes: [u8; 32] = *miner_pk.as_bytes();
        let block_hash = block.hash.clone();
        let block_height = block.height;
        let topoheight = block.topoheight;
        let timestamp = topoheight.saturating_mul(15);

        let mut results = Vec::new();

        for block_vrf in vrf_nodes {
            let mut provider = InMemoryContractProvider {
                storage: HashMap::new(),
                bytecodes: HashMap::from([(contract.clone(), bytecode.to_vec())]),
                topoheight,
            };

            let vrf = block_vrf_to_executor_vrf(&block_vrf).expect("vrf conversion");
            let tx_hash = hash(vrf.output.as_bytes());
            let exec = TakoExecutor::execute_with_all_providers(
                bytecode,
                &provider,
                topoheight,
                &contract,
                &block_hash,
                block_height,
                timestamp,
                &tx_hash,
                &contract,
                &input_data,
                Some(1_000_000),
                &SVMFeatureSet::production(),
                Some(&vrf),
                Some(&miner_pk_bytes),
                None,
            )
            .expect("contract execute");

            apply_contract_cache(&mut provider.storage, &contract, &exec.cache);

            let key = b"vrf_random".to_vec();
            let stored = provider
                .storage
                .get(&(contract.clone(), key))
                .expect("vrf_random stored")
                .clone();
            results.push(stored);
        }

        assert_eq!(results[0], results[1]);
        assert_eq!(results[1], results[2]);
    }

    // ========================================================================
    // Late Join and Sync Tests
    // ========================================================================

    #[tokio::test]
    async fn late_join_node_verifies_vrf() {
        // Setup: 2-node network with VRF
        let mut network = LocalTosNetworkBuilder::new()
            .with_nodes(2)
            .with_random_vrf_keys()
            .build()
            .await
            .expect("Failed to build network");

        // Mine 10 blocks on node 0 (each with VRF proof)
        for _ in 0..10 {
            network.mine_and_propagate(0).await.expect("mine");
        }

        // Verify node 0 has VRF data for all blocks
        for height in 1..=10 {
            let vrf_data = network.node(0).get_block_vrf_data(height);
            assert!(
                vrf_data.is_some(),
                "Node 0 should have VRF data for height {}",
                height
            );
        }

        // Add a late-joining node that syncs from node 0
        // The sync process will verify VRF proofs for each block
        let new_node_id = network
            .add_node(0, None)
            .await
            .expect("Failed to add node - VRF verification may have failed");

        // Verify new node synced to correct height
        let new_node_height = network.node(new_node_id).get_tip_height().await.unwrap();
        assert_eq!(
            new_node_height, 10,
            "New node should sync to height 10 after verifying all VRF proofs"
        );

        // Verify new node has the same tip as node 0
        let node0_tips = network.node(0).get_tips().await.unwrap();
        let new_node_tips = network.node(new_node_id).get_tips().await.unwrap();
        assert_eq!(
            node0_tips, new_node_tips,
            "New node should have same tips as source node"
        );
    }

    // ========================================================================
    // Multi-Tip DAG Tests
    // ========================================================================

    #[tokio::test]
    async fn vrf_with_multi_tip_dag() {
        use crate::tier3_e2e_dag::LocalTosNetworkDagBuilder;
        use std::collections::HashSet;

        let network = LocalTosNetworkDagBuilder::new()
            .with_nodes(3)
            .with_random_vrf_keys()
            .build()
            .expect("Failed to build DAG network");

        let genesis_tip = network
            .node(0)
            .daemon()
            .get_tips()
            .first()
            .cloned()
            .expect("genesis tip");

        let _block0 = network
            .node(0)
            .daemon()
            .mine_block_on_tip(&genesis_tip)
            .expect("mine node0");
        let block1 = network
            .node(1)
            .daemon()
            .mine_block_on_tip(&genesis_tip)
            .expect("mine node1");
        let block2 = network
            .node(2)
            .daemon()
            .mine_block_on_tip(&genesis_tip)
            .expect("mine node2");

        network
            .propagate_block_from(1, &block1.hash)
            .await
            .expect("propagate node1 -> node0");
        network
            .propagate_block_from(2, &block2.hash)
            .await
            .expect("propagate node2 -> node0");

        let tips = network.node(0).daemon().get_tips();
        assert_eq!(tips.len(), 3, "node0 should have 3 tips in DAG");

        let mut vrf_outputs = HashSet::new();
        for tip in tips {
            let vrf = network
                .node(0)
                .get_block_vrf_data_by_hash(&tip)
                .expect("vrf data");
            vrf_outputs.insert(vrf.output);
        }

        assert_eq!(
            vrf_outputs.len(),
            3,
            "each tip should have a unique VRF output"
        );
    }

    // ========================================================================
    // Rapid Block Production Tests
    // ========================================================================

    #[tokio::test]
    async fn rapid_blocks_vrf_uniqueness() {
        use std::collections::HashSet;

        // Setup: single node with VRF
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(1)
            .with_random_vrf_keys()
            .build()
            .await
            .expect("Failed to build network");

        // Mine 20 blocks rapidly
        let mut vrf_outputs = HashSet::new();
        for _ in 0..20 {
            network.node(0).daemon().mine_block().await.expect("mine");
        }

        // Collect all VRF outputs
        for height in 1..=20 {
            let vrf = network
                .node(0)
                .get_block_vrf_data(height)
                .expect("VRF should be present");
            vrf_outputs.insert(vrf.output);
        }

        // All 20 outputs should be unique
        assert_eq!(vrf_outputs.len(), 20, "All 20 VRF outputs should be unique");
    }

    // ========================================================================
    // Partition Isolation Tests
    // ========================================================================

    #[tokio::test]
    async fn partition_isolation_vrf_divergence() {
        // Setup: 3-node network with different VRF keys
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_random_vrf_keys()
            .build()
            .await
            .expect("Failed to build network");

        // Create 3-way partition: [0] | [1] | [2]
        // Each node is isolated from all others
        network
            .partition_groups(&[0], &[1, 2])
            .await
            .expect("partition 0");
        network
            .partition_groups(&[1], &[2])
            .await
            .expect("partition 1-2");

        // Each node mines 2 blocks independently
        for _ in 0..2 {
            network.node(0).daemon().mine_block().await.expect("mine 0");
            network.node(1).daemon().mine_block().await.expect("mine 1");
            network.node(2).daemon().mine_block().await.expect("mine 2");
        }

        // Get VRF outputs at height 1 from each node
        let vrf_node0_h1 = network.node(0).get_block_vrf_data(1).expect("VRF 0");
        let vrf_node1_h1 = network.node(1).get_block_vrf_data(1).expect("VRF 1");
        let vrf_node2_h1 = network.node(2).get_block_vrf_data(1).expect("VRF 2");

        // All VRF outputs should be different (different miners, different block hashes)
        assert_ne!(
            vrf_node0_h1.output, vrf_node1_h1.output,
            "Node 0 and 1 should have different VRF outputs"
        );
        assert_ne!(
            vrf_node0_h1.output, vrf_node2_h1.output,
            "Node 0 and 2 should have different VRF outputs"
        );
        assert_ne!(
            vrf_node1_h1.output, vrf_node2_h1.output,
            "Node 1 and 2 should have different VRF outputs"
        );

        // Public keys should also differ (different VRF keys)
        assert_ne!(
            vrf_node0_h1.public_key, vrf_node1_h1.public_key,
            "Different VRF keys means different public keys"
        );
        assert_ne!(
            vrf_node0_h1.public_key, vrf_node2_h1.public_key,
            "Different VRF keys means different public keys"
        );

        // Get VRF at height 2 - should also all be different
        let vrf_node0_h2 = network.node(0).get_block_vrf_data(2).expect("VRF 0 h2");
        let vrf_node1_h2 = network.node(1).get_block_vrf_data(2).expect("VRF 1 h2");
        let vrf_node2_h2 = network.node(2).get_block_vrf_data(2).expect("VRF 2 h2");

        assert_ne!(vrf_node0_h2.output, vrf_node1_h2.output);
        assert_ne!(vrf_node1_h2.output, vrf_node2_h2.output);

        // Each node's VRF at height 1 and 2 should differ
        assert_ne!(
            vrf_node0_h1.output, vrf_node0_h2.output,
            "Same node different heights should have different VRF"
        );
    }
}
