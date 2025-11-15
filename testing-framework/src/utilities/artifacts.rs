// File: testing-framework/src/utilities/artifacts.rs
//
// Failure Artifact Collection System
//
// This module provides utilities for collecting test failure artifacts,
// enabling reproduction and debugging of failed tests.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Network topology snapshot for multi-node tests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologySnapshot {
    /// Number of nodes in the network
    pub node_count: usize,
    /// Adjacency list (node_id → connected peer IDs)
    pub connections: HashMap<usize, Vec<usize>>,
    /// Active partitions (if any)
    pub partitions: Vec<Partition>,
}

/// Network partition state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Partition {
    /// Nodes in group A
    pub group_a: Vec<usize>,
    /// Nodes in group B
    pub group_b: Vec<usize>,
}

/// Blockchain state snapshot for a single node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainStateSnapshot {
    /// Node ID
    pub node_id: usize,
    /// Current tip height
    pub tip_height: u64,
    /// Tip hash
    pub tip_hash: String,
    /// Account balances (address → balance)
    pub balances: HashMap<String, u64>,
    /// Account nonces (address → nonce)
    pub nonces: HashMap<String, u64>,
    /// Total supply
    pub total_supply: u128,
    /// Fees burned
    pub fees_burned: u64,
}

/// Transaction history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    /// Transaction hash
    pub hash: String,
    /// Sender address
    pub sender: String,
    /// Recipient address
    pub recipient: String,
    /// Amount transferred
    pub amount: u64,
    /// Transaction fee
    pub fee: u64,
    /// Nonce
    pub nonce: u64,
    /// Block height where confirmed (if any)
    pub block_height: Option<u64>,
}

/// Complete test failure artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestArtifact {
    /// Test metadata
    pub metadata: TestMetadata,
    /// Network topology snapshot (if multi-node test)
    pub topology: Option<TopologySnapshot>,
    /// Blockchain state for each node
    pub blockchain_states: Vec<BlockchainStateSnapshot>,
    /// Transaction history
    pub transactions: Vec<TransactionRecord>,
    /// Captured logs
    pub logs: Vec<LogEntry>,
}

/// Test metadata for reproduction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestMetadata {
    /// Test name
    pub test_name: String,
    /// RNG seed used (if available)
    pub rng_seed: Option<u64>,
    /// Timestamp when test failed
    pub timestamp: String,
    /// Test duration (milliseconds)
    pub duration_ms: u64,
    /// Failure reason (if available)
    pub failure_reason: Option<String>,
}

/// Log entry captured during test execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Log level (ERROR, WARN, INFO, DEBUG, TRACE)
    pub level: String,
    /// Log message
    pub message: String,
    /// Timestamp
    pub timestamp: String,
}

/// Artifact collector for capturing test failure state
///
/// # Examples
///
/// ```rust,ignore
/// use tos_testing_framework::utilities::artifacts::ArtifactCollector;
///
/// #[tokio::test]
/// async fn test_with_artifacts() -> Result<()> {
///     let mut collector = ArtifactCollector::new("test_multi_node_consensus");
///     collector.set_rng_seed(0x1234567890abcdef);
///
///     // Run test...
///     let network = setup_network().await?;
///
///     // On failure, capture artifacts
///     if let Err(e) = run_test(&network).await {
///         collector.capture_blockchain_state(0, &network.node(0).daemon()).await?;
///         collector.capture_blockchain_state(1, &network.node(1).daemon()).await?;
///         collector.set_failure_reason(format!("{:?}", e));
///         collector.save("./artifacts/").await?;
///     }
///
///     Ok(())
/// }
/// ```
pub struct ArtifactCollector {
    metadata: TestMetadata,
    topology: Option<TopologySnapshot>,
    blockchain_states: Vec<BlockchainStateSnapshot>,
    transactions: Vec<TransactionRecord>,
    logs: Vec<LogEntry>,
    start_time: std::time::Instant,
}

