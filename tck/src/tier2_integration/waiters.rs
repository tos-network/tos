// File: testing-framework/src/tier2_integration/waiters.rs
//
// Tier 2 Waiter Primitives
//
// This module provides deterministic waiting utilities to replace sleep-based
// timing in integration tests. These primitives poll for specific conditions
// with configurable timeouts, ensuring tests wait exactly as long as needed.
//
// **Design Principle**: Never use `tokio::time::sleep()` in tests when waiting
// for blockchain state changes. Always use these waiter primitives instead.

use super::NodeRpc;
use anyhow::Result;
use tokio::time::{sleep, timeout, Duration};

/// Wait for a node to reach a specific block height.
///
/// This function polls the node's tip height every 100ms until it reaches
/// or exceeds the target height, or times out.
///
/// # Arguments
///
/// * `node` - A reference to any type implementing `NodeRpc`
/// * `height` - The target topoheight to wait for
/// * `timeout_duration` - Maximum time to wait before giving up
///
/// # Returns
///
/// * `Ok(())` - The node reached the target height
/// * `Err(_)` - Timeout occurred before reaching the target height
///
/// # Example
///
/// ```ignore
/// use tos_tck::tier2_integration::waiters::*;
/// use tokio::time::Duration;
///
/// // Wait for node to reach height 100, with 10 second timeout
/// wait_for_block(&node, 100, Duration::from_secs(10)).await?;
/// ```
///
/// # Polling Interval
///
/// 100ms - balances responsiveness with CPU usage
pub async fn wait_for_block<N: NodeRpc>(
    node: &N,
    height: u64,
    timeout_duration: Duration,
) -> Result<()> {
    const POLL_INTERVAL: Duration = Duration::from_millis(100);

    timeout(timeout_duration, async {
        loop {
            let current_height = node.get_tip_height().await?;
            if current_height >= height {
                return Ok(());
            }
            sleep(POLL_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "Timeout waiting for block height {} after {:?}",
            height,
            timeout_duration
        )
    })?
}

// TODO: Add wait_for_tx once Transaction type is integrated
// Currently disabled as NodeRpc doesn't have get_transaction method

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)]

    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Mock node that simulates height progression
    struct MockProgressingNode {
        height: Arc<Mutex<u64>>,
    }

    impl MockProgressingNode {
        fn new(initial_height: u64) -> Self {
            Self {
                height: Arc::new(Mutex::new(initial_height)),
            }
        }

        async fn advance_height(&self, increment: u64) {
            let mut height = self.height.lock().await;
            // Use saturating_add to prevent overflow
            *height = height.saturating_add(increment);
        }
    }

    #[async_trait]
    impl NodeRpc for MockProgressingNode {
        async fn get_tip_height(&self) -> Result<u64> {
            Ok(*self.height.lock().await)
        }

        async fn get_tips(&self) -> Result<Vec<super::super::Hash>> {
            Ok(vec![])
        }

        async fn get_balance(&self, _address: &super::super::Hash) -> Result<u64> {
            Ok(0)
        }

        async fn get_nonce(&self, _address: &super::super::Hash) -> Result<u64> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn test_wait_for_block_immediate() {
        let node = MockProgressingNode::new(100);

        // Height is already 100, should return immediately
        let result = wait_for_block(&node, 100, Duration::from_secs(1)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_block_progression() {
        let node = Arc::new(MockProgressingNode::new(50));

        // Clone for the background task
        let node_clone = node.clone();

        // Spawn a task to advance height after 200ms
        tokio::spawn(async move {
            sleep(Duration::from_millis(200)).await;
            node_clone.advance_height(60).await; // Advance to 110
        });

        // Wait for height 100 (should succeed after ~200ms)
        let result = wait_for_block(&*node, 100, Duration::from_secs(2)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_block_timeout() {
        let node = MockProgressingNode::new(50);

        // Wait for height 100 with short timeout (should fail)
        let result = wait_for_block(&node, 100, Duration::from_millis(50)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Timeout"));
    }
}
