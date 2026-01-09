// File: testing-framework/src/tier3_e2e/mod.rs
//
// Tier 3 End-to-End Testing Components
//
// This module provides E2E testing utilities for multi-node TOS blockchain
// networks, including consensus convergence primitives and network coordination.

pub mod network;
/// Waiter primitives for multi-node consensus convergence
pub mod waiters;

#[cfg(test)]
mod e2e_tests;

#[cfg(test)]
mod advanced_scenarios;

// Re-export commonly used types from tier2
pub use crate::tier2_integration::{Hash, NodeRpc};
