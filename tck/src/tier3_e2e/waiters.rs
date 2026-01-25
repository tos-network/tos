// File: testing-framework/src/tier3_e2e/waiters.rs
//
// Tier 3 E2E Waiter Primitives
//
// This module provides deterministic waiting utilities for multi-node consensus
// convergence in end-to-end tests. These primitives ensure all nodes in a network
// reach agreement before proceeding with test assertions.
//
// **Design Principle**: In multi-node tests, never assume immediate consensus.
// Always use these waiter primitives to verify network-wide agreement.

use super::{Hash, NodeRpc};
use anyhow::{bail, Result};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::{sleep, timeout, Duration};

/// Type alias for progress callback functions.
pub type ProgressCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Configurable wait parameters for polling operations.
///
/// Provides fine-grained control over timeouts, polling intervals,
/// and optional progress callbacks for long-running waits.
///
/// # Example
///
/// ```ignore
/// let config = WaitConfig::new(Duration::from_secs(30))
///     .with_poll_interval(Duration::from_millis(200))
///     .with_progress(|msg| println!("Progress: {}", msg));
///
/// wait_all_tips_equal_with_config(&nodes, &config).await?;
/// ```
#[derive(Clone)]
pub struct WaitConfig {
    /// Maximum time to wait before timing out
    pub timeout: Duration,
    /// Interval between polls (default: 500ms)
    pub poll_interval: Duration,
    /// Optional progress callback invoked on each poll iteration
    pub progress_callback: Option<ProgressCallback>,
}

impl WaitConfig {
    /// Create a new WaitConfig with the given timeout and default poll interval.
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            poll_interval: Duration::from_millis(500),
            progress_callback: None,
        }
    }

    /// Set the poll interval.
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Set a progress callback that is invoked on each poll iteration.
    pub fn with_progress<F: Fn(&str) + Send + Sync + 'static>(mut self, callback: F) -> Self {
        self.progress_callback = Some(Arc::new(callback));
        self
    }

    /// Report progress if a callback is configured.
    fn report_progress(&self, message: &str) {
        if let Some(ref cb) = self.progress_callback {
            cb(message);
        }
    }
}

impl std::fmt::Debug for WaitConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaitConfig")
            .field("timeout", &self.timeout)
            .field("poll_interval", &self.poll_interval)
            .field("has_progress_callback", &self.progress_callback.is_some())
            .finish()
    }
}

impl Default for WaitConfig {
    fn default() -> Self {
        Self::new(Duration::from_secs(10))
    }
}

