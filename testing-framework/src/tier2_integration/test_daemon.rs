//! TestDaemon - In-process daemon for Tier 2 integration testing
//!
//! Provides a lightweight daemon wrapper around TestBlockchain with RPC-like
//! interface for integration testing without requiring actual RPC server setup.

use crate::orchestrator::Clock;
use crate::tier1_component::{TestBlockchain, TestTransaction};
use crate::tier2_integration::NodeRpc;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tos_common::crypto::Hash;

/// In-process test daemon for Tier 2 integration testing
///
/// TestDaemon wraps TestBlockchain and provides:
/// - RPC-like interface for realistic API testing
/// - Direct state access for assertions
/// - Clock control for deterministic testing
/// - Lifecycle management (start/stop/restart)
///
/// # Example
///
/// ```rust,ignore
/// use tos_testing_framework::tier2_integration::TestDaemonBuilder;
///
/// let daemon = TestDaemonBuilder::new()
///     .with_clock(clock)
///     .with_funded_accounts(10, 1_000_000)
///     .build()
///     .await?;
///
/// // Submit transaction via RPC-like interface
/// let tx = daemon.create_transaction(alice, bob, 1000, 100)?;
/// let txid = daemon.submit_transaction(tx).await?;
///
/// // Mine block
/// daemon.mine_block().await?;
///
/// // Assert via direct access
/// assert_eq!(daemon.get_balance(&alice).await?, 999_000);
/// ```
pub struct TestDaemon {
    /// Underlying blockchain state
    blockchain: TestBlockchain,

    /// Injected clock for deterministic time control
    clock: Arc<dyn Clock>,

    /// Whether the daemon is currently "running"
    /// (Used for lifecycle testing - start/stop/restart)
    is_running: bool,
}

impl TestDaemon {
    /// Create a new TestDaemon from components
    ///
    /// This is an internal constructor. Use `TestDaemonBuilder` instead.
    pub(crate) fn new(blockchain: TestBlockchain, clock: Arc<dyn Clock>) -> Self {
        Self {
            blockchain,
            clock,
            is_running: true,
        }
    }

    // ========================================================================
    // RPC-like Interface (for realistic API testing)
    // ========================================================================

    /// Submit a transaction to the mempool (RPC-like interface)
    ///
    /// This mimics the `submit_transaction` RPC endpoint.
    ///
    /// # Arguments
    ///
    /// * `tx` - The transaction to submit
    ///
    /// # Returns
    ///
    /// The transaction hash if successfully added to mempool
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Daemon is not running
    /// - Transaction validation fails
    /// - Sender has insufficient balance
    /// - Nonce is invalid
    pub async fn submit_transaction(&self, tx: TestTransaction) -> Result<Hash> {
        self.ensure_running()?;
        self.blockchain.submit_transaction(tx).await
    }

    /// Mine a new block (RPC-like interface)
    ///
    /// This mimics the mining workflow where:
    /// 1. Get block template
    /// 2. Solve PoW (instant in tests)
    /// 3. Submit block
    ///
    /// # Returns
    ///
    /// The hash of the newly mined block
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Daemon is not running
    /// - Block creation fails
    pub async fn mine_block(&self) -> Result<Hash> {
        self.ensure_running()?;
        let block = self.blockchain.mine_block().await?;
        Ok(block.hash)
    }

    /// Receive a block from a peer (P2P-like interface)
    ///
    /// This simulates receiving a block via P2P network propagation.
    /// The block is validated and applied to the local blockchain.
    ///
    /// # Arguments
    ///
    /// * `block` - The block received from a peer
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Daemon is not running
    /// - Block validation fails
    /// - Block height is not sequential
    /// - Block is duplicate
    pub async fn receive_block(&self, block: crate::tier1_component::TestBlock) -> Result<()> {
        self.ensure_running()?;
        self.blockchain.receive_block(block).await
    }

