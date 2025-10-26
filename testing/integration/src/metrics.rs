//! Metrics Collection and Reporting
//!
//! Tracks key performance metrics for multi-node testing:
//! - Transaction propagation latency
//! - Block propagation time
//! - Consensus TPS (transactions confirmed by all nodes)
//! - Network overhead (bandwidth, retransmissions)
//! - Node synchronization time

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Network performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    /// Average transaction propagation latency (milliseconds)
    /// Time for a transaction to reach all nodes
    pub avg_tx_propagation_ms: f64,

    /// P50, P90, P99 percentiles for transaction propagation
    pub tx_propagation_p50_ms: f64,
    pub tx_propagation_p90_ms: f64,
    pub tx_propagation_p99_ms: f64,

    /// Average block propagation time (milliseconds)
    /// Time for a block to reach all nodes
    pub avg_block_propagation_ms: f64,

    /// P50, P90, P99 percentiles for block propagation
    pub block_propagation_p50_ms: f64,
    pub block_propagation_p90_ms: f64,
    pub block_propagation_p99_ms: f64,

    /// Total bytes sent across all nodes
    pub total_bytes_sent: u64,

    /// Total bytes received across all nodes
    pub total_bytes_received: u64,

    /// Estimated network overhead (%)
    /// Ratio of actual bandwidth used vs minimum required
    pub network_overhead_percent: f64,

    /// Number of packet retransmissions
    pub packet_retransmissions: u64,

    /// Number of packets lost
    pub packets_lost: u64,

    /// Average round-trip time (milliseconds)
    pub avg_rtt_ms: f64,
}

impl NetworkMetrics {
    /// Create empty network metrics
    pub fn new() -> Self {
        Self {
            avg_tx_propagation_ms: 0.0,
            tx_propagation_p50_ms: 0.0,
            tx_propagation_p90_ms: 0.0,
            tx_propagation_p99_ms: 0.0,
            avg_block_propagation_ms: 0.0,
            block_propagation_p50_ms: 0.0,
            block_propagation_p90_ms: 0.0,
            block_propagation_p99_ms: 0.0,
            total_bytes_sent: 0,
            total_bytes_received: 0,
            network_overhead_percent: 0.0,
            packet_retransmissions: 0,
            packets_lost: 0,
            avg_rtt_ms: 0.0,
        }
    }

    /// Calculate metrics from raw propagation times
    pub fn from_propagation_times(
        tx_times_ms: Vec<f64>,
        block_times_ms: Vec<f64>,
    ) -> Self {
        let mut metrics = Self::new();

        if !tx_times_ms.is_empty() {
            let mut sorted_tx = tx_times_ms.clone();
            sorted_tx.sort_by(|a, b| a.partial_cmp(b).unwrap());

            metrics.avg_tx_propagation_ms = sorted_tx.iter().sum::<f64>() / sorted_tx.len() as f64;
            metrics.tx_propagation_p50_ms = percentile(&sorted_tx, 0.50);
            metrics.tx_propagation_p90_ms = percentile(&sorted_tx, 0.90);
            metrics.tx_propagation_p99_ms = percentile(&sorted_tx, 0.99);
        }

        if !block_times_ms.is_empty() {
            let mut sorted_block = block_times_ms.clone();
            sorted_block.sort_by(|a, b| a.partial_cmp(b).unwrap());

            metrics.avg_block_propagation_ms = sorted_block.iter().sum::<f64>() / sorted_block.len() as f64;
            metrics.block_propagation_p50_ms = percentile(&sorted_block, 0.50);
            metrics.block_propagation_p90_ms = percentile(&sorted_block, 0.90);
            metrics.block_propagation_p99_ms = percentile(&sorted_block, 0.99);
        }

        metrics
    }
}

impl Default for NetworkMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Consensus performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusMetrics {
    /// Consensus TPS (transactions confirmed by all nodes)
    pub consensus_tps: f64,

    /// Number of transactions submitted
    pub transactions_submitted: u64,

    /// Number of transactions confirmed by all nodes
    pub transactions_confirmed: u64,

    /// Average confirmation time (milliseconds)
    /// Time from submission to confirmation by all nodes
    pub avg_confirmation_time_ms: f64,

    /// Number of blocks produced
    pub blocks_produced: u64,

    /// Average block time (seconds)
    pub avg_block_time_sec: f64,

    /// Number of orphaned blocks
    pub orphaned_blocks: u64,

