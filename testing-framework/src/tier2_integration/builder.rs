//! TestDaemonBuilder - Fluent API for configuring TestDaemon instances

use super::test_daemon::TestDaemon;
use crate::orchestrator::{Clock, SystemClock};
use crate::tier1_component::TestBlockchainBuilder;
use anyhow::Result;
use std::sync::Arc;
use tos_common::crypto::Hash;

/// Builder for TestDaemon instances with fluent API
///
/// # Example
///
/// ```rust,ignore
/// use tos_testing_framework::tier2_integration::TestDaemonBuilder;
///
/// let daemon = TestDaemonBuilder::new()
///     .with_clock(clock)
///     .with_funded_accounts(10, 1_000_000)
///     .with_default_balance(5_000_000)
///     .build()
///     .await?;
/// ```
pub struct TestDaemonBuilder {
    /// Clock implementation for deterministic time
    clock: Option<Arc<dyn Clock>>,

    /// Funded accounts (address, balance)
    funded_accounts: Vec<(Hash, u64)>,

    /// Default balance for accounts created by count
    default_balance: u64,

    /// Number of funded accounts to create
    funded_account_count: Option<usize>,
}

impl TestDaemonBuilder {
    /// Create new builder with defaults
    ///
    /// Default configuration:
    /// - SystemClock (real time)
    /// - 1 funded account with 1,000 TOS
    pub fn new() -> Self {
        Self {
            clock: None,
            funded_accounts: Vec::new(),
            default_balance: 1_000_000_000_000, // 1000 TOS in nanoTOS
            funded_account_count: None,
        }
    }

    /// Set clock implementation
    ///
    /// If not set, uses `SystemClock` by default.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use tos_testing_framework::orchestrator::PausedClock;
    ///
    /// let clock = Arc::new(PausedClock::new());
    /// let builder = TestDaemonBuilder::new()
    ///     .with_clock(clock);
    /// ```
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Create N funded accounts with default balance
    ///
    /// Accounts will be generated with sequential IDs starting from 1.
    ///
    /// **Note**: This will replace any previously specified account count,
    /// but will not remove individually added accounts via `with_funded_account()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Creates 10 accounts, each with 1000 TOS
    /// let builder = TestDaemonBuilder::new()
    ///     .with_funded_accounts(10);
    /// ```
    pub fn with_funded_accounts(mut self, count: usize) -> Self {
        self.funded_account_count = Some(count);
        self
    }

    /// Add a specific funded account
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let alice = create_test_address(1);
    /// let builder = TestDaemonBuilder::new()
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
    /// let builder = TestDaemonBuilder::new()
    ///     .with_default_balance(10_000_000_000_000) // 10,000 TOS
    ///     .with_funded_accounts(5);
    /// ```
    pub fn with_default_balance(mut self, balance: u64) -> Self {
        self.default_balance = balance;
        self
    }

    /// Build the TestDaemon instance
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Blockchain creation fails
    /// - Storage initialization fails
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let daemon = TestDaemonBuilder::new()
    ///     .with_funded_accounts(10)
    ///     .build()
    ///     .await?;
    /// ```
    pub async fn build(self) -> Result<TestDaemon> {
        let clock = self.clock.unwrap_or_else(|| Arc::new(SystemClock));

        // Build underlying blockchain using TestBlockchainBuilder
        let mut blockchain_builder = TestBlockchainBuilder::new()
            .with_clock(clock.clone())
            .with_default_balance(self.default_balance);

        // Set account count if specified (must be done before adding individual accounts
        // because with_funded_account_count() clears the accounts vector)
        if let Some(count) = self.funded_account_count {
            blockchain_builder = blockchain_builder.with_funded_account_count(count);
        }

        // Add individual funded accounts (these will be appended to the accounts from count)
        for (addr, balance) in self.funded_accounts {
            blockchain_builder = blockchain_builder.with_funded_account(addr, balance);
        }

        let blockchain = blockchain_builder.build().await?;

        Ok(TestDaemon::new(blockchain, clock))
    }
}

impl Default for TestDaemonBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)]

    use super::*;
    use crate::orchestrator::PausedClock;

    fn create_test_address(id: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        Hash::new(bytes)
    }

    #[tokio::test]
    async fn test_builder_default() {
        let daemon = TestDaemonBuilder::new().build().await.unwrap();

        // Should create 1 default account with default balance
        assert_eq!(daemon.get_tip_height().await.unwrap(), 0);
        assert!(daemon.is_running());
    }

    #[tokio::test]
    async fn test_builder_with_funded_accounts() {
        let daemon = TestDaemonBuilder::new()
            .with_funded_accounts(5)
            .build()
            .await
            .unwrap();

        // Verify accounts via blockchain
        let accounts = daemon.blockchain().accounts_kv().await.unwrap();
        assert_eq!(accounts.len(), 5);
    }

    #[tokio::test]
    async fn test_builder_with_custom_balance() {
        let custom_balance = 5_000_000_000_000u64; // 5000 TOS

        let daemon = TestDaemonBuilder::new()
            .with_default_balance(custom_balance)
            .with_funded_accounts(3)
            .build()
            .await
            .unwrap();

        // Verify balance via blockchain
        let accounts = daemon.blockchain().accounts_kv().await.unwrap();
        for (_addr, state) in accounts {
            assert_eq!(state.balance, custom_balance);
        }
    }

    #[tokio::test]
    async fn test_builder_with_specific_account() {
        let alice = create_test_address(0xFF);
        let custom_balance = 10_000_000_000_000u64;

        let daemon = TestDaemonBuilder::new()
            .with_funded_account(alice.clone(), custom_balance)
            .build()
            .await
            .unwrap();

        assert_eq!(daemon.get_balance(&alice).await.unwrap(), custom_balance);
    }

    #[tokio::test]
    async fn test_builder_with_paused_clock() {
        let clock = Arc::new(PausedClock::new());

        let daemon = TestDaemonBuilder::new()
            .with_clock(clock.clone())
            .build()
            .await
            .unwrap();

        // Verify clock is injected
        let daemon_clock = daemon.clock();
        let start = daemon_clock.now();

        // Advance time
        clock.advance(tokio::time::Duration::from_secs(100)).await;

        let elapsed = daemon_clock.now() - start;
        assert_eq!(elapsed, tokio::time::Duration::from_secs(100));
    }

    #[tokio::test]
    async fn test_builder_mixed_accounts() {
        let alice = create_test_address(0xAA);
        let bob = create_test_address(0xBB);

        let daemon = TestDaemonBuilder::new()
            .with_funded_accounts(3) // Add 3 accounts with default balance first
            .with_funded_account(alice.clone(), 1_000_000) // Then add specific accounts
            .with_funded_account(bob.clone(), 2_000_000)
            .build()
            .await
            .unwrap();

        // Verify specific accounts
        assert_eq!(daemon.get_balance(&alice).await.unwrap(), 1_000_000);
        assert_eq!(daemon.get_balance(&bob).await.unwrap(), 2_000_000);

        // Verify total account count
        let accounts = daemon.blockchain().accounts_kv().await.unwrap();
        assert_eq!(accounts.len(), 5); // 3 from count + 2 specific
    }
}
