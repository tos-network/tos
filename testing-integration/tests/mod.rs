//! Migrated parallel execution tests
//!
//! This module was used for tests that were previously ignored due to sled deadlock issues.
//! All tests have been migrated to the new RocksDB-based test framework in daemon/tests/
//!
//! Old test files have been renamed to .old for reference:
//! - helpers.rs.old
//! - migrated_receive_then_spend.rs.old
//! - migrated_multiple_spends.rs.old
//! - migrated_balance_preservation.rs.old
//! - migrated_fee_deduction.rs.old
//! - migrated_double_spend_prevention.rs.old
//!
//! New RocksDB-based tests are located in:
//! - daemon/tests/rocksdb_basic_test.rs
//! - daemon/tests/parallel_execution_security_tests_rocksdb.rs
//! - daemon/tests/miner_reward_tests_rocksdb.rs
//! - daemon/tests/parallel_execution_parity_tests_rocksdb.rs

// No active tests in this module - all migrated to daemon/tests/
