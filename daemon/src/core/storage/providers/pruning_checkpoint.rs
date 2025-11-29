//! Pruning Checkpoint Provider
//!
//! This module provides crash recovery for pruning operations.
//! When pruning is interrupted (crash, power loss, etc.), the checkpoint
//! allows resuming from the last known position.
//!
//! # Checkpoint Phases
//!
//! Pruning progresses through distinct phases:
//! 1. `BlockDeletion` - Deleting block data (headers, txs, mappings)
//! 2. `GhostdagCleanup` - Cleaning GHOSTDAG data for pruned blocks
//! 3. `VersionedDataCleanup` - Cleaning versioned account/contract data
//! 4. `Complete` - Pruning finished successfully
//!
//! # Recovery Process
//!
//! On startup, if a checkpoint exists:
//! 1. Load checkpoint to get target and current position
//! 2. Resume from current position in the appropriate phase
//! 3. Continue until complete
//! 4. Clear checkpoint

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tos_common::{block::TopoHeight, serializer::Serializer};

use crate::core::error::BlockchainError;

/// Pruning operation phase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum PruningPhase {
    /// Deleting block data (headers, transactions, mappings)
    BlockDeletion = 0,
    /// Cleaning GHOSTDAG data for pruned blocks
    GhostdagCleanup = 1,
    /// Cleaning versioned data (balances, nonces, contracts)
    VersionedDataCleanup = 2,
    /// Pruning completed successfully
    Complete = 3,
}

impl Default for PruningPhase {
    fn default() -> Self {
        Self::BlockDeletion
    }
}

impl Serializer for PruningPhase {
    fn write(&self, writer: &mut tos_common::serializer::Writer) {
        writer.write_u8(*self as u8);
    }

    fn read(
        reader: &mut tos_common::serializer::Reader,
    ) -> Result<Self, tos_common::serializer::ReaderError> {
        let value = reader.read_u8()?;
        match value {
            0 => Ok(Self::BlockDeletion),
            1 => Ok(Self::GhostdagCleanup),
            2 => Ok(Self::VersionedDataCleanup),
            3 => Ok(Self::Complete),
            _ => Err(tos_common::serializer::ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1
    }
}

/// Pruning checkpoint for crash recovery
///
/// This checkpoint is persisted to disk during pruning operations
/// and allows recovery if the process is interrupted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningCheckpoint {
    /// Target topoheight to prune until
    pub target_topoheight: TopoHeight,
    /// Current position in the pruning process
    pub current_position: TopoHeight,
    /// Current phase of pruning
    pub phase: PruningPhase,
    /// Start topoheight (where pruning began)
    pub start_topoheight: TopoHeight,
    /// Timestamp when pruning started (for metrics)
    pub started_at: u64,
}

impl PruningCheckpoint {
    /// Create a new pruning checkpoint
    pub fn new(start_topoheight: TopoHeight, target_topoheight: TopoHeight) -> Self {
        Self {
            target_topoheight,
            current_position: start_topoheight,
            phase: PruningPhase::BlockDeletion,
            start_topoheight,
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }

    /// Check if pruning is complete
    pub fn is_complete(&self) -> bool {
        self.phase == PruningPhase::Complete
    }

    /// Update checkpoint position
    pub fn update_position(&mut self, position: TopoHeight) {
        self.current_position = position;
    }

    /// Advance to next phase
    pub fn advance_phase(&mut self) {
        self.phase = match self.phase {
            PruningPhase::BlockDeletion => PruningPhase::GhostdagCleanup,
            PruningPhase::GhostdagCleanup => PruningPhase::VersionedDataCleanup,
            PruningPhase::VersionedDataCleanup => PruningPhase::Complete,
            PruningPhase::Complete => PruningPhase::Complete,
        };
    }

    /// Get progress percentage (0-100)
    pub fn progress_percentage(&self) -> u8 {
        if self.target_topoheight <= self.start_topoheight {
            return 100;
        }

        let total = self.target_topoheight - self.start_topoheight;
        let done = self.current_position.saturating_sub(self.start_topoheight);

        // Each phase is roughly 1/3 of the work
        let phase_weight = match self.phase {
            PruningPhase::BlockDeletion => 0,
            PruningPhase::GhostdagCleanup => 33,
            PruningPhase::VersionedDataCleanup => 66,
            PruningPhase::Complete => 100,
        };

        if self.phase == PruningPhase::Complete {
            return 100;
        }

        let phase_progress = if total > 0 {
            ((done as u128 * 33) / total as u128) as u8
        } else {
            33
        };

        std::cmp::min(phase_weight + phase_progress, 100)
    }
}

impl Serializer for PruningCheckpoint {
    fn write(&self, writer: &mut tos_common::serializer::Writer) {
        writer.write_u64(&self.target_topoheight);
        writer.write_u64(&self.current_position);
        self.phase.write(writer);
        writer.write_u64(&self.start_topoheight);
        writer.write_u64(&self.started_at);
    }

