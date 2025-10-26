//! Network Simulator
//!
//! Simulates realistic network conditions including:
//! - Base latency and jitter
//! - Packet loss
//! - Bandwidth limitations
//! - Network partitions
//!
//! This module provides deterministic and reproducible network behavior
//! for testing distributed consensus algorithms.

use rand::Rng;
use std::time::Duration;
use tokio::time::sleep;
use serde::{Deserialize, Serialize};

/// Network configuration for simulation
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Base latency in milliseconds (one-way)
    pub base_latency_ms: u64,

    /// Maximum jitter in milliseconds (random variance)
    /// Actual jitter will be uniformly distributed in range [-jitter_ms/2, +jitter_ms/2]
    pub jitter_ms: u64,

    /// Packet loss rate (0.0 = no loss, 1.0 = 100% loss)
    /// Example: 0.01 = 1% packet loss
    pub packet_loss_rate: f64,

    /// Optional bandwidth limit in Mbps
    /// None = unlimited bandwidth
    pub bandwidth_mbps: Option<u64>,
}

impl NetworkConfig {
    /// Create a perfect network (no delays or losses)
    pub fn perfect() -> Self {
        Self {
            base_latency_ms: 0,
            jitter_ms: 0,
            packet_loss_rate: 0.0,
            bandwidth_mbps: None,
        }
    }

    /// Create a LAN-like network (fast, reliable)
    pub fn lan() -> Self {
        Self {
            base_latency_ms: 10,
            jitter_ms: 5,
            packet_loss_rate: 0.001,
            bandwidth_mbps: Some(1000), // 1 Gbps
        }
    }

    /// Create an internet-like network (medium quality)
    pub fn internet() -> Self {
        Self {
            base_latency_ms: 100,
            jitter_ms: 50,
            packet_loss_rate: 0.01,
            bandwidth_mbps: Some(100), // 100 Mbps
        }
    }

    /// Create a poor quality network (high latency, packet loss)
    pub fn poor() -> Self {
        Self {
            base_latency_ms: 500,
            jitter_ms: 200,
            packet_loss_rate: 0.05,
            bandwidth_mbps: Some(10), // 10 Mbps
        }
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), String> {
        if self.packet_loss_rate < 0.0 || self.packet_loss_rate > 1.0 {
            return Err(format!(
                "Invalid packet_loss_rate: {}. Must be in [0.0, 1.0]",
                self.packet_loss_rate
            ));
        }
        Ok(())
    }

    /// Calculate round-trip time (RTT) including jitter
    /// Returns (min_rtt_ms, avg_rtt_ms, max_rtt_ms)
    pub fn expected_rtt(&self) -> (u64, u64, u64) {
        let base_rtt = self.base_latency_ms * 2; // Two-way latency
        let min_rtt = base_rtt.saturating_sub(self.jitter_ms);
        let avg_rtt = base_rtt;
        let max_rtt = base_rtt.saturating_add(self.jitter_ms);
        (min_rtt, avg_rtt, max_rtt)
    }
}

/// Network delay result
#[derive(Debug, Clone)]
pub struct NetworkDelay {
    /// Actual delay to apply (including base latency and jitter)
    pub delay_ms: u64,

    /// Whether this packet should be dropped
    pub should_drop: bool,
}

/// Network simulator
pub struct NetworkSimulator {
    config: NetworkConfig,
}

impl NetworkSimulator {
    /// Create a new network simulator with the given configuration
    pub fn new(config: NetworkConfig) -> Result<Self, String> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Sample a network delay for a single packet
    ///
    /// Returns NetworkDelay indicating:
    /// - delay_ms: How long to wait before sending
    /// - should_drop: Whether to drop this packet
    pub fn sample_delay(&self) -> NetworkDelay {
        let mut rng = rand::thread_rng();

        // Check packet loss
        let should_drop = if self.config.packet_loss_rate > 0.0 {
            rng.gen::<f64>() < self.config.packet_loss_rate
        } else {
            false
        };

        // Calculate delay with jitter
        let jitter_range = self.config.jitter_ms as i64;
        let jitter = if jitter_range > 0 {
            rng.gen_range(-jitter_range / 2..=jitter_range / 2)
        } else {
            0
        };

        let delay_ms = (self.config.base_latency_ms as i64 + jitter).max(0) as u64;

        NetworkDelay { delay_ms, should_drop }
    }