    /// Get block at specific height (RPC-like interface)
    ///
    /// This allows peers to request blocks for synchronization.
    ///
    /// # Arguments
    ///
    /// * `height` - The block height to retrieve
    ///
    /// # Returns
    ///
    /// The block at the specified height, or None if height doesn't exist
    ///
    /// # Errors
    ///
    /// Returns an error if daemon is not running
    pub async fn get_block_at_height(
        &self,
        height: u64,
    ) -> Result<Option<crate::tier1_component::TestBlock>> {
        self.ensure_running()?;
        self.blockchain.get_block_at_height(height).await
    }

    /// Get current tip height (RPC-like interface)
    ///
    /// # Errors
    ///
    /// Returns an error if daemon is not running
    pub async fn get_tip_height(&self) -> Result<u64> {
        self.ensure_running()?;
        self.blockchain.get_tip_height().await
    }

    /// Get current DAG tips (RPC-like interface)
    ///
    /// # Errors
    ///
    /// Returns an error if daemon is not running
    pub async fn get_tips(&self) -> Result<Vec<Hash>> {
        self.ensure_running()?;
        self.blockchain.get_tips().await
    }

    /// Get account balance (RPC-like interface)
    ///
    /// # Arguments
    ///
    /// * `address` - The account address to query
    ///
    /// # Errors
    ///
    /// Returns an error if daemon is not running
    pub async fn get_balance(&self, address: &Hash) -> Result<u64> {
        self.ensure_running()?;
        self.blockchain.get_balance(address).await
    }

    /// Get account nonce (RPC-like interface)
    ///
    /// # Arguments
    ///
    /// * `address` - The account address to query
    ///
    /// # Errors
    ///
    /// Returns an error if daemon is not running
    pub async fn get_nonce(&self, address: &Hash) -> Result<u64> {
        self.ensure_running()?;
        self.blockchain.get_nonce(address).await
    }

    // ========================================================================
    // Direct State Access (for test assertions)
    // ========================================================================

    /// Get direct reference to underlying blockchain
    ///
    /// This allows tests to perform deep assertions on blockchain state
    /// that wouldn't be possible via RPC alone.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Access internal state for detailed assertions
    /// let accounts = daemon.blockchain().accounts_kv().await?;
    /// assert_eq!(accounts.len(), 10);
    /// ```
    pub fn blockchain(&self) -> &TestBlockchain {
        &self.blockchain
    }

    /// Get reference to injected clock
    ///
    /// Allows tests to control time progression.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Advance time by 1 hour
    /// daemon.clock().sleep(Duration::from_secs(3600)).await;
    /// ```
    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    // ========================================================================
    // Lifecycle Management (start/stop/restart)
    // ========================================================================

    /// Check if daemon is currently running
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Stop the daemon
    ///
    /// This marks the daemon as stopped, causing RPC methods to fail.
    /// Used for testing daemon restart scenarios.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// daemon.stop();
    /// assert!(daemon.submit_transaction(tx).await.is_err());
    /// ```
    pub fn stop(&mut self) {
        self.is_running = false;
    }

    /// Start the daemon
    ///
    /// This marks the daemon as running, allowing RPC methods to work.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// daemon.stop();
    /// daemon.start();
    /// assert!(daemon.submit_transaction(tx).await.is_ok());
    /// ```
    pub fn start(&mut self) {
        self.is_running = true;
    }

    /// Restart the daemon
    ///
    /// Convenience method that stops and starts the daemon.
    /// Useful for testing recovery scenarios.
    pub fn restart(&mut self) {
        self.stop();
        self.start();
    }

    // ========================================================================
    // Helper Methods
    // ========================================================================

    /// Ensure daemon is running, return error if not
    fn ensure_running(&self) -> Result<()> {
        if !self.is_running {
            anyhow::bail!("Daemon is not running");
        }
        Ok(())
    }
}

// ========================================================================
// NodeRpc trait implementation
// ========================================================================

