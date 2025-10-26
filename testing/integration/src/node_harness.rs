//! Node Harness
//!
//! Manages TOS daemon processes for multi-node testing.
//! Provides:
//! - Process spawning and lifecycle management
//! - RPC client for node interaction
//! - Network simulation integration
//! - Health monitoring and cleanup

use crate::{IntegrationError, NetworkConfig, NetworkSimulator, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

/// Configuration for a single node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Node ID (0-indexed)
    pub node_id: usize,

    /// RPC bind address
    pub rpc_bind_address: String,

    /// P2P bind address
    pub p2p_bind_address: String,

    /// Data directory path
    pub data_dir: PathBuf,

    /// Network type (mainnet, testnet, devnet)
    pub network: String,

    /// Log level
    pub log_level: String,

    /// Seed nodes to connect to (P2P addresses)
    pub seed_nodes: Vec<String>,

    /// Auto-compress logs
    pub auto_compress_logs: bool,
}

impl NodeConfig {
    /// Create a default devnet node configuration
    ///
    /// Default ports: RPC=9080+, P2P=3125+ (offset to avoid conflicts with default daemon ports)
    pub fn devnet(node_id: usize, base_rpc_port: u16, base_p2p_port: u16) -> Self {
        let rpc_port = base_rpc_port + node_id as u16;
        let p2p_port = base_p2p_port + node_id as u16;

        Self {
            node_id,
            rpc_bind_address: format!("127.0.0.1:{}", rpc_port),
            p2p_bind_address: format!("0.0.0.0:{}", p2p_port),
            data_dir: PathBuf::from(format!("/tmp/tos_test_node_{}", node_id)),
            network: "devnet".to_string(),
            log_level: "info".to_string(),
            seed_nodes: vec![],
            auto_compress_logs: false,
        }
    }

    /// Get RPC port
    pub fn rpc_port(&self) -> u16 {
        self.rpc_bind_address
            .split(':')
            .last()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8080)
    }

    /// Get P2P port
    pub fn p2p_port(&self) -> u16 {
        self.p2p_bind_address
            .split(':')
            .last()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2125)
    }

    /// Get RPC URL
    pub fn rpc_url(&self) -> String {
        format!("http://{}", self.rpc_bind_address)
    }
}

/// Handle to a running node
pub struct NodeHandle {
    /// Node configuration
    config: NodeConfig,

    /// Process handle
    process: Option<Child>,

    /// Network simulator for this node's connections
    network_sim: Option<NetworkSimulator>,

    /// Whether the node is currently running
    running: bool,
}

impl NodeHandle {
    /// Create a new node handle (does not spawn the process)
    pub fn new(config: NodeConfig) -> Self {
        Self {
            config,
            process: None,
            network_sim: None,
            running: false,
        }
    }

