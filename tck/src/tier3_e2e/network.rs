//! Multi-node local network orchestration for E2E testing
//!
//! This module provides `LocalTosNetwork` for spawning and managing multiple
//! TOS nodes in a single test process, enabling end-to-end testing of consensus,
//! network partitions, and multi-node scenarios.
//!
//! # Architecture
//!
//! ```text
//! LocalTosNetwork
//!   ├── Node 0 (TestDaemon + PausedClock)
//!   ├── Node 1 (TestDaemon + PausedClock)
//!   ├── Node 2 (TestDaemon + PausedClock)
//!   └── ...
//!
//! All nodes share:
//!   - Synchronized clocks (can advance time globally)
//!   - Deterministic RNG (seeded)
//!   - In-process communication (no real network)
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use tos_tck::tier3_e2e::LocalTosNetworkBuilder;
//! use tokio::time::Duration;
//!
//! #[tokio::test]
//! async fn test_3_node_consensus() {
//!     // Create 3-node network
//!     let network = LocalTosNetworkBuilder::new()
//!         .with_nodes(3)
//!         .with_initial_balance(1_000_000)
//!         .build()
//!         .await
//!         .unwrap();
//!
//!     // Submit transaction to node 0
//!     network.node(0).submit_transaction(tx).await?;
//!
//!     // Mine block on node 0
//!     network.node(0).mine_block().await?;
//!
//!     // Wait for all nodes to converge
//!     network.wait_for_convergence(Duration::from_secs(5)).await?;
//!
//!     // Verify all nodes at same height
//!     assert_eq!(network.node(0).get_tip_height().await?, 1);
//!     assert_eq!(network.node(1).get_tip_height().await?, 1);
//!     assert_eq!(network.node(2).get_tip_height().await?, 1);
//! }
//! ```

use crate::orchestrator::{Clock, PausedClock};
use crate::tier1_component::{TestBlockchainBuilder, VrfConfig};
use crate::tier2_integration::{Hash, NodeRpc, TestDaemon};
use crate::tier3_e2e::waiters::*;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Duration;
use tos_daemon::vrf::VrfKeyManager;

/// Network topology defining connectivity between nodes
#[derive(Debug, Clone, Default)]
pub enum NetworkTopology {
    /// All nodes can communicate with all other nodes (default)
    #[default]
    FullMesh,

    /// Nodes form a ring (0→1→2→...→N→0)
    Ring,

    /// Star topology: all nodes connect through a central hub node
    Star {
        /// Index of the center node that connects to all others
        center: usize,
    },

    /// Custom topology defined by adjacency list
    /// Map: node_id → list of connected peer node_ids
    Custom(HashMap<usize, Vec<usize>>),
}

/// Handle to a single node in the network
pub struct NodeHandle {
    /// Node ID (0-indexed)
    pub id: usize,

    /// TestDaemon instance
    daemon: TestDaemon,

    /// Clock (shared across network for synchronization)
    clock: Arc<PausedClock>,

    /// Connected peer IDs (for topology enforcement)
    peers: Vec<usize>,
}

impl NodeHandle {
    /// Get reference to underlying daemon
    pub fn daemon(&self) -> &TestDaemon {
        &self.daemon
    }

    /// Get reference to the clock
    pub fn clock(&self) -> &Arc<PausedClock> {
        &self.clock
    }

    /// Get list of connected peers
    pub fn peers(&self) -> &[usize] {
        &self.peers
    }

    /// Check if this node is connected to another node
    pub fn is_connected_to(&self, peer_id: usize) -> bool {
        self.peers.contains(&peer_id)
    }

    /// Get VRF data for block at specific height
    pub fn get_block_vrf_data(&self, height: u64) -> Option<tos_common::block::BlockVrfData> {
        self.daemon.get_block_vrf_data(height)
    }

    /// Get VRF data for block at specific topoheight
    pub fn get_block_vrf_data_at_topoheight(
        &self,
        topoheight: u64,
    ) -> Option<tos_common::block::BlockVrfData> {
        self.daemon.get_block_vrf_data_at_topoheight(topoheight)
    }

    /// Check if VRF is configured for this node
    pub fn has_vrf(&self) -> bool {
        self.daemon.has_vrf()
    }
}

// Delegate NodeRpc trait to underlying daemon
#[async_trait::async_trait]
impl NodeRpc for NodeHandle {
    async fn get_tip_height(&self) -> Result<u64> {
        self.daemon.get_tip_height().await
    }