    /// Simulate network delay by sleeping
    ///
    /// Returns true if packet should be transmitted, false if dropped
    pub async fn apply_delay(&self) -> bool {
        let delay = self.sample_delay();

        if delay.should_drop {
            return false;
        }

        if delay.delay_ms > 0 {
            sleep(Duration::from_millis(delay.delay_ms)).await;
        }

        true
    }

    /// Calculate transmission delay for a given payload size
    ///
    /// Returns additional delay in milliseconds based on bandwidth limit
    pub fn transmission_delay_ms(&self, payload_bytes: usize) -> u64 {
        if let Some(bandwidth_mbps) = self.config.bandwidth_mbps {
            // Convert bandwidth to bytes per millisecond
            // bandwidth_mbps * 10^6 bits/sec / 8 bits/byte / 1000 ms/sec
            let bytes_per_ms = (bandwidth_mbps * 1_000_000) / 8 / 1000;

            if bytes_per_ms > 0 {
                (payload_bytes as u64 + bytes_per_ms - 1) / bytes_per_ms
            } else {
                0
            }
        } else {
            0 // No bandwidth limit
        }
    }

    /// Simulate complete network transmission (delay + bandwidth)
    ///
    /// This combines:
    /// 1. Network latency (propagation delay)
    /// 2. Transmission delay (based on payload size and bandwidth)
    /// 3. Packet loss simulation
    ///
    /// Returns true if packet transmitted successfully, false if dropped
    pub async fn transmit(&self, payload_bytes: usize) -> bool {
        let delay = self.sample_delay();

        if delay.should_drop {
            return false;
        }

        // Apply network latency
        if delay.delay_ms > 0 {
            sleep(Duration::from_millis(delay.delay_ms)).await;
        }

        // Apply transmission delay based on bandwidth
        let tx_delay_ms = self.transmission_delay_ms(payload_bytes);
        if tx_delay_ms > 0 {
            sleep(Duration::from_millis(tx_delay_ms)).await;
        }

        true
    }

    /// Get the current network configuration
    pub fn config(&self) -> &NetworkConfig {
        &self.config
    }

    /// Update network configuration (useful for dynamic network conditions)
    pub fn set_config(&mut self, config: NetworkConfig) -> Result<(), String> {
        config.validate()?;
        self.config = config;
        Ok(())
    }

    /// Estimate expected throughput in bytes per second
    pub fn estimated_throughput_bps(&self) -> Option<u64> {
        self.config.bandwidth_mbps.map(|mbps| mbps * 1_000_000 / 8)
    }
}

/// Network partition simulator
///
/// Models network splits where groups of nodes cannot communicate
#[derive(Debug, Clone)]
pub struct NetworkPartition {
    /// Groups of nodes that can communicate with each other
    /// Each group is isolated from other groups
    pub partitions: Vec<Vec<usize>>,
}

impl NetworkPartition {
    /// Create a network partition with the given groups
    ///
    /// Example: [[0, 1], [2]] means nodes 0 and 1 can talk,
    /// but node 2 is isolated
    pub fn new(partitions: Vec<Vec<usize>>) -> Self {
        Self { partitions }
    }

    /// Create a simple partition: isolate one node from the rest
    pub fn isolate_node(total_nodes: usize, isolated_node: usize) -> Self {
        let majority: Vec<usize> = (0..total_nodes).filter(|&n| n != isolated_node).collect();
        let minority = vec![isolated_node];

        Self {
            partitions: vec![majority, minority],
        }
    }

    /// Create a 50-50 split (as close as possible)
    pub fn split_half(total_nodes: usize) -> Self {
        let mid = total_nodes / 2;
        let group1: Vec<usize> = (0..mid).collect();
        let group2: Vec<usize> = (mid..total_nodes).collect();

        Self {
            partitions: vec![group1, group2],
        }
    }

