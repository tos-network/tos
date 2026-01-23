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

/// Enhanced cluster configuration types
pub mod cluster_config;
/// Cluster-level operations (mining, transfers, sync)
pub mod operations;
/// Network partition testing
pub mod partition;
/// Transport abstraction with fault injection hooks
pub mod transport;

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

pub use clone::mock_cloned_state;
pub use cluster_config::{ClusterConfig, MiningConfig, NodeConfig, NodeRole, NodeState, SyncMode};
pub use network::NetworkTopology;
pub use operations::{
    create_transfer_tx, mine_and_propagate, run_transaction_sequence, send_and_verify_transfer,
    verify_cluster_consistency,
};
pub use partition::{
    run_partition_test, AsyncFn, Partition, PartitionController, PartitionTestConfig,
    PartitionTestResult,
};
pub use transport::{
    InterceptRule, LocalhostTransport, MessageType, TransportAction, TransportMessage,
    TransportStats,
};
pub use verification::{
    verify_balance_conservation, verify_comprehensive, verify_energy_consistency,
    verify_nonce_monotonicity,
};
pub use waiters::{
    wait_all_tips_equal_with_config, wait_for_new_blocks, wait_for_sync_complete,
    wait_for_tx_confirmed, WaitConfig,
};
