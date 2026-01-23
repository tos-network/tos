//! BlockWarp trait for advancing chain state by creating blocks.
//!
//! Unlike MockClock which only advances wall-clock time, BlockWarp creates
//! actual blocks that modify blockchain state (topoheight, balances, nonces).
//! In a BlockDAG, time and chain height are independent dimensions.

use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tos_common::crypto::Hash;

use crate::tier1_component::TestTransaction;

/// Target time between blocks in milliseconds (3 seconds).
pub const BLOCK_TIME_MS: u64 = 3000;

/// Error type for block warp operations.
#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum WarpError {
    /// Target topoheight is behind current state
    #[error("target topoheight {target} is behind current {current}")]
    TargetBehindCurrent { target: u64, current: u64 },

    /// Block creation failed during warp
    #[error("block creation failed: {0}")]
    BlockCreationFailed(String),

    /// State transition error during block application
    #[error("state transition error: {0}")]
    StateTransition(String),

    /// Warp would exceed maximum allowed advancement
    #[error("warp of {requested} blocks exceeds maximum {max}")]
    ExceedsMaxWarp { requested: u64, max: u64 },
}

/// Maximum number of blocks that can be warped in a single call.
/// Prevents accidental infinite loops or excessive state creation.
pub const MAX_WARP_BLOCKS: u64 = 100_000;

/// Trait for advancing chain state by creating blocks.
///
/// Implementors create valid blocks that pass full validation and update
/// the blockchain state (topoheight, account balances, mining rewards, etc.).
///
/// # Relationship with MockClock
///
/// | Concern       | MockClock            | BlockWarp              |
/// |---------------|---------------------|------------------------|
/// | What advances | Wall-clock time     | Chain topoheight       |
/// | Side effects  | Wakes sleeping tasks| Creates blocks, state  |
/// | Use case      | Timeout testing     | TX testing, contracts  |
/// | State change  | None                | Balances, nonces, etc  |
#[async_trait]
pub trait BlockWarp: Send + Sync {
    /// Advance chain by N empty blocks, returns new topoheight.
    ///
    /// Each block advances the mock clock by `BLOCK_TIME_MS` and applies
    /// mining rewards to the miner account.
    async fn warp_blocks(&mut self, n: u64) -> Result<u64, WarpError>;

    /// Advance chain to a specific topoheight by creating empty blocks.
    ///
    /// Returns error if target is behind current topoheight.
    async fn warp_to_topoheight(&mut self, target: u64) -> Result<(), WarpError>;

    /// Create a single block containing the given transactions.
    ///
    /// Returns the hash of the created block. Transactions are validated
    /// and applied in order. If any transaction fails, it is still included
    /// in the block but marked as failed in the TxResult.
    async fn create_block_with_txs(&mut self, txs: Vec<TestTransaction>)
        -> Result<Hash, WarpError>;

    /// Get the current topoheight of the chain.
    fn current_topoheight(&self) -> u64;

    /// Mine a single empty block and advance state.
    /// Convenience wrapper around `warp_blocks(1)`.
    async fn mine_empty_block(&mut self) -> Result<u64, WarpError> {
        self.warp_blocks(1).await
    }

    /// Mine multiple blocks, equivalent to `warp_blocks`.
    /// Alias for readability in test code.
    async fn mine_blocks(&mut self, count: u64) -> Result<u64, WarpError> {
        self.warp_blocks(count).await
    }

    /// Get the block time target duration.
    fn block_time_target(&self) -> Duration {
        Duration::from_millis(BLOCK_TIME_MS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_warp_error_display() {
        let err = WarpError::TargetBehindCurrent {
            target: 50,
            current: 100,
        };
        assert!(format!("{}", err).contains("50"));
        assert!(format!("{}", err).contains("100"));
    }

    #[test]
    fn test_block_time_constant() {
        assert_eq!(BLOCK_TIME_MS, 3000);
        assert_eq!(MAX_WARP_BLOCKS, 100_000);
    }
}