/// Wait for all nodes' tips to converge to the same set of block hashes.
///
/// This function polls all nodes every 500ms to check if their tip sets
/// are identical, indicating consensus convergence. This is critical for
/// BlockDAG-based blockchains where multiple tips can temporarily exist.
///
/// # Arguments
///
/// * `nodes` - A slice of references to types implementing `NodeRpc`
/// * `timeout_duration` - Maximum time to wait before giving up
///
/// # Returns
///
/// * `Ok(())` - All nodes have converged to the same tip set
/// * `Err(_)` - Timeout occurred or nodes provided is empty
///
/// # Example
///
/// ```ignore
/// use tos_tck::tier3_e2e::waiters::*;
/// use tokio::time::Duration;
///
/// // Create a 5-node network
/// let nodes = vec![node0, node1, node2, node3, node4];
///
/// // Partition network: [0,1,2] vs [3,4]
/// partition(&toxi, &["node0", "node1", "node2"], &["node3", "node4"]).await?;
///
/// // Each partition mines blocks independently
/// nodes[0].mine_block().await?;
/// nodes[3].mine_block().await?;
///
/// // Heal partition
/// partition_handle.heal().await?;
///
/// // Wait for consensus convergence (instead of sleep!)
/// wait_all_tips_equal(&nodes, Duration::from_secs(10)).await?;
///
/// // Now safe to assert that all nodes agree
/// assert_eq!(nodes[0].get_tips().await?, nodes[4].get_tips().await?);
/// ```
///
/// # Polling Interval
///
/// 500ms - longer than tier2 due to network propagation and consensus overhead
///
/// # Consensus Convergence
///
/// In BlockDAG, nodes may temporarily have different tip sets due to:
/// - Network latency and block propagation delays
/// - Concurrent block mining
/// - Network partitions being healed
///
/// This function ensures all nodes have processed all blocks and converged
/// to the same understanding of the DAG frontier.
pub async fn wait_all_tips_equal<N: NodeRpc>(
    nodes: &[N],
    timeout_duration: Duration,
) -> Result<()> {
    const POLL_INTERVAL: Duration = Duration::from_millis(500);

    if nodes.is_empty() {
        bail!("No nodes provided to wait_all_tips_equal");
    }

    timeout(timeout_duration, async {
        loop {
            // Collect tips from all nodes
            let mut all_tips = Vec::new();
            for node in nodes {
                match node.get_tips().await {
                    Ok(tips) => all_tips.push(tips),
                    Err(e) => {
                        // Node might be temporarily unreachable, continue polling
                        if log::log_enabled!(log::Level::Debug) {
                            log::debug!("Failed to get tips from node: {}", e);
                        }
                        sleep(POLL_INTERVAL).await;
                        continue;
                    }
                }
            }

            // Check if all nodes returned tips
            if all_tips.len() != nodes.len() {
                sleep(POLL_INTERVAL).await;
                continue;
            }

            // Convert tips to HashSets for comparison (order doesn't matter)
            let tip_sets: Vec<HashSet<Hash>> = all_tips
                .iter()
                .map(|tips| tips.iter().cloned().collect())
                .collect();

            // Check if all tip sets are equal
            // Note: windows(2).all() returns true for 0 or 1 elements (vacuous truth)
            // We need at least 2 nodes to meaningfully compare tips
            if tip_sets.len() >= 2 && tip_sets.windows(2).all(|w| w[0] == w[1]) {
                return Ok(());
            } else if tip_sets.len() == 1 {
                // Single node case - trivially converged
                return Ok(());
            }

            sleep(POLL_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "Timeout waiting for tips to converge after {:?}",
            timeout_duration
        )
    })?
}

/// Wait for all nodes to reach the same tip height.
///
/// This is a simpler check than `wait_all_tips_equal` - it only verifies
/// that all nodes have processed blocks up to the same topoheight, but
/// doesn't verify they agree on which blocks are tips.
///
/// # Use Cases
///
/// - Quick convergence check when tip hash agreement isn't critical
/// - Verifying all nodes have processed recent blocks
/// - Testing scenarios where topoheight is the primary concern
///
/// # Arguments
///
/// * `nodes` - A slice of references to types implementing `NodeRpc`
/// * `timeout_duration` - Maximum time to wait before giving up
///
/// # Returns
///
/// * `Ok(())` - All nodes have the same tip height
/// * `Err(_)` - Timeout occurred or nodes provided is empty
///
/// # Example
///
/// ```ignore
/// use tos_tck::tier3_e2e::waiters::*;
/// use tokio::time::Duration;
///
/// // Wait for all nodes to reach the same height
/// wait_all_heights_equal(&nodes, Duration::from_secs(5)).await?;
/// ```
pub async fn wait_all_heights_equal<N: NodeRpc>(
    nodes: &[N],
    timeout_duration: Duration,
) -> Result<()> {
    const POLL_INTERVAL: Duration = Duration::from_millis(500);

    if nodes.is_empty() {
        bail!("No nodes provided to wait_all_heights_equal");
    }

    timeout(timeout_duration, async {
        loop {
            // Collect heights from all nodes
            let mut heights = Vec::new();
            for node in nodes {
                match node.get_tip_height().await {
                    Ok(height) => heights.push(height),
                    Err(e) => {
                        // Node might be temporarily unreachable, continue polling
                        if log::log_enabled!(log::Level::Debug) {
                            log::debug!("Failed to get height from node: {}", e);
                        }
                        sleep(POLL_INTERVAL).await;
                        continue;
                    }
                }
            }

            // Check if all nodes returned heights
            if heights.len() != nodes.len() {
                sleep(POLL_INTERVAL).await;
                continue;
            }

            // Check if all heights are equal
            if heights.windows(2).all(|w| w[0] == w[1]) {
                return Ok(());
            }

            sleep(POLL_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "Timeout waiting for heights to converge after {:?}",
            timeout_duration
        )
    })?
}

/// Wait for a node to produce at least `count` new blocks beyond the current height.
///
/// # Arguments
///
/// * `node` - The node to monitor
/// * `count` - Number of new blocks to wait for
/// * `config` - Wait configuration
///
/// # Returns
///
/// * `Ok(u64)` - The new tip height after blocks were produced
/// * `Err(_)` - Timeout or node error
///
/// # Example
///
/// ```ignore
/// let new_height = wait_for_new_blocks(
///     &node,
///     3,
///     &WaitConfig::new(Duration::from_secs(10)),
/// ).await?;
/// assert!(new_height >= initial_height + 3);
/// ```
pub async fn wait_for_new_blocks<N: NodeRpc>(
    node: &N,
    count: u64,
    config: &WaitConfig,
) -> Result<u64> {
    if count == 0 {
        return node.get_tip_height().await;
    }

    let initial_height = node.get_tip_height().await?;
    let target_height = initial_height.saturating_add(count);

    timeout(config.timeout, async {
        loop {
            match node.get_tip_height().await {
                Ok(height) if height >= target_height => {
                    return Ok(height);
                }
                Ok(height) => {
                    config.report_progress(&format!(
                        "Waiting for blocks: {}/{} (current height: {})",
                        height.saturating_sub(initial_height),
                        count,
                        height
                    ));
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Debug) {
                        log::debug!("Failed to get height: {}", e);
                    }
                }
            }
            sleep(config.poll_interval).await;
        }
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "Timeout waiting for {} new blocks (target height: {}) after {:?}",
            count,
            target_height,
            config.timeout
        )
    })?
}

