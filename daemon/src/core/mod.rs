mod tx_cache;

pub mod blockchain;
pub mod blockdag;
pub mod config;
pub mod difficulty;
pub mod error;
pub mod executor;
pub mod mempool;
pub mod merkle;
pub mod nonce_checker;
pub mod simulator;
pub mod state;
pub mod storage;
pub mod tx_selector;

pub mod bps; // TIP-BPS: Blocks Per Second configuration system
pub mod compact_block_reconstructor; // TIP-2 Phase 2B: Compact blocks
pub mod ghostdag; // TIP-2 Phase 1: GHOSTDAG implementation
pub mod hard_fork;
pub mod mining;
pub mod reachability; // TIP-2 Phase 2: Reachability service // TIP-2 Phase 3: Mining optimizations

#[cfg(test)]
mod tests; // Test modules (performance, integration tests, etc.)

pub use compact_block_reconstructor::*;
pub use tx_cache::*;