impl ArtifactCollector {
    /// Create a new artifact collector for a test
    ///
    /// # Arguments
    ///
    /// * `test_name` - Name of the test being run
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_testing_framework::utilities::artifacts::ArtifactCollector;
    ///
    /// let collector = ArtifactCollector::new("test_consensus_convergence");
    /// ```
    pub fn new(test_name: impl Into<String>) -> Self {
        Self {
            metadata: TestMetadata {
                test_name: test_name.into(),
                rng_seed: None,
                timestamp: chrono::Utc::now().to_rfc3339(),
                duration_ms: 0,
                failure_reason: None,
            },
            topology: None,
            blockchain_states: Vec::new(),
            transactions: Vec::new(),
            logs: Vec::new(),
            start_time: std::time::Instant::now(),
        }
    }

    /// Set the RNG seed used in the test
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_testing_framework::utilities::artifacts::ArtifactCollector;
    /// use tos_testing_framework::orchestrator::rng::TestRng;
    ///
    /// let rng = TestRng::new_from_env_or_random();
    /// let mut collector = ArtifactCollector::new("test_name");
    /// collector.set_rng_seed(rng.seed());
    /// ```
    pub fn set_rng_seed(&mut self, seed: u64) {
        self.metadata.rng_seed = Some(seed);
    }

    /// Set the failure reason
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_testing_framework::utilities::artifacts::ArtifactCollector;
    ///
    /// let mut collector = ArtifactCollector::new("test_name");
    /// collector.set_failure_reason("Height mismatch: expected 5, got 3".to_string());
    /// ```
    pub fn set_failure_reason(&mut self, reason: String) {
        self.metadata.failure_reason = Some(reason);
    }

    /// Capture network topology snapshot
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_testing_framework::utilities::artifacts::{ArtifactCollector, TopologySnapshot};
    /// use std::collections::HashMap;
    ///
    /// let mut collector = ArtifactCollector::new("test_name");
    ///
    /// let topology = TopologySnapshot {
    ///     node_count: 3,
    ///     connections: HashMap::from([
    ///         (0, vec![1, 2]),
    ///         (1, vec![0, 2]),
    ///         (2, vec![0, 1]),
    ///     ]),
    ///     partitions: vec![],
    /// };
    ///
    /// collector.save_topology(topology);
    /// ```
    pub fn save_topology(&mut self, topology: TopologySnapshot) {
        self.topology = Some(topology);
    }

    /// Add blockchain state snapshot for a node
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_testing_framework::utilities::artifacts::{ArtifactCollector, BlockchainStateSnapshot};
    /// use std::collections::HashMap;
    ///
    /// let mut collector = ArtifactCollector::new("test_name");
    ///
    /// let state = BlockchainStateSnapshot {
    ///     node_id: 0,
    ///     tip_height: 5,
    ///     tip_hash: "0xabcd...".to_string(),
    ///     balances: HashMap::new(),
    ///     nonces: HashMap::new(),
    ///     total_supply: 1000000,
    ///     fees_burned: 100,
    /// };
    ///
    /// collector.add_blockchain_state(state);
    /// ```
    pub fn add_blockchain_state(&mut self, state: BlockchainStateSnapshot) {
        self.blockchain_states.push(state);
    }

    /// Add transaction record to history
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_testing_framework::utilities::artifacts::{ArtifactCollector, TransactionRecord};
    ///
    /// let mut collector = ArtifactCollector::new("test_name");
    ///
    /// let tx = TransactionRecord {
    ///     hash: "0x1234...".to_string(),
    ///     sender: "0xaaa...".to_string(),
    ///     recipient: "0xbbb...".to_string(),
    ///     amount: 1000,
    ///     fee: 10,
    ///     nonce: 1,
    ///     block_height: Some(5),
    /// };
    ///
    /// collector.add_transaction(tx);
    /// ```
    pub fn add_transaction(&mut self, tx: TransactionRecord) {
        self.transactions.push(tx);
    }

