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
///
/// # Durability Requirements
///
/// Implementations MUST ensure durability for crash recovery correctness:
///
/// - `set_pruning_checkpoint` MUST use synchronous/durable writes (e.g., fsync).
///   If the method returns `Ok(())`, the checkpoint MUST be persisted to stable
///   storage before any subsequent block deletions occur.
///
/// - `clear_pruning_checkpoint` MUST also be durable to prevent stale checkpoints
///   from causing unnecessary recovery on restart.
///
/// # Checkpoint Semantics
///
/// The `current_position` field in `PruningCheckpoint` represents the **next**
/// topoheight to process, not the last processed one. This invariant ensures
/// that crash recovery never re-deletes already pruned blocks.
///
/// For example:
/// - `current_position = 100` means topoheights 0..100 have been processed
/// - On crash recovery, pruning resumes from topoheight 100
#[async_trait]
pub trait PruningCheckpointProvider {
    /// Get the current pruning checkpoint if one exists
    async fn get_pruning_checkpoint(&self) -> Result<Option<PruningCheckpoint>, BlockchainError>;

    /// Set or update the pruning checkpoint
    ///
    /// # Durability
    ///
    /// This method MUST ensure the checkpoint is durably persisted before returning.
    /// Implementations should use synchronous writes (e.g., RocksDB with `sync: true`
    /// or explicit fsync) to guarantee crash recovery correctness.
    async fn set_pruning_checkpoint(
        &mut self,
        checkpoint: &PruningCheckpoint,
    ) -> Result<(), BlockchainError>;

    /// Clear the pruning checkpoint (called when pruning completes)
    ///
    /// # Durability
    ///
    /// This method MUST ensure the checkpoint removal is durably persisted.
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

    // ============================================================================
    // Checkpoint Semantic Invariant Tests
    // ============================================================================

    /// Test that current_position represents the NEXT topoheight to process.
    ///
    /// Invariant: After processing topoheight T, current_position should be T+1.
    /// This ensures crash recovery never re-deletes already pruned blocks.
    #[test]
    fn test_checkpoint_position_invariant_initial() {
        let checkpoint = PruningCheckpoint::new(100, 500);

        // Invariant: current_position starts at start_topoheight (the first to process)
        assert_eq!(
            checkpoint.current_position, checkpoint.start_topoheight,
            "current_position should equal start_topoheight on creation"
        );
    }

    /// Test that after updating position to simulate processing,
    /// the checkpoint stores the next-to-process value.
    #[test]
    fn test_checkpoint_position_after_processing() {
        let mut checkpoint = PruningCheckpoint::new(0, 100);

        // Simulate processing topoheights 0..50
        // After processing topo 49, the next to process is 50
        checkpoint.update_position(50);

        assert_eq!(
            checkpoint.current_position, 50,
            "After processing 0..50, current_position should be 50 (next to process)"
        );
    }

    /// Test that checkpoint semantics are correct at boundaries.
    #[test]
    fn test_checkpoint_boundary_semantics() {
        let start = 100;
        let end = 200;
        let mut checkpoint = PruningCheckpoint::new(start, end);

        // At start: next to process is start
        assert_eq!(checkpoint.current_position, start);

        // After processing everything: next to process is end
        checkpoint.update_position(end);
        assert_eq!(checkpoint.current_position, end);

        // When current_position == target_topoheight, all blocks in [start, end) are processed
        // So starting from current_position would give empty range [end, end)
        let resume_range = checkpoint.current_position..checkpoint.target_topoheight;
        assert!(
            resume_range.is_empty(),
            "Resume range should be empty when current_position == target_topoheight"
        );
    }

    // ============================================================================
    // Crash Recovery Simulation Tests
    // ============================================================================

    /// Simulates crash recovery scenarios to verify checkpoint semantics.
    ///
    /// The test verifies that after a "crash" at any checkpoint position,
    /// resuming from the saved checkpoint would not cause double processing.
    #[test]
    fn test_crash_recovery_no_double_processing() {
        let start = 0u64;
        let end = 100u64;
        const CHECKPOINT_INTERVAL: u64 = 10;

        // Simulate processing with checkpoint updates at each interval
        // Track which topoheights would be processed after a crash at each point
        for crash_point in (start..end).step_by(CHECKPOINT_INTERVAL as usize) {
            if crash_point == start {
                continue; // Skip the first point (no processing done yet)
            }

            // Simulate: we've processed [start, crash_point) and saved checkpoint
            // The checkpoint should store crash_point as current_position
            // (meaning next to process is crash_point, NOT crash_point-1)
            let mut checkpoint = PruningCheckpoint::new(start, end);
            checkpoint.update_position(crash_point);

            // After crash, resuming should process [crash_point, end)
            let resume_start = checkpoint.current_position;
            let resume_end = checkpoint.target_topoheight;

            // Verify: resume_start should be crash_point
            assert_eq!(
                resume_start, crash_point,
                "Resume should start from crash_point, not re-process already done work"
            );

            // Verify: we're not re-processing any block in [start, crash_point)
            for already_processed in start..crash_point {
                assert!(
                    already_processed < resume_start,
                    "Topoheight {} was already processed but would be re-processed after crash",
                    already_processed
                );
            }

            // Verify: all remaining blocks in [crash_point, end) will be processed
            for remaining in crash_point..end {
                assert!(
                    remaining >= resume_start && remaining < resume_end,
                    "Topoheight {} should be in the resume range",
                    remaining
                );
            }
        }
    }

    /// Test that resuming with current_position == target_topoheight
    /// results in no processing (idempotent resume).
    #[test]
    fn test_resume_is_idempotent_when_complete() {
        let mut checkpoint = PruningCheckpoint::new(0, 100);

        // Simulate: all blocks processed
        checkpoint.update_position(100);

        // The resume range should be empty
        let resume_range = checkpoint.current_position..checkpoint.target_topoheight;
        assert!(
            resume_range.is_empty(),
            "No blocks should be processed when phase is complete"
        );

        // Multiple "resumes" should still be empty
        for _ in 0..5 {
            let range = checkpoint.current_position..checkpoint.target_topoheight;
            assert!(range.is_empty(), "Resume should be idempotent");
        }
    }

    /// Test cross-phase checkpoint semantics.
    ///
    /// When transitioning from one phase to another, the checkpoint
    /// should correctly track position for each phase.
    #[test]
    fn test_cross_phase_checkpoint_semantics() {
        let start = 50u64;
        let end = 150u64;
        let mut checkpoint = PruningCheckpoint::new(start, end);

        // Phase 1: BlockDeletion
        assert_eq!(checkpoint.phase, PruningPhase::BlockDeletion);
        assert_eq!(checkpoint.current_position, start);

        // Simulate completing Phase 1
        checkpoint.update_position(end);
        checkpoint.advance_phase();

        // Phase 2: GhostdagCleanup
        assert_eq!(checkpoint.phase, PruningPhase::GhostdagCleanup);
        // Reset position for new phase (as done in resume_pruning_if_needed)
        checkpoint.update_position(start);

        // Simulate crash during Phase 2 at position 100
        checkpoint.update_position(100);

        // Verify resume semantics for Phase 2
        assert_eq!(checkpoint.current_position, 100);
        assert_eq!(checkpoint.phase, PruningPhase::GhostdagCleanup);

        // Resume would process [100, 150)
        let resume_range = checkpoint.current_position..checkpoint.target_topoheight;
        assert_eq!(resume_range, 100..150);
    }
}
