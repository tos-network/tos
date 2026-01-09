//! Doc-test helpers for TOS Testing Framework
//!
//! This module provides simplified helpers to enable doc-tests to run without
//! complex setup. Since this is a testing framework, these helpers are always
//! available.
//!
//! Design philosophy:
//! - Fast: In-memory storage, minimal initialization
//! - Simple: Clear APIs for documentation examples
//! - Complete: Support Tier 1 and Tier 2 examples
//! - Isolated: Each test gets fresh state

use crate::orchestrator::{Clock, PausedClock, SystemClock};
use crate::tier1_component::{TestBlockchain, TestBlockchainBuilder};
use crate::utilities::create_temp_rocksdb;
use anyhow::Result;
use std::future::Future;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tos_common::crypto::Hash;

// Re-export commonly used types for doc-tests
pub use tokio::time::Duration;

/// Minimal blockchain for quick doc-test examples
///
/// This is a simplified version of TestBlockchain optimized for doc-tests.
/// It uses in-memory storage and minimal configuration.
///
/// # Example
///
/// ```rust
/// # use tos_tck::doc_test_helpers::MinimalBlockchain;
/// # tokio_test::block_on(async {
/// let blockchain = MinimalBlockchain::new().await.unwrap();
/// let tip_height = blockchain.get_tip_height().await.unwrap();
/// assert_eq!(tip_height, 0);
/// # });
/// ```
pub struct MinimalBlockchain {
    inner: TestBlockchain,
}

impl MinimalBlockchain {
    /// Create a new minimal blockchain for doc-tests
    ///
    /// # Example
    ///
    /// ```rust
    /// # use tos_tck::doc_test_helpers::MinimalBlockchain;
    /// # tokio_test::block_on(async {
    /// let blockchain = MinimalBlockchain::new().await.unwrap();
    /// # });
    /// ```
    pub async fn new() -> Result<Self> {
        let blockchain = TestBlockchainBuilder::new()
            .with_clock(Arc::new(SystemClock))
            .with_funded_account_count(1)
            .build()
            .await?;

        Ok(Self { inner: blockchain })
    }

    /// Create a minimal blockchain with paused time
    ///
    /// Useful for time-dependent tests in documentation.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use tos_tck::doc_test_helpers::MinimalBlockchain;
    /// # tokio_test::block_on(async {
    /// let (blockchain, clock) = MinimalBlockchain::with_paused_time().await.unwrap();
    /// # });
    /// ```
    pub async fn with_paused_time() -> Result<(Self, Arc<PausedClock>)> {
        let clock = Arc::new(PausedClock::new());
        let blockchain = TestBlockchainBuilder::new()
            .with_clock(clock.clone() as Arc<dyn Clock>)
            .with_funded_account_count(1)
            .build()
            .await?;

        Ok((Self { inner: blockchain }, clock))
    }

    /// Get the current tip height
    pub async fn get_tip_height(&self) -> Result<u64> {
        self.inner.get_tip_height().await
    }

    /// Get reference to the underlying TestBlockchain
    pub fn inner(&self) -> &TestBlockchain {
        &self.inner
    }
}

/// Create a minimal blockchain for doc-tests (convenience function)
///
/// This is a shorthand for `MinimalBlockchain::new()`.
///
/// # Example
///
/// ```rust
/// # use tos_tck::doc_test_helpers::create_minimal_blockchain;
/// # tokio_test::block_on(async {
/// let blockchain = create_minimal_blockchain().await.unwrap();
/// # });
/// ```
pub async fn create_minimal_blockchain() -> Result<MinimalBlockchain> {
    MinimalBlockchain::new().await
}

/// Run an async function in a doc-test context
///
/// This helper creates a tokio runtime and executes the async function,
/// making it easy to write async examples in doc-tests.
///
/// # Example
///
/// ```rust
/// use tos_tck::doc_test_helpers::run_doc_test_async;
///
/// run_doc_test_async(|| async {
///     // Your async code here
///     tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
/// });
/// ```
///
/// # Panics
///
/// This function will panic if the tokio runtime cannot be created.
/// This is acceptable because:
/// 1. This is a testing-only helper function
/// 2. Runtime creation failure is a fatal error in test context
/// 3. Tests should fail loudly rather than silently
#[allow(clippy::panic)]
pub fn run_doc_test_async<F, Fut>(f: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    match Runtime::new() {
        Ok(runtime) => runtime.block_on(f()),
        Err(e) => panic!("Failed to create tokio runtime for doc test: {}", e),
    }
}

