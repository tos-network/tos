//! Tier 1: Component-level testing
//!
//! In-process blockchain testing without RPC/P2P.
//! Fast, deterministic, perfect for unit and component tests.
//!
//! ## Key Features
//!
//! - Clock injection for time control
//! - Real RocksDB storage (RAII cleanup)
//! - Parallel ≡ Sequential execution verification
//! - < 1s per test target
//!
//! ## Example
//!
//! ```rust,ignore
//! use tos_testing_framework::prelude::*;
//!
//! #[tokio::test(start_paused = true)]
//! async fn test_balance_transfer() {
//!     let blockchain = TestBlockchainBuilder::new()
//!         .with_funded_account_count(10)
//!         .build()
//!         .await
//!         .unwrap();
//!
//!     // Test logic here...
//! }
//! ```

mod blockchain;
mod builder;

pub use blockchain::{
    AccountState, BlockchainCounters, TestBlock, TestBlockchain, TestTransaction, PRUNING_DEPTH,
};
pub use builder::TestBlockchainBuilder;