    /// Spawn the daemon process
    pub async fn spawn(&mut self, daemon_path: &str) -> Result<()> {
        if self.running {
            return Err(IntegrationError::Other(
                "Node is already running".to_string(),
            ));
        }

        // Create data directory
        std::fs::create_dir_all(&self.config.data_dir)?;

        // Build command arguments
        // Canonicalize the daemon path to ensure it works from any directory
        let daemon_path = std::path::Path::new(daemon_path)
            .canonicalize()
            .map_err(|e| {
                IntegrationError::NodeSpawnError(format!(
                    "Failed to find daemon binary at {}: {}",
                    daemon_path, e
                ))
            })?;

        let mut cmd = Command::new(daemon_path);

        // Ensure data_dir ends with / as required by daemon
        let data_dir_str = self.config.data_dir.to_string_lossy();
        let data_dir_with_slash = if data_dir_str.ends_with('/') || data_dir_str.ends_with('\\') {
            data_dir_str.to_string()
        } else {
            format!("{}/", data_dir_str)
        };

        cmd.arg("--network")
            .arg(&self.config.network)
            .arg("--dir-path")
            .arg(&data_dir_with_slash)
            .arg("--rpc-bind-address")
            .arg(&self.config.rpc_bind_address)
            .arg("--p2p-bind-address")
            .arg(&self.config.p2p_bind_address)
            .arg("--log-level")
            .arg(&self.config.log_level);

        if self.config.auto_compress_logs {
            cmd.arg("--auto-compress-logs");
        }

        // Add seed nodes
        for seed in &self.config.seed_nodes {
            cmd.arg("--seed-node").arg(seed);
        }

        // Redirect stdout/stderr to files for debugging
        let stdout_path = self.config.data_dir.join("stdout.log");
        let stderr_path = self.config.data_dir.join("stderr.log");

        let stdout_file = std::fs::File::create(&stdout_path)?;
        let stderr_file = std::fs::File::create(&stderr_path)?;

        cmd.stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file));

        // Spawn the process
        let child = cmd.spawn().map_err(|e| {
            IntegrationError::NodeSpawnError(format!(
                "Failed to spawn node {}: {}",
                self.config.node_id, e
            ))
        })?;

        self.process = Some(child);
        self.running = true;

        log::info!(
            "Spawned node {} with RPC={} P2P={}",
            self.config.node_id,
            self.config.rpc_bind_address,
            self.config.p2p_bind_address
        );

        Ok(())
    }

    /// Wait for the node to be ready (RPC responding)
    pub async fn wait_for_ready(&self, timeout_secs: u64) -> Result<()> {
        let start = std::time::Instant::now();
        let timeout_duration = Duration::from_secs(timeout_secs);

        while start.elapsed() < timeout_duration {
            if self.is_rpc_ready().await {
                log::info!("Node {} is ready", self.config.node_id);
                return Ok(());
            }
            sleep(Duration::from_millis(500)).await;
        }

        Err(IntegrationError::Timeout(format!(
            "Node {} did not become ready within {} seconds",
            self.config.node_id, timeout_secs
        )))
    }

    /// Check if RPC is responding
    async fn is_rpc_ready(&self) -> bool {
        // Try to make a JSON-RPC call to check if daemon is ready
        let url = format!("{}/json_rpc", self.config.rpc_url());
        let client = reqwest::Client::new();

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "1",
            "method": "get_info"
        });

        match client.post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    /// Stop the node
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(mut child) = self.process.take() {
            log::info!("Stopping node {}", self.config.node_id);

            // Send SIGTERM
            #[cfg(unix)]
            {
                use nix::sys::signal::{self, Signal};
                use nix::unistd::Pid;

                if let Some(pid) = child.id() {
                    let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                }
            }

            // Wait for graceful shutdown (with timeout)
            match timeout(Duration::from_secs(10), child.wait()).await {
                Ok(Ok(status)) => {
                    log::info!("Node {} exited with status: {:?}", self.config.node_id, status);
                }
                Ok(Err(e)) => {
                    log::warn!("Error waiting for node {} to exit: {}", self.config.node_id, e);
                }
                Err(_) => {
                    log::warn!("Node {} did not stop within timeout, killing", self.config.node_id);
                    let _ = child.kill().await;
                }
            }

            self.running = false;
        }

        Ok(())
    }

    /// Clean up node data directory
    pub fn cleanup(&self) -> Result<()> {
        if self.config.data_dir.exists() {
            std::fs::remove_dir_all(&self.config.data_dir)?;
            log::info!("Cleaned up data directory for node {}", self.config.node_id);
        }
        Ok(())
    }

    /// Get node configuration
    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    /// Set network simulator for this node
    pub fn set_network_simulator(&mut self, sim: NetworkSimulator) {
        self.network_sim = Some(sim);
    }

    /// Get network simulator
    pub fn network_simulator(&self) -> Option<&NetworkSimulator> {
        self.network_sim.as_ref()
    }

    /// Check if node is running
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Make an RPC call (with network simulation if configured)
    pub async fn rpc_call<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
    ) -> Result<T> {
        // Apply network delay if simulator is configured
        if let Some(sim) = &self.network_sim {
            if !sim.apply_delay().await {
                return Err(IntegrationError::NetworkError(
                    "Packet dropped by network simulator".to_string(),
                ));
            }
        }

        let url = format!("{}{}", self.config.rpc_url(), endpoint);

        let response = reqwest::get(&url)
            .await
            .map_err(|e| IntegrationError::RpcError(e.to_string()))?;

        let data = response
            .json::<T>()
            .await
            .map_err(|e| IntegrationError::RpcError(e.to_string()))?;

        Ok(data)
    }
}

impl Drop for NodeHandle {
    fn drop(&mut self) {
        if self.running {
            // Attempt to stop the node on drop
            if let Some(mut child) = self.process.take() {
                let _ = child.start_kill();
            }
        }
    }
}

/// Multi-node test harness
pub struct MultiNodeHarness {
    /// All nodes in the test
    nodes: Vec<NodeHandle>,

    /// Global network configuration
    network_config: NetworkConfig,

    /// Path to daemon binary
    daemon_path: String,
}