    async fn get_tips(&self) -> Result<Vec<Hash>> {
        self.daemon.get_tips().await
    }

    async fn get_balance(&self, address: &Hash) -> Result<u64> {
        self.daemon.get_balance(address).await
    }

    async fn get_nonce(&self, address: &Hash) -> Result<u64> {
        self.daemon.get_nonce(address).await
    }
}

// Also implement for &NodeHandle to support RPC helpers
#[async_trait::async_trait]
impl NodeRpc for &NodeHandle {
    async fn get_tip_height(&self) -> Result<u64> {
        self.daemon.get_tip_height().await
    }

    async fn get_tips(&self) -> Result<Vec<Hash>> {
        self.daemon.get_tips().await
    }

    async fn get_balance(&self, address: &Hash) -> Result<u64> {
        self.daemon.get_balance(address).await
    }

    async fn get_nonce(&self, address: &Hash) -> Result<u64> {
        self.daemon.get_nonce(address).await
    }
}

/// Local multi-node TOS network for E2E testing
///
/// Manages multiple TestDaemon instances with synchronized clocks and
/// deterministic behavior for end-to-end consensus testing.
pub struct LocalTosNetwork {
    /// All nodes in the network
    nodes: Vec<NodeHandle>,

    /// Shared clock for all nodes (synchronized time)
    clock: Arc<PausedClock>,

    /// Network topology
    topology: NetworkTopology,

    /// Genesis accounts (shared across all nodes)
    genesis_accounts: HashMap<String, (Hash, u64)>,

    /// Network-wide partition state
    /// Map: (node_a, node_b) → is_partitioned
    partitions: Arc<RwLock<HashMap<(usize, usize), bool>>>,
}

impl LocalTosNetwork {
    /// Get node by ID
    ///
    /// # Panics
    ///
    /// Panics if node_id is out of bounds
    pub fn node(&self, node_id: usize) -> &NodeHandle {
        &self.nodes[node_id]
    }

    /// Get all nodes
    pub fn nodes(&self) -> &[NodeHandle] {
        &self.nodes
    }

    /// Get number of nodes in network
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get shared clock
    pub fn clock(&self) -> Arc<PausedClock> {
        self.clock.clone()
    }

    /// Get network topology
    pub fn topology(&self) -> &NetworkTopology {
        &self.topology
    }

    /// Advance time globally for all nodes
    ///
    /// This advances the shared clock, affecting all nodes simultaneously.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Advance time by 1 hour for entire network
    /// network.advance_time(Duration::from_secs(3600)).await;
    /// ```
    pub async fn advance_time(&self, duration: Duration) {
        self.clock.advance(duration).await;
    }

    /// Wait for all nodes to reach same tip height
    ///
    /// Polls nodes every 100ms until all have the same tip height.
    ///
    /// # Arguments
    ///
    /// * `timeout` - Maximum time to wait
    ///
    /// # Errors
    ///
    /// Returns error if timeout occurs before convergence
    pub async fn wait_for_height_convergence(&self, timeout: Duration) -> Result<()> {
        wait_all_heights_equal(&self.nodes, timeout).await
    }

    /// Wait for all nodes to have same tips (consensus convergence)
    ///
    /// This is stronger than height convergence - it verifies all nodes
    /// agree on which blocks are tips.
    ///
    /// # Arguments
    ///
    /// * `timeout` - Maximum time to wait
    ///
    /// # Errors
    ///
    /// Returns error if timeout occurs before convergence
    pub async fn wait_for_tip_convergence(&self, timeout: Duration) -> Result<()> {
        wait_all_tips_equal(&self.nodes, timeout).await
    }

    /// Wait for complete consensus convergence
    ///
    /// Shorthand for waiting for both height and tip convergence.
    pub async fn wait_for_convergence(&self, timeout: Duration) -> Result<()> {
        self.wait_for_height_convergence(timeout).await?;
        self.wait_for_tip_convergence(timeout).await?;
        Ok(())
    }

    /// Create a network partition between two groups of nodes
    ///
    /// After partitioning, nodes in `group_a` cannot communicate with
    /// nodes in `group_b` and vice versa.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Partition network: [0,1,2] vs [3,4]
    /// network.partition_groups(&[0, 1, 2], &[3, 4]).await?;
    /// ```
    pub async fn partition_groups(&self, group_a: &[usize], group_b: &[usize]) -> Result<()> {
        let mut partitions = self.partitions.write().await;

        for &node_a in group_a {
            for &node_b in group_b {
                partitions.insert((node_a, node_b), true);
                partitions.insert((node_b, node_a), true);
            }
        }

        if log::log_enabled!(log::Level::Info) {
            log::info!("Network partitioned: {:?} ⚡ {:?}", group_a, group_b);
        }

        Ok(())
    }