    /// Orphan rate (%)
    pub orphan_rate_percent: f64,

    /// Number of chain reorganizations
    pub chain_reorgs: u64,

    /// Average blue score across all nodes
    pub avg_blue_score: u64,

    /// Blue score deviation (standard deviation)
    /// Low deviation = good consensus
    pub blue_score_stddev: f64,
}

impl ConsensusMetrics {
    /// Create empty consensus metrics
    pub fn new() -> Self {
        Self {
            consensus_tps: 0.0,
            transactions_submitted: 0,
            transactions_confirmed: 0,
            avg_confirmation_time_ms: 0.0,
            blocks_produced: 0,
            avg_block_time_sec: 0.0,
            orphaned_blocks: 0,
            orphan_rate_percent: 0.0,
            chain_reorgs: 0,
            avg_blue_score: 0,
            blue_score_stddev: 0.0,
        }
    }

    /// Calculate consensus TPS from duration and transaction count
    pub fn calculate_tps(&mut self, duration_secs: f64) {
        if duration_secs > 0.0 {
            self.consensus_tps = self.transactions_confirmed as f64 / duration_secs;
        }
    }

    /// Calculate orphan rate
    pub fn calculate_orphan_rate(&mut self) {
        if self.blocks_produced > 0 {
            self.orphan_rate_percent =
                (self.orphaned_blocks as f64 / self.blocks_produced as f64) * 100.0;
        }
    }
}

impl Default for ConsensusMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined metrics report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsReport {
    /// Test name/description
    pub test_name: String,

    /// Test duration (seconds)
    pub duration_secs: f64,

    /// Number of nodes in the test
    pub num_nodes: usize,

    /// Network metrics
    pub network: NetworkMetrics,

    /// Consensus metrics
    pub consensus: ConsensusMetrics,

    /// Timestamp when test started
    pub start_time: String,

    /// Timestamp when test ended
    pub end_time: String,
}

impl MetricsReport {
    /// Create a new metrics report
    pub fn new(test_name: String, num_nodes: usize) -> Self {
        Self {
            test_name,
            duration_secs: 0.0,
            num_nodes,
            network: NetworkMetrics::new(),
            consensus: ConsensusMetrics::new(),
            start_time: chrono::Utc::now().to_rfc3339(),
            end_time: String::new(),
        }
    }

    /// Mark test as completed
    pub fn complete(&mut self, duration: Duration) {
        self.duration_secs = duration.as_secs_f64();
        self.end_time = chrono::Utc::now().to_rfc3339();
    }

    /// Print a human-readable summary
    pub fn print_summary(&self) {
        println!("\n{}", "=".repeat(80));
        println!("Test Report: {}", self.test_name);
        println!("{}", "=".repeat(80));
        println!("\nTest Configuration:");
        println!("  Nodes:         {}", self.num_nodes);
        println!("  Duration:      {:.2} seconds", self.duration_secs);
        println!("  Start time:    {}", self.start_time);
        println!("  End time:      {}", self.end_time);

        println!("\nConsensus Metrics:");
        println!("  Consensus TPS:           {:.2}", self.consensus.consensus_tps);
        println!("  Transactions submitted:  {}", self.consensus.transactions_submitted);
        println!("  Transactions confirmed:  {}", self.consensus.transactions_confirmed);
        println!(
            "  Avg confirmation time:   {:.2} ms",
            self.consensus.avg_confirmation_time_ms
        );
        println!("  Blocks produced:         {}", self.consensus.blocks_produced);
        println!("  Avg block time:          {:.2} sec", self.consensus.avg_block_time_sec);
        println!("  Orphaned blocks:         {}", self.consensus.orphaned_blocks);
        println!("  Orphan rate:             {:.2}%", self.consensus.orphan_rate_percent);
        println!("  Chain reorgs:            {}", self.consensus.chain_reorgs);
        println!("  Avg blue score:          {}", self.consensus.avg_blue_score);
        println!("  Blue score stddev:       {:.2}", self.consensus.blue_score_stddev);

        println!("\nNetwork Metrics:");
        println!(
            "  Avg TX propagation:      {:.2} ms",
            self.network.avg_tx_propagation_ms
        );
        println!(
            "  TX propagation P50/P90/P99: {:.2} / {:.2} / {:.2} ms",
            self.network.tx_propagation_p50_ms,
            self.network.tx_propagation_p90_ms,
            self.network.tx_propagation_p99_ms
        );
        println!(
            "  Avg block propagation:   {:.2} ms",
            self.network.avg_block_propagation_ms
        );
        println!(
            "  Block propagation P50/P90/P99: {:.2} / {:.2} / {:.2} ms",
            self.network.block_propagation_p50_ms,
            self.network.block_propagation_p90_ms,
            self.network.block_propagation_p99_ms
        );
        println!("  Total bytes sent:        {} bytes", self.network.total_bytes_sent);
        println!(
            "  Total bytes received:    {} bytes",
            self.network.total_bytes_received
        );
        println!(
            "  Network overhead:        {:.2}%",
            self.network.network_overhead_percent
        );
        println!("  Packets lost:            {}", self.network.packets_lost);
        println!("  Packet retransmissions:  {}", self.network.packet_retransmissions);
        println!("  Avg RTT:                 {:.2} ms", self.network.avg_rtt_ms);

        println!("\n{}", "=".repeat(80));
    }