/// Create a test account hash for doc-tests
///
/// Generates a deterministic hash based on an account ID.
/// Useful for creating consistent examples.
///
/// # Example
///
/// ```rust
/// use tos_tck::doc_test_helpers::create_test_address;
///
/// let alice = create_test_address(1);
/// let bob = create_test_address(2);
/// assert_ne!(alice, bob);
/// ```
pub fn create_test_address(account_id: u8) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[0] = account_id;
    Hash::new(bytes)
}

/// Create a test pubkey for doc-tests (alias for create_test_address)
///
/// # Example
///
/// ```rust
/// use tos_tck::doc_test_helpers::create_test_pubkey;
///
/// let alice = create_test_pubkey(1);
/// ```
pub fn create_test_pubkey(account_id: u8) -> Hash {
    create_test_address(account_id)
}

/// Create a test transaction hash for doc-tests
///
/// Generates a deterministic transaction hash based on a transaction ID.
///
/// # Example
///
/// ```rust
/// use tos_tck::doc_test_helpers::create_test_tx_hash;
///
/// let tx1 = create_test_tx_hash(1);
/// let tx2 = create_test_tx_hash(2);
/// assert_ne!(tx1, tx2);
/// ```
pub fn create_test_tx_hash(tx_id: u64) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&tx_id.to_le_bytes());
    Hash::new(bytes)
}

/// Create a test block hash for doc-tests
///
/// Generates a deterministic block hash based on a block height.
///
/// # Example
///
/// ```rust
/// use tos_tck::doc_test_helpers::create_test_block_hash;
///
/// let block1 = create_test_block_hash(1);
/// let block2 = create_test_block_hash(2);
/// assert_ne!(block1, block2);
/// ```
pub fn create_test_block_hash(height: u64) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&height.to_le_bytes());
    bytes[8] = 0xFF; // Mark as block hash (different from tx hash)
    Hash::new(bytes)
}

/// Mock RPC client for doc-test examples
///
/// This provides a simplified RPC client interface for documentation examples.
/// It's not meant for real testing, just for showing API usage in docs.
///
/// # Example
///
/// ```rust
/// use tos_tck::doc_test_helpers::MockRpcClient;
///
/// let client = MockRpcClient::new();
/// ```
pub struct MockRpcClient {
    tip_height: u64,
}

impl MockRpcClient {
    /// Create a new mock RPC client
    pub fn new() -> Self {
        Self { tip_height: 0 }
    }

    /// Get the current tip height (mocked)
    pub async fn get_tip_height(&self) -> Result<u64> {
        Ok(self.tip_height)
    }

    /// Get account balance (mocked, always returns 1000 TOS)
    pub async fn get_balance(&self, _address: &Hash) -> Result<u64> {
        Ok(1_000_000_000_000) // 1000 TOS in nanoTOS
    }

    /// Set the mock tip height (for testing)
    pub fn set_tip_height(&mut self, height: u64) {
        self.tip_height = height;
    }
}

impl Default for MockRpcClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Format TOS amount for display in doc-test examples
///
/// Converts nanoTOS to TOS with proper formatting.
///
/// # Example
///
/// ```rust
/// use tos_tck::doc_test_helpers::format_tos;
///
/// let amount = 1_234_567_890_000u64; // 1234.56789 TOS
/// let formatted = format_tos(amount);
/// assert_eq!(formatted, "1234.56789 TOS");
/// ```
pub fn format_tos(nano_tos: u64) -> String {
    let tos = nano_tos as f64 / 1_000_000_000.0;
    format!("{:.5} TOS", tos)
}

/// Parse TOS amount from string (for doc-test examples)
///
/// Converts TOS string to nanoTOS.
///
/// # Example
///
/// ```rust
/// use tos_tck::doc_test_helpers::parse_tos;
///
/// let amount = parse_tos("1234.5").unwrap();
/// assert_eq!(amount, 1_234_500_000_000u64);
/// ```
pub fn parse_tos(tos_str: &str) -> Result<u64> {
    let tos: f64 = tos_str.parse()?;
    Ok((tos * 1_000_000_000.0) as u64)
}

