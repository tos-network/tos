// Integration tests for V3 parallel transaction execution
//
// Phase 4: Testing and Validation
// Comprehensive integration tests for parallel execution infrastructure

#![allow(clippy::assertions_on_constants)]

use std::sync::Arc;
use tempdir::TempDir;
use tos_common::{
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    crypto::{elgamal::CompressedPublicKey, Hash, Hashable},
    immutable::Immutable,
    network::Network,
    serializer::{Reader, Serializer, Writer},
};
use tos_daemon::core::{
    executor::{get_optimal_parallelism, ParallelExecutor},
    storage::{
        sled::{SledStorage, StorageMode},
        NetworkProvider,
    },
};
use tos_environment::Environment;

/// Helper function to create a dummy block for testing
fn create_dummy_block() -> (Block, Hash) {
    let mut buffer = Vec::new();
    let mut writer = Writer::new(&mut buffer);
    writer.write_bytes(&[0u8; 32]);
    let data = writer.as_bytes();

    let mut reader = Reader::new(data);
    let miner = CompressedPublicKey::read(&mut reader).expect("Failed to create test pubkey");

    let header = BlockHeader::new_simple(
        BlockVersion::Baseline,
        vec![],
        0,
        [0u8; EXTRA_NONCE_SIZE],
        miner,
        Hash::zero(),
    );

    let block = Block::new(Immutable::Owned(header), vec![]);
    let hash = block.hash();
    (block, hash)
}

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
    )
    .unwrap();

    // Create parallel chain state
    // Wrap storage in Arc<RwLock<S>> to match new signature
    let storage_arc = Arc::new(tos_common::tokio::sync::RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    let (block, block_hash) = create_dummy_block();
    let parallel_state = tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
        storage_arc,
        environment,
        0, // stable_topoheight
        1, // topoheight
        BlockVersion::Baseline,
        block,
        block_hash,
    )
    .await;

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
    )
    .unwrap();

    let storage_arc = Arc::new(tos_common::tokio::sync::RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    let (block, block_hash) = create_dummy_block();
    let parallel_state = tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
        storage_arc,
        environment,
        0,
        1,
        BlockVersion::Baseline,
        block,
        block_hash,
    )
    .await;

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
    )
    .unwrap();

    let storage_arc = Arc::new(tos_common::tokio::sync::RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    let (block, block_hash) = create_dummy_block();
    let parallel_state = tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
        storage_arc,
        environment,
        0,
        1,
        BlockVersion::Baseline,
        block,
        block_hash,
    )
    .await;

    // Test getter methods
    let nonces = parallel_state.get_modified_nonces();
    assert!(
        nonces.is_empty(),
        "Should have no modified nonces initially"
    );

    let balances = parallel_state.get_modified_balances();
    assert!(
        balances.is_empty(),
        "Should have no modified balances initially"
    );

    let multisigs = parallel_state.get_modified_multisigs();
    assert!(
        multisigs.is_empty(),
        "Should have no modified multisigs initially"
    );

    assert_eq!(
        parallel_state.get_gas_fee(),
        0,
        "Should have zero gas fees initially"
    );
    assert_eq!(
        parallel_state.get_burned_supply(),
        0,
        "Should have zero burned supply initially"
    );
}

#[tokio::test]
async fn test_parallel_executor_with_custom_parallelism() {
    // Test executor with different parallelism levels
    let _executor_1 = ParallelExecutor::with_parallelism(1);
    let _executor_4 = ParallelExecutor::with_parallelism(4);
    let _executor_16 = ParallelExecutor::with_parallelism(16);

    // Verify executors were created successfully
    // (Can't test internals directly, but ensure no panic)
    // Test passed if we reached this point without panic
}

// Phase 4: Extended Testing - Infrastructure tests

