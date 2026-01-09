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
use tokio::time::{sleep, timeout, Duration};

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
}
