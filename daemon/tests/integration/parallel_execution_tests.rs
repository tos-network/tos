// Integration tests for V3 parallel transaction execution
//
// Phase 4: Testing and Validation
// Comprehensive integration tests for parallel execution infrastructure

use std::sync::Arc;
use tempdir::TempDir;
use tos_common::{
    block::BlockVersion,
    network::Network,
};
use tos_daemon::core::{
    executor::{ParallelExecutor, get_optimal_parallelism},
    storage::sled::{SledStorage, StorageMode},
};
use tos_environment::Environment;

#[tokio::test]
async fn test_optimal_parallelism_sanity() {
    let parallelism = get_optimal_parallelism();
    assert!(parallelism > 0, "Parallelism should be > 0");
    assert!(parallelism <= 1024, "Parallelism should be reasonable");
    assert_eq!(parallelism, num_cpus::get(), "Should match CPU count");
}

#[tokio::test]
async fn test_parallel_chain_state_initialization() {
    // Create temporary storage
    let temp_dir = TempDir::new("tos_test_parallel_init").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    ).unwrap();

    // Create parallel chain state
    // Wrap storage in Arc<RwLock<S>> to match new signature
    let storage_arc = Arc::new(tokio::sync::RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    let parallel_state = tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
        storage_arc,
        environment,
        0,  // stable_topoheight
        1,  // topoheight
        BlockVersion::V0,
    ).await;

    // Verify state initialization
    assert_eq!(parallel_state.get_burned_supply(), 0);
    assert_eq!(parallel_state.get_gas_fee(), 0);
    assert!(parallel_state.get_modified_nonces().is_empty());
    assert!(parallel_state.get_modified_balances().is_empty());
}

#[tokio::test]
async fn test_parallel_executor_empty_batch() {
    // Create temporary storage
    let temp_dir = TempDir::new("tos_test_empty").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    ).unwrap();

    let storage_arc = Arc::new(tokio::sync::RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    let parallel_state = tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
        storage_arc,
        environment,
        0,
        1,
        BlockVersion::V0,
    ).await;

    // Execute empty batch
    let executor = ParallelExecutor::new();
    let results = executor.execute_batch(parallel_state, vec![]).await;

    // Verify empty results
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn test_parallel_state_getters() {
    // Create temporary storage
    let temp_dir = TempDir::new("tos_test_getters").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    ).unwrap();

    let storage_arc = Arc::new(tokio::sync::RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    let parallel_state = tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
        storage_arc,
        environment,
        0,
        1,
        BlockVersion::V0,
    ).await;

    // Test getter methods
    let nonces = parallel_state.get_modified_nonces();
    assert!(nonces.is_empty(), "Should have no modified nonces initially");

    let balances = parallel_state.get_modified_balances();
    assert!(balances.is_empty(), "Should have no modified balances initially");

    let multisigs = parallel_state.get_modified_multisigs();
    assert!(multisigs.is_empty(), "Should have no modified multisigs initially");

    assert_eq!(parallel_state.get_gas_fee(), 0, "Should have zero gas fees initially");
    assert_eq!(parallel_state.get_burned_supply(), 0, "Should have zero burned supply initially");
}

#[tokio::test]
async fn test_parallel_executor_with_custom_parallelism() {
    // Test executor with different parallelism levels
    let _executor_1 = ParallelExecutor::with_parallelism(1);
    let _executor_4 = ParallelExecutor::with_parallelism(4);
    let _executor_16 = ParallelExecutor::with_parallelism(16);

    // Verify executors were created successfully
    // (Can't test internals directly, but ensure no panic)
    assert!(true);
}

// Note: Additional integration tests for transaction execution will be added
// when the blockchain integration methods are implemented:
// - test_parallel_sequential_equivalence() - Compare results with sequential execution
// - test_parallel_execution_with_conflicts() - Test conflict detection
// - test_parallel_merge_correctness() - Test state merging
// - test_large_batch_parallel() - Test performance with 50+ transactions