    /// Heal all network partitions
    ///
    /// Restores full connectivity between all nodes.
    pub async fn heal_all_partitions(&self) {
        let mut partitions = self.partitions.write().await;
        partitions.clear();

        if log::log_enabled!(log::Level::Info) {
            log::info!("All network partitions healed");
        }
    }

    /// Check if two nodes are partitioned
    pub async fn is_partitioned(&self, node_a: usize, node_b: usize) -> bool {
        let partitions = self.partitions.read().await;
        partitions.get(&(node_a, node_b)).copied().unwrap_or(false)
    }

    /// Get genesis account by name
    ///
    /// Returns the address and initial balance for a named genesis account.
    pub fn get_genesis_account(&self, name: &str) -> Option<&(Hash, u64)> {
        self.genesis_accounts.get(name)
    }

    /// Propagate transaction from source node to connected peers
    ///
    /// This simulates P2P transaction propagation. The transaction is submitted
    /// to all nodes that are:
    /// 1. Connected to the source node (based on topology)
    /// 2. Not partitioned from the source node
    ///
    /// # Arguments
    ///
    /// * `source_node_id` - Node that originated the transaction
    /// * `tx` - Transaction to propagate
    ///
    /// # Returns
    ///
    /// Number of nodes that received the transaction
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let tx = create_test_tx(sender, receiver, 100, 1, 1);
    /// network.node(0).daemon().submit_transaction(tx.clone()).await?;
    /// network.propagate_transaction_from(0, tx).await?;
    /// ```
    pub async fn propagate_transaction_from(
        &self,
        source_node_id: usize,
        tx: crate::tier1_component::TestTransaction,
    ) -> Result<usize> {
        let source_node = &self.nodes[source_node_id];
        let mut propagated_count = 0;

        // Get peers of source node
        let peer_ids = source_node.peers();

        for &peer_id in peer_ids {
            // Check if partitioned
            if self.is_partitioned(source_node_id, peer_id).await {
                continue;
            }

            // Submit transaction to peer
            let peer_daemon = &self.nodes[peer_id].daemon;
            if peer_daemon.submit_transaction(tx.clone()).await.is_ok() {
                propagated_count += 1;
            }
        }

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "Transaction {} propagated from node {} to {} peers",
                tx.hash,
                source_node_id,
                propagated_count
            );
        }

        Ok(propagated_count)
    }

    /// Propagate block from source node to all connected peers
    ///
    /// This simulates P2P block propagation after mining. The block is fetched
    /// from the source node and sent to all nodes that are:
    /// 1. Connected to the source node (based on topology)
    /// 2. Not partitioned from the source node
    ///
    /// # Arguments
    ///
    /// * `source_node_id` - Node that mined the block
    /// * `height` - Height of the block to propagate
    ///
    /// # Returns
    ///
    /// Number of nodes that received the block
    ///
    /// # Errors
    ///
    /// Returns an error if the block at the specified height doesn't exist
    pub async fn propagate_block_from(&self, source_node_id: usize, height: u64) -> Result<usize> {
        let source_node = &self.nodes[source_node_id];

        // Get the block from the source node
        let block = source_node
            .daemon
            .get_block_at_height(height)
            .await?
            .context(format!(
                "Block at height {} not found on node {}",
                height, source_node_id
            ))?;

        let mut propagated_count = 0;

        // Get peers of source node
        let peer_ids = source_node.peers();

        for &peer_id in peer_ids {
            // Check if partitioned
            if self.is_partitioned(source_node_id, peer_id).await {
                continue;
            }

            // Send block to peer
            let peer_daemon = &self.nodes[peer_id].daemon;
            if peer_daemon.receive_block(block.clone()).await.is_ok() {
                propagated_count += 1;
            }
        }

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "Block {} at height {} propagated from node {} to {} peers",
                block.hash,
                height,
                source_node_id,
                propagated_count
            );
        }

        Ok(propagated_count)
    }

    /// Submit transaction to a node and propagate to all connected peers
    ///
    /// This is a convenience method that combines submit_transaction and
    /// propagate_transaction_from. It simulates the typical P2P workflow where
    /// a transaction is submitted to one node and then gossiped to the network.
    ///
    /// # Arguments
    ///
    /// * `node_id` - Node to submit transaction to
    /// * `tx` - Transaction to submit and propagate
    ///
    /// # Returns
    ///
    /// The transaction hash
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let tx = create_test_tx(sender, receiver, 100, 1, 1);
    /// network.submit_and_propagate(0, tx).await?;
    /// // Transaction now submitted to node 0 and all its connected peers
    /// ```
    pub async fn submit_and_propagate(
        &self,
        node_id: usize,
        tx: crate::tier1_component::TestTransaction,
    ) -> Result<Hash> {
        // Submit to origin node
        let hash = self.nodes[node_id]
            .daemon
            .submit_transaction(tx.clone())
            .await?;

        // Propagate to peers
        self.propagate_transaction_from(node_id, tx).await?;

        Ok(hash)
    }

    /// Mine a block on a node and propagate to all connected peers
    ///
    /// This is a convenience method that combines mine_block and propagate_block_from.
    /// It simulates the typical P2P workflow where a miner creates a block and
    /// broadcasts it to the network.
    ///
    /// # Arguments
    ///
    /// * `node_id` - Node to mine the block on
    ///
    /// # Returns
    ///
    /// The hash of the newly mined block
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// network.submit_and_propagate(0, tx).await?;
    /// network.mine_and_propagate(0).await?;
    /// // Block mined on node 0 and propagated to all connected peers
    /// ```
    pub async fn mine_and_propagate(&self, node_id: usize) -> Result<Hash> {
        // Mine block on the node
        let hash = self.nodes[node_id].daemon.mine_block().await?;

        // Get the height of the mined block
        let height = self.nodes[node_id].get_tip_height().await?;

        // Propagate to peers
        self.propagate_block_from(node_id, height).await?;

        Ok(hash)
    }

    /// Shutdown network
    ///
    /// Stops all nodes gracefully.
    pub async fn shutdown(self) -> Result<()> {
        for mut node in self.nodes {
            node.daemon.stop();
        }
        Ok(())
    }
}