/// Simplified test environment setup for doc-tests
///
/// Creates a complete test environment with:
/// - Temporary RocksDB storage
/// - Paused clock for deterministic time
/// - Funded test accounts
///
/// # Example
///
/// ```rust
/// # use tos_tck::doc_test_helpers::DocTestEnv;
/// # tokio_test::block_on(async {
/// let env = DocTestEnv::new().await.unwrap();
/// let alice = env.get_test_address(1);
/// # });
/// ```
pub struct DocTestEnv {
    _temp_db: crate::utilities::TempRocksDB,
    clock: Arc<PausedClock>,
}

impl DocTestEnv {
    /// Create a new doc-test environment
    pub async fn new() -> Result<Self> {
        let temp_db = create_temp_rocksdb()?;
        let clock = Arc::new(PausedClock::new());

        Ok(Self {
            _temp_db: temp_db,
            clock,
        })
    }

    /// Get the paused clock for time manipulation
    pub fn clock(&self) -> Arc<PausedClock> {
        self.clock.clone()
    }

    /// Get a test address by ID
    pub fn get_test_address(&self, account_id: u8) -> Hash {
        create_test_address(account_id)
    }

    /// Advance time by the specified duration
    pub async fn advance_time(&self, duration: Duration) {
        self.clock.advance(duration).await;
    }
}

