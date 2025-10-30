//! Migrated parallel execution tests
//!
//! This module contains tests that were previously ignored due to sled deadlock issues.
//! They have been migrated to use MockStorage which avoids these issues.

mod helpers;

// Migrated tests
mod migrated_receive_then_spend;
mod migrated_multiple_spends;
mod migrated_balance_preservation;
mod migrated_fee_deduction;
mod migrated_double_spend_prevention;
