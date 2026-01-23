//! Tier 1.5: ChainClient direct blockchain access layer.
//!
//! This module provides a high-level testing API that operates directly
//! on blockchain state without network overhead. It sits between:
//! - Tier 1 (TestBlockchain): In-memory, no validation pipeline
//! - Tier 2 (TestDaemon): Full daemon with RPC
//!
//! Tier 1.5 provides:
//! - `ChainClient`: Direct state access with full transaction pipeline
//! - `ContractTest`: Builder-pattern contract testing harness
//! - `BlockWarp`: Fast chain advancement by creating blocks
//! - `FeatureSet`: Protocol upgrade testing via feature gates
//! - `ConfirmationDepth`: Transaction finality levels
//!
//! # Architecture
//! ```text
//! ┌───────────────────────────────────────┐
//! │          ContractTest (Builder)        │
//! │  new() → add_account() → start()     │
//! └─────────────────┬─────────────────────┘
//!                   │
//! ┌─────────────────▼─────────────────────┐
//! │            ChainClient                 │
//! │  process_transaction() → TxResult      │
//! │  simulate_transaction()                │
//! │  get_balance(), get_nonce()            │
//! │  force_set_balance() [test override]   │
//! └─────────────────┬─────────────────────┘
//!                   │ implements BlockWarp
//! ┌─────────────────▼─────────────────────┐
//! │         TestBlockchain (Tier 1)        │
//! │  In-memory state, O(1) counters       │
//! └───────────────────────────────────────┘
//! ```

pub mod block_warp;
pub mod chain_client;
pub mod chain_client_config;
pub mod confirmation;
pub mod contract_test;
pub mod features;
pub mod tx_result;

// Re-export primary types for convenience
pub use block_warp::{BlockWarp, WarpError, BLOCK_TIME_MS, MAX_WARP_BLOCKS};
pub use chain_client::{ChainClient, TransactionType};
pub use chain_client_config::{AutoMineConfig, ChainClientConfig, GenesisAccount, GenesisContract};
pub use confirmation::ConfirmationDepth;
pub use contract_test::{ContractTest, ContractTestContext};
pub use features::{Feature, FeatureBase, FeatureRegistry, FeatureSet};
pub use tx_result::{
    CallDeposit, ContractEvent, InnerCall, SimulationResult, StateChange, StateDiff,
    TransactionError, TxResult,
};
