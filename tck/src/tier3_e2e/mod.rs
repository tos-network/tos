// File: testing-framework/src/tier3_e2e/mod.rs
//
// Tier 3 End-to-End Testing Components
//
// This module provides E2E testing utilities for multi-node TOS blockchain
// networks, including consensus convergence primitives and network coordination.

pub mod network;
/// Waiter primitives for multi-node consensus convergence
pub mod waiters;

/// Byzantine fault injection for adversarial testing
pub mod byzantine;
/// Live network state cloning for fork testing
pub mod clone;
/// Confirmation depth queries for multi-node testing
pub mod confirmation;
/// Node restart and recovery testing
pub mod restart;
/// Multi-layer state verification across cluster nodes
pub mod verification;

#[cfg(test)]
mod e2e_tests;

#[cfg(test)]
mod advanced_scenarios;

// Re-export commonly used types from tier2
pub use crate::tier2_integration::{Hash, NodeRpc};

// Re-export Phase 8 types
pub use byzantine::{BlockInvalidity, DropTarget, FaultInjector, FaultStats, NodeFaultType};
pub use clone::{CloneConfig, ClonedAccount, ClonedContract, ClonedState};
pub use confirmation::TxConfirmation;
pub use restart::{PreStopState, RestartMode};