/// Wait for a specific transaction to be confirmed at a target height.
///
/// This function polls the node until the tip height reaches a height
/// sufficient to confirm the transaction at the required depth.
///
/// # Arguments
///
/// * `node` - The node to query
/// * `tx_hash` - The transaction hash (for logging)
/// * `required_height` - The height at which the TX is considered confirmed
/// * `config` - Wait configuration
///
/// # Returns
///
/// * `Ok(())` - Transaction reached the required confirmation height
/// * `Err(_)` - Timeout or node error
pub async fn wait_for_tx_confirmed<N: NodeRpc>(
    node: &N,
    tx_hash: &Hash,
    required_height: u64,
    config: &WaitConfig,
) -> Result<()> {
    timeout(config.timeout, async {
        loop {
            match node.get_tip_height().await {
                Ok(height) if height >= required_height => {
                    if log::log_enabled!(log::Level::Debug) {
                        log::debug!(
                            "Transaction {} confirmed at height {} (required: {})",
                            tx_hash,
                            height,
                            required_height
                        );
                    }
                    return Ok(());
                }
                Ok(height) => {
                    config.report_progress(&format!(
                        "Waiting for tx {} confirmation: height {}/{}",
                        tx_hash, height, required_height
                    ));
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Debug) {
                        log::debug!(
                            "Failed to get height while waiting for tx {}: {}",
                            tx_hash,
                            e
                        );
                    }
                }
            }
            sleep(config.poll_interval).await;
        }
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "Timeout waiting for tx {} to be confirmed at height {} after {:?}",
            tx_hash,
            required_height,
            config.timeout
        )
    })?
}

/// Wait for a node to sync to a target height.
///
/// Useful for testing bootstrap sync and late-joining nodes that need
/// to catch up to the rest of the network.
///
/// # Arguments
///
/// * `node` - The node to monitor
/// * `target_height` - The height to wait for
/// * `config` - Wait configuration
///
/// # Returns
///
/// * `Ok(())` - Node reached the target height
/// * `Err(_)` - Timeout or node error
///
/// # Example
///
/// ```ignore
/// // Wait for bootstrap node to sync to height 100
/// wait_for_sync_complete(
///     &bootstrap_node,
///     100,
///     &WaitConfig::new(Duration::from_secs(60))
///         .with_poll_interval(Duration::from_secs(1)),
/// ).await?;
/// ```
pub async fn wait_for_sync_complete<N: NodeRpc>(
    node: &N,
    target_height: u64,
    config: &WaitConfig,
) -> Result<()> {
    timeout(config.timeout, async {
        loop {
            match node.get_tip_height().await {
                Ok(height) if height >= target_height => {
                    if log::log_enabled!(log::Level::Debug) {
                        log::debug!(
                            "Sync complete: reached height {} (target: {})",
                            height,
                            target_height
                        );
                    }
                    return Ok(());
                }
                Ok(height) => {
                    config.report_progress(&format!(
                        "Syncing: height {}/{} ({:.1}%)",
                        height,
                        target_height,
                        if target_height > 0 {
                            (height as f64 / target_height as f64) * 100.0
                        } else {
                            100.0
                        }
                    ));
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Debug) {
                        log::debug!("Failed to get height during sync: {}", e);
                    }
                }
            }
            sleep(config.poll_interval).await;
        }
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "Timeout waiting for sync to height {} after {:?}",
            target_height,
            config.timeout
        )
    })?
}

