//! Integration Testing Framework for TOS Blockchain
//!
//! This crate provides utilities and abstractions for writing integration tests
//! for the TOS blockchain. It addresses the sled storage deadlock issues that
//! plague direct storage manipulation in tests.
//!
//! # Key Components
//!
//! - **MockStorage**: In-memory storage backend that avoids sled deadlocks
//! - **TestDaemon**: Wrapper around daemon with automatic lifecycle management
//! - **Test Utilities**: Helper functions for mining, accounts, transactions
//!
//! # Example
//!
//! ```rust,ignore
//! use tos_testing_integration::{MockStorage, setup_account_mock};
//!
//! #[tokio::test]
//! async fn test_parallel_execution() {
//!     let storage = MockStorage::new();
//!     setup_account_mock(&storage, &account_a, 1000, 0);
//!
//!     // Create parallel state (no deadlocks!)
//!     let parallel_state = ParallelChainState::new(
//!         Arc::new(RwLock::new(storage)),
//!         0
//!     ).await.unwrap();
//!
//!     // Test logic...
//! }
//! ```

pub mod daemon;
pub mod storage;
pub mod utils;

// Re-export commonly used types
pub use daemon::TestDaemon;
pub use storage::MockStorage;
pub use utils::{
    accounts::{get_balance_from_storage, get_nonce_from_storage, setup_account_mock},
    blockchain::{mine_block, mine_blocks},
    storage_helpers::{
        // RocksDB storage helpers (recommended for new tests)
        create_test_rocksdb_storage,
        create_test_rocksdb_storage_with_accounts,
        // Sled storage helpers (legacy, for existing tests)
        create_test_storage,
        create_test_storage_with_accounts,
        // Genesis-funded accounts helpers (avoids mining 300+ blocks)
        create_test_storage_with_funded_accounts,
        create_test_storage_with_tos_asset,
        flush_storage_and_wait,
        fund_accounts_at_genesis,
        setup_account_rocksdb,
        setup_account_safe,
    },
    transactions::create_simple_transfer,
};

/// Common test result type
pub type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