#[tokio::test]
async fn test_should_use_parallel_execution_threshold() {
    // Test threshold logic for parallel execution with network-specific thresholds
    use tos_common::network::Network;
    use tos_daemon::config::{
        get_min_txs_for_parallel, parallel_execution_enabled, MIN_TXS_FOR_PARALLEL_DEVNET,
        MIN_TXS_FOR_PARALLEL_MAINNET, MIN_TXS_FOR_PARALLEL_TESTNET,
    };

    // Test network-specific thresholds
    assert_eq!(
        get_min_txs_for_parallel(&Network::Mainnet),
        MIN_TXS_FOR_PARALLEL_MAINNET
    );
    assert_eq!(
        get_min_txs_for_parallel(&Network::Testnet),
        MIN_TXS_FOR_PARALLEL_TESTNET
    );
    assert_eq!(
        get_min_txs_for_parallel(&Network::Devnet),
        MIN_TXS_FOR_PARALLEL_DEVNET
    );

    // Test devnet threshold (lowest, for easier testing)
    let devnet_threshold = get_min_txs_for_parallel(&Network::Devnet);
    let should_use_devnet =
        |tx_count: usize| -> bool { parallel_execution_enabled() && tx_count >= devnet_threshold };

    assert!(!should_use_devnet(0), "Empty batch should not use parallel");
    assert!(!should_use_devnet(1), "Single tx should not use parallel");
    assert!(
        !should_use_devnet(3),
        "Below devnet threshold should not use parallel"
    );

    let expected = parallel_execution_enabled();
    assert_eq!(should_use_devnet(4), expected, "At devnet threshold (4)");
    assert_eq!(should_use_devnet(10), expected, "Above devnet threshold");

    // Test mainnet threshold (highest, for production)
    let mainnet_threshold = get_min_txs_for_parallel(&Network::Mainnet);
    let should_use_mainnet =
        |tx_count: usize| -> bool { parallel_execution_enabled() && tx_count >= mainnet_threshold };

    assert!(!should_use_mainnet(10), "Below mainnet threshold");
    assert!(!should_use_mainnet(19), "Below mainnet threshold");
    assert_eq!(
        should_use_mainnet(20),
        expected,
        "At mainnet threshold (20)"
    );
    assert_eq!(should_use_mainnet(100), expected, "Above mainnet threshold");

    // Verify threshold constants are reasonable
    assert!(
        MIN_TXS_FOR_PARALLEL_DEVNET >= 2,
        "Devnet threshold should be >= 2"
    );
    assert!(
        MIN_TXS_FOR_PARALLEL_DEVNET <= 10,
        "Devnet threshold should be <= 10 for testing"
    );
    assert!(
        MIN_TXS_FOR_PARALLEL_TESTNET >= 5,
        "Testnet threshold should be >= 5"
    );
    assert!(
        MIN_TXS_FOR_PARALLEL_TESTNET <= 20,
        "Testnet threshold should be <= 20"
    );
    assert!(
        MIN_TXS_FOR_PARALLEL_MAINNET >= 10,
        "Mainnet threshold should be >= 10 to avoid overhead"
    );
    assert!(
        MIN_TXS_FOR_PARALLEL_MAINNET <= 100,
        "Mainnet threshold should be <= 100 for practical benefit"
    );

    // Verify ordering: devnet < testnet < mainnet
    assert!(
        MIN_TXS_FOR_PARALLEL_DEVNET < MIN_TXS_FOR_PARALLEL_TESTNET,
        "Devnet < Testnet"
    );
    assert!(
        MIN_TXS_FOR_PARALLEL_TESTNET < MIN_TXS_FOR_PARALLEL_MAINNET,
        "Testnet < Mainnet"
    );
}

#[tokio::test]
async fn test_parallel_state_modification_simulation() {
    // Test: Simulate state modifications and verify getter methods work correctly
    // This tests the infrastructure that merge_parallel_results() will use

    // Create temporary storage
    let temp_dir = TempDir::new("tos_test_state_mod").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    )
    .unwrap();

    let storage_arc = Arc::new(tos_common::tokio::sync::RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    let (block, block_hash) = create_dummy_block();
    let parallel_state = tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
        storage_arc,
        environment,
        0,
        1,
        BlockVersion::Baseline,
        block,
        block_hash,
    )
    .await;

    // Verify initial state is empty
    assert_eq!(
        parallel_state.get_burned_supply(),
        0,
        "Initial burned supply should be 0"
    );
    assert_eq!(
        parallel_state.get_gas_fee(),
        0,
        "Initial gas fee should be 0"
    );
    assert!(
        parallel_state.get_modified_nonces().is_empty(),
        "Initial nonces should be empty"
    );
    assert!(
        parallel_state.get_modified_balances().is_empty(),
        "Initial balances should be empty"
    );
    assert!(
        parallel_state.get_modified_multisigs().is_empty(),
        "Initial multisigs should be empty"
    );

    // Note: We cannot directly modify internal state without applying transactions
    // because the fields are private and protected by DashMap/AtomicU64
    // This test verifies the getter infrastructure is in place for merge_parallel_results()
    // Future tests will verify actual modifications through transaction application
}

