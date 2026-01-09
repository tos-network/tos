//! Builder for TestBlockchain
//!
//! Fluent API for configuring test blockchain instances.

use super::TestBlockchain;
use crate::orchestrator::{Clock, SystemClock};
use crate::utilities::create_temp_rocksdb;
use anyhow::Result;
use std::sync::Arc;
use tos_common::crypto::Hash;

/// Builder for TestBlockchain instances
///
/// # Example
///
/// ```rust,ignore
/// use tos_tck::tier1_component::TestBlockchainBuilder;
///
/// let blockchain = TestBlockchainBuilder::new()
///     .with_clock(clock)
///     .with_funded_account_count(10)
///     .with_default_balance(1_000_000)
///     .build()
///     .await?;
/// ```
pub struct TestBlockchainBuilder {
    clock: Option<Arc<dyn Clock>>,
    funded_accounts: Vec<(Hash, u64)>,
    default_balance: u64,
    seed: Option<u64>,
}

impl TestBlockchainBuilder {
    /// Create new builder with defaults
    pub fn new() -> Self {
        Self {
            clock: None,
            funded_accounts: Vec::new(),
            default_balance: 1_000_000_000_000, // 1000 TOS in nanoTOS
            seed: None,
        }
    }

    /// Set clock implementation
    ///
    /// If not set, uses `SystemClock` by default.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use tos_tck::orchestrator::PausedClock;
    ///
    /// let clock = Arc::new(PausedClock::new());
    /// let builder = TestBlockchainBuilder::new()
    ///     .with_clock(clock);
    /// ```
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Create N funded accounts in genesis with default balance
    ///
    /// Accounts will be generated with sequential IDs starting from 1.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Creates 10 accounts, each with 1000 TOS
    /// let builder = TestBlockchainBuilder::new()
    ///     .with_funded_account_count(10);
    /// ```
    pub fn with_funded_account_count(mut self, count: usize) -> Self {
        self.funded_accounts.clear();

        for i in 1..=count {
            let pubkey = Self::generate_pubkey(i as u8);
            self.funded_accounts.push((pubkey, self.default_balance));
        }

        self
    }

    /// Add a specific funded account in genesis
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let alice = create_test_pubkey(1);
    /// let builder = TestBlockchainBuilder::new()
    ///     .with_funded_account(alice, 5_000_000_000_000); // 5000 TOS
    /// ```
    pub fn with_funded_account(mut self, addr: Hash, balance: u64) -> Self {
        self.funded_accounts.push((addr, balance));
        self
    }

    /// Set default balance for funded accounts created by count
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let builder = TestBlockchainBuilder::new()
    ///     .with_default_balance(10_000_000_000_000) // 10,000 TOS
    ///     .with_funded_account_count(5);
    /// ```
    pub fn with_default_balance(mut self, balance: u64) -> Self {
        self.default_balance = balance;
        self
    }

    /// Set random seed for deterministic account generation
    ///
    /// This is useful for creating reproducible test environments.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let builder = TestBlockchainBuilder::new()
    ///     .with_seed(0x1234567890abcdef);
    /// ```
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Build the TestBlockchain instance
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Temporary storage creation fails
    /// - Blockchain initialization fails
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let blockchain = TestBlockchainBuilder::new()
    ///     .with_funded_account_count(10)
    ///     .build()
    ///     .await?;
    /// ```
    pub async fn build(self) -> Result<TestBlockchain> {
        let clock = self.clock.unwrap_or_else(|| Arc::new(SystemClock));

        // Create temporary storage
        let temp_db = create_temp_rocksdb()?;

        // If no accounts specified, create 1 default account
        let funded_accounts = if self.funded_accounts.is_empty() {
            vec![(Self::generate_pubkey(1), self.default_balance)]
        } else {
            self.funded_accounts
        };

        // Create blockchain
        TestBlockchain::new(clock, temp_db, funded_accounts)
    }

    /// Generate a test public key from an ID
    ///
    /// Creates deterministic public keys for testing purposes.
    fn generate_pubkey(id: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        // Fill rest with pattern for easier debugging
        for (i, byte) in bytes.iter_mut().enumerate().skip(1) {
            *byte = (id.wrapping_mul(i as u8)).wrapping_add(i as u8);
        }
        Hash::new(bytes)
    }
}

impl Default for TestBlockchainBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::orchestrator::PausedClock;

    #[tokio::test]
    async fn test_builder_default() {
        let blockchain = TestBlockchainBuilder::new().build().await.unwrap();

        // Should create 1 default account with default balance
        assert_eq!(blockchain.get_tip_height().await.unwrap(), 0);

        let default_pubkey = TestBlockchainBuilder::generate_pubkey(1);
        assert_eq!(
            blockchain.get_balance(&default_pubkey).await.unwrap(),
            1_000_000_000_000
        );
    }

    #[tokio::test]
    async fn test_builder_with_count() {
        let blockchain = TestBlockchainBuilder::new()
            .with_funded_account_count(5)
            .build()
            .await
            .unwrap();

        // Check all 5 accounts have default balance
        for i in 1..=5 {
            let pubkey = TestBlockchainBuilder::generate_pubkey(i);
            assert_eq!(
                blockchain.get_balance(&pubkey).await.unwrap(),
                1_000_000_000_000
            );
        }
    }

    #[tokio::test]
    async fn test_builder_with_custom_balance() {
        let custom_balance = 5_000_000_000_000u64; // 5000 TOS

        let blockchain = TestBlockchainBuilder::new()
            .with_default_balance(custom_balance)
            .with_funded_account_count(3)
            .build()
            .await
            .unwrap();

        let pubkey = TestBlockchainBuilder::generate_pubkey(1);
        assert_eq!(
            blockchain.get_balance(&pubkey).await.unwrap(),
            custom_balance
        );
    }

    #[tokio::test]
    async fn test_builder_with_paused_clock() {
        let clock = Arc::new(PausedClock::new());

        let blockchain = TestBlockchainBuilder::new()
            .with_clock(clock.clone())
            .build()
            .await
            .unwrap();

        // Verify clock is injected
        let blockchain_clock = blockchain.clock();
        let start = blockchain_clock.now();

        // Advance time
        clock.advance(tokio::time::Duration::from_secs(100)).await;

        let elapsed = blockchain_clock.now() - start;
        assert_eq!(elapsed, tokio::time::Duration::from_secs(100));
    }

    #[tokio::test]
    async fn test_builder_with_specific_account() {
        let mut bytes = [0u8; 32];
        bytes[0] = 0xFF; // Custom ID
        let alice = Hash::new(bytes);

        let custom_balance = 10_000_000_000_000u64;

        let blockchain = TestBlockchainBuilder::new()
            .with_funded_account(alice.clone(), custom_balance)
            .build()
            .await
            .unwrap();

        assert_eq!(
            blockchain.get_balance(&alice).await.unwrap(),
            custom_balance
        );
    }
}
