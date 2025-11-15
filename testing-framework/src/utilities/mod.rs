// File: testing-framework/src/utilities/mod.rs
//
// Testing Utilities
//
// This module provides common utilities for testing TOS blockchain,
// including storage management, test data generation, and helper functions.

/// Storage utilities for creating temporary RocksDB instances in tests
pub mod storage;

/// Failure artifact collection for test debugging and reproduction
pub mod artifacts;

/// Artifact replay utilities for reproducing test failures
pub mod replay;

/// Daemon test helpers for RocksDB storage setup
/// Migrated from deprecated testing-integration package
pub mod daemon_helpers;

// Re-export commonly used utilities
pub use artifacts::{ArtifactCollector, TestArtifact};
pub use daemon_helpers::{
    create_test_rocksdb_storage, create_test_storage_with_funded_accounts, setup_account_rocksdb,
};
pub use replay::{get_replay_command, load_artifact, print_artifact_summary, validate_artifact};
pub use storage::{create_temp_rocksdb, TempRocksDB};