#[async_trait]
impl NodeRpc for TestDaemon {
    async fn get_tip_height(&self) -> Result<u64> {
        self.ensure_running()?;
        self.blockchain.get_tip_height().await
    }

    async fn get_tips(&self) -> Result<Vec<Hash>> {
        self.ensure_running()?;
        self.blockchain.get_tips().await
    }

    async fn get_balance(&self, address: &Hash) -> Result<u64> {
        self.ensure_running()?;
        self.blockchain.get_balance(address).await
    }

    async fn get_nonce(&self, address: &Hash) -> Result<u64> {
        self.ensure_running()?;
        self.blockchain.get_nonce(address).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)]

    use super::*;
    use crate::orchestrator::SystemClock;
    use crate::tier1_component::TestBlockchainBuilder;

    fn create_test_address(id: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        Hash::new(bytes)
    }

    #[tokio::test]
    async fn test_daemon_lifecycle() {
        let clock = Arc::new(SystemClock);
        let blockchain = TestBlockchainBuilder::new()
            .with_clock(clock.clone())
            .with_funded_account_count(1)
            .build()
            .await
            .unwrap();

        let mut daemon = TestDaemon::new(blockchain, clock);

        // Initially running
        assert!(daemon.is_running());

        // Stop daemon
        daemon.stop();
        assert!(!daemon.is_running());

        // RPC methods should fail
        assert!(daemon.get_tip_height().await.is_err());

        // Restart daemon
        daemon.restart();
        assert!(daemon.is_running());

        // RPC methods should work again
        assert_eq!(daemon.get_tip_height().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_daemon_rpc_interface() {
        let clock = Arc::new(SystemClock);
        let alice = create_test_address(1);

        let blockchain = TestBlockchainBuilder::new()
            .with_clock(clock.clone())
            .with_funded_account(alice.clone(), 1_000_000)
            .build()
            .await
            .unwrap();

        let daemon = TestDaemon::new(blockchain, clock);

        // Test RPC methods
        assert_eq!(daemon.get_tip_height().await.unwrap(), 0);
        assert_eq!(daemon.get_balance(&alice).await.unwrap(), 1_000_000);
        assert_eq!(daemon.get_nonce(&alice).await.unwrap(), 0);

        let tips = daemon.get_tips().await.unwrap();
        assert_eq!(tips.len(), 1);
    }

    #[tokio::test]
    async fn test_daemon_transaction_and_mining() {
        let clock = Arc::new(SystemClock);
        let alice = create_test_address(1);
        let bob = create_test_address(2);

        let blockchain = TestBlockchainBuilder::new()
            .with_clock(clock.clone())
            .with_funded_account(alice.clone(), 1_000_000)
            .build()
            .await
            .unwrap();

        let daemon = TestDaemon::new(blockchain, clock);

        // Create and submit transaction
        let tx = TestTransaction {
            hash: Hash::zero(),
            sender: alice.clone(),
            recipient: bob.clone(),
            amount: 1000,
            fee: 100,
            nonce: 1,
        };

        let txid = daemon.submit_transaction(tx).await.unwrap();
        assert_eq!(txid, Hash::zero());

        // Mine block
        let block_hash = daemon.mine_block().await.unwrap();
        assert_ne!(block_hash, Hash::zero());

        // Verify state updated
        assert_eq!(daemon.get_tip_height().await.unwrap(), 1);
        assert_eq!(daemon.get_nonce(&alice).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_daemon_direct_state_access() {
        let clock = Arc::new(SystemClock);
        let blockchain = TestBlockchainBuilder::new()
            .with_clock(clock.clone())
            .with_funded_account_count(5)
            .build()
            .await
            .unwrap();

        let daemon = TestDaemon::new(blockchain, clock);

        // Access blockchain directly for deep assertions
        let accounts = daemon.blockchain().accounts_kv().await.unwrap();
        assert_eq!(accounts.len(), 5);

        let counters = daemon.blockchain().read_counters().await.unwrap();
        assert!(counters.supply > 0);
    }
}