    /// Capture a log entry
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_testing_framework::utilities::artifacts::ArtifactCollector;
    ///
    /// let mut collector = ArtifactCollector::new("test_name");
    /// collector.capture_log("ERROR", "Block validation failed");
    /// collector.capture_log("INFO", "Mining block at height 5");
    /// ```
    pub fn capture_log(&mut self, level: impl Into<String>, message: impl Into<String>) {
        self.logs.push(LogEntry {
            level: level.into(),
            message: message.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }

    /// Save artifact to disk
    ///
    /// Creates a JSON file with all collected data. The filename includes
    /// the test name and timestamp for uniqueness.
    ///
    /// # Arguments
    ///
    /// * `output_dir` - Directory where artifact will be saved
    ///
    /// # Returns
    ///
    /// Path to the saved artifact file
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use tos_testing_framework::utilities::artifacts::ArtifactCollector;
    ///
    /// #[tokio::test]
    /// async fn test_example() -> Result<()> {
    ///     let mut collector = ArtifactCollector::new("test_example");
    ///     // ... collect data ...
    ///     let path = collector.save("./artifacts/").await?;
    ///     println!("Artifact saved to: {}", path.display());
    ///     Ok(())
    /// }
    /// ```
    pub async fn save(&mut self, output_dir: impl AsRef<Path>) -> Result<PathBuf> {
        // Update duration
        self.metadata.duration_ms = self.start_time.elapsed().as_millis() as u64;

        // Create artifact
        let artifact = TestArtifact {
            metadata: self.metadata.clone(),
            topology: self.topology.clone(),
            blockchain_states: self.blockchain_states.clone(),
            transactions: self.transactions.clone(),
            logs: self.logs.clone(),
        };

        // Create output directory
        let output_dir = output_dir.as_ref();
        fs::create_dir_all(output_dir)
            .await
            .context("Failed to create artifact directory")?;

        // Generate filename
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!("{}_{}.json", self.metadata.test_name, timestamp);
        let filepath = output_dir.join(filename);

        // Serialize to pretty JSON
        let json =
            serde_json::to_string_pretty(&artifact).context("Failed to serialize artifact")?;

        // Write to file
        let mut file = fs::File::create(&filepath)
            .await
            .context("Failed to create artifact file")?;
        file.write_all(json.as_bytes())
            .await
            .context("Failed to write artifact data")?;
        file.flush()
            .await
            .context("Failed to flush artifact file")?;

        Ok(filepath)
    }

    /// Load artifact from disk
    ///
    /// # Arguments
    ///
    /// * `filepath` - Path to the artifact JSON file
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use tos_testing_framework::utilities::artifacts::ArtifactCollector;
    ///
    /// #[tokio::test]
    /// async fn test_replay() -> Result<()> {
    ///     let artifact = ArtifactCollector::load("./artifacts/test_example_20251115.json").await?;
    ///     println!("RNG seed: {:?}", artifact.metadata.rng_seed);
    ///     Ok(())
    /// }
    /// ```
    pub async fn load(filepath: impl AsRef<Path>) -> Result<TestArtifact> {
        let filepath = filepath.as_ref();
        let content = fs::read_to_string(filepath)
            .await
            .context("Failed to read artifact file")?;

        let artifact: TestArtifact =
            serde_json::from_str(&content).context("Failed to parse artifact JSON")?;

        Ok(artifact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_artifact_collector_creation() {
        let collector = ArtifactCollector::new("test_example");
        assert_eq!(collector.metadata.test_name, "test_example");
        assert!(collector.metadata.rng_seed.is_none());
        assert!(collector.metadata.failure_reason.is_none());
    }

    #[tokio::test]
    async fn test_set_rng_seed() {
        let mut collector = ArtifactCollector::new("test_example");
        collector.set_rng_seed(0x1234567890abcdef);
        assert_eq!(collector.metadata.rng_seed, Some(0x1234567890abcdef));
    }

    #[tokio::test]
    async fn test_set_failure_reason() {
        let mut collector = ArtifactCollector::new("test_example");
        collector.set_failure_reason("Test failed!".to_string());
        assert_eq!(
            collector.metadata.failure_reason,
            Some("Test failed!".to_string())
        );
    }

    #[tokio::test]
    async fn test_save_topology() {
        let mut collector = ArtifactCollector::new("test_example");

        let topology = TopologySnapshot {
            node_count: 2,
            connections: HashMap::from([(0, vec![1]), (1, vec![0])]),
            partitions: vec![],
        };

        collector.save_topology(topology.clone());
        assert_eq!(collector.topology.unwrap().node_count, 2);
    }

    #[tokio::test]
    async fn test_add_blockchain_state() {
        let mut collector = ArtifactCollector::new("test_example");

        let state = BlockchainStateSnapshot {
            node_id: 0,
            tip_height: 5,
            tip_hash: "0xabcd".to_string(),
            balances: HashMap::new(),
            nonces: HashMap::new(),
            total_supply: 1000000,
            fees_burned: 100,
        };

        collector.add_blockchain_state(state);
        assert_eq!(collector.blockchain_states.len(), 1);
        assert_eq!(collector.blockchain_states[0].tip_height, 5);
    }

    #[tokio::test]
    async fn test_add_transaction() {
        let mut collector = ArtifactCollector::new("test_example");

        let tx = TransactionRecord {
            hash: "0x1234".to_string(),
            sender: "0xaaa".to_string(),
            recipient: "0xbbb".to_string(),
            amount: 1000,
            fee: 10,
            nonce: 1,
            block_height: Some(5),
        };

        collector.add_transaction(tx);
        assert_eq!(collector.transactions.len(), 1);
        assert_eq!(collector.transactions[0].amount, 1000);
    }

    #[tokio::test]
    async fn test_capture_log() {
        let mut collector = ArtifactCollector::new("test_example");
        collector.capture_log("ERROR", "Test error message");
        collector.capture_log("INFO", "Test info message");

        assert_eq!(collector.logs.len(), 2);
        assert_eq!(collector.logs[0].level, "ERROR");
        assert_eq!(collector.logs[1].level, "INFO");
    }

    #[tokio::test]
    async fn test_save_and_load_artifact() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let mut collector = ArtifactCollector::new("test_save_load");
        collector.set_rng_seed(0xdeadbeef);
        collector.set_failure_reason("Test failure".to_string());
        collector.capture_log("ERROR", "Test log");

        // Save artifact
        let filepath = collector.save(temp_dir.path()).await?;
        assert!(filepath.exists());

        // Load artifact
        let loaded = ArtifactCollector::load(&filepath).await?;
        assert_eq!(loaded.metadata.test_name, "test_save_load");
        assert_eq!(loaded.metadata.rng_seed, Some(0xdeadbeef));
        assert_eq!(
            loaded.metadata.failure_reason,
            Some("Test failure".to_string())
        );
        assert_eq!(loaded.logs.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_artifact_serialization() {
        let mut collector = ArtifactCollector::new("test_serialization");
        collector.set_rng_seed(12345);

        let topology = TopologySnapshot {
            node_count: 3,
            connections: HashMap::from([(0, vec![1, 2]), (1, vec![0]), (2, vec![0])]),
            partitions: vec![],
        };
        collector.save_topology(topology);

        let state = BlockchainStateSnapshot {
            node_id: 0,
            tip_height: 10,
            tip_hash: "0xhash".to_string(),
            balances: HashMap::from([("0xaddr1".to_string(), 1000)]),
            nonces: HashMap::from([("0xaddr1".to_string(), 5)]),
            total_supply: 10000,
            fees_burned: 50,
        };
        collector.add_blockchain_state(state);

        // Update metadata duration
        collector.metadata.duration_ms = 1234;

        // Create artifact
        let artifact = TestArtifact {
            metadata: collector.metadata.clone(),
            topology: collector.topology.clone(),
            blockchain_states: collector.blockchain_states.clone(),
            transactions: collector.transactions.clone(),
            logs: collector.logs.clone(),
        };

        // Serialize
        let json = serde_json::to_string_pretty(&artifact).unwrap();
        assert!(json.contains("test_serialization"));
        assert!(json.contains("12345"));

        // Deserialize
        let deserialized: TestArtifact = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.metadata.test_name, "test_serialization");
        assert_eq!(deserialized.metadata.rng_seed, Some(12345));
    }
}