impl MultiNodeHarness {
    /// Create a new multi-node harness
    ///
    /// # Arguments
    /// * `num_nodes` - Number of nodes to spawn
    /// * `network_config` - Network simulation configuration
    pub async fn new(num_nodes: usize, network_config: NetworkConfig) -> Result<Self> {
        Self::new_with_daemon(num_nodes, network_config, "./target/debug/tos_daemon").await
    }

    /// Create a new multi-node harness with custom daemon path
    pub async fn new_with_daemon(
        num_nodes: usize,
        network_config: NetworkConfig,
        daemon_path: &str,
    ) -> Result<Self> {
        let mut nodes = Vec::new();

        // Create node configurations
        // Use ports 9080+ for RPC and 3125+ for P2P to avoid conflicts with default daemon (8080, 2125)
        for i in 0..num_nodes {
            let config = NodeConfig::devnet(i, 9080, 3125);
            let mut node = NodeHandle::new(config);

            // Attach network simulator to each node
            let sim = NetworkSimulator::new(network_config)
                .map_err(|e| IntegrationError::NetworkError(e))?;
            node.set_network_simulator(sim);

            nodes.push(node);
        }

        Ok(Self {
            nodes,
            network_config,
            daemon_path: daemon_path.to_string(),
        })
    }

    /// Spawn all nodes
    pub async fn spawn_all(&mut self) -> Result<()> {
        log::info!("Spawning {} nodes", self.nodes.len());

        for node in &mut self.nodes {
            node.spawn(&self.daemon_path).await?;
        }

        Ok(())
    }

    /// Wait for all nodes to be ready
    pub async fn wait_for_all_ready(&self, timeout_secs: u64) -> Result<()> {
        log::info!("Waiting for all nodes to be ready...");

        for node in &self.nodes {
            node.wait_for_ready(timeout_secs).await?;
        }

        log::info!("All nodes are ready");
        Ok(())
    }

    /// Connect nodes in a full mesh topology
    pub async fn connect_full_mesh(&mut self) -> Result<()> {
        log::info!("Connecting nodes in full mesh topology");

        // Build seed node list from all P2P addresses
        let seed_nodes: Vec<String> = self
            .nodes
            .iter()
            .map(|n| n.config().p2p_bind_address.clone())
            .collect();

        // Update each node's seed list (excluding itself)
        for i in 0..self.nodes.len() {
            let mut seeds = seed_nodes.clone();
            seeds.remove(i);
            self.nodes[i].config.seed_nodes = seeds;
        }

        Ok(())
    }

    /// Stop all nodes
    pub async fn stop_all(&mut self) -> Result<()> {
        log::info!("Stopping all nodes");

        for node in &mut self.nodes {
            node.stop().await?;
        }

        Ok(())
    }

    /// Clean up all node data directories
    pub fn cleanup_all(&self) -> Result<()> {
        log::info!("Cleaning up all node data");

        for node in &self.nodes {
            node.cleanup()?;
        }

        Ok(())
    }

    /// Get a reference to a specific node
    pub fn node(&self, index: usize) -> Option<&NodeHandle> {
        self.nodes.get(index)
    }

    /// Get a mutable reference to a specific node
    pub fn node_mut(&mut self, index: usize) -> Option<&mut NodeHandle> {
        self.nodes.get_mut(index)
    }

    /// Get all nodes
    pub fn nodes(&self) -> &[NodeHandle] {
        &self.nodes
    }

    /// Get number of nodes
    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Get network configuration
    pub fn network_config(&self) -> &NetworkConfig {
        &self.network_config
    }
}

impl Drop for MultiNodeHarness {
    fn drop(&mut self) {
        // Ensure cleanup on drop
        // TEMPORARILY DISABLED FOR DEBUGGING - uncomment after fixing tests
        // let _ = self.cleanup_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_config_devnet() {
        let config = NodeConfig::devnet(0, 9080, 3125);

        assert_eq!(config.node_id, 0);
        assert_eq!(config.rpc_port(), 9080);
        assert_eq!(config.p2p_port(), 3125);
        assert_eq!(config.rpc_url(), "http://127.0.0.1:9080");
    }

    #[test]
    fn test_node_config_multiple() {
        let config0 = NodeConfig::devnet(0, 9080, 3125);
        let config1 = NodeConfig::devnet(1, 9080, 3125);
        let config2 = NodeConfig::devnet(2, 9080, 3125);

        assert_eq!(config0.rpc_port(), 9080);
        assert_eq!(config1.rpc_port(), 9081);
        assert_eq!(config2.rpc_port(), 9082);

        assert_eq!(config0.p2p_port(), 3125);
        assert_eq!(config1.p2p_port(), 3126);
        assert_eq!(config2.p2p_port(), 3127);
    }
}