    fn read(
        reader: &mut tos_common::serializer::Reader,
    ) -> Result<Self, tos_common::serializer::ReaderError> {
        let target_topoheight = reader.read_u64()?;
        let current_position = reader.read_u64()?;
        let phase = PruningPhase::read(reader)?;
        let start_topoheight = reader.read_u64()?;
        let started_at = reader.read_u64()?;

        Ok(Self {
            target_topoheight,
            current_position,
            phase,
            start_topoheight,
            started_at,
        })
    }

    fn size(&self) -> usize {
        // 4 u64s + 1 phase byte
        8 + 8 + 1 + 8 + 8
    }
}

/// Provider trait for pruning checkpoint operations
#[async_trait]
pub trait PruningCheckpointProvider {
    /// Get the current pruning checkpoint if one exists
    async fn get_pruning_checkpoint(&self) -> Result<Option<PruningCheckpoint>, BlockchainError>;

    /// Set or update the pruning checkpoint
    async fn set_pruning_checkpoint(
        &mut self,
        checkpoint: &PruningCheckpoint,
    ) -> Result<(), BlockchainError>;

    /// Clear the pruning checkpoint (called when pruning completes)
    async fn clear_pruning_checkpoint(&mut self) -> Result<(), BlockchainError>;

    /// Check if there is an incomplete pruning operation
    async fn has_incomplete_pruning(&self) -> Result<bool, BlockchainError> {
        match self.get_pruning_checkpoint().await? {
            Some(checkpoint) => Ok(!checkpoint.is_complete()),
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_creation() {
        let checkpoint = PruningCheckpoint::new(100, 500);
        assert_eq!(checkpoint.start_topoheight, 100);
        assert_eq!(checkpoint.target_topoheight, 500);
        assert_eq!(checkpoint.current_position, 100);
        assert_eq!(checkpoint.phase, PruningPhase::BlockDeletion);
        assert!(!checkpoint.is_complete());
    }

    #[test]
    fn test_checkpoint_phase_advance() {
        let mut checkpoint = PruningCheckpoint::new(0, 100);

        assert_eq!(checkpoint.phase, PruningPhase::BlockDeletion);

        checkpoint.advance_phase();
        assert_eq!(checkpoint.phase, PruningPhase::GhostdagCleanup);

        checkpoint.advance_phase();
        assert_eq!(checkpoint.phase, PruningPhase::VersionedDataCleanup);

        checkpoint.advance_phase();
        assert_eq!(checkpoint.phase, PruningPhase::Complete);
        assert!(checkpoint.is_complete());

        // Should stay at Complete
        checkpoint.advance_phase();
        assert_eq!(checkpoint.phase, PruningPhase::Complete);
    }

    #[test]
    fn test_checkpoint_progress() {
        let mut checkpoint = PruningCheckpoint::new(0, 100);

        // Start: 0%
        assert_eq!(checkpoint.progress_percentage(), 0);

        // Half through block deletion: ~16%
        checkpoint.update_position(50);
        assert!(checkpoint.progress_percentage() > 0);
        assert!(checkpoint.progress_percentage() < 33);

        // Complete block deletion, start ghostdag cleanup
        checkpoint.update_position(100);
        checkpoint.advance_phase();
        checkpoint.update_position(0);
        assert!(checkpoint.progress_percentage() >= 33);

        // Complete
        checkpoint.phase = PruningPhase::Complete;
        assert_eq!(checkpoint.progress_percentage(), 100);
    }

    #[test]
    fn test_checkpoint_serialization() {
        let checkpoint = PruningCheckpoint {
            target_topoheight: 1000,
            current_position: 500,
            phase: PruningPhase::GhostdagCleanup,
            start_topoheight: 100,
            started_at: 1234567890,
        };

        let bytes = checkpoint.to_bytes();
        let decoded = PruningCheckpoint::from_bytes(&bytes).expect("Failed to decode");

        assert_eq!(decoded.target_topoheight, 1000);
        assert_eq!(decoded.current_position, 500);
        assert_eq!(decoded.phase, PruningPhase::GhostdagCleanup);
        assert_eq!(decoded.start_topoheight, 100);
        assert_eq!(decoded.started_at, 1234567890);
    }

    #[test]
    fn test_pruning_phase_serialization() {
        for phase in [
            PruningPhase::BlockDeletion,
            PruningPhase::GhostdagCleanup,
            PruningPhase::VersionedDataCleanup,
            PruningPhase::Complete,
        ] {
            let bytes = phase.to_bytes();
            let decoded = PruningPhase::from_bytes(&bytes).expect("Failed to decode");
            assert_eq!(decoded, phase);
        }
    }
}
