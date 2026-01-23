//! Enhanced cluster configuration for multi-node testing.
//!
//! Provides fine-grained control over node roles, mining behavior,
//! network topology, and bootstrap sync configuration.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use tos_common::crypto::Hash;

/// Role of a node in the cluster.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NodeRole {
    /// Full node that participates in mining
    #[default]
    Miner,
    /// Full node that does not mine
    FullNode,
    /// Node that only syncs and verifies
    LightNode,
}

/// Mining configuration for the cluster.
#[derive(Debug, Clone)]
pub struct MiningConfig {
    /// Which nodes are allowed to mine
    pub miners: Vec<usize>,
    /// Block interval for auto-mining (None = manual)
    pub interval: Option<Duration>,
    /// Maximum transactions per block
    pub max_txs_per_block: usize,
}

impl Default for MiningConfig {
    fn default() -> Self {
        Self {
            miners: vec![0], // Only first node mines by default
            interval: None,
            max_txs_per_block: 100,
        }
    }
}

/// Per-node configuration overrides.
#[derive(Debug, Clone)]
pub struct NodeConfig {
    /// Node role in the cluster
    pub role: NodeRole,
    /// Whether mining is enabled for this node
    pub mining_enabled: bool,
    /// Maximum number of peers this node will connect to
    pub max_peers: usize,
    /// Custom storage path (None = auto-generated TempDir)
    pub storage_path: Option<PathBuf>,
    /// Initial delay before this node starts (simulates late join)
    pub start_delay: Option<Duration>,
    /// Whether this node uses bootstrap sync
    pub bootstrap_sync: bool,
    /// Custom P2P port (None = auto-assigned)
    pub p2p_port: Option<u16>,
    /// Custom RPC port (None = auto-assigned)
    pub rpc_port: Option<u16>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            role: NodeRole::default(),
            mining_enabled: false,
            max_peers: 8,
            storage_path: None,
            start_delay: None,
            bootstrap_sync: false,
            p2p_port: None,
            rpc_port: None,
        }
    }
}

impl NodeConfig {
    /// Create a miner node configuration.
    pub fn miner() -> Self {
        Self {
            role: NodeRole::Miner,
            mining_enabled: true,
            ..Default::default()
        }
    }

    /// Create a full node configuration (non-mining).
    pub fn full_node() -> Self {
        Self {
            role: NodeRole::FullNode,
            mining_enabled: false,
            ..Default::default()
        }
    }

    /// Create a bootstrap sync node configuration.
    pub fn bootstrap() -> Self {
        Self {
            role: NodeRole::FullNode,
            mining_enabled: false,
            bootstrap_sync: true,
            ..Default::default()
        }
    }

    /// Set a start delay for this node.
    pub fn with_start_delay(mut self, delay: Duration) -> Self {
        self.start_delay = Some(delay);
        self
    }

    /// Set the maximum number of peers.
    pub fn with_max_peers(mut self, max_peers: usize) -> Self {
        self.max_peers = max_peers;
        self
    }
}

/// Complete cluster configuration.
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// Number of nodes in the cluster
    pub node_count: usize,
    /// Per-node configuration overrides (index -> config)
    pub node_configs: HashMap<usize, NodeConfig>,
    /// Genesis accounts with initial balances
    pub genesis_accounts: Vec<(Hash, u64)>,
    /// Mining configuration
    pub mining: MiningConfig,
    /// Whether to enable bootstrap sync for non-miner nodes
    pub bootstrap_sync: bool,
    /// P2P base port (incremented per node)
    pub p2p_base_port: u16,
    /// RPC base port (incremented per node)
    pub rpc_base_port: u16,
    /// Timeout for cluster operations
    pub operation_timeout: Duration,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            node_count: 3,
            node_configs: HashMap::new(),
            genesis_accounts: Vec::new(),
            mining: MiningConfig::default(),
            bootstrap_sync: false,
            p2p_base_port: 18000,
            rpc_base_port: 19000,
            operation_timeout: Duration::from_secs(30),
        }
    }
}

impl ClusterConfig {
    /// Create a cluster with the given number of nodes.
    pub fn with_nodes(mut self, count: usize) -> Self {
        self.node_count = count;
        self
    }

    /// Add a genesis account.
    pub fn with_genesis_account(mut self, address: Hash, balance: u64) -> Self {
        self.genesis_accounts.push((address, balance));
        self
    }

    /// Set the mining configuration.
    pub fn with_mining(mut self, mining: MiningConfig) -> Self {
        self.mining = mining;
        self
    }

    /// Override configuration for a specific node.
    pub fn with_node_config(mut self, index: usize, config: NodeConfig) -> Self {
        self.node_configs.insert(index, config);
        self
    }

    /// Enable bootstrap sync for non-miner nodes.
    pub fn with_bootstrap_sync(mut self) -> Self {
        self.bootstrap_sync = true;
        self
    }

    /// Set the operation timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.operation_timeout = timeout;
        self
    }

    /// Get the effective configuration for a specific node.
    pub fn effective_node_config(&self, index: usize) -> NodeConfig {
        if let Some(config) = self.node_configs.get(&index) {
            config.clone()
        } else if self.mining.miners.contains(&index) {
            NodeConfig::miner()
        } else {
            let mut config = NodeConfig::full_node();
            if self.bootstrap_sync {
                config.bootstrap_sync = true;
            }
            config
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_cluster_config() {
        let config = ClusterConfig::default();
        assert_eq!(config.node_count, 3);
        assert!(config.node_configs.is_empty());
        assert_eq!(config.mining.miners, vec![0]);
    }

    #[test]
    fn test_effective_node_config() {
        let config = ClusterConfig::default()
            .with_nodes(3)
            .with_node_config(1, NodeConfig::bootstrap());

        let node0 = config.effective_node_config(0);
        assert!(node0.mining_enabled);

        let node1 = config.effective_node_config(1);
        assert!(node1.bootstrap_sync);
        assert!(!node1.mining_enabled);

        let node2 = config.effective_node_config(2);
        assert!(!node2.mining_enabled);
    }

    #[test]
    fn test_mining_config() {
        let mining = MiningConfig {
            miners: vec![0, 2],
            interval: Some(Duration::from_secs(1)),
            max_txs_per_block: 50,
        };
        assert_eq!(mining.miners.len(), 2);
    }

    #[test]
    fn test_node_config_builders() {
        let miner = NodeConfig::miner();
        assert_eq!(miner.role, NodeRole::Miner);
        assert!(miner.mining_enabled);

        let full = NodeConfig::full_node();
        assert_eq!(full.role, NodeRole::FullNode);
        assert!(!full.mining_enabled);

        let bootstrap = NodeConfig::bootstrap()
            .with_start_delay(Duration::from_secs(5))
            .with_max_peers(16);
        assert!(bootstrap.bootstrap_sync);
        assert_eq!(bootstrap.max_peers, 16);
        assert_eq!(bootstrap.start_delay, Some(Duration::from_secs(5)));
    }
}
