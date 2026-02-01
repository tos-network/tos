//! Configuration for the discovery protocol.

use serde::{Deserialize, Serialize};
use tos_common::crypto::ed25519::WrappedEd25519Secret;

use super::routing_table::DEFAULT_BUCKET_SIZE;

/// Default discovery port.
pub const DEFAULT_DISCOVERY_PORT: u16 = 2126;

/// Default bucket size for Kademlia routing table.
const fn default_bucket_size() -> usize {
    DEFAULT_BUCKET_SIZE
}

/// Default discovery port.
const fn default_discovery_port() -> u16 {
    DEFAULT_DISCOVERY_PORT
}

/// Configuration for the discovery protocol.
#[derive(Debug, Clone, clap::Args, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Run in discovery-only (bootnode) mode.
    ///
    /// In this mode, the node only participates in peer discovery
    /// and does not sync or validate the blockchain.
    #[clap(name = "p2p-discovery-only", long)]
    #[serde(default)]
    pub discovery_only: bool,

    /// UDP port for discovery protocol.
    #[clap(name = "discovery-port", long, default_value_t = default_discovery_port())]
    #[serde(default = "default_discovery_port")]
    pub port: u16,

    /// Private key for node identity (hex format, 32 bytes).
    ///
    /// If not provided, a new key will be generated on startup.
    /// For persistent node identity, save and reuse the generated key.
    /// The key is used to derive a Schnorr key pair for signing discovery messages.
    #[clap(name = "discovery-private-key", long, env = "DISCOVERY_PRIVATE_KEY")]
    #[serde(default)]
    pub private_key: Option<WrappedEd25519Secret>,

    /// Bootstrap nodes to connect to on startup.
    ///
    /// Format: tosnode://<node_id_hex>@<ip>:<port>
    #[clap(name = "discovery-bootstrap", long)]
    #[serde(default)]
    pub bootstrap_nodes: Vec<String>,

    /// Kademlia bucket size (k parameter).
    ///
    /// Number of nodes stored per bucket in the routing table.
    #[clap(name = "discovery-bucket-size", long, default_value_t = default_bucket_size())]
    #[serde(default = "default_bucket_size")]
    pub bucket_size: usize,

    /// Disable the discovery protocol.
    ///
    /// When disabled, the node will not participate in peer discovery
    /// and will only connect to manually specified peers.
    #[clap(name = "disable-discovery", long)]
    #[serde(default)]
    pub disable: bool,

    /// Bind address for discovery UDP socket.
    ///
    /// If not specified, binds to 0.0.0.0.
    #[clap(name = "discovery-bind-address", long)]
    #[serde(default)]
    pub bind_address: Option<String>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            discovery_only: false,
            port: DEFAULT_DISCOVERY_PORT,
            private_key: None,
            bootstrap_nodes: Vec::new(),
            bucket_size: DEFAULT_BUCKET_SIZE,
            disable: false,
            bind_address: None,
        }
    }
}

impl DiscoveryConfig {
    /// Get the bind address for the UDP socket.
    pub fn get_bind_address(&self) -> String {
        self.bind_address
            .clone()
            .unwrap_or_else(|| format!("0.0.0.0:{}", self.port))
    }

    /// Check if discovery is enabled.
    pub fn is_enabled(&self) -> bool {
        !self.disable
    }

    /// Check if running in bootnode mode.
    pub fn is_bootnode(&self) -> bool {
        self.discovery_only
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = DiscoveryConfig::default();

        assert!(!config.discovery_only);
        assert_eq!(config.port, DEFAULT_DISCOVERY_PORT);
        assert!(config.private_key.is_none());
        assert!(config.bootstrap_nodes.is_empty());
        assert_eq!(config.bucket_size, DEFAULT_BUCKET_SIZE);
        assert!(!config.disable);
        assert!(config.bind_address.is_none());
    }

    #[test]
    fn test_get_bind_address_default() {
        let config = DiscoveryConfig::default();
        assert_eq!(config.get_bind_address(), "0.0.0.0:2126");
    }

    #[test]
    fn test_get_bind_address_custom() {
        let mut config = DiscoveryConfig::default();
        config.bind_address = Some("127.0.0.1:9999".to_string());
        assert_eq!(config.get_bind_address(), "127.0.0.1:9999");
    }

    #[test]
    fn test_is_enabled() {
        let mut config = DiscoveryConfig::default();
        assert!(config.is_enabled());

        config.disable = true;
        assert!(!config.is_enabled());
    }

    #[test]
    fn test_is_bootnode() {
        let mut config = DiscoveryConfig::default();
        assert!(!config.is_bootnode());

        config.discovery_only = true;
        assert!(config.is_bootnode());
    }
}