/// Builder for LocalTosNetwork
///
/// Provides a fluent API for configuring and creating multi-node networks.
pub struct LocalTosNetworkBuilder {
    /// Number of nodes to create
    node_count: usize,

    /// Network topology
    topology: NetworkTopology,

    /// Genesis accounts (name → balance)
    genesis_accounts: HashMap<String, u64>,

    /// Default balance for auto-created accounts
    default_balance: u64,

    /// Clock seed (for deterministic testing)
    seed: Option<u64>,

    /// Per-node VRF secret keys (hex strings)
    /// If fewer keys than nodes, remaining nodes have no VRF
    vrf_keys: Vec<String>,

    /// Chain ID for VRF binding (default: 3 = devnet)
    chain_id: u64,
}

impl LocalTosNetworkBuilder {
    /// Create new builder with default settings
    pub fn new() -> Self {
        Self {
            node_count: 3, // Minimum for consensus testing
            topology: NetworkTopology::FullMesh,
            genesis_accounts: HashMap::new(),
            default_balance: 0,
            seed: None,
            vrf_keys: Vec::new(),
            chain_id: 3, // devnet default
        }
    }

    /// Set number of nodes
    ///
    /// # Arguments
    ///
    /// * `count` - Number of nodes (must be ≥ 1)
    ///
    /// # Panics
    ///
    /// Panics if count is 0
    pub fn with_nodes(mut self, count: usize) -> Self {
        assert!(count > 0, "Node count must be at least 1");
        self.node_count = count;
        self
    }

    /// Set network topology
    pub fn with_topology(mut self, topology: NetworkTopology) -> Self {
        self.topology = topology;
        self
    }

    /// Add a named genesis account
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// builder.with_genesis_account("alice", 1_000_000);
    /// ```
    pub fn with_genesis_account(mut self, name: impl Into<String>, balance: u64) -> Self {
        self.genesis_accounts.insert(name.into(), balance);
        self
    }

    /// Set default balance for all nodes' default accounts
    pub fn with_default_balance(mut self, balance: u64) -> Self {
        self.default_balance = balance;
        self
    }

    /// Set deterministic seed for RNG
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Set VRF keys for nodes
    ///
    /// Keys are assigned in order. If fewer keys than nodes, remaining
    /// nodes will not have VRF configured.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// builder.with_vrf_keys(vec![
    ///     "abcd1234...".to_string(),
    ///     "efgh5678...".to_string(),
    /// ]);
    /// ```
    pub fn with_vrf_keys(mut self, keys: Vec<String>) -> Self {
        self.vrf_keys = keys;
        self
    }