    /// Export to JSON file
    pub fn export_json(&self, path: &str) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        println!("\nMetrics exported to: {}", path);
        Ok(())
    }

    /// Export to CSV file
    pub fn export_csv(&self, path: &str) -> Result<(), std::io::Error> {
        let csv = format!(
            "test_name,num_nodes,duration_secs,consensus_tps,tx_submitted,tx_confirmed,avg_confirmation_ms,blocks_produced,avg_block_time_sec,orphan_rate_percent,avg_tx_propagation_ms,avg_block_propagation_ms\n\
             {},{},{},{},{},{},{},{},{},{},{},{}\n",
            self.test_name,
            self.num_nodes,
            self.duration_secs,
            self.consensus.consensus_tps,
            self.consensus.transactions_submitted,
            self.consensus.transactions_confirmed,
            self.consensus.avg_confirmation_time_ms,
            self.consensus.blocks_produced,
            self.consensus.avg_block_time_sec,
            self.consensus.orphan_rate_percent,
            self.network.avg_tx_propagation_ms,
            self.network.avg_block_propagation_ms
        );

        std::fs::write(path, csv)?;
        println!("Metrics exported to: {}", path);
        Ok(())
    }
}

/// Metrics collector that tracks events during testing
pub struct MetricsCollector {
    /// Test start time
    start_time: Instant,

    /// Transaction propagation times (milliseconds)
    tx_propagation_times: Vec<f64>,

    /// Block propagation times (milliseconds)
    block_propagation_times: Vec<f64>,

    /// Transaction confirmation times (tx_id -> (submit_time, confirm_time))
    tx_confirmations: HashMap<String, (Instant, Option<Instant>)>,

    /// Block production times (block_hash -> timestamp)
    block_times: Vec<Instant>,

    /// Packets lost counter
    packets_lost: u64,

