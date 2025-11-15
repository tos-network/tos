// File: testing-framework/src/tier2_integration/mod.rs
//
// Tier 2 Integration Testing Components
//
// This module provides integration testing utilities for TOS blockchain,
// including RPC abstractions and waiter primitives for deterministic testing.

pub mod builder;
pub mod rpc_helpers;
pub mod strategies;
pub mod test_daemon;
/// Waiter primitives for waiting on blockchain state changes
pub mod waiters;

#[cfg(test)]
pub mod integration_tests;

#[cfg(test)]
pub mod property_tests;

// Re-export main types for convenience
pub use builder::TestDaemonBuilder;
pub use test_daemon::TestDaemon;

use anyhow::Result;
use async_trait::async_trait;

// Re-export Hash for use across the testing framework
pub use tos_common::crypto::Hash;

/// Basic RPC trait for node interactions in integration tests.
///
/// This trait abstracts the common operations needed for testing against
/// a TOS node, whether it's a test daemon or a real node.
///
/// # Implementation Note
///
/// Implementations should handle errors gracefully and provide meaningful
/// error messages for debugging test failures.
#[async_trait]
pub trait NodeRpc: Send + Sync {
    /// Get the current tip height (the maximum topoheight of all tips).
    ///
    /// # Returns
    ///
    /// The highest topoheight among all current tips.
    ///
    /// # Errors
    ///
    /// Returns an error if the node is unreachable or the query fails.
    async fn get_tip_height(&self) -> Result<u64>;

    /// Get all current tips (block hashes at the frontier of the DAG).
    ///
    /// # Returns
    ///
    /// A vector of block hashes representing the current tips.
    /// In GHOSTDAG, there can be multiple tips in the DAG frontier.
    ///
    /// # Errors
    ///
    /// Returns an error if the node is unreachable or the query fails.
    async fn get_tips(&self) -> Result<Vec<Hash>>;

    /// Get account balance at current tip.
    ///
    /// # Arguments
    ///
    /// * `address` - The account address to query
    ///
    /// # Errors
    ///
    /// Returns an error if the node is unreachable or the query fails.
    async fn get_balance(&self, address: &Hash) -> Result<u64>;

    /// Get account nonce at current tip.
    ///
    /// # Arguments
    ///
    /// * `address` - The account address to query
    ///
    /// # Errors
    ///
    /// Returns an error if the node is unreachable or the query fails.
    async fn get_nonce(&self, address: &Hash) -> Result<u64>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_hash(id: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        Hash::new(bytes)
    }

    /// Mock implementation of NodeRpc for testing
    struct MockNode {
        tip_height: u64,
        tips: Vec<Hash>,
    }

    #[async_trait]
    impl NodeRpc for MockNode {
        async fn get_tip_height(&self) -> Result<u64> {
            Ok(self.tip_height)
        }

        async fn get_tips(&self) -> Result<Vec<Hash>> {
            Ok(self.tips.clone())
        }

        async fn get_balance(&self, _address: &Hash) -> Result<u64> {
            Ok(1_000_000)
        }

        async fn get_nonce(&self, _address: &Hash) -> Result<u64> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn test_mock_node_rpc() {
        let node = MockNode {
            tip_height: 100,
            tips: vec![create_test_hash(1), create_test_hash(2)],
        };

        assert_eq!(node.get_tip_height().await.unwrap(), 100);
        assert_eq!(node.get_tips().await.unwrap().len(), 2);

        let addr = create_test_hash(0xFF);
        assert_eq!(node.get_balance(&addr).await.unwrap(), 1_000_000);
        assert_eq!(node.get_nonce(&addr).await.unwrap(), 0);
    }
}
