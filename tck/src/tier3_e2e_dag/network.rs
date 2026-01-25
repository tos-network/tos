#![allow(missing_docs)]

//! Multi-node DAG network orchestration for VRF tests

use crate::orchestrator::PausedClock;
use crate::tier1_component::VrfConfig;
use crate::tier1_component_dag::TestBlockchainDagBuilder;
use crate::tier2_integration_dag::TestDaemonDag;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tos_common::crypto::Hash;
use tos_daemon::vrf::VrfKeyManager;

pub struct NodeHandleDag {
    pub id: usize,
    daemon: TestDaemonDag,
    clock: Arc<PausedClock>,
    peers: Vec<usize>,
}

impl NodeHandleDag {
    pub fn daemon(&self) -> &TestDaemonDag {
        &self.daemon
    }

    pub fn clock(&self) -> &Arc<PausedClock> {
        &self.clock
    }

    pub fn peers(&self) -> &[usize] {
        &self.peers
    }

    pub fn get_block_vrf_data_by_hash(
        &self,
        hash: &Hash,
    ) -> Option<tos_common::block::BlockVrfData> {
        self.daemon.get_block_vrf_data_by_hash(hash)
    }
}

pub struct LocalTosNetworkDag {
    nodes: Vec<NodeHandleDag>,
    clock: Arc<PausedClock>,
    partitions: Arc<RwLock<HashMap<(usize, usize), bool>>>,
}

impl LocalTosNetworkDag {
    pub fn node(&self, node_id: usize) -> &NodeHandleDag {
        &self.nodes[node_id]
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub async fn partition_groups(&self, group_a: &[usize], group_b: &[usize]) -> Result<()> {
        let mut partitions = self.partitions.write().await;
        for &node_a in group_a {
            for &node_b in group_b {
                partitions.insert((node_a, node_b), true);
                partitions.insert((node_b, node_a), true);
            }
        }
        Ok(())
    }

    pub async fn heal_all_partitions(&self) {
        let mut partitions = self.partitions.write().await;
        partitions.clear();
    }

    pub async fn is_partitioned(&self, node_a: usize, node_b: usize) -> bool {
        let partitions = self.partitions.read().await;
        partitions.get(&(node_a, node_b)).copied().unwrap_or(false)
    }

    pub fn mine_block_on_tip(&self, node_id: usize, tip: &Hash) -> Result<Hash> {
        let block = self.nodes[node_id].daemon.mine_block_on_tip(tip)?;
        Ok(block.hash)
    }

    pub async fn propagate_block_from(
        &self,
        source_node_id: usize,
        block_hash: &Hash,
    ) -> Result<usize> {
        let source_node = &self.nodes[source_node_id];
        let block = source_node
            .daemon
            .get_block(block_hash)
            .ok_or_else(|| anyhow::anyhow!("Block not found on source node"))?;

        let mut count = 0usize;
        for &peer_id in source_node.peers() {
            if self.is_partitioned(source_node_id, peer_id).await {
                continue;
            }
            let peer_daemon = &self.nodes[peer_id].daemon;
            peer_daemon.receive_block(block.clone())?;
            count = count.saturating_add(1);
        }

        Ok(count)
    }

    pub fn clock(&self) -> Arc<PausedClock> {
        self.clock.clone()
    }
}

pub struct LocalTosNetworkDagBuilder {
    node_count: usize,
    vrf_keys: Vec<String>,
    chain_id: u64,
}

impl LocalTosNetworkDagBuilder {
    pub fn new() -> Self {
        Self {
            node_count: 3,
            vrf_keys: Vec::new(),
            chain_id: 3,
        }
    }

    pub fn with_nodes(mut self, count: usize) -> Self {
        assert!(count > 0, "Node count must be at least 1");
        self.node_count = count;
        self
    }

    pub fn with_vrf_keys(mut self, keys: Vec<String>) -> Self {
        self.vrf_keys = keys;
        self
    }

    pub fn with_random_vrf_keys(mut self) -> Self {
        self.vrf_keys = (0..self.node_count)
            .map(|_| VrfKeyManager::new().secret_key_hex())
            .collect();
        self
    }

    pub fn with_chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = chain_id;
        self
    }

    pub fn build(self) -> Result<LocalTosNetworkDag> {
        let clock = Arc::new(PausedClock::new());

        let mut nodes = Vec::with_capacity(self.node_count);
        for node_id in 0..self.node_count {
            let mut builder = TestBlockchainDagBuilder::new().with_clock(clock.clone());

            if let Some(vrf_secret_hex) = self.vrf_keys.get(node_id) {
                let vrf_config =
                    VrfConfig::new(vrf_secret_hex.clone()).with_chain_id(self.chain_id);
                builder = builder.with_vrf_config(vrf_config);
            }

            let blockchain = builder.build()?;
            let daemon = TestDaemonDag::new(blockchain, clock.clone());

            let peers: Vec<usize> = (0..self.node_count).filter(|&j| j != node_id).collect();

            nodes.push(NodeHandleDag {
                id: node_id,
                daemon,
                clock: clock.clone(),
                peers,
            });
        }

        Ok(LocalTosNetworkDag {
            nodes,
            clock,
            partitions: Arc::new(RwLock::new(HashMap::new())),
        })
    }
}

impl Default for LocalTosNetworkDagBuilder {
    fn default() -> Self {
        Self::new()
    }
}