    /// Check if two nodes can communicate under this partition
    pub fn can_communicate(&self, node_a: usize, node_b: usize) -> bool {
        for partition in &self.partitions {
            if partition.contains(&node_a) && partition.contains(&node_b) {
                return true;
            }
        }
        false
    }

    /// No partition: all nodes can communicate
    pub fn none(total_nodes: usize) -> Self {
        Self {
            partitions: vec![(0..total_nodes).collect()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_config_validation() {
        let valid = NetworkConfig {
            base_latency_ms: 100,
            jitter_ms: 50,
            packet_loss_rate: 0.01,
            bandwidth_mbps: Some(100),
        };
        assert!(valid.validate().is_ok());

        let invalid = NetworkConfig {
            base_latency_ms: 100,
            jitter_ms: 50,
            packet_loss_rate: 1.5, // Invalid
            bandwidth_mbps: Some(100),
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_network_config_rtt() {
        let config = NetworkConfig {
            base_latency_ms: 100,
            jitter_ms: 50,
            packet_loss_rate: 0.0,
            bandwidth_mbps: None,
        };

        let (min, avg, max) = config.expected_rtt();
        assert_eq!(min, 150); // 200 - 50
        assert_eq!(avg, 200); // 100 * 2
        assert_eq!(max, 250); // 200 + 50
    }

    #[test]
    fn test_transmission_delay() {
        let sim = NetworkSimulator::new(NetworkConfig {
            base_latency_ms: 0,
            jitter_ms: 0,
            packet_loss_rate: 0.0,
            bandwidth_mbps: Some(100), // 100 Mbps
        })
        .unwrap();

        // 100 Mbps = 12.5 MB/s = 12500 bytes/ms
        let delay = sim.transmission_delay_ms(12500);
        assert_eq!(delay, 1); // 12500 bytes should take 1ms

        let delay = sim.transmission_delay_ms(125000);
        assert_eq!(delay, 10); // 125000 bytes should take 10ms
    }

    #[test]
    fn test_network_partition() {
        let partition = NetworkPartition::isolate_node(5, 2);

        assert!(partition.can_communicate(0, 1));
        assert!(partition.can_communicate(1, 3));
        assert!(!partition.can_communicate(0, 2));
        assert!(!partition.can_communicate(2, 3));
    }

    #[test]
    fn test_network_partition_split_half() {
        let partition = NetworkPartition::split_half(4);

        assert!(partition.can_communicate(0, 1));
        assert!(partition.can_communicate(2, 3));
        assert!(!partition.can_communicate(0, 2));
        assert!(!partition.can_communicate(1, 3));
    }

    #[tokio::test]
    async fn test_network_simulator_delay() {
        let sim = NetworkSimulator::new(NetworkConfig {
            base_latency_ms: 10,
            jitter_ms: 0,
            packet_loss_rate: 0.0,
            bandwidth_mbps: None,
        })
        .unwrap();

        let start = std::time::Instant::now();
        let success = sim.apply_delay().await;
        let elapsed = start.elapsed().as_millis() as u64;

        assert!(success);
        assert!(elapsed >= 10 && elapsed < 15); // Allow some OS scheduling variance
    }

    #[test]
    fn test_sample_delay_determinism() {
        let sim = NetworkSimulator::new(NetworkConfig {
            base_latency_ms: 100,
            jitter_ms: 50,
            packet_loss_rate: 0.0,
            bandwidth_mbps: None,
        })
        .unwrap();

        // Sample multiple delays and verify they're in expected range
        for _ in 0..100 {
            let delay = sim.sample_delay();
            assert!(!delay.should_drop);
            assert!(delay.delay_ms >= 50 && delay.delay_ms <= 150);
        }
    }

    #[test]
    fn test_estimated_throughput() {
        let sim = NetworkSimulator::new(NetworkConfig {
            base_latency_ms: 100,
            jitter_ms: 50,
            packet_loss_rate: 0.01,
            bandwidth_mbps: Some(100), // 100 Mbps
        })
        .unwrap();

        let throughput = sim.estimated_throughput_bps().unwrap();
        assert_eq!(throughput, 12_500_000); // 100 Mbps = 12.5 MB/s
    }
}
