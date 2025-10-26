//! TOS Multi-Node Integration Testing Framework
//!
//! This module provides infrastructure for testing TOS blockchain behavior
//! across multiple nodes with simulated network conditions.
//!
//! # Architecture
//!
//! - `network_simulator`: Simulates network latency, jitter, packet loss
//! - `node_harness`: Manages daemon process lifecycle
//! - `metrics`: Tracks propagation time, TPS, consensus metrics
//! - `scenarios`: Pre-built test scenarios for common patterns
//!
//! # Usage
//!
//! ```rust,no_run
//! use tos_integration::*;
//!
//! #[tokio::test]
//! async fn test_multi_node_consensus() {
//!     let config = NetworkConfig {
//!         base_latency_ms: 200,
//!         jitter_ms: 100,
//!         packet_loss_rate: 0.01,
//!         bandwidth_mbps: None,
//!     };
//!
//!     let harness = MultiNodeHarness::new(3, config).await.unwrap();
//!     // Test scenarios...
//! }
//! ```

pub mod network_simulator;
pub mod node_harness;
pub mod metrics;
pub mod scenarios;
pub mod utils;

pub use network_simulator::{NetworkConfig, NetworkSimulator, NetworkDelay};
pub use node_harness::{NodeConfig, NodeHandle, MultiNodeHarness};
pub use metrics::{NetworkMetrics, ConsensusMetrics, MetricsCollector};
pub use scenarios::{ScenarioRunner, TestScenario};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum IntegrationError {
    #[error("Failed to spawn node: {0}")]
    NodeSpawnError(String),

    #[error("Node RPC error: {0}")]
    RpcError(String),

    #[error("Network simulation error: {0}")]
    NetworkError(String),

    #[error("Timeout waiting for condition: {0}")]
    Timeout(String),

    #[error("Consensus error: {0}")]
    ConsensusError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, IntegrationError>;

// Default test configurations

/// Fast local network (LAN)
pub const NETWORK_CONFIG_FAST: NetworkConfig = NetworkConfig {
    base_latency_ms: 10,
    jitter_ms: 5,
    packet_loss_rate: 0.001,
    bandwidth_mbps: None,
};

/// Medium quality network (good internet)
pub const NETWORK_CONFIG_MEDIUM: NetworkConfig = NetworkConfig {
    base_latency_ms: 100,
    jitter_ms: 50,
    packet_loss_rate: 0.01,
    bandwidth_mbps: Some(100),
};

/// Poor quality network (congested/mobile)
pub const NETWORK_CONFIG_POOR: NetworkConfig = NetworkConfig {
    base_latency_ms: 500,
    jitter_ms: 200,
    packet_loss_rate: 0.05,
    bandwidth_mbps: Some(10),
};

/// Production-like network (realistic conditions)
pub const NETWORK_CONFIG_PRODUCTION: NetworkConfig = NetworkConfig {
    base_latency_ms: 200,
    jitter_ms: 100,
    packet_loss_rate: 0.02,
    bandwidth_mbps: Some(50),
};