    /// Generate random VRF keys for all nodes
    ///
    /// Each node will get a unique randomly generated VRF keypair.
    pub fn with_random_vrf_keys(mut self) -> Self {
        self.vrf_keys = (0..self.node_count)
            .map(|_| VrfKeyManager::new().secret_key_hex())
            .collect();
        self
    }

    /// Set chain ID for VRF binding (default: 3 = devnet)
    pub fn with_chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = chain_id;
        self
    }

    /// Build the network
    ///
    /// Creates all nodes, initializes their blockchains with genesis state,
    /// and establishes network topology.
    pub async fn build(self) -> Result<LocalTosNetwork> {
        // Create shared clock
        let clock = Arc::new(PausedClock::new());

        // Build peer connectivity based on topology
        let peer_map = self.build_peer_map();

        // Create genesis accounts mapping
        let mut genesis_accounts_map = HashMap::new();
        for (name, balance) in &self.genesis_accounts {
            let addr = Self::create_account_address(name);
            genesis_accounts_map.insert(name.clone(), (addr, *balance));
        }

        // Create nodes
        let mut nodes = Vec::with_capacity(self.node_count);

        for node_id in 0..self.node_count {
            // Build blockchain with genesis state
            let mut builder = TestBlockchainBuilder::new()
                .with_clock(clock.clone() as Arc<dyn Clock>)
                .with_default_balance(self.default_balance);

            // Add genesis accounts
            for (addr, balance) in genesis_accounts_map.values() {
                builder = builder.with_funded_account(addr.clone(), *balance);
            }

            // Add VRF config if this node has a VRF key
            if let Some(vrf_secret_hex) = self.vrf_keys.get(node_id) {
                let vrf_config =
                    VrfConfig::new(vrf_secret_hex.clone()).with_chain_id(self.chain_id);
                builder = builder.with_vrf_config(vrf_config);
            }

            let blockchain = builder
                .build()
                .await
                .with_context(|| format!("Failed to build blockchain for node {}", node_id))?;

            // Create daemon
            let daemon = TestDaemon::new(blockchain, clock.clone() as Arc<dyn Clock>);

            // Get peers for this node
            let peers = peer_map.get(&node_id).cloned().unwrap_or_default();

            // Create node handle
            let node = NodeHandle {
                id: node_id,
                daemon,
                clock: clock.clone(),
                peers,
            };

            nodes.push(node);

            if log::log_enabled!(log::Level::Info) {
                log::info!(
                    "Created node {} with {} peers",
                    node_id,
                    nodes[node_id].peers.len()
                );
            }
        }

        Ok(LocalTosNetwork {
            nodes,
            clock,
            topology: self.topology,
            genesis_accounts: genesis_accounts_map,
            partitions: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Build peer connectivity map based on topology
    fn build_peer_map(&self) -> HashMap<usize, Vec<usize>> {
        match &self.topology {
            NetworkTopology::FullMesh => {
                // Each node connects to all other nodes
                let mut map = HashMap::new();
                for i in 0..self.node_count {
                    let peers: Vec<usize> = (0..self.node_count).filter(|&j| j != i).collect();
                    map.insert(i, peers);
                }
                map
            }

            NetworkTopology::Ring => {
                // Each node connects to next node in ring
                let mut map = HashMap::new();
                for i in 0..self.node_count {
                    let next = (i + 1) % self.node_count;
                    let prev = if i == 0 { self.node_count - 1 } else { i - 1 };
                    map.insert(i, vec![prev, next]);
                }
                map
            }

            NetworkTopology::Star { center } => {
                // Center node connects to all others, others connect only to center
                let mut map = HashMap::new();
                let center_peers: Vec<usize> =
                    (0..self.node_count).filter(|&j| j != *center).collect();
                map.insert(*center, center_peers);
                for i in 0..self.node_count {
                    if i != *center {
                        map.insert(i, vec![*center]);
                    }
                }
                map
            }

            NetworkTopology::Custom(custom_map) => custom_map.clone(),
        }
    }

    /// Create deterministic account address from name
    fn create_account_address(name: &str) -> Hash {
        let mut bytes = [0u8; 32];

        // Mix name into hash
        let name_bytes = name.as_bytes();
        for (i, b) in name_bytes.iter().take(32).enumerate() {
            bytes[i] = *b;
        }

        Hash::new(bytes)
    }
}

impl Default for LocalTosNetworkBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::expect_used)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::tier1_component::TestTransaction;

    fn create_test_address(id: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        Hash::new(bytes)
    }

    #[tokio::test]
    async fn test_network_creation() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .build()
            .await
            .unwrap();

        assert_eq!(network.node_count(), 3);

        // Verify all nodes start at height 0
        for i in 0..3 {
            let height = network.node(i).get_tip_height().await.unwrap();
            assert_eq!(height, 0, "Node {} should start at height 0", i);
        }
    }

    #[tokio::test]
    async fn test_genesis_accounts() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(2)
            .with_genesis_account("alice", 1_000_000)
            .with_genesis_account("bob", 500_000)
            .build()
            .await
            .unwrap();

        // Verify genesis accounts exist on all nodes
        let (alice_addr, alice_balance) = network.get_genesis_account("alice").unwrap();
        let (bob_addr, bob_balance) = network.get_genesis_account("bob").unwrap();

        assert_eq!(*alice_balance, 1_000_000);
        assert_eq!(*bob_balance, 500_000);

        // Check node 0
        assert_eq!(
            network.node(0).get_balance(alice_addr).await.unwrap(),
            1_000_000
        );
        assert_eq!(
            network.node(0).get_balance(bob_addr).await.unwrap(),
            500_000
        );

        // Check node 1
        assert_eq!(
            network.node(1).get_balance(alice_addr).await.unwrap(),
            1_000_000
        );
        assert_eq!(
            network.node(1).get_balance(bob_addr).await.unwrap(),
            500_000
        );
    }

    #[tokio::test]
    async fn test_time_advancement() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(2)
            .build()
            .await
            .unwrap();

        let start_0 = network.node(0).daemon().blockchain().clock().now();
        let start_1 = network.node(1).daemon().blockchain().clock().now();

        // Advance time globally
        network.advance_time(Duration::from_secs(100)).await;

        let elapsed_0 = network.node(0).daemon().blockchain().clock().now() - start_0;
        let elapsed_1 = network.node(1).daemon().blockchain().clock().now() - start_1;

        // Both nodes should have advanced by same amount
        assert_eq!(elapsed_0, Duration::from_secs(100));
        assert_eq!(elapsed_1, Duration::from_secs(100));
    }

    #[tokio::test]
    async fn test_full_mesh_topology() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_topology(NetworkTopology::FullMesh)
            .build()
            .await
            .unwrap();

        // Each node should connect to 3 others (N-1)
        for i in 0..4 {
            assert_eq!(network.node(i).peers().len(), 3);

            // Verify connectivity
            for j in 0..4 {
                if i != j {
                    assert!(network.node(i).is_connected_to(j));
                }
            }
        }
    }

    #[tokio::test]
    async fn test_ring_topology() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(5)
            .with_topology(NetworkTopology::Ring)
            .build()
            .await
            .unwrap();

        // Each node should connect to exactly 2 peers (prev and next)
        for i in 0..5 {
            assert_eq!(network.node(i).peers().len(), 2);
        }

        // Node 0 should connect to 4 and 1
        assert!(network.node(0).is_connected_to(4));
        assert!(network.node(0).is_connected_to(1));

        // Node 2 should connect to 1 and 3
        assert!(network.node(2).is_connected_to(1));
        assert!(network.node(2).is_connected_to(3));
    }

    #[tokio::test]
    async fn test_star_topology() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(5)
            .with_topology(NetworkTopology::Star { center: 0 })
            .build()
            .await
            .unwrap();

        // Center node (0) should connect to all other nodes
        assert_eq!(network.node(0).peers().len(), 4);
        assert!(network.node(0).is_connected_to(1));
        assert!(network.node(0).is_connected_to(2));
        assert!(network.node(0).is_connected_to(3));
        assert!(network.node(0).is_connected_to(4));

        // Non-center nodes should only connect to center
        for i in 1..5 {
            assert_eq!(network.node(i).peers().len(), 1);
            assert!(network.node(i).is_connected_to(0));
            // Non-center nodes should NOT directly connect to each other
            for j in 1..5 {
                if i != j {
                    assert!(!network.node(i).is_connected_to(j));
                }
            }
        }
    }

    #[tokio::test]
    async fn test_network_partition() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(5)
            .build()
            .await
            .unwrap();

        // Create partition: [0,1,2] vs [3,4]
        network.partition_groups(&[0, 1, 2], &[3, 4]).await.unwrap();

        // Verify partition state
        assert!(network.is_partitioned(0, 3).await);
        assert!(network.is_partitioned(1, 4).await);
        assert!(network.is_partitioned(2, 3).await);

        // Nodes within same group should not be partitioned
        assert!(!network.is_partitioned(0, 1).await);
        assert!(!network.is_partitioned(3, 4).await);

        // Heal partition
        network.heal_all_partitions().await;

        // Verify all partitions cleared
        assert!(!network.is_partitioned(0, 3).await);
        assert!(!network.is_partitioned(1, 4).await);
    }

    #[tokio::test]
    async fn test_basic_consensus() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_genesis_account("alice", 1_000_000)
            .build()
            .await
            .unwrap();

        let (alice_addr, _) = network.get_genesis_account("alice").unwrap();
        let bob_addr = create_test_address(99);

        // Submit transaction on node 0
        let tx = TestTransaction {
            hash: Hash::zero(),
            sender: alice_addr.clone(),
            recipient: bob_addr.clone(),
            amount: 100_000,
            fee: 50,
            nonce: 1,
        };

        network
            .node(0)
            .daemon()
            .submit_transaction(tx)
            .await
            .unwrap();

        // Mine block on node 0
        network.node(0).daemon().mine_block().await.unwrap();

        // In a real multi-node scenario, we'd propagate blocks here
        // For now, verify node 0 has the transaction

        assert_eq!(network.node(0).get_tip_height().await.unwrap(), 1);
        assert_eq!(
            network.node(0).get_balance(&bob_addr).await.unwrap(),
            100_000
        );
    }

    #[tokio::test]
    async fn test_transaction_propagation() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_topology(NetworkTopology::FullMesh)
            .with_genesis_account("alice", 1_000_000)
            .with_seed(700)
            .build()
            .await
            .unwrap();

        let (alice_addr, _) = network.get_genesis_account("alice").unwrap();
        let bob_addr = create_test_address(100);

        // Create transaction
        let tx = TestTransaction {
            hash: Hash::zero(),
            sender: alice_addr.clone(),
            recipient: bob_addr.clone(),
            amount: 500_000,
            fee: 100,
            nonce: 1,
        };

        // Submit to node 0 and propagate
        network.submit_and_propagate(0, tx.clone()).await.unwrap();

        // In full mesh, node 0 connects to nodes 1 and 2
        // Verify propagation happened (nodes should have tx in mempool)
        // Since we can't directly check mempool, mine on node 1 and verify
        network.node(1).daemon().mine_block().await.unwrap();

        // Node 1 should have mined the transaction
        assert_eq!(
            network.node(1).get_balance(&bob_addr).await.unwrap(),
            500_000
        );
    }

    #[tokio::test]
    async fn test_propagation_respects_partitions() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_topology(NetworkTopology::FullMesh)
            .with_genesis_account("alice", 1_000_000)
            .with_seed(800)
            .build()
            .await
            .unwrap();

        // Partition node 0 from nodes 1 and 2
        network.partition_groups(&[0], &[1, 2]).await.unwrap();

        let (alice_addr, _) = network.get_genesis_account("alice").unwrap();
        let bob_addr = create_test_address(101);

        let tx = TestTransaction {
            hash: Hash::zero(),
            sender: alice_addr.clone(),
            recipient: bob_addr.clone(),
            amount: 500_000,
            fee: 100,
            nonce: 1,
        };

        // Try to propagate from node 0
        let propagated = network
            .propagate_transaction_from(0, tx.clone())
            .await
            .unwrap();

        // Should propagate to 0 nodes due to partition
        assert_eq!(propagated, 0);

        // Heal partition and try again
        network.heal_all_partitions().await;
        let propagated = network.propagate_transaction_from(0, tx).await.unwrap();

        // Should now propagate to 2 peers (nodes 1 and 2)
        assert_eq!(propagated, 2);
    }

    #[tokio::test]
    async fn test_ring_topology_propagation() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_topology(NetworkTopology::Ring) // 0→1→2→3→0
            .with_genesis_account("alice", 1_000_000)
            .with_seed(900)
            .build()
            .await
            .unwrap();

        let (alice_addr, _) = network.get_genesis_account("alice").unwrap();
        let bob_addr = create_test_address(102);

        let tx = TestTransaction {
            hash: Hash::zero(),
            sender: alice_addr.clone(),
            recipient: bob_addr.clone(),
            amount: 300_000,
            fee: 50,
            nonce: 1,
        };

        // In ring topology, each node connects to only 2 neighbors
        // Node 0 connects to node 1 (next) and node 3 (prev)
        let propagated = network.propagate_transaction_from(0, tx).await.unwrap();

        // Should propagate to exactly 2 peers in ring
        assert_eq!(propagated, 2);
    }

    #[tokio::test]
    async fn test_block_propagation() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(3)
            .with_topology(NetworkTopology::FullMesh)
            .with_genesis_account("alice", 1_000_000)
            .with_seed(1000)
            .build()
            .await
            .unwrap();

        let (alice_addr, _) = network.get_genesis_account("alice").unwrap();
        let bob_addr = create_test_address(103);

        // Submit transaction and propagate
        let tx = TestTransaction {
            hash: Hash::zero(),
            sender: alice_addr.clone(),
            recipient: bob_addr.clone(),
            amount: 400_000,
            fee: 100,
            nonce: 1,
        };
        network.submit_and_propagate(0, tx).await.unwrap();

        // Mine block on node 0 and propagate
        network.mine_and_propagate(0).await.unwrap();

        // Verify all nodes now have the block at height 1
        assert_eq!(network.node(0).get_tip_height().await.unwrap(), 1);
        assert_eq!(network.node(1).get_tip_height().await.unwrap(), 1);
        assert_eq!(network.node(2).get_tip_height().await.unwrap(), 1);

        // Verify all nodes have the same balance for bob
        assert_eq!(
            network.node(0).get_balance(&bob_addr).await.unwrap(),
            400_000
        );
        assert_eq!(
            network.node(1).get_balance(&bob_addr).await.unwrap(),
            400_000
        );
        assert_eq!(
            network.node(2).get_balance(&bob_addr).await.unwrap(),
            400_000
        );
    }

    #[tokio::test]
    async fn test_block_propagation_respects_partitions() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(4)
            .with_topology(NetworkTopology::FullMesh)
            .with_genesis_account("alice", 1_000_000)
            .with_seed(1100)
            .build()
            .await
            .unwrap();

        // Partition: [0, 1] vs [2, 3]
        network.partition_groups(&[0, 1], &[2, 3]).await.unwrap();

        // Mine block on node 0
        network.node(0).daemon().mine_block().await.unwrap();

        // Propagate from node 0
        let propagated = network.propagate_block_from(0, 1).await.unwrap();

        // Should only propagate to node 1 (not across partition to 2, 3)
        assert_eq!(propagated, 1);

        // Verify heights
        assert_eq!(network.node(0).get_tip_height().await.unwrap(), 1);
        assert_eq!(network.node(1).get_tip_height().await.unwrap(), 1);
        assert_eq!(network.node(2).get_tip_height().await.unwrap(), 0); // Partitioned
        assert_eq!(network.node(3).get_tip_height().await.unwrap(), 0); // Partitioned
    }

    #[tokio::test]
    async fn test_full_consensus_convergence() {
        let network = LocalTosNetworkBuilder::new()
            .with_nodes(5)
            .with_topology(NetworkTopology::FullMesh)
            .with_genesis_account("alice", 10_000_000)
            .with_seed(1200)
            .build()
            .await
            .unwrap();

        let (alice_addr, _) = network.get_genesis_account("alice").unwrap();
        let bob_addr = create_test_address(104);

        // Submit multiple transactions and propagate
        for i in 1..=3 {
            let tx = TestTransaction {
                hash: Hash::zero(),
                sender: alice_addr.clone(),
                recipient: bob_addr.clone(),
                amount: 100_000 * i,
                fee: 50,
                nonce: i,
            };
            network.submit_and_propagate(0, tx).await.unwrap();
        }

        // Mine and propagate blocks
        network.mine_and_propagate(0).await.unwrap();
        network.mine_and_propagate(1).await.unwrap();
        network.mine_and_propagate(2).await.unwrap();

        // All nodes should converge to height 3
        for i in 0..5 {
            assert_eq!(network.node(i).get_tip_height().await.unwrap(), 3);
        }

        // All nodes should have consistent state for bob
        let expected_balance = 100_000 + 200_000 + 300_000; // Sum of all transfers
        for i in 0..5 {
            assert_eq!(
                network.node(i).get_balance(&bob_addr).await.unwrap(),
                expected_balance
            );
        }

        // All nodes should have alice's nonce as 3
        for i in 0..5 {
            assert_eq!(network.node(i).get_nonce(alice_addr).await.unwrap(), 3);
        }
    }
}
