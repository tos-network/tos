//! Tier 1 Component Tests
//!
//! Component-level tests using TestBlockchain (no RPC/P2P overhead)
//!
//! Characteristics:
//! - Uses TestBlockchain with clock injection
//! - Real storage (RocksDB) with RAII cleanup
//! - Tests invariants after operations
//! - Fast execution (< 1s per test)
//! - Deterministic and reproducible
//!
//! Test Organization:
//! - simple_transfer_test: Basic transfer functionality
//! - batch_transfers_test: Multiple transfers in one block
//! - scenario_driven_test: YAML scenario execution
//! - parallel_sequential_equivalence_test: Parallel ≡ Sequential
//! - miner_reward_test: Block reward handling
//! - nonce_management_test: Nonce increment/rollback
//! - conflict_detection_test: Transaction conflict detection
//! - balance_conservation_test: Supply conservation invariant
//! - deterministic_time_test: Clock abstraction
//! - edge_cases_test: Boundary conditions

mod balance_conservation_test;
mod batch_transfers_test;
mod conflict_detection_test;
mod deterministic_time_test;
mod edge_cases_test;
mod miner_reward_test;
mod nonce_management_test;
mod parallel_sequential_equivalence_test;
mod scenario_driven_test;
mod simple_transfer_test;
