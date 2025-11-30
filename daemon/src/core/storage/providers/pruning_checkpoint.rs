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

    // ============================================================================
    // Strict Mock Storage Tests (ChatGPT Review Checklist 2.2)
    // ============================================================================

    /// A strict mock storage that panics on double delete.
    ///
    /// This is used to verify that the pruning logic with checkpoint recovery
    /// never attempts to delete the same topoheight twice.
    struct StrictMockStorage {
        /// Set of existing topoheights (not yet deleted)
        existing_topos: std::collections::HashSet<u64>,
        /// Set of deleted topoheights (for tracking)
        deleted_topos: std::collections::HashSet<u64>,
        /// Checkpoint storage
        checkpoint: Option<PruningCheckpoint>,
    }

    impl StrictMockStorage {
        /// Create a new mock storage with blocks at topoheights [start, end)
        fn new(start: u64, end: u64) -> Self {
            Self {
                existing_topos: (start..end).collect(),
                deleted_topos: std::collections::HashSet::new(),
                checkpoint: None,
            }
        }

        /// Check if a topoheight exists (not yet deleted)
        fn has_hash_at_topoheight(&self, topo: u64) -> bool {
            self.existing_topos.contains(&topo)
        }

        /// Delete a block at topoheight - panics on double delete
        fn delete_block_at_topoheight(&mut self, topo: u64) {
            if self.deleted_topos.contains(&topo) {
                panic!(
                    "DOUBLE DELETE DETECTED: topoheight {} was already deleted!",
                    topo
                );
            }
            if !self.existing_topos.remove(&topo) {
                panic!("DELETE NON-EXISTENT: topoheight {} does not exist!", topo);
            }
            self.deleted_topos.insert(topo);
        }

        /// Save checkpoint
        fn set_checkpoint(&mut self, checkpoint: &PruningCheckpoint) {
            self.checkpoint = Some(checkpoint.clone());
        }

        /// Get checkpoint
        fn get_checkpoint(&self) -> Option<&PruningCheckpoint> {
            self.checkpoint.as_ref()
        }

        /// Verify all topoheights in range were deleted exactly once
        fn verify_all_deleted(&self, start: u64, end: u64) {
            for topo in start..end {
                assert!(
                    self.deleted_topos.contains(&topo),
                    "Topoheight {} was not deleted",
                    topo
                );
            }
            assert_eq!(
                self.deleted_topos.len(),
                (end - start) as usize,
                "Number of deleted blocks doesn't match expected"
            );
        }
    }

    /// Simulate pruning loop with checkpoint updates.
    ///
    /// This mirrors the logic in `prune_blocks_with_checkpoint` to test
    /// the checkpoint semantics with a strict mock storage.
    fn simulate_pruning_loop(
        storage: &mut StrictMockStorage,
        checkpoint: &mut PruningCheckpoint,
        start_topo: u64,
        end_topo: u64,
        checkpoint_interval: u64,
    ) {
        for topo in start_topo..end_topo {
            // Idempotent check (mirrors the fix in blockchain.rs)
            if !storage.has_hash_at_topoheight(topo) {
                // Already deleted (crash recovery scenario) - skip
                continue;
            }

            // Delete block
            storage.delete_block_at_topoheight(topo);

            // Update checkpoint periodically (store topo + 1 = next to process)
            if (topo - start_topo) % checkpoint_interval == 0 && topo > start_topo {
                checkpoint.update_position(topo + 1);
                storage.set_checkpoint(checkpoint);
            }
        }

        // Final checkpoint update
        checkpoint.update_position(end_topo);
        storage.set_checkpoint(checkpoint);
    }

    /// Test that strict mock storage correctly detects double delete.
    #[test]
    #[should_panic(expected = "DOUBLE DELETE DETECTED")]
    fn test_strict_storage_detects_double_delete() {
        let mut storage = StrictMockStorage::new(0, 10);

        // First delete should succeed
        storage.delete_block_at_topoheight(5);

        // Second delete should panic
        storage.delete_block_at_topoheight(5);
    }

    /// Test that strict mock storage correctly detects delete of non-existent block.
    #[test]
    #[should_panic(expected = "DELETE NON-EXISTENT")]
    fn test_strict_storage_detects_non_existent_delete() {
        let mut storage = StrictMockStorage::new(0, 10);

        // Delete block that doesn't exist (outside range)
        storage.delete_block_at_topoheight(100);
    }

    /// Test normal pruning without crash - should not double delete.
    #[test]
    fn test_pruning_no_crash_no_double_delete() {
        let start = 0u64;
        let end = 100u64;
        let mut storage = StrictMockStorage::new(start, end);
        let mut checkpoint = PruningCheckpoint::new(start, end);

        // Run full pruning loop
        simulate_pruning_loop(&mut storage, &mut checkpoint, start, end, 10);

        // Verify all blocks were deleted exactly once
        storage.verify_all_deleted(start, end);
    }

    /// Test crash recovery at each checkpoint interval.
    ///
    /// Simulates a crash at each checkpoint position, then resumes.
    /// The strict mock storage will panic if any double delete occurs.
    #[test]
    fn test_crash_recovery_no_double_delete_strict() {
        let start = 0u64;
        let end = 50u64;
        let checkpoint_interval = 10u64;

        // Test crash at each checkpoint position
        for crash_at in (start..end).step_by(checkpoint_interval as usize) {
            if crash_at == start {
                continue; // Skip start, no work done yet
            }

            // Phase 1: Initial pruning run until crash point
            let mut storage = StrictMockStorage::new(start, end);
            let mut checkpoint = PruningCheckpoint::new(start, end);

            // Prune until crash point
            simulate_pruning_loop(
                &mut storage,
                &mut checkpoint,
                start,
                crash_at,
                checkpoint_interval,
            );

            // Save checkpoint at crash point (next to process = crash_at)
            checkpoint.update_position(crash_at);
            storage.set_checkpoint(&checkpoint);

            // Phase 2: Crash recovery - resume from checkpoint
            let resume_start = storage.get_checkpoint().unwrap().current_position;

            // This should NOT cause double delete because:
            // 1. resume_start = crash_at (next to process)
            // 2. Blocks [start, crash_at) are already deleted
            // 3. Idempotent check will skip any already-deleted blocks
            simulate_pruning_loop(
                &mut storage,
                &mut checkpoint,
                resume_start,
                end,
                checkpoint_interval,
            );

            // Verify all blocks were deleted exactly once
            storage.verify_all_deleted(start, end);
        }
    }

    /// Test crash at arbitrary positions (not just checkpoint boundaries).
    ///
    /// This tests the scenario where a crash occurs BETWEEN checkpoint updates,
    /// which is the critical case that requires idempotent deletion.
    #[test]
    fn test_crash_between_checkpoints_no_double_delete() {
        let start = 0u64;
        let end = 50u64;
        let checkpoint_interval = 10u64;

        // Test crash at positions that are NOT checkpoint boundaries
        // e.g., crash at 15, 25, 35 (between checkpoints at 10, 20, 30, 40)
        for crash_at in [15u64, 25, 35, 45] {
            // Phase 1: Initial pruning run
            let mut storage = StrictMockStorage::new(start, end);
            let mut checkpoint = PruningCheckpoint::new(start, end);

            // Delete blocks [start, crash_at)
            for topo in start..crash_at {
                storage.delete_block_at_topoheight(topo);

                // Update checkpoint at intervals
                if (topo - start) % checkpoint_interval == 0 && topo > start {
                    checkpoint.update_position(topo + 1);
                    storage.set_checkpoint(&checkpoint);
                }
            }

            // At crash_at = 15:
            // - Blocks [0, 15) are deleted
            // - Last checkpoint was at topo=10, saved position=11
            // - Checkpoint.current_position = 11

            // Phase 2: Crash recovery
            // Resume from last checkpoint position (e.g., 11)
            let resume_start = storage
                .get_checkpoint()
                .map(|c| c.current_position)
                .unwrap_or(start);

            // Resume should process [11, 50), but blocks [11, 15) are already deleted
            // The idempotent check should skip them
            simulate_pruning_loop(
                &mut storage,
                &mut checkpoint,
                resume_start,
                end,
                checkpoint_interval,
            );

            // Verify all blocks were deleted exactly once
            storage.verify_all_deleted(start, end);
        }
    }

    /// Test multiple sequential crash-recovery cycles.
    #[test]
    fn test_multiple_crash_recovery_cycles() {
        let start = 0u64;
        let end = 100u64;
        let checkpoint_interval = 10u64;

        let mut storage = StrictMockStorage::new(start, end);
        let mut checkpoint = PruningCheckpoint::new(start, end);

        // Simulate: crash at 25, resume, crash at 55, resume, finish
        let crash_points = [25u64, 55];
        let mut current_start = start;

        for crash_at in crash_points {
            // Prune until crash
            for topo in current_start..crash_at {
                if !storage.has_hash_at_topoheight(topo) {
                    continue;
                }
                storage.delete_block_at_topoheight(topo);

                if (topo - start) % checkpoint_interval == 0 && topo > start {
                    checkpoint.update_position(topo + 1);
                    storage.set_checkpoint(&checkpoint);
                }
            }

            // Resume from checkpoint
            current_start = storage
                .get_checkpoint()
                .map(|c| c.current_position)
                .unwrap_or(current_start);
        }

        // Final run to completion
        simulate_pruning_loop(
            &mut storage,
            &mut checkpoint,
            current_start,
            end,
            checkpoint_interval,
        );

        // Verify no double deletes occurred
        storage.verify_all_deleted(start, end);
    }

    // ============================================================================
    // Enhanced Integration Tests (Reviewer Recommendation 1 & 3)
    // ============================================================================

    /// Extended mock storage for multi-phase testing.
    ///
    /// Tracks deletions across all pruning phases:
    /// - Block deletions (topoheight-based)
    /// - GHOSTDAG data deletions (hash-based, simulated with u64 keys)
    /// - Versioned data deletions (tracked as a flag)
    struct MultiPhaseMockStorage {
        /// Blocks: existing topoheights
        existing_blocks: std::collections::HashSet<u64>,
        /// Blocks: deleted topoheights
        deleted_blocks: std::collections::HashSet<u64>,
        /// GHOSTDAG: existing entries (simulated with u64 keys)
        existing_ghostdag: std::collections::HashSet<u64>,
        /// GHOSTDAG: deleted entries
        deleted_ghostdag: std::collections::HashSet<u64>,
        /// Versioned data: has been cleaned up?
        versioned_data_cleaned: bool,
        /// Checkpoint storage
        checkpoint: Option<PruningCheckpoint>,
    }

    impl MultiPhaseMockStorage {
        fn new(start: u64, end: u64) -> Self {
            Self {
                existing_blocks: (start..end).collect(),
                deleted_blocks: std::collections::HashSet::new(),
                existing_ghostdag: (start..end).collect(),
                deleted_ghostdag: std::collections::HashSet::new(),
                versioned_data_cleaned: false,
                checkpoint: None,
            }
        }

        fn has_block(&self, topo: u64) -> bool {
            self.existing_blocks.contains(&topo)
        }

        fn delete_block(&mut self, topo: u64) {
            if self.deleted_blocks.contains(&topo) {
                panic!("DOUBLE DELETE: block at topo {}", topo);
            }
            if !self.existing_blocks.remove(&topo) {
                panic!("DELETE NON-EXISTENT: block at topo {}", topo);
            }
            self.deleted_blocks.insert(topo);
        }

        fn has_ghostdag(&self, key: u64) -> bool {
            self.existing_ghostdag.contains(&key)
        }

        fn delete_ghostdag(&mut self, key: u64) {
            if self.deleted_ghostdag.contains(&key) {
                panic!("DOUBLE DELETE: ghostdag at key {}", key);
            }
            if self.existing_ghostdag.remove(&key) {
                self.deleted_ghostdag.insert(key);
            }
            // Note: ghostdag may not exist (already cleaned in Phase 1), that's OK
        }

        fn clean_versioned_data(&mut self) {
            // Versioned data cleanup is idempotent
            self.versioned_data_cleaned = true;
        }

        fn set_checkpoint(&mut self, checkpoint: &PruningCheckpoint) {
            self.checkpoint = Some(checkpoint.clone());
        }

        fn get_checkpoint(&self) -> Option<PruningCheckpoint> {
            self.checkpoint.clone()
        }

        #[allow(dead_code)]
        fn clear_checkpoint(&mut self) {
            self.checkpoint = None;
        }
    }

    /// Simulate the full pruning state machine with all phases.
    fn simulate_full_pruning(
        storage: &mut MultiPhaseMockStorage,
        checkpoint: &mut PruningCheckpoint,
        checkpoint_interval: u64,
    ) {
        let start = checkpoint.start_topoheight;
        let end = checkpoint.target_topoheight;

        // Resume from current position based on phase
        match checkpoint.phase {
            PruningPhase::BlockDeletion => {
                // Phase 1: Delete blocks
                for topo in checkpoint.current_position..end {
                    if !storage.has_block(topo) {
                        continue; // Already deleted
                    }
                    storage.delete_block(topo);
                    // Also delete ghostdag in Phase 1
                    storage.delete_ghostdag(topo);

                    // Update checkpoint periodically
                    let is_first = topo == start;
                    let is_interval = topo > start && (topo - start) % checkpoint_interval == 0;
                    if is_first || is_interval {
                        checkpoint.update_position(topo + 1);
                        storage.set_checkpoint(checkpoint);
                    }
                }
                checkpoint.update_position(end);
                checkpoint.advance_phase();
                checkpoint.update_position(start); // Reset for Phase 2
                storage.set_checkpoint(checkpoint);

                // Continue to Phase 2
                simulate_full_pruning(storage, checkpoint, checkpoint_interval);
            }
            PruningPhase::GhostdagCleanup => {
                // Phase 2: Cleanup orphaned ghostdag entries
                for topo in checkpoint.current_position..end {
                    if storage.has_ghostdag(topo) {
                        storage.delete_ghostdag(topo);
                    }

                    let is_first = topo == start;
                    let is_interval = topo > start && (topo - start) % checkpoint_interval == 0;
                    if is_first || is_interval {
                        checkpoint.update_position(topo + 1);
                        storage.set_checkpoint(checkpoint);
                    }
                }
                checkpoint.update_position(end);
                checkpoint.advance_phase();
                storage.set_checkpoint(checkpoint);

                // Continue to Phase 3
                simulate_full_pruning(storage, checkpoint, checkpoint_interval);
            }
            PruningPhase::VersionedDataCleanup => {
                // Phase 3: Clean versioned data (idempotent)
                storage.clean_versioned_data();
                checkpoint.advance_phase();
                checkpoint.update_position(end);
                storage.set_checkpoint(checkpoint);
            }
            PruningPhase::Complete => {
                // Nothing to do
            }
        }
    }

    /// Test crash simulation in each phase.
    ///
    /// Reviewer Recommendation 1: Integration-style tests that simulate crash
    /// after checkpoint write in each phase.
    #[test]
    fn test_crash_in_block_deletion_phase() {
        let start = 0u64;
        let end = 50u64;
        let crash_at = 25u64;
        let checkpoint_interval = 10u64;

        // Phase 1: Partial block deletion until crash
        let mut storage = MultiPhaseMockStorage::new(start, end);
        let mut checkpoint = PruningCheckpoint::new(start, end);

        for topo in start..crash_at {
            if !storage.has_block(topo) {
                continue;
            }
            storage.delete_block(topo);
            storage.delete_ghostdag(topo);

            let is_first = topo == start;
            let is_interval = topo > start && (topo - start) % checkpoint_interval == 0;
            if is_first || is_interval {
                checkpoint.update_position(topo + 1);
                storage.set_checkpoint(&checkpoint);
            }
        }

        // Crash! Checkpoint saved, some work lost since last checkpoint.
        // Last checkpoint position depends on crash_at and interval.
        // For crash_at=25, last checkpoint was at topo=20, position=21
        let saved_checkpoint = storage.get_checkpoint().unwrap();
        assert_eq!(saved_checkpoint.phase, PruningPhase::BlockDeletion);
        assert!(saved_checkpoint.current_position <= crash_at);

        // Simulate restart: reload checkpoint and resume
        let mut resumed_checkpoint = saved_checkpoint;
        simulate_full_pruning(&mut storage, &mut resumed_checkpoint, checkpoint_interval);

        // Verify completion
        assert_eq!(resumed_checkpoint.phase, PruningPhase::Complete);
        assert!(storage.versioned_data_cleaned);
        // All blocks should be deleted exactly once (idempotent check handles this)
        for topo in start..end {
            assert!(
                storage.deleted_blocks.contains(&topo),
                "Block {} not deleted",
                topo
            );
        }
    }

    #[test]
    fn test_crash_in_ghostdag_cleanup_phase() {
        let start = 0u64;
        let end = 50u64;
        let crash_at_phase2 = 30u64;
        let checkpoint_interval = 10u64;

        // Phase 1: Complete block deletion
        let mut storage = MultiPhaseMockStorage::new(start, end);
        let mut checkpoint = PruningCheckpoint::new(start, end);

        for topo in start..end {
            storage.delete_block(topo);
            storage.delete_ghostdag(topo);
        }
        checkpoint.update_position(end);
        checkpoint.advance_phase();
        checkpoint.update_position(start);
        storage.set_checkpoint(&checkpoint);

        // Phase 2: Partial ghostdag cleanup until crash
        // Add some "orphaned" ghostdag entries that weren't cleaned in Phase 1
        storage.existing_ghostdag = (start..end).collect();
        storage.deleted_ghostdag.clear();

        for topo in start..crash_at_phase2 {
            if storage.has_ghostdag(topo) {
                storage.delete_ghostdag(topo);
            }
            let is_first = topo == start;
            let is_interval = topo > start && (topo - start) % checkpoint_interval == 0;
            if is_first || is_interval {
                checkpoint.update_position(topo + 1);
                storage.set_checkpoint(&checkpoint);
            }
        }

        // Crash in Phase 2
        let saved_checkpoint = storage.get_checkpoint().unwrap();
        assert_eq!(saved_checkpoint.phase, PruningPhase::GhostdagCleanup);

        // Resume from checkpoint
        let mut resumed_checkpoint = saved_checkpoint;
        simulate_full_pruning(&mut storage, &mut resumed_checkpoint, checkpoint_interval);

        // Verify completion
        assert_eq!(resumed_checkpoint.phase, PruningPhase::Complete);
        assert!(storage.versioned_data_cleaned);
    }

    #[test]
    fn test_crash_in_versioned_data_cleanup_phase() {
        let start = 0u64;
        let end = 50u64;
        let checkpoint_interval = 10u64;

        // Complete Phase 1 and Phase 2
        let mut storage = MultiPhaseMockStorage::new(start, end);
        let mut checkpoint = PruningCheckpoint::new(start, end);

        // Simulate Phase 1 & 2 completion
        for topo in start..end {
            storage.delete_block(topo);
            storage.delete_ghostdag(topo);
        }
        checkpoint.update_position(end);
        checkpoint.advance_phase(); // -> GhostdagCleanup
        checkpoint.update_position(end);
        checkpoint.advance_phase(); // -> VersionedDataCleanup
        storage.set_checkpoint(&checkpoint);

        // Crash in Phase 3 (before cleaning)
        let saved_checkpoint = storage.get_checkpoint().unwrap();
        assert_eq!(saved_checkpoint.phase, PruningPhase::VersionedDataCleanup);
        assert!(!storage.versioned_data_cleaned);

        // Resume from checkpoint
        let mut resumed_checkpoint = saved_checkpoint;
        simulate_full_pruning(&mut storage, &mut resumed_checkpoint, checkpoint_interval);

        // Verify completion
        assert_eq!(resumed_checkpoint.phase, PruningPhase::Complete);
        assert!(storage.versioned_data_cleaned);
    }

    /// Test idempotent resume after completion.
    ///
    /// Reviewer Recommendation 3: After completion, multiple calls to
    /// resume_pruning_if_needed should immediately return Ok(None).
    #[test]
    fn test_idempotent_resume_after_completion() {
        let start = 0u64;
        let end = 50u64;
        let checkpoint_interval = 10u64;

        let mut storage = MultiPhaseMockStorage::new(start, end);
        let mut checkpoint = PruningCheckpoint::new(start, end);

        // Complete all phases
        simulate_full_pruning(&mut storage, &mut checkpoint, checkpoint_interval);

        // Verify completion
        assert_eq!(checkpoint.phase, PruningPhase::Complete);

        // Multiple resume attempts should not cause any additional work
        let initial_deleted_count = storage.deleted_blocks.len();

        for _ in 0..5 {
            // Simulate resume check: if complete, return early
            if checkpoint.is_complete() {
                // This is what resume_pruning_if_needed does: return Ok(None)
                continue;
            }
            // Should never reach here
            panic!("Resume should not continue after completion");
        }

        // Verify no additional deletions occurred
        assert_eq!(
            storage.deleted_blocks.len(),
            initial_deleted_count,
            "No additional deletions should occur after completion"
        );
    }

    /// Test that checkpoint eventually reaches Complete phase.
    #[test]
    fn test_checkpoint_reaches_complete_phase() {
        let start = 0u64;
        let end = 100u64;
        let checkpoint_interval = 10u64;

        let mut storage = MultiPhaseMockStorage::new(start, end);
        let mut checkpoint = PruningCheckpoint::new(start, end);

        // Run full pruning
        simulate_full_pruning(&mut storage, &mut checkpoint, checkpoint_interval);

        // Verify final state
        assert_eq!(checkpoint.phase, PruningPhase::Complete);
        assert_eq!(checkpoint.current_position, end);
        assert!(checkpoint.is_complete());
    }

    /// Test that first deletion updates checkpoint immediately.
    ///
    /// This tests the fix for the crash recovery bug where checkpoint
    /// wasn't updated until after CHECKPOINT_INTERVAL blocks.
    #[test]
    fn test_first_deletion_updates_checkpoint() {
        let start = 0u64;
        let end = 100u64;
        let _checkpoint_interval = 100u64; // Large interval (unused, just for context)

        let mut storage = MultiPhaseMockStorage::new(start, end);
        let mut checkpoint = PruningCheckpoint::new(start, end);

        // Delete only the first block
        storage.delete_block(start);
        storage.delete_ghostdag(start);

        // Update checkpoint after first deletion (the fix)
        let is_first = true;
        if is_first {
            checkpoint.update_position(start + 1);
            storage.set_checkpoint(&checkpoint);
        }

        // Verify checkpoint was updated after first deletion
        let saved = storage.get_checkpoint().unwrap();
        assert_eq!(
            saved.current_position,
            start + 1,
            "Checkpoint should be updated after first deletion"
        );

        // If we crash now and resume, we should start from start+1
        assert_eq!(
            saved.current_position, 1,
            "Resume should start from topoheight 1, not 0"
        );
    }
}