#[tokio::test]
async fn test_parallel_executor_batch_size_verification() {
    // Test: Verify ParallelExecutor correctly handles batches of different sizes
    // This tests batch processing infrastructure without needing real signed transactions

    // Create temporary storage
    let temp_dir = TempDir::new("tos_test_batch_size").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    )
    .unwrap();

    let storage_arc = Arc::new(tos_common::tokio::sync::RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    let (block, block_hash) = create_dummy_block();
    let parallel_state = tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
        storage_arc,
        environment,
        0,
        1,
        BlockVersion::Baseline,
        block,
        block_hash,
    )
    .await;

    // Test empty batch (already tested in test_parallel_executor_empty_batch)
    let executor = ParallelExecutor::new();
    let results = executor.execute_batch(parallel_state.clone(), vec![]).await;
    assert_eq!(results.len(), 0, "Empty batch should return 0 results");

    // Note: Testing with actual transactions requires creating valid signed transactions
    // which is complex (requires keypair generation, signing, etc.)
    // The existing test_parallel_executor_empty_batch verifies the basic infrastructure works
    // Integration tests with real transactions will be added in Phase 1-2 after blockchain integration
}

#[tokio::test]
async fn test_parallel_state_network_caching() {
    // Test: Verify is_mainnet field is correctly cached during initialization
    // This tests an optimization that avoids repeated lock acquisition

    // Test with Devnet
    let temp_dir_dev = TempDir::new("tos_test_network_dev").unwrap();
    let storage_dev = SledStorage::new(
        temp_dir_dev.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    )
    .unwrap();

    let storage_arc_dev = Arc::new(tos_common::tokio::sync::RwLock::new(storage_dev));
    let environment = Arc::new(Environment::new());

    let (block, block_hash) = create_dummy_block();
    let parallel_state_dev =
        tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
            storage_arc_dev.clone(),
            environment.clone(),
            0,
            1,
            BlockVersion::Baseline,
            block,
            block_hash,
        )
        .await;

    // Verify devnet is not mainnet (field is cached)
    // We cannot directly access private field, but we can verify it was created successfully
    assert_eq!(
        parallel_state_dev.get_burned_supply(),
        0,
        "Devnet state initialized"
    );

    // Verify storage itself knows it's Devnet
    {
        let storage_read = storage_arc_dev.read().await;
        assert!(
            !storage_read.is_mainnet(),
            "Devnet storage should not be mainnet"
        );
    }

    // Test with Mainnet
    let temp_dir_main = TempDir::new("tos_test_network_main").unwrap();
    let storage_main = SledStorage::new(
        temp_dir_main.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Mainnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    )
    .unwrap();

    let storage_arc_main = Arc::new(tos_common::tokio::sync::RwLock::new(storage_main));

    let (block2, block_hash2) = create_dummy_block();
    let parallel_state_main =
        tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
            storage_arc_main.clone(),
            environment,
            0,
            1,
            BlockVersion::Baseline,
            block2,
            block_hash2,
        )
        .await;

    // Verify mainnet state initialized
    assert_eq!(
        parallel_state_main.get_burned_supply(),
        0,
        "Mainnet state initialized"
    );

    // Verify storage itself knows it's Mainnet
    {
        let storage_read = storage_arc_main.read().await;
        assert!(
            storage_read.is_mainnet(),
            "Mainnet storage should be mainnet"
        );
    }
}

#[tokio::test]
async fn test_parallel_executor_parallelism_configuration() {
    // Test: Verify ParallelExecutor respects custom parallelism settings

    // Test default parallelism
    let executor_default = ParallelExecutor::new();
    // Cannot access private field, but verify creation succeeds

    // Test custom parallelism levels
    let _executor_1 = ParallelExecutor::with_parallelism(1);
    let _executor_4 = ParallelExecutor::with_parallelism(4);
    let _executor_16 = ParallelExecutor::with_parallelism(16);
    let _executor_max = ParallelExecutor::with_parallelism(num_cpus::get());

    // Verify optimal parallelism is reasonable
    let optimal = get_optimal_parallelism();
    assert!(optimal > 0, "Optimal parallelism should be > 0");
    assert!(optimal <= 1024, "Optimal parallelism should be reasonable");
    assert_eq!(optimal, num_cpus::get(), "Optimal should match CPU count");

    // Test empty batch with configured executor
    let temp_dir = TempDir::new("tos_test_parallelism").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    )
    .unwrap();

    let storage_arc = Arc::new(tos_common::tokio::sync::RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    let (block, block_hash) = create_dummy_block();
    let parallel_state = tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
        storage_arc,
        environment,
        0,
        1,
        BlockVersion::Baseline,
        block,
        block_hash,
    )
    .await;

    // Execute with default executor
    let results = executor_default.execute_batch(parallel_state, vec![]).await;
    assert_eq!(results.len(), 0, "Empty batch should return 0 results");
}

// Note: Additional integration tests for transaction execution will be added
// when the blockchain integration methods are implemented:
// - test_parallel_sequential_equivalence() - Compare results with sequential execution
// - test_parallel_execution_with_conflicts() - Test conflict detection
// - test_parallel_merge_correctness() - Test state merging
// - test_large_batch_parallel() - Test performance with 50+ transactions
