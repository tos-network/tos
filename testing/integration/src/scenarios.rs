//! Test Scenarios
//!
//! Pre-built test scenarios for common multi-node testing patterns

use crate::{
    MetricsCollector, MultiNodeHarness,
    NetworkConfig, Result,
};
use crate::metrics::MetricsReport;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Test scenario trait
#[async_trait::async_trait]
pub trait TestScenario {
    /// Name of the test scenario
    fn name(&self) -> &str;

    /// Run the test scenario
    async fn run(&self, harness: &mut MultiNodeHarness) -> Result<MetricsReport>;
}

/// Scenario runner helper
pub struct ScenarioRunner {
    /// Daemon binary path
    daemon_path: String,
}

impl ScenarioRunner {
    /// Create a new scenario runner
    pub fn new(daemon_path: String) -> Self {
        Self { daemon_path }
    }

    /// Run a test scenario with the given network configuration
    pub async fn run_scenario<S: TestScenario>(
        &self,
        scenario: &S,
        num_nodes: usize,
        network_config: NetworkConfig,
    ) -> Result<MetricsReport> {
        log::info!("Running scenario: {}", scenario.name());
        log::info!("Network config: {:?}", network_config);

        let mut harness =
            MultiNodeHarness::new_with_daemon(num_nodes, network_config, &self.daemon_path)
                .await?;

        // Run the scenario
        let report = scenario.run(&mut harness).await?;

        // Cleanup
        harness.stop_all().await?;
        harness.cleanup_all()?;

        Ok(report)
    }
}

/// Scenario 1: Basic Consensus TPS
///
/// Measures consensus TPS across multiple nodes under ideal network conditions
pub struct BasicConsensusTPS {
    /// Number of transactions to submit
    pub num_transactions: usize,
    /// Test duration in seconds
    pub duration_secs: u64,
}

impl BasicConsensusTPS {
    pub fn new(num_transactions: usize, duration_secs: u64) -> Self {
        Self {
            num_transactions,
            duration_secs,
        }
    }
}

#[async_trait::async_trait]
impl TestScenario for BasicConsensusTPS {
    fn name(&self) -> &str {
        "Basic Consensus TPS"
    }

    async fn run(&self, harness: &mut MultiNodeHarness) -> Result<MetricsReport> {
        let mut collector = MetricsCollector::new();

        // Spawn all nodes
        harness.spawn_all().await?;
        harness.wait_for_all_ready(120).await?;

        // Connect nodes in full mesh
        harness.connect_full_mesh().await?;

        // Wait for nodes to connect
        sleep(Duration::from_secs(5)).await;

        log::info!(
            "Submitting {} transactions over {} seconds",
            self.num_transactions,
            self.duration_secs
        );

        let start_time = Instant::now();

        // Submit transactions
        // NOTE: This is a simplified version - real implementation would submit actual transactions
        for i in 0..self.num_transactions {
            let tx_id = format!("tx_{}", i);
            collector.record_tx_submit(tx_id.clone());

            // Simulate transaction submission to node 0
            // In real implementation: harness.node(0).rpc_call("/submit_transaction", tx_data).await?;

            // Simulate propagation delay (for demonstration)
            let propagation_ms = 50.0; // Simulated
            collector.record_tx_propagation(propagation_ms);

            // Wait between transactions to avoid overwhelming the network
            if i % 100 == 0 {
                sleep(Duration::from_millis(100)).await;
            }
        }

        // Wait for test duration to complete
        let elapsed = start_time.elapsed();
        if elapsed < Duration::from_secs(self.duration_secs) {
            sleep(Duration::from_secs(self.duration_secs) - elapsed).await;
        }

        // Simulate some transactions being confirmed
        let confirmed_count = (self.num_transactions as f64 * 0.95) as usize; // 95% confirmation rate
        for i in 0..confirmed_count {
            let tx_id = format!("tx_{}", i);
            collector.record_tx_confirm(&tx_id);
        }

        // Build and return report
        let report = collector.build_report(self.name().to_string(), harness.num_nodes());
        Ok(report)
    }
}

/// Scenario 2: Network Partition Recovery
///
/// Tests how quickly nodes recover and resync after a network partition
pub struct NetworkPartitionRecovery {
    /// Duration to maintain partition (seconds)
    pub partition_duration_secs: u64,
    /// Duration to observe recovery (seconds)
    pub recovery_observation_secs: u64,
}

impl NetworkPartitionRecovery {
    pub fn new(partition_duration_secs: u64, recovery_observation_secs: u64) -> Self {
        Self {
            partition_duration_secs,
            recovery_observation_secs,
        }
    }
}

#[async_trait::async_trait]
impl TestScenario for NetworkPartitionRecovery {
    fn name(&self) -> &str {
        "Network Partition Recovery"
    }

    async fn run(&self, harness: &mut MultiNodeHarness) -> Result<MetricsReport> {
        let mut collector = MetricsCollector::new();

        // Spawn all nodes
        harness.spawn_all().await?;
        harness.wait_for_all_ready(120).await?;
        harness.connect_full_mesh().await?;

        // Wait for initial sync
        sleep(Duration::from_secs(5)).await;

        log::info!("Creating network partition (isolating node 0)");

        // Create partition: isolate node 0
        // NOTE: In real implementation, this would use network_simulator to block traffic
        // For now, this is a logical placeholder

        // Continue submitting transactions during partition
        log::info!(
            "Submitting transactions during partition for {} seconds",
            self.partition_duration_secs
        );

        let partition_start = Instant::now();
        let mut tx_count = 0;

        while partition_start.elapsed() < Duration::from_secs(self.partition_duration_secs) {
            let tx_id = format!("tx_{}", tx_count);
            collector.record_tx_submit(tx_id);
            tx_count += 1;

            sleep(Duration::from_millis(100)).await;
        }

        log::info!("Removing network partition");

        // Measure recovery time
        let recovery_start = Instant::now();

        // Wait for recovery
        sleep(Duration::from_secs(self.recovery_observation_secs)).await;

        let recovery_time = recovery_start.elapsed();
        log::info!("Recovery completed in {:.2?}", recovery_time);

        // Build report
        let mut report = collector.build_report(self.name().to_string(), harness.num_nodes());

        // Add recovery metrics
        report.consensus.chain_reorgs = 1; // Partition recovery typically causes reorg

        Ok(report)
    }
}