    /// Bytes sent/received counters
    bytes_sent: u64,
    bytes_received: u64,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            tx_propagation_times: Vec::new(),
            block_propagation_times: Vec::new(),
            tx_confirmations: HashMap::new(),
            block_times: Vec::new(),
            packets_lost: 0,
            bytes_sent: 0,
            bytes_received: 0,
        }
    }

    /// Record transaction submission
    pub fn record_tx_submit(&mut self, tx_id: String) {
        self.tx_confirmations.insert(tx_id, (Instant::now(), None));
    }

    /// Record transaction confirmation
    pub fn record_tx_confirm(&mut self, tx_id: &str) {
        if let Some((_submit_time, confirm_slot)) = self.tx_confirmations.get_mut(tx_id) {
            *confirm_slot = Some(Instant::now());
        }
    }

    /// Record transaction propagation time
    pub fn record_tx_propagation(&mut self, propagation_ms: f64) {
        self.tx_propagation_times.push(propagation_ms);
    }

    /// Record block propagation time
    pub fn record_block_propagation(&mut self, propagation_ms: f64) {
        self.block_propagation_times.push(propagation_ms);
    }

    /// Record block production
    pub fn record_block_produced(&mut self) {
        self.block_times.push(Instant::now());
    }

    /// Record packet loss
    pub fn record_packet_loss(&mut self) {
        self.packets_lost += 1;
    }

    /// Record bytes sent
    pub fn record_bytes_sent(&mut self, bytes: u64) {
        self.bytes_sent += bytes;
    }

    /// Record bytes received
    pub fn record_bytes_received(&mut self, bytes: u64) {
        self.bytes_received += bytes;
    }

    /// Build final metrics report
    pub fn build_report(&self, test_name: String, num_nodes: usize) -> MetricsReport {
        let mut report = MetricsReport::new(test_name, num_nodes);
        let duration = self.start_time.elapsed();

        // Calculate network metrics
        report.network = NetworkMetrics::from_propagation_times(
            self.tx_propagation_times.clone(),
            self.block_propagation_times.clone(),
        );
        report.network.packets_lost = self.packets_lost;
        report.network.total_bytes_sent = self.bytes_sent;
        report.network.total_bytes_received = self.bytes_received;

        // Calculate consensus metrics
        let confirmed_count = self
            .tx_confirmations
            .values()
            .filter(|(_, confirm)| confirm.is_some())
            .count();

        report.consensus.transactions_submitted = self.tx_confirmations.len() as u64;
        report.consensus.transactions_confirmed = confirmed_count as u64;

        // Calculate average confirmation time
        let confirmation_times: Vec<f64> = self
            .tx_confirmations
            .values()
            .filter_map(|(submit, confirm)| {
                confirm.map(|c| c.duration_since(*submit).as_millis() as f64)
            })
            .collect();

        if !confirmation_times.is_empty() {
            report.consensus.avg_confirmation_time_ms =
                confirmation_times.iter().sum::<f64>() / confirmation_times.len() as f64;
        }

        report.consensus.blocks_produced = self.block_times.len() as u64;

        // Calculate average block time
        if self.block_times.len() > 1 {
            let mut block_intervals: Vec<f64> = Vec::new();
            for i in 1..self.block_times.len() {
                let interval = self.block_times[i]
                    .duration_since(self.block_times[i - 1])
                    .as_secs_f64();
                block_intervals.push(interval);
            }
            if !block_intervals.is_empty() {
                report.consensus.avg_block_time_sec =
                    block_intervals.iter().sum::<f64>() / block_intervals.len() as f64;
            }
        }

        // Calculate consensus TPS
        report.consensus.calculate_tps(duration.as_secs_f64());
        report.complete(duration);

        report
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate percentile from sorted data
fn percentile(sorted_data: &[f64], p: f64) -> f64 {
    if sorted_data.is_empty() {
        return 0.0;
    }

    let index = (p * (sorted_data.len() - 1) as f64).round() as usize;
    sorted_data[index.min(sorted_data.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percentile() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];

        // Percentile uses round() for index: p50 → index = round(0.5 * 9) = round(4.5) = 5 → data[5] = 6.0
        assert_eq!(percentile(&data, 0.50), 6.0);
        assert_eq!(percentile(&data, 0.90), 9.0);  // round(0.9 * 9) = round(8.1) = 8 → data[8] = 9.0
        assert_eq!(percentile(&data, 0.99), 10.0); // round(0.99 * 9) = round(8.91) = 9 → data[9] = 10.0
    }

    #[test]
    fn test_network_metrics_from_propagation() {
        let tx_times = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let block_times = vec![100.0, 200.0, 300.0];

        let metrics = NetworkMetrics::from_propagation_times(tx_times, block_times);

        assert_eq!(metrics.avg_tx_propagation_ms, 30.0);
        assert_eq!(metrics.tx_propagation_p50_ms, 30.0);
        assert_eq!(metrics.avg_block_propagation_ms, 200.0);
    }

    #[test]
    fn test_consensus_metrics_tps_calculation() {
        let mut metrics = ConsensusMetrics::new();
        metrics.transactions_confirmed = 1000;

        metrics.calculate_tps(10.0);
        assert_eq!(metrics.consensus_tps, 100.0);

        metrics.calculate_tps(5.0);
        assert_eq!(metrics.consensus_tps, 200.0);
    }

    #[test]
    fn test_metrics_collector() {
        let mut collector = MetricsCollector::new();

        collector.record_tx_submit("tx1".to_string());
        collector.record_tx_submit("tx2".to_string());
        collector.record_tx_confirm("tx1");

        collector.record_tx_propagation(50.0);
        collector.record_block_propagation(100.0);

        let report = collector.build_report("test".to_string(), 3);

        assert_eq!(report.consensus.transactions_submitted, 2);
        assert_eq!(report.consensus.transactions_confirmed, 1);
        assert_eq!(report.network.avg_tx_propagation_ms, 50.0);
        assert_eq!(report.network.avg_block_propagation_ms, 100.0);
    }
}