/// Wait for a condition to become true (with timeout)
///
/// Useful for doc-test examples that need to wait for state changes.
///
/// # Example
///
/// ```rust
/// use tos_tck::doc_test_helpers::wait_for;
/// use tokio::time::Duration;
/// use std::sync::atomic::{AtomicU32, Ordering};
/// use std::sync::Arc;
///
/// # tokio_test::block_on(async {
/// let counter = Arc::new(AtomicU32::new(0));
/// let counter_clone = counter.clone();
/// wait_for(
///     Duration::from_secs(1),
///     Duration::from_millis(10),
///     move || {
///         let counter = counter_clone.clone();
///         async move {
///             counter.fetch_add(1, Ordering::SeqCst);
///             counter.load(Ordering::SeqCst) >= 5
///         }
///     }
/// ).await.unwrap();
/// assert!(counter.load(Ordering::SeqCst) >= 5);
/// # });
/// ```
pub async fn wait_for<F, Fut>(
    timeout: Duration,
    poll_interval: Duration,
    condition: F,
) -> Result<()>
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    let start = tokio::time::Instant::now();

    loop {
        if condition().await {
            return Ok(());
        }

        if start.elapsed() > timeout {
            anyhow::bail!("Timeout waiting for condition");
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Assert that two values are approximately equal (for doc-tests)
///
/// Useful when testing balances with fees or other scenarios where
/// exact equality is not guaranteed.
///
/// # Example
///
/// ```rust
/// use tos_tck::doc_test_helpers::assert_approx_eq;
///
/// let expected = 1000u64;
/// let actual = 1005u64;
/// assert_approx_eq(expected, actual, 10).unwrap();
/// ```
pub fn assert_approx_eq(expected: u64, actual: u64, tolerance: u64) -> Result<()> {
    let diff = actual.abs_diff(expected);

    if diff > tolerance {
        anyhow::bail!(
            "Values not approximately equal: expected {}, got {}, tolerance {} (diff: {})",
            expected,
            actual,
            tolerance,
            diff
        );
    }

    Ok(())
}

/// Create a minimal test scenario (for scenario doc-tests)
///
/// # Example
///
/// ```rust
/// use tos_tck::doc_test_helpers::create_test_scenario;
///
/// let scenario = create_test_scenario("simple_transfer");
/// assert_eq!(scenario.name, "simple_transfer");
/// ```
pub fn create_test_scenario(name: &str) -> TestScenario {
    TestScenario {
        name: name.to_string(),
        steps: Vec::new(),
    }
}

/// Minimal test scenario structure for doc-tests
#[derive(Debug, Clone)]
pub struct TestScenario {
    /// Scenario name
    pub name: String,
    /// Test steps (simplified)
    pub steps: Vec<String>,
}

impl TestScenario {
    /// Add a step to the scenario
    pub fn add_step(&mut self, step: impl Into<String>) {
        self.steps.push(step.into());
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::assertions_on_constants)]
    #![allow(clippy::disallowed_methods)]

    use super::*;

    #[test]
    fn test_create_test_address() {
        let alice = create_test_address(1);
        let bob = create_test_address(2);
        assert_ne!(alice, bob);
    }

    #[test]
    fn test_create_test_tx_hash() {
        let tx1 = create_test_tx_hash(1);
        let tx2 = create_test_tx_hash(2);
        assert_ne!(tx1, tx2);
    }

    #[test]
    fn test_create_test_block_hash() {
        let block1 = create_test_block_hash(1);
        let block2 = create_test_block_hash(2);
        assert_ne!(block1, block2);

        // Verify block hash is different from tx hash with same ID
        let tx1 = create_test_tx_hash(1);
        assert_ne!(block1, tx1);
    }

    #[test]
    fn test_format_tos() {
        assert_eq!(format_tos(1_000_000_000), "1.00000 TOS");
        assert_eq!(format_tos(1_234_567_890_000), "1234.56789 TOS");
        assert_eq!(format_tos(0), "0.00000 TOS");
    }

    #[test]
    fn test_parse_tos() {
        assert_eq!(parse_tos("1").unwrap(), 1_000_000_000);
        assert_eq!(parse_tos("1234.5").unwrap(), 1_234_500_000_000);
        assert_eq!(parse_tos("0.001").unwrap(), 1_000_000);
    }

    #[test]
    fn test_assert_approx_eq() {
        // Within tolerance
        assert!(assert_approx_eq(1000, 1005, 10).is_ok());
        assert!(assert_approx_eq(1005, 1000, 10).is_ok());
        assert!(assert_approx_eq(1000, 1000, 0).is_ok());

        // Outside tolerance
        assert!(assert_approx_eq(1000, 1020, 10).is_err());
        assert!(assert_approx_eq(1020, 1000, 10).is_err());
    }

    #[tokio::test]
    async fn test_mock_rpc_client() {
        let mut client = MockRpcClient::new();
        assert_eq!(client.get_tip_height().await.unwrap(), 0);

        client.set_tip_height(10);
        assert_eq!(client.get_tip_height().await.unwrap(), 10);

        let alice = create_test_address(1);
        assert_eq!(client.get_balance(&alice).await.unwrap(), 1_000_000_000_000);
    }

    #[tokio::test]
    async fn test_wait_for_success() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();
        let result = wait_for(
            Duration::from_secs(1),
            Duration::from_millis(10),
            move || {
                let counter = counter_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    counter.load(Ordering::SeqCst) >= 3
                }
            },
        )
        .await;

        assert!(result.is_ok());
        assert!(counter.load(Ordering::SeqCst) >= 3);
    }

    #[tokio::test]
    async fn test_wait_for_timeout() {
        let result = wait_for(
            Duration::from_millis(50),
            Duration::from_millis(10),
            || async { false },
        )
        .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_create_test_scenario() {
        let mut scenario = create_test_scenario("test");
        assert_eq!(scenario.name, "test");
        assert!(scenario.steps.is_empty());

        scenario.add_step("step1");
        scenario.add_step("step2");
        assert_eq!(scenario.steps.len(), 2);
    }

    #[tokio::test]
    async fn test_minimal_blockchain() {
        let blockchain = MinimalBlockchain::new().await.unwrap();
        assert_eq!(blockchain.get_tip_height().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_minimal_blockchain_with_paused_time() {
        let (blockchain, clock) = MinimalBlockchain::with_paused_time().await.unwrap();
        assert_eq!(blockchain.get_tip_height().await.unwrap(), 0);

        let start = clock.now();
        clock.advance(Duration::from_secs(100)).await;
        let elapsed = clock.now() - start;
        assert_eq!(elapsed, Duration::from_secs(100));
    }

    #[tokio::test]
    async fn test_doc_test_env() {
        let env = DocTestEnv::new().await.unwrap();
        let alice = env.get_test_address(1);
        let bob = env.get_test_address(2);
        assert_ne!(alice, bob);

        let start = env.clock().now();
        env.advance_time(Duration::from_secs(60)).await;
        let elapsed = env.clock().now() - start;
        assert_eq!(elapsed, Duration::from_secs(60));
    }

    #[test]
    fn test_run_doc_test_async() {
        run_doc_test_async(|| async {
            tokio::time::sleep(Duration::from_millis(1)).await;
        });
    }
}