/// Wait for all nodes' tips to converge, with configurable wait parameters.
///
/// This is the configurable version of `wait_all_tips_equal` that uses
/// `WaitConfig` for fine-grained control over polling behavior.
pub async fn wait_all_tips_equal_with_config<N: NodeRpc>(
    nodes: &[N],
    config: &WaitConfig,
) -> Result<()> {
    if nodes.is_empty() {
        bail!("No nodes provided to wait_all_tips_equal_with_config");
    }

    timeout(config.timeout, async {
        let mut iteration = 0u64;
        loop {
            iteration = iteration.saturating_add(1);
            let mut all_tips = Vec::new();
            for node in nodes {
                match node.get_tips().await {
                    Ok(tips) => all_tips.push(tips),
                    Err(_) => {
                        sleep(config.poll_interval).await;
                        continue;
                    }
                }
            }

            if all_tips.len() != nodes.len() {
                sleep(config.poll_interval).await;
                continue;
            }

            let tip_sets: Vec<HashSet<Hash>> = all_tips
                .iter()
                .map(|tips| tips.iter().cloned().collect())
                .collect();

            // Single node trivially converges; multiple nodes must all agree
            if tip_sets.len() <= 1 || tip_sets.windows(2).all(|w| w[0] == w[1]) {
                return Ok(());
            }

            config.report_progress(&format!(
                "Waiting for tip convergence (iteration {})",
                iteration
            ));
            sleep(config.poll_interval).await;
        }
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "Timeout waiting for tips to converge after {:?}",
            config.timeout
        )
    })?
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::expect_used)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Mock node with controllable tips
    struct MockNode {
        tips: Arc<Mutex<Vec<Hash>>>,
        height: Arc<Mutex<u64>>,
    }

    impl MockNode {
        fn new(tips: Vec<Hash>, height: u64) -> Self {
            Self {
                tips: Arc::new(Mutex::new(tips)),
                height: Arc::new(Mutex::new(height)),
            }
        }

        #[allow(dead_code)]
        async fn set_tips(&self, new_tips: Vec<Hash>) {
            *self.tips.lock().await = new_tips;
        }

        #[allow(dead_code)]
        async fn set_height(&self, new_height: u64) {
            *self.height.lock().await = new_height;
        }
    }

    #[async_trait]
    impl NodeRpc for MockNode {
        async fn get_tip_height(&self) -> Result<u64> {
            Ok(*self.height.lock().await)
        }

        async fn get_tips(&self) -> Result<Vec<Hash>> {
            Ok(self.tips.lock().await.clone())
        }

        async fn get_balance(&self, _address: &Hash) -> Result<u64> {
            Ok(0)
        }

        async fn get_nonce(&self, _address: &Hash) -> Result<u64> {
            Ok(0)
        }
    }

    fn create_test_hash(id: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        Hash::new(bytes)
    }

    #[tokio::test]
    async fn test_wait_all_tips_equal_immediate() {
        let common_tips = vec![create_test_hash(1), create_test_hash(2)];
        let nodes = [
            MockNode::new(common_tips.clone(), 100),
            MockNode::new(common_tips.clone(), 100),
            MockNode::new(common_tips.clone(), 100),
        ];

        // All nodes already have same tips, should return immediately
        let result = wait_all_tips_equal(&nodes[..], Duration::from_secs(1)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_all_tips_equal_convergence() {
        let node1 = MockNode::new(vec![create_test_hash(1)], 100);
        let node2 = MockNode::new(vec![create_test_hash(2)], 100);
        let node3 = MockNode::new(vec![create_test_hash(3)], 100);

        // Clone Arc references for background task
        let node2_tips = node2.tips.clone();
        let node3_tips = node3.tips.clone();

        // Spawn a task to converge tips after 300ms
        tokio::spawn(async move {
            sleep(Duration::from_millis(300)).await;
            let common_tips = vec![create_test_hash(1)];
            *node2_tips.lock().await = common_tips.clone();
            *node3_tips.lock().await = common_tips;
        });

        // Wait for convergence
        let nodes = [node1, node2, node3];
        let result = wait_all_tips_equal(&nodes, Duration::from_secs(2)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_all_tips_equal_timeout() {
        let node1 = MockNode::new(vec![create_test_hash(1)], 100);
        let node2 = MockNode::new(vec![create_test_hash(2)], 100);

        // Nodes never converge, should timeout
        let nodes = [node1, node2];
        let result = wait_all_tips_equal(&nodes, Duration::from_millis(100)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Timeout"));
    }

    #[tokio::test]
    async fn test_wait_all_tips_equal_empty_nodes() {
        let nodes: Vec<MockNode> = vec![];
        let result = wait_all_tips_equal(&nodes, Duration::from_secs(1)).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No nodes provided"));
    }

    #[tokio::test]
    async fn test_wait_all_heights_equal_immediate() {
        let nodes = [
            MockNode::new(vec![], 100),
            MockNode::new(vec![], 100),
            MockNode::new(vec![], 100),
        ];

        let result = wait_all_heights_equal(&nodes, Duration::from_secs(1)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_all_heights_equal_convergence() {
        let node1 = MockNode::new(vec![], 100);
        let node2 = MockNode::new(vec![], 90);
        let node3 = MockNode::new(vec![], 95);

        let node2_height = node2.height.clone();
        let node3_height = node3.height.clone();

        // Converge heights after 200ms
        tokio::spawn(async move {
            sleep(Duration::from_millis(200)).await;
            *node2_height.lock().await = 100;
            *node3_height.lock().await = 100;
        });

        let nodes = [node1, node2, node3];
        let result = wait_all_heights_equal(&nodes, Duration::from_secs(2)).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_wait_config_builder() {
        let config =
            WaitConfig::new(Duration::from_secs(30)).with_poll_interval(Duration::from_millis(200));
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.poll_interval, Duration::from_millis(200));
        assert!(config.progress_callback.is_none());
    }

    #[test]
    fn test_wait_config_with_progress() {
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();

        let config = WaitConfig::new(Duration::from_secs(5)).with_progress(move |_msg| {
            // In a real scenario, we'd track calls
            let _ = &called_clone;
        });
        assert!(config.progress_callback.is_some());
    }

    #[test]
    fn test_wait_config_default() {
        let config = WaitConfig::default();
        assert_eq!(config.timeout, Duration::from_secs(10));
        assert_eq!(config.poll_interval, Duration::from_millis(500));
    }

    #[tokio::test]
    async fn test_wait_for_new_blocks() {
        let node = MockNode::new(vec![], 5);
        let node_height = node.height.clone();

        // Spawn a task to increment height after 100ms
        tokio::spawn(async move {
            sleep(Duration::from_millis(100)).await;
            *node_height.lock().await = 8;
        });

        let config =
            WaitConfig::new(Duration::from_secs(2)).with_poll_interval(Duration::from_millis(50));
        let result = wait_for_new_blocks(&node, 3, &config).await;
        assert!(result.is_ok());
        assert!(result.unwrap() >= 8);
    }

    #[tokio::test]
    async fn test_wait_for_new_blocks_zero() {
        let node = MockNode::new(vec![], 10);
        let config = WaitConfig::new(Duration::from_secs(1));
        let result = wait_for_new_blocks(&node, 0, &config).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 10);
    }

    #[tokio::test]
    async fn test_wait_for_sync_complete() {
        let node = MockNode::new(vec![], 50);
        let node_height = node.height.clone();

        // Spawn a task to reach target height
        tokio::spawn(async move {
            sleep(Duration::from_millis(100)).await;
            *node_height.lock().await = 100;
        });

        let config =
            WaitConfig::new(Duration::from_secs(2)).with_poll_interval(Duration::from_millis(50));
        let result = wait_for_sync_complete(&node, 100, &config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_sync_complete_timeout() {
        let node = MockNode::new(vec![], 50);
        // Node never reaches target height
        let config = WaitConfig::new(Duration::from_millis(200))
            .with_poll_interval(Duration::from_millis(50));
        let result = wait_for_sync_complete(&node, 1000, &config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Timeout"));
    }

    #[tokio::test]
    async fn test_wait_for_tx_confirmed() {
        let node = MockNode::new(vec![], 5);
        let node_height = node.height.clone();

        tokio::spawn(async move {
            sleep(Duration::from_millis(100)).await;
            *node_height.lock().await = 10;
        });

        let tx_hash = create_test_hash(42);
        let config =
            WaitConfig::new(Duration::from_secs(2)).with_poll_interval(Duration::from_millis(50));
        let result = wait_for_tx_confirmed(&node, &tx_hash, 10, &config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_all_tips_equal_with_config() {
        let common_tips = vec![create_test_hash(1), create_test_hash(2)];
        let nodes = [
            MockNode::new(common_tips.clone(), 100),
            MockNode::new(common_tips.clone(), 100),
            MockNode::new(common_tips.clone(), 100),
        ];

        let config =
            WaitConfig::new(Duration::from_secs(1)).with_poll_interval(Duration::from_millis(50));
        let result = wait_all_tips_equal_with_config(&nodes[..], &config).await;
        assert!(result.is_ok());
    }
}
