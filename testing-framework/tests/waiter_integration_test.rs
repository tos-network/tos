// Integration test for waiter primitives
// This tests the waiters in isolation from other framework components

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

// Import our waiter implementations directly
use tos_testing_framework::tier2_integration::waiters::wait_for_block;
use tos_testing_framework::tier2_integration::{Hash, NodeRpc};
use tos_testing_framework::tier3_e2e::waiters::{wait_all_heights_equal, wait_all_tips_equal};

// Mock node implementation
struct TestNode {
    height: Arc<Mutex<u64>>,
    tips: Arc<Mutex<Vec<Hash>>>,
}

impl TestNode {
    fn new(height: u64, tips: Vec<Hash>) -> Self {
        Self {
            height: Arc::new(Mutex::new(height)),
            tips: Arc::new(Mutex::new(tips)),
        }
    }

    async fn set_height(&self, h: u64) {
        *self.height.lock().await = h;
    }
}

#[async_trait]
impl NodeRpc for TestNode {
    async fn get_tip_height(&self) -> Result<u64> {
        Ok(*self.height.lock().await)
    }

    async fn get_tips(&self) -> Result<Vec<Hash>> {
        Ok(self.tips.lock().await.clone())
    }

    async fn get_balance(&self, _address: &Hash) -> Result<u64> {
        Ok(1_000_000)
    }

    async fn get_nonce(&self, _address: &Hash) -> Result<u64> {
        Ok(0)
    }
}

// Implement NodeRpc for &TestNode to allow passing &[&TestNode]
#[async_trait]
impl NodeRpc for &TestNode {
    async fn get_tip_height(&self) -> Result<u64> {
        (*self).get_tip_height().await
    }

    async fn get_tips(&self) -> Result<Vec<Hash>> {
        (*self).get_tips().await
    }

    async fn get_balance(&self, address: &Hash) -> Result<u64> {
        (*self).get_balance(address).await
    }

    async fn get_nonce(&self, address: &Hash) -> Result<u64> {
        (*self).get_nonce(address).await
    }
}

#[tokio::test]
async fn test_wait_for_block_basic() {
    let node = Arc::new(TestNode::new(100, vec![]));

    // Already at height 100, should return immediately
    let result = wait_for_block(&*node, 100, Duration::from_secs(1)).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_wait_for_block_with_progression() {
    let node = Arc::new(TestNode::new(50, vec![]));
    let node_clone = node.clone();

    // Advance height in background
    tokio::spawn(async move {
        sleep(Duration::from_millis(200)).await;
        node_clone.set_height(110).await;
    });

    // Wait for height 100
    let result = wait_for_block(&*node, 100, Duration::from_secs(2)).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_wait_all_tips_equal_basic() {
    let common_tips = vec![Hash::new([1u8; 32])];
    let nodes = vec![
        TestNode::new(100, common_tips.clone()),
        TestNode::new(100, common_tips.clone()),
    ];

    let nodes_ref: Vec<&TestNode> = nodes.iter().collect();
    let result = wait_all_tips_equal(&nodes_ref[..], Duration::from_secs(1)).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_wait_all_heights_equal_basic() {
    let nodes = vec![TestNode::new(100, vec![]), TestNode::new(100, vec![])];

    let nodes_ref: Vec<&TestNode> = nodes.iter().collect();
    let result = wait_all_heights_equal(&nodes_ref, Duration::from_secs(1)).await;
    assert!(result.is_ok());
}
