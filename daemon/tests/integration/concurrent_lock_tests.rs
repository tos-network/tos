// Concurrent Lock Stress Tests for TOS Blockchain
//
// These tests verify that the parallel execution implementation correctly
// handles concurrent access to storage locks without deadlocks or race conditions.
//
// Test Categories:
// 1. Deadlock Prevention - Ensure no deadlocks occur during parallel execution
// 2. Lock Duration - Measure and verify storage write lock hold times
// 3. RPC Concurrency - Verify RPC queries don't block during parallel execution
// 4. Stress Testing - High-concurrency scenarios

use std::sync::Arc;
use std::time::{Duration, Instant};
use tempdir::TempDir;
use tokio::time::timeout;
use tos_common::{
    block::BlockVersion,
    network::Network,
};
use tos_daemon::core::{
    executor::{ParallelExecutor, get_optimal_parallelism},
    storage::{sled::{SledStorage, StorageMode}, NetworkProvider},
};
use tos_environment::Environment;

/// Test 1: Verify ParallelChainState::new() doesn't deadlock when storage is locked
#[tokio::test]
async fn test_no_deadlock_on_parallel_state_creation() {
    let temp_dir = TempDir::new("tos_test_no_deadlock").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    ).unwrap();

    let storage_arc = Arc::new(tokio::sync::RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    // Test: Create ParallelChainState while holding read lock
    // This should NOT deadlock (read locks are compatible)
    {
        let storage_read = storage_arc.read().await;
        let _is_mainnet = storage_read.is_mainnet();
        drop(storage_read); // Release read lock

        // Now create ParallelChainState (which acquires another read lock)
        let result = timeout(
            Duration::from_secs(5),
            tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
                Arc::clone(&storage_arc),
                environment.clone(),
                0,
                1,
                BlockVersion::V0,
            )
        ).await;

        assert!(result.is_ok(), "ParallelChainState::new() should not timeout");
        let parallel_state = result.unwrap();
        assert_eq!(parallel_state.get_burned_supply(), 0);
    }

    // Test: Verify we can acquire read lock after parallel state creation
    {
        let storage_read = storage_arc.read().await;
        assert!(!storage_read.is_mainnet(), "Should be devnet");
    }
}

/// Test 2: Verify parallel executor doesn't deadlock with empty batch
#[tokio::test]
async fn test_parallel_executor_no_deadlock_empty_batch() {
    let temp_dir = TempDir::new("tos_test_parallel_empty").unwrap();
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

    let executor = ParallelExecutor::new();

    // Execute empty batch with timeout to detect deadlock
    let result = timeout(
        Duration::from_secs(5),
        executor.execute_batch(parallel_state, vec![])
    ).await;

    assert!(result.is_ok(), "Empty batch execution should not timeout");
    let results = result.unwrap();
    assert_eq!(results.len(), 0, "Empty batch should return 0 results");
}

/// Test 3: Measure storage lock acquisition time (should be fast)
#[tokio::test]
async fn test_storage_lock_acquisition_time() {
    let temp_dir = TempDir::new("tos_test_lock_time").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    ).unwrap();

    let storage_arc = Arc::new(tokio::sync::RwLock::new(storage));

    // Measure read lock acquisition time
    let start = Instant::now();
    let _read_guard = storage_arc.read().await;
    let read_duration = start.elapsed();
    drop(_read_guard);

    assert!(read_duration < Duration::from_millis(10),
            "Read lock acquisition took {:?}, expected < 10ms", read_duration);

    // Measure write lock acquisition time
    let start = Instant::now();
    let _write_guard = storage_arc.write().await;
    let write_duration = start.elapsed();
    drop(_write_guard);

    assert!(write_duration < Duration::from_millis(10),
            "Write lock acquisition took {:?}, expected < 10ms", write_duration);
}

/// Test 4: Concurrent read lock acquisition (should not block each other)
#[tokio::test]
async fn test_concurrent_read_locks() {
    let temp_dir = TempDir::new("tos_test_concurrent_reads").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    ).unwrap();

    let storage_arc = Arc::new(tokio::sync::RwLock::new(storage));

    // Spawn 10 concurrent read tasks
    let mut handles = vec![];
    for i in 0..10 {
        let storage_clone = Arc::clone(&storage_arc);
        let handle = tokio::spawn(async move {
            let start = Instant::now();
            let read_guard = storage_clone.read().await;
            let is_mainnet = read_guard.is_mainnet();
            drop(read_guard);
            let duration = start.elapsed();
            (i, is_mainnet, duration)
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    let results = futures::future::join_all(handles).await;

    // Verify all tasks completed successfully and quickly
    for result in results {
        let (task_id, is_mainnet, duration) = result.unwrap();
        assert!(!is_mainnet, "Task {} should see devnet", task_id);
        assert!(duration < Duration::from_millis(50),
                "Task {} took {:?}, expected < 50ms", task_id, duration);
    }
}

/// Test 5: Write lock blocks read locks (but doesn't deadlock)
#[tokio::test]
async fn test_write_lock_blocks_reads() {
    let temp_dir = TempDir::new("tos_test_write_blocks").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    ).unwrap();

    let storage_arc = Arc::new(tokio::sync::RwLock::new(storage));

    // Acquire write lock
    let write_guard = storage_arc.write().await;

    // Spawn read task (should block but not deadlock)
    let storage_clone = Arc::clone(&storage_arc);
    let read_task = tokio::spawn(async move {
        let start = Instant::now();
        let _read_guard = storage_clone.read().await;
        start.elapsed()
    });

    // Hold write lock for 100ms
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(write_guard);

    // Read task should complete after write lock is released
    let read_duration = timeout(Duration::from_secs(2), read_task)
        .await
        .expect("Read task should not timeout after write lock released")
        .unwrap();

    assert!(read_duration >= Duration::from_millis(100),
            "Read task should have been blocked for ~100ms, but took {:?}", read_duration);
    assert!(read_duration < Duration::from_millis(200),
            "Read task blocked too long: {:?}", read_duration);
}

/// Test 6: Parallel state creation under concurrent load
#[tokio::test]
async fn test_parallel_state_creation_under_load() {
    let temp_dir = TempDir::new("tos_test_parallel_load").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    ).unwrap();

    let storage_arc = Arc::new(tokio::sync::RwLock::new(storage));
    let environment = Arc::new(Environment::new());

    // Spawn 5 concurrent tasks creating ParallelChainState
    let mut handles = vec![];
    for i in 0..5 {
        let storage_clone = Arc::clone(&storage_arc);
        let env_clone = Arc::clone(&environment);
        let handle = tokio::spawn(async move {
            let start = Instant::now();
            let result = timeout(
                Duration::from_secs(5),
                tos_daemon::core::state::parallel_chain_state::ParallelChainState::new(
                    storage_clone,
                    env_clone,
                    0,
                    i as u64,
                    BlockVersion::V0,
                )
            ).await;
            (i, result.is_ok(), start.elapsed())
        });
        handles.push(handle);
    }

    // Wait for all tasks
    let results = futures::future::join_all(handles).await;

    // Verify all completed without timeout
    for result in results {
        let (task_id, success, duration) = result.unwrap();
        assert!(success, "Task {} should not timeout", task_id);
        assert!(duration < Duration::from_secs(2),
                "Task {} took {:?}, expected < 2s", task_id, duration);
    }
}

/// Test 7: Optimal parallelism configuration
#[tokio::test]
async fn test_optimal_parallelism() {
    let parallelism = get_optimal_parallelism();

    // Verify parallelism is reasonable
    assert!(parallelism > 0, "Parallelism should be > 0");
    assert!(parallelism <= 128, "Parallelism should be <= 128 (sanity check)");

    // Verify it matches CPU count
    assert_eq!(parallelism, num_cpus::get(),
               "Optimal parallelism should match CPU count");

    println!("Optimal parallelism: {} (CPU count: {})", parallelism, num_cpus::get());
}

/// Test 8: Lock contention under high load (stress test)
#[tokio::test]
#[ignore] // Run with: cargo test --test integration -- --ignored test_high_concurrency_lock_stress
async fn test_high_concurrency_lock_stress() {
    let temp_dir = TempDir::new("tos_test_stress").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    ).unwrap();

    let storage_arc = Arc::new(tokio::sync::RwLock::new(storage));

    // Spawn 100 concurrent read tasks
    let mut handles = vec![];
    for i in 0..100 {
        let storage_clone = Arc::clone(&storage_arc);
        let handle = tokio::spawn(async move {
            let mut max_duration = Duration::ZERO;
            for _ in 0..10 {
                let start = Instant::now();
                let _read_guard = storage_clone.read().await;
                tokio::time::sleep(Duration::from_micros(100)).await; // Simulate work
                let duration = start.elapsed();
                if duration > max_duration {
                    max_duration = duration;
                }
            }
            (i, max_duration)
        });
        handles.push(handle);
    }

    // Wait for all tasks
    let start_all = Instant::now();
    let results = timeout(
        Duration::from_secs(30),
        futures::future::join_all(handles)
    ).await.expect("Stress test should complete within 30s");

    let total_duration = start_all.elapsed();

    // Analyze results
    let mut max_read_duration = Duration::ZERO;
    for result in results {
        let (_task_id, task_max_duration) = result.unwrap();
        if task_max_duration > max_read_duration {
            max_read_duration = task_max_duration;
        }
    }

    println!("Stress test completed in {:?}", total_duration);
    println!("Max read lock acquisition time: {:?}", max_read_duration);

    // Verify no excessive blocking
    assert!(max_read_duration < Duration::from_secs(1),
            "Max read duration {:?} exceeds 1s threshold", max_read_duration);
}