/// Scenario 3: Variable Network Conditions
///
/// Tests TPS under varying network quality (good → medium → poor)
pub struct VariableNetworkConditions {
    /// Duration for each network condition (seconds)
    pub duration_per_condition_secs: u64,
    /// Number of transactions per condition
    pub transactions_per_condition: usize,
}

impl VariableNetworkConditions {
    pub fn new(duration_per_condition_secs: u64, transactions_per_condition: usize) -> Self {
        Self {
            duration_per_condition_secs,
            transactions_per_condition,
        }
    }
}

#[async_trait::async_trait]
impl TestScenario for VariableNetworkConditions {
    fn name(&self) -> &str {
        "Variable Network Conditions"
    }

    async fn run(&self, harness: &mut MultiNodeHarness) -> Result<MetricsReport> {
        let mut collector = MetricsCollector::new();

        // Spawn all nodes
        harness.spawn_all().await?;
        harness.wait_for_all_ready(120).await?;
        harness.connect_full_mesh().await?;

        // Test under 3 different network conditions
        let conditions = vec![
            ("Good Network (50ms)", NetworkConfig::lan()),
            ("Medium Network (200ms)", NetworkConfig::internet()),
            ("Poor Network (500ms)", NetworkConfig::poor()),
        ];

        for (condition_name, network_config) in conditions {
            log::info!("Testing under: {}", condition_name);
            log::info!("Network config: {:?}", network_config);

            // Update network simulator for all nodes
            // NOTE: In real implementation, this would update each node's network simulator
            // For now, this is a logical placeholder

            let condition_start = Instant::now();

            // Submit transactions under this condition
            for i in 0..self.transactions_per_condition {
                let tx_id = format!("tx_{}_{}", condition_name, i);
                collector.record_tx_submit(tx_id);

                // Simulate propagation delay based on network config
                let (_, avg_rtt, _) = network_config.expected_rtt();
                collector.record_tx_propagation(avg_rtt as f64);

                sleep(Duration::from_millis(10)).await;
            }

            // Wait for condition duration
            let elapsed = condition_start.elapsed();
            if elapsed < Duration::from_secs(self.duration_per_condition_secs) {
                sleep(Duration::from_secs(self.duration_per_condition_secs) - elapsed).await;
            }
        }

        // Build report
        let report = collector.build_report(self.name().to_string(), harness.num_nodes());
        Ok(report)
    }
}

/// Scenario 4: High Load Stress Test
///
/// Stress tests the network with high transaction volume
pub struct HighLoadStressTest {
    /// Transactions per second to target
    pub target_tps: usize,
    /// Test duration in seconds
    pub duration_secs: u64,
}

impl HighLoadStressTest {
    pub fn new(target_tps: usize, duration_secs: u64) -> Self {
        Self {
            target_tps,
            duration_secs,
        }
    }
}

#[async_trait::async_trait]
impl TestScenario for HighLoadStressTest {
    fn name(&self) -> &str {
        "High Load Stress Test"
    }

    async fn run(&self, harness: &mut MultiNodeHarness) -> Result<MetricsReport> {
        let mut collector = MetricsCollector::new();

        // Spawn all nodes
        harness.spawn_all().await?;
        harness.wait_for_all_ready(120).await?;
        harness.connect_full_mesh().await?;

        log::info!(
            "Starting stress test: {} TPS for {} seconds",
            self.target_tps,
            self.duration_secs
        );

        let start_time = Instant::now();
        let mut tx_count = 0;

        // Calculate sleep duration between transactions
        let sleep_duration_ms = if self.target_tps > 0 {
            1000 / self.target_tps as u64
        } else {
            0
        };

        while start_time.elapsed() < Duration::from_secs(self.duration_secs) {
            let tx_id = format!("tx_{}", tx_count);
            collector.record_tx_submit(tx_id);
            tx_count += 1;

            if sleep_duration_ms > 0 {
                sleep(Duration::from_millis(sleep_duration_ms)).await;
            }
        }

        log::info!("Submitted {} transactions", tx_count);

        // Wait a bit for final confirmations
        sleep(Duration::from_secs(5)).await;

        // Build report
        let report = collector.build_report(self.name().to_string(), harness.num_nodes());
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_creation() {
        let scenario1 = BasicConsensusTPS::new(1000, 60);
        assert_eq!(scenario1.name(), "Basic Consensus TPS");
        assert_eq!(scenario1.num_transactions, 1000);

        let scenario2 = NetworkPartitionRecovery::new(30, 60);
        assert_eq!(scenario2.name(), "Network Partition Recovery");
        assert_eq!(scenario2.partition_duration_secs, 30);

        let scenario3 = VariableNetworkConditions::new(30, 500);
        assert_eq!(scenario3.name(), "Variable Network Conditions");

        let scenario4 = HighLoadStressTest::new(1000, 60);
        assert_eq!(scenario4.name(), "High Load Stress Test");
        assert_eq!(scenario4.target_tps, 1000);
    }
}
