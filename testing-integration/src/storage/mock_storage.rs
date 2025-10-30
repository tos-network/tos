//! Mock Storage for Testing
//!
//! **IMPORTANT**: This is NOT a full Storage implementation!
//!
//! This module provides a simple test helper for basic account setup in integration tests.
//! It is intentionally minimal and does NOT implement any Storage provider traits.
//!
//! # What This Is
//!
//! - A simple in-memory store for test account setup
//! - Basic balance and nonce tracking
//! - Helper methods for easy test initialization
//!
//! # What This Is NOT
//!
//! - NOT a full Storage implementation
//! - NOT suitable for integration with ParallelChainState or other components that require Storage traits
//! - NOT thread-safe for concurrent access
//! - NOT persistent
//!
//! # Usage
//!
//! Use this ONLY for simple test setup:
//!
//! ```rust,ignore
//! use tos_testing_integration::MockStorage;
//!
//! let storage = MockStorage::new_with_tos_asset();
//! storage.setup_account(&account_a, 1000, 0);
//! storage.setup_account(&account_b, 2000, 0);
//!
//! // Read back for test verification
//! assert_eq!(storage.get_balance(&account_a, &TOS_ASSET), 1000);
//! assert_eq!(storage.get_nonce(&account_a), 0);
//! ```
//!
//! # For Real Storage Operations
//!
//! If you need actual Storage provider implementations for testing:
//! - Use the helpers in `utils/storage_helpers.rs`
//! - Use TestDaemon which provides a real Storage instance
//! - Use the full blockchain infrastructure, not this mock

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

use tos_common::{
    config::TOS_ASSET,
    crypto::{Hash, PublicKey},
};

/// Simple mock storage for test account setup
///
/// This is a minimal test helper that stores balances and nonces in memory.
/// It does NOT implement any Storage provider traits and should NOT be used
/// as a drop-in replacement for real Storage.
///
/// **Use this ONLY for basic test setup, not for integration with ParallelChainState
/// or other components that require Storage traits.**
#[derive(Clone)]
pub struct MockStorage {
    // Simple flat maps - no topoheight versioning
    balances: Arc<RwLock<HashMap<(PublicKey, Hash), u64>>>,
    nonces: Arc<RwLock<HashMap<PublicKey, u64>>>,
}

impl MockStorage {
    /// Create a new empty MockStorage
    pub fn new() -> Self {
        Self {
            balances: Arc::new(RwLock::new(HashMap::new())),
            nonces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new MockStorage (TOS asset exists by default, no need to register)
    ///
    /// This is the recommended constructor for most tests.
    pub fn new_with_tos_asset() -> Self {
        // Note: We don't actually register the TOS asset here since we removed
        // asset tracking. The TOS_ASSET constant is available for use, and that's
        // sufficient for simple balance tracking.
        Self::new()
    }

    /// Setup account with initial balance and nonce
    ///
    /// This is the primary helper method for test setup.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let storage = MockStorage::new_with_tos_asset();
    /// storage.setup_account(&account_a, 1000, 0);
    /// storage.setup_account(&account_b, 2000, 0);
    /// ```
    pub fn setup_account(&self, account: &PublicKey, balance: u64, nonce: u64) {
        // Set balance for TOS asset
        {
            let mut balances = self.balances.write();
            balances.insert((account.clone(), TOS_ASSET.clone()), balance);
        }

        // Set nonce
        {
            let mut nonces = self.nonces.write();
            nonces.insert(account.clone(), nonce);
        }
    }

    /// Get balance for account and asset
    ///
    /// Returns 0 if no balance is set.
    pub fn get_balance(&self, account: &PublicKey, asset: &Hash) -> u64 {
        self.balances
            .read()
            .get(&(account.clone(), asset.clone()))
            .copied()
            .unwrap_or(0)
    }

    /// Get nonce for account
    ///
    /// Returns 0 if no nonce is set.
    pub fn get_nonce(&self, account: &PublicKey) -> u64 {
        self.nonces
            .read()
            .get(account)
            .copied()
            .unwrap_or(0)
    }

    /// Clear all state (for test cleanup)
    pub fn clear(&self) {
        self.balances.write().clear();
        self.nonces.write().clear();
    }
}

impl Default for MockStorage {
    fn default() -> Self {
        Self::new_with_tos_asset()
    }
}
