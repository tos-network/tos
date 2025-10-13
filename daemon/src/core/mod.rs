mod tx_cache;

pub mod config;
pub mod blockchain;
pub mod mempool;
pub mod error;
pub mod blockdag;
pub mod storage;
pub mod difficulty;
pub mod simulator;
pub mod nonce_checker;
pub mod tx_selector;
pub mod state;
pub mod merkle;

pub mod hard_fork;
pub mod ghostdag; // TIP-2 Phase 1: GHOSTDAG implementation
pub mod reachability; // TIP-2 Phase 2: Reachability service
pub mod compact_block_reconstructor; // TIP-2 Phase 2B: Compact blocks
pub mod mining; // TIP-2 Phase 3: Mining optimizations

#[cfg(test)]
mod tests; // Test modules (performance, integration tests, etc.)

pub use tx_cache::*;
pub use compact_block_reconstructor::*;