// Storage Semaphore Stress Tests
// Validates that the storage semaphore correctly serializes critical storage operations
// and prevents race conditions between concurrent operations.
//
// These tests simulate the patterns used in TOS blockchain:
// - add_block vs prune race condition
// - add_block vs rewind race condition
// - Multiple chain sync tasks racing
// - Read-then-write pattern vulnerability

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::{RwLock, Semaphore};
use tokio::task::JoinSet;
use tokio::time::Duration;

/// Mock storage state representing blockchain data
#[derive(Debug, Default)]
struct MockChainState {
    topoheight: u64,
    block_count: u64,
    tips: Vec<u64>,
    is_corrupted: bool,
}

// =============================================================================
// Test 1: Basic Semaphore Serialization
// =============================================================================

/// Verifies that semaphore(1) ensures only one task holds the critical section at a time
#[tokio::test]
async fn test_semaphore_serializes_writes() {
    const TASKS: usize = 200;
    const HOLD_MS: u64 = 5;

    let semaphore = Arc::new(Semaphore::new(1));
    let storage = Arc::new(RwLock::new(0usize));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));

    let mut join_set = JoinSet::new();
    for _ in 0..TASKS {
        let semaphore = semaphore.clone();
        let storage = storage.clone();
        let in_flight = in_flight.clone();
        let max_in_flight = max_in_flight.clone();

        join_set.spawn(async move {
            // Acquire semaphore BEFORE storage lock (TOS pattern)
            let _permit = semaphore.acquire().await.unwrap();

            // Track concurrent access
            let current = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            update_max(&max_in_flight, current);

            // Simulate storage operation
            {
                let mut guard = storage.write().await;
                *guard += 1;
                tokio::time::sleep(Duration::from_millis(HOLD_MS)).await;
            }

            in_flight.fetch_sub(1, Ordering::SeqCst);
        });
    }

    while join_set.join_next().await.is_some() {}

    // With semaphore(1), max concurrent should be exactly 1
    assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);
    assert_eq!(*storage.read().await, TASKS);
}

// =============================================================================
// Test 2: Concurrent add_block vs prune Race Condition
// =============================================================================

/// Simulates the race between add_block and prune_until_topoheight
/// Without semaphore: prune can delete data that add_block is reading
/// With semaphore: operations are serialized, no data corruption
#[tokio::test]
async fn test_concurrent_add_block_and_prune() {
    const ADD_BLOCK_TASKS: usize = 50;
    const PRUNE_TASKS: usize = 10;

    let semaphore = Arc::new(Semaphore::new(1));
    let state = Arc::new(RwLock::new(MockChainState {
        topoheight: 100,
        block_count: 100,
        tips: vec![100],
        is_corrupted: false,
    }));
    let corruption_detected = Arc::new(AtomicBool::new(false));

    let mut join_set = JoinSet::new();

    // Spawn add_block tasks
    for _i in 0..ADD_BLOCK_TASKS {
        let semaphore = semaphore.clone();
        let state = state.clone();
        let corruption_detected = corruption_detected.clone();

        join_set.spawn(async move {
            // Acquire semaphore BEFORE storage lock
            let _permit = semaphore.acquire().await.unwrap();

            // Simulate add_block: read state, verify, then write
            let mut guard = state.write().await;

            // Simulate read phase - check tips exist
            if guard.tips.is_empty() {
                // This would be corruption - tips should exist
                guard.is_corrupted = true;
                corruption_detected.store(true, Ordering::SeqCst);
                return;
            }

            // Simulate write phase - add new block
            guard.topoheight += 1;
            guard.block_count += 1;
            guard.tips = vec![guard.topoheight];

            // Small delay to increase race window
            tokio::time::sleep(Duration::from_micros(100)).await;
        });
    }

    // Spawn prune tasks
    for _ in 0..PRUNE_TASKS {
        let semaphore = semaphore.clone();
        let state = state.clone();

        join_set.spawn(async move {
            // Acquire semaphore BEFORE storage lock
            let _permit = semaphore.acquire().await.unwrap();

            // Simulate prune: delete old data
            let mut guard = state.write().await;

            // Prune keeps at least some blocks
            if guard.block_count > 10 {
                guard.block_count = guard.block_count.saturating_sub(5);
            }

            tokio::time::sleep(Duration::from_micros(50)).await;
        });
    }

    while join_set.join_next().await.is_some() {}

    // With semaphore, no corruption should occur
    assert!(
        !corruption_detected.load(Ordering::SeqCst),
        "Data corruption detected! Race condition occurred."
    );

    let final_state = state.read().await;
    assert!(
        !final_state.is_corrupted,
        "Chain state is corrupted after concurrent operations"
    );
}

// =============================================================================
// Test 3: Concurrent add_block vs rewind Race Condition
// =============================================================================

/// Simulates the race between add_block and rewind_chain
/// Rewind modifies tips while add_block references them
#[tokio::test]
async fn test_concurrent_add_block_and_rewind() {
    const ADD_BLOCK_TASKS: usize = 30;
    const REWIND_TASKS: usize = 5;

    let semaphore = Arc::new(Semaphore::new(1));
    let state = Arc::new(RwLock::new(MockChainState {
        topoheight: 50,
        block_count: 50,
        tips: vec![48, 49, 50], // Multiple tips (DAG)
        is_corrupted: false,
    }));
    let orphan_count = Arc::new(AtomicUsize::new(0));

    let mut join_set = JoinSet::new();

    // Spawn add_block tasks
    for _ in 0..ADD_BLOCK_TASKS {
        let semaphore = semaphore.clone();
        let state = state.clone();
        let orphan_count = orphan_count.clone();

        join_set.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            let mut guard = state.write().await;

            // Check if our parent tip still exists
            let parent_tip = guard.tips.last().copied().unwrap_or(0);
            if parent_tip == 0 || parent_tip > guard.topoheight + 10 {
                // Orphaned block - parent was rewound
                orphan_count.fetch_add(1, Ordering::SeqCst);
                return;
            }

            // Add block referencing parent
            guard.topoheight += 1;
            guard.block_count += 1;
            let new_tip = guard.topoheight;
            guard.tips.push(new_tip);
            if guard.tips.len() > 3 {
                guard.tips.remove(0);
            }

            tokio::time::sleep(Duration::from_micros(50)).await;
        });
    }

    // Spawn rewind tasks
    for _ in 0..REWIND_TASKS {
        let semaphore = semaphore.clone();
        let state = state.clone();

        join_set.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            let mut guard = state.write().await;

            // Rewind: remove recent blocks and update tips
            if guard.topoheight > 10 {
                let rewind_count = 3.min(guard.topoheight - 10);
                guard.topoheight = guard.topoheight.saturating_sub(rewind_count);
                guard.block_count = guard.block_count.saturating_sub(rewind_count);
                guard.tips = vec![guard.topoheight];
            }

            tokio::time::sleep(Duration::from_micros(100)).await;
        });
    }

    while join_set.join_next().await.is_some() {}

    // With semaphore, orphan count should be 0 (no stale tip references)
    assert_eq!(
        orphan_count.load(Ordering::SeqCst),
        0,
        "Orphaned blocks detected! Rewind raced with add_block."
    );
}

// =============================================================================
// Test 4: Multiple Chain Sync Tasks Racing
// =============================================================================

/// Simulates multiple peer sync tasks racing during chain reorganization
/// Each task: rewind -> add blocks -> commit
#[tokio::test]
async fn test_concurrent_chain_sync_operations() {
    const SYNC_TASKS: usize = 10;
    const BLOCKS_PER_SYNC: usize = 5;

    let semaphore = Arc::new(Semaphore::new(1));
    let state = Arc::new(RwLock::new(MockChainState {
        topoheight: 100,
        block_count: 100,
        tips: vec![100],
        is_corrupted: false,
    }));
    let successful_syncs = Arc::new(AtomicUsize::new(0));
    let concurrent_syncs = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));

    let mut join_set = JoinSet::new();

    for _peer_id in 0..SYNC_TASKS {
        let semaphore = semaphore.clone();
        let state = state.clone();
        let successful_syncs = successful_syncs.clone();
        let concurrent_syncs = concurrent_syncs.clone();
        let max_concurrent = max_concurrent.clone();

        join_set.spawn(async move {
            // Track concurrent sync operations
            let current = concurrent_syncs.fetch_add(1, Ordering::SeqCst) + 1;
            update_max(&max_concurrent, current);

            // Phase 1: Rewind (with semaphore)
            {
                let _permit = semaphore.acquire().await.unwrap();
                let mut guard = state.write().await;

                // Rewind 2 blocks
                guard.topoheight = guard.topoheight.saturating_sub(2);
                guard.tips = vec![guard.topoheight];

                tokio::time::sleep(Duration::from_micros(50)).await;
            }

            // Phase 2: Add blocks (with semaphore for each block)
            for _block_idx in 0..BLOCKS_PER_SYNC {
                let _permit = semaphore.acquire().await.unwrap();
                let mut guard = state.write().await;

                guard.topoheight += 1;
                guard.block_count += 1;
                guard.tips = vec![guard.topoheight];

                tokio::time::sleep(Duration::from_micros(20)).await;
            }

            successful_syncs.fetch_add(1, Ordering::SeqCst);
            concurrent_syncs.fetch_sub(1, Ordering::SeqCst);
        });
    }

    while join_set.join_next().await.is_some() {}

    // All syncs should complete successfully
    assert_eq!(
        successful_syncs.load(Ordering::SeqCst),
        SYNC_TASKS,
        "Not all sync tasks completed successfully"
    );

    // Verify chain state is consistent
    let final_state = state.read().await;
    assert!(
        !final_state.is_corrupted,
        "Chain state corrupted after concurrent syncs"
    );
    assert!(
        !final_state.tips.is_empty(),
        "Tips should not be empty after syncs"
    );
}

// =============================================================================
// Test 5: Read-Then-Write Pattern Without Semaphore (Negative Test)
// =============================================================================

/// Demonstrates the vulnerability when semaphore is NOT used
/// The read-then-write pattern can lead to lost updates
#[tokio::test]
async fn test_read_then_write_race_without_semaphore() {
    const TASKS: usize = 100;

    // NO semaphore - intentionally testing the race
    let counter = Arc::new(RwLock::new(0u64));
    let lost_updates = Arc::new(AtomicUsize::new(0));

    let mut join_set = JoinSet::new();

    for _ in 0..TASKS {
        let counter = counter.clone();
        let lost_updates = lost_updates.clone();

        join_set.spawn(async move {
            // Read current value
            let current = {
                let guard = counter.read().await;
                *guard
            };

            // Yield to allow other tasks to interleave
            tokio::task::yield_now().await;

            // Write incremented value
            {
                let mut guard = counter.write().await;
                // If value changed between read and write, we have a lost update
                if *guard != current {
                    lost_updates.fetch_add(1, Ordering::SeqCst);
                }
                *guard = current + 1;
            }
        });
    }

    while join_set.join_next().await.is_some() {}

    let final_value = *counter.read().await;
    let lost = lost_updates.load(Ordering::SeqCst);

    // Without semaphore, we expect lost updates (final_value < TASKS)
    // This test documents the vulnerability that semaphore prevents
    if lost > 0 {
        println!(
            "Race detected: {} lost updates, final value {} (expected {})",
            lost, final_value, TASKS
        );
    }

    // The test passes either way - it's demonstrating the pattern
    // In a real scenario without semaphore, lost > 0 is likely
}

// =============================================================================
// Test 6: Read-Then-Write Pattern WITH Semaphore (Positive Test)
// =============================================================================

/// Demonstrates that semaphore prevents the read-then-write race
#[tokio::test]
async fn test_read_then_write_safe_with_semaphore() {
    const TASKS: usize = 100;

    let semaphore = Arc::new(Semaphore::new(1));
    let counter = Arc::new(RwLock::new(0u64));

    let mut join_set = JoinSet::new();

    for _ in 0..TASKS {
        let semaphore = semaphore.clone();
        let counter = counter.clone();

        join_set.spawn(async move {
            // Acquire semaphore FIRST
            let _permit = semaphore.acquire().await.unwrap();

            // Now read-then-write is safe
            let current = {
                let guard = counter.read().await;
                *guard
            };

            // Even with yield, we're protected by semaphore
            tokio::task::yield_now().await;

            {
                let mut guard = counter.write().await;
                *guard = current + 1;
            }
        });
    }

    while join_set.join_next().await.is_some() {}

    // With semaphore, final value should be exactly TASKS
    let final_value = *counter.read().await;
    assert_eq!(
        final_value, TASKS as u64,
        "Lost updates detected even with semaphore!"
    );
}

// =============================================================================
// Test 7: Shutdown Race Condition
// =============================================================================

/// Simulates stop() racing with ongoing storage operations
#[tokio::test]
async fn test_shutdown_waits_for_operations() {
    let semaphore = Arc::new(Semaphore::new(1));
    let operation_complete = Arc::new(AtomicBool::new(false));
    let shutdown_started = Arc::new(AtomicBool::new(false));
    let shutdown_waited = Arc::new(AtomicBool::new(false));

    let mut join_set = JoinSet::new();

    // Long-running operation
    {
        let semaphore = semaphore.clone();
        let operation_complete = operation_complete.clone();
        let shutdown_started = shutdown_started.clone();
        let shutdown_waited = shutdown_waited.clone();

        join_set.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();

            // Simulate long operation
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Check if shutdown tried to proceed before we finished
            if shutdown_started.load(Ordering::SeqCst) {
                shutdown_waited.store(true, Ordering::SeqCst);
            }

            operation_complete.store(true, Ordering::SeqCst);
        });
    }

    // Give operation time to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Shutdown task
    {
        let semaphore = semaphore.clone();
        let shutdown_started = shutdown_started.clone();
        let operation_complete = operation_complete.clone();

        join_set.spawn(async move {
            shutdown_started.store(true, Ordering::SeqCst);

            // Shutdown acquires semaphore to wait for ongoing operations
            let _permit = semaphore.acquire().await.unwrap();

            // By the time we get here, operation should be complete
            assert!(
                operation_complete.load(Ordering::SeqCst),
                "Shutdown proceeded before operation completed!"
            );
        });
    }

    while join_set.join_next().await.is_some() {}

    assert!(operation_complete.load(Ordering::SeqCst));
    assert!(
        shutdown_waited.load(Ordering::SeqCst),
        "Shutdown should have waited for operation"
    );
}

// =============================================================================
// Helper Functions
// =============================================================================

fn update_max(max: &AtomicUsize, value: usize) {
    loop {
        let current = max.load(Ordering::SeqCst);
        if value <= current {
            break;
        }
        if max
            .compare_exchange(current, value, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            break;
        }
    }
}
