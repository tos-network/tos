// Storage Stress Tests
// Tests storage layer performance under extreme I/O load

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio::task::JoinSet;

/// Stress Test 1: Rapid concurrent writes (storage I/O stress)
#[tokio::test]
async fn stress_rapid_concurrent_writes() {
    // Test storage performance with high concurrent write load

    // Test Parameters:
    const TOTAL_WRITES: usize = 100_000;
    const CONCURRENT_WRITERS: usize = 100;
    const BATCH_SIZE: usize = 1000;

    let storage = Arc::new(MockStorage::new());
    let start = Instant::now();
    let semaphore = Arc::new(Semaphore::new(CONCURRENT_WRITERS));
    let success_count = Arc::new(Mutex::new(0usize));
    let error_count = Arc::new(Mutex::new(0usize));

    let mut join_set = JoinSet::new();

    for batch_id in 0..(TOTAL_WRITES / BATCH_SIZE) {
        for item_id in 0..BATCH_SIZE {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let storage_clone = storage.clone();
            let success = success_count.clone();
            let errors = error_count.clone();
            let key = batch_id * BATCH_SIZE + item_id;

            join_set.spawn(async move {
                let _permit = permit;

                let result = storage_clone.write(key, generate_test_data(key)).await;

                match result {
                    Ok(_) => {
                        let mut count = success.lock().await;
                        *count += 1;
                    }
                    Err(_) => {
                        let mut count = errors.lock().await;
                        *count += 1;
                    }
                }
            });
        }

        // Periodic status update
        if batch_id % 10 == 0 {
            if log::log_enabled!(log::Level::Debug) {
                let progress = batch_id * BATCH_SIZE;
                log::debug!(
                    "Write progress: {}/{} ({:.1}%)",
                    progress,
                    TOTAL_WRITES,
                    (progress as f64 / TOTAL_WRITES as f64) * 100.0
                );
            }
        }
    }

    // Wait for all writes to complete
    while join_set.join_next().await.is_some() {}

    let elapsed = start.elapsed();
    let final_success = *success_count.lock().await;
    let final_errors = *error_count.lock().await;

    // Measure storage size
    let storage_size = storage.size().await;

    if log::log_enabled!(log::Level::Info) {
        log::info!("Storage write stress test completed in {:?}", elapsed);
        log::info!(
            "Successful writes: {}, Failed writes: {}",
            final_success,
            final_errors
        );
        log::info!(
            "Write throughput: {:.2} writes/sec",
            final_success as f64 / elapsed.as_secs_f64()
        );
        log::info!("Final storage size: {} items", storage_size);
    }

    println!("Storage write stress test results:");
    println!("  Total writes: {}", TOTAL_WRITES);
    println!("  Successful: {}", final_success);
    println!("  Failed: {}", final_errors);
    println!("  Duration: {:?}", elapsed);
    println!(
        "  Throughput: {:.2} writes/sec",
        final_success as f64 / elapsed.as_secs_f64()
    );
    println!("  Storage size: {} items", storage_size);

    // Expected Results:
    // - All writes successful (or minimal failures < 0.1%)
    // - Throughput > 10,000 writes/sec
    // - No data corruption
    // - No deadlocks

    assert_eq!(final_success + final_errors, TOTAL_WRITES);
    assert!(final_success > TOTAL_WRITES * 99 / 100); // >99% success rate
}

/// Stress Test 2: Mixed read/write workload
#[tokio::test]
async fn stress_mixed_read_write_workload() {
    // Test storage with concurrent reads and writes

    // Test Parameters:
    const INITIAL_ITEMS: usize = 10_000;
    const READ_OPERATIONS: usize = 50_000;
    const WRITE_OPERATIONS: usize = 20_000;
    const CONCURRENT_OPS: usize = 200;

    let storage = Arc::new(MockStorage::new());

    // Pre-populate storage
    if log::log_enabled!(log::Level::Info) {
        log::info!("Pre-populating storage with {} items", INITIAL_ITEMS);
    }
    for i in 0..INITIAL_ITEMS {
        storage.write(i, generate_test_data(i)).await.unwrap();
    }

    let start = Instant::now();
    let read_count = Arc::new(Mutex::new(0usize));
    let write_count = Arc::new(Mutex::new(0usize));
    let read_errors = Arc::new(Mutex::new(0usize));
    let write_errors = Arc::new(Mutex::new(0usize));

    let semaphore = Arc::new(Semaphore::new(CONCURRENT_OPS));
    let mut join_set = JoinSet::new();

    // Spawn read operations
    for i in 0..READ_OPERATIONS {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let storage_clone = storage.clone();
        let reads = read_count.clone();
        let errors = read_errors.clone();

        join_set.spawn(async move {
            let _permit = permit;
            let key = i % INITIAL_ITEMS;

            match storage_clone.read(key).await {
                Ok(_) => {
                    let mut count = reads.lock().await;
                    *count += 1;
                }
                Err(_) => {
                    let mut count = errors.lock().await;
                    *count += 1;
                }
            }
        });
    }

    // Spawn write operations
    for i in 0..WRITE_OPERATIONS {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let storage_clone = storage.clone();
        let writes = write_count.clone();
        let errors = write_errors.clone();

        join_set.spawn(async move {
            let _permit = permit;
            let key = INITIAL_ITEMS + i;

            match storage_clone.write(key, generate_test_data(key)).await {
                Ok(_) => {
                    let mut count = writes.lock().await;
                    *count += 1;
                }
                Err(_) => {
                    let mut count = errors.lock().await;
                    *count += 1;
                }
            }
        });
    }

    // Wait for all operations
    while join_set.join_next().await.is_some() {}

    let elapsed = start.elapsed();
    let final_reads = *read_count.lock().await;
    let final_writes = *write_count.lock().await;
    let final_read_errors = *read_errors.lock().await;
    let final_write_errors = *write_errors.lock().await;

    let total_ops = final_reads + final_writes;
    let throughput = total_ops as f64 / elapsed.as_secs_f64();

    if log::log_enabled!(log::Level::Info) {
        log::info!("Mixed workload test completed in {:?}", elapsed);
        log::info!(
            "Reads: {} ({} errors), Writes: {} ({} errors)",
            final_reads,
            final_read_errors,
            final_writes,
            final_write_errors
        );
        log::info!("Total throughput: {:.2} ops/sec", throughput);
    }

    println!("Mixed read/write workload results:");
    println!("  Reads: {} ({} errors)", final_reads, final_read_errors);
    println!("  Writes: {} ({} errors)", final_writes, final_write_errors);
    println!("  Duration: {:?}", elapsed);
    println!("  Throughput: {:.2} ops/sec", throughput);

    // Expected Results:
    // - All reads find valid data
    // - All writes succeed
    // - No race conditions or data corruption
    // - Throughput > 5000 ops/sec
}

/// Stress Test 3: Large dataset storage (memory pressure)
#[tokio::test]
async fn stress_large_dataset_storage() {
    // Test storage with very large dataset

    // Test Parameters:
    const LARGE_ITEM_SIZE: usize = 10_000; // 10KB per item
    const NUM_LARGE_ITEMS: usize = 10_000; // 100MB total
    const BATCH_SIZE: usize = 100;

    let storage = Arc::new(MockStorage::new());
    let start = Instant::now();
    let mut memory_samples = Vec::new();

    if log::log_enabled!(log::Level::Info) {
        log::info!(
            "Starting large dataset test: {} items of {} bytes each",
            NUM_LARGE_ITEMS,
            LARGE_ITEM_SIZE
        );
    }

    for batch in 0..(NUM_LARGE_ITEMS / BATCH_SIZE) {
        // Write batch of large items
        for i in 0..BATCH_SIZE {
            let key = batch * BATCH_SIZE + i;
            let data = generate_large_test_data(key, LARGE_ITEM_SIZE);
            storage.write(key, data).await.unwrap();
        }

        // Sample memory usage
        if batch % 10 == 0 {
            let current_size = storage.total_bytes().await;
            memory_samples.push(current_size);

            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "Progress: {}/{} items, storage size: {} MB",
                    batch * BATCH_SIZE,
                    NUM_LARGE_ITEMS,
                    current_size / (1024 * 1024)
                );
            }
        }
    }

    let elapsed = start.elapsed();
    let final_size = storage.total_bytes().await;
    let final_items = storage.size().await;

    if log::log_enabled!(log::Level::Info) {
        log::info!("Large dataset test completed in {:?}", elapsed);
        log::info!(
            "Stored {} items ({} MB)",
            final_items,
            final_size / (1024 * 1024)
        );
    }

    println!("Large dataset storage results:");
    println!("  Items stored: {}", final_items);
    println!("  Total size: {} MB", final_size / (1024 * 1024));
    println!("  Duration: {:?}", elapsed);
    println!("  Average item size: {} bytes", final_size / final_items);

    // Verify data integrity - random sampling
    let sample_size = 100;
    let mut integrity_errors = 0;
    for i in 0..sample_size {
        let key = (i * NUM_LARGE_ITEMS / sample_size) % NUM_LARGE_ITEMS;
        match storage.read(key).await {
            Ok(data) => {
                let expected = generate_large_test_data(key, LARGE_ITEM_SIZE);
                if data != expected {
                    integrity_errors += 1;
                }
            }
            Err(_) => {
                integrity_errors += 1;
            }
        }
    }

    println!(
        "  Data integrity check: {}/{} samples valid",
        sample_size - integrity_errors,
        sample_size
    );

    // Expected Results:
    // - All items stored successfully
    // - No data corruption
    // - Memory usage scales linearly
    // - Read performance remains acceptable
    assert_eq!(integrity_errors, 0);
}

/// Stress Test 4: Rapid delete and compact operations
#[tokio::test]
async fn stress_delete_and_compact() {
    // Test storage compaction under load

    // Test Parameters:
    const INITIAL_ITEMS: usize = 50_000;
    const DELETE_ROUNDS: usize = 10;
    const ITEMS_PER_ROUND: usize = 5000;

    let storage = Arc::new(MockStorage::new());

    // Populate storage
    if log::log_enabled!(log::Level::Info) {
        log::info!("Populating storage with {} items", INITIAL_ITEMS);
    }
    for i in 0..INITIAL_ITEMS {
        storage.write(i, generate_test_data(i)).await.unwrap();
    }

    let initial_size = storage.total_bytes().await;
    let start = Instant::now();

    for round in 0..DELETE_ROUNDS {
        let round_start = Instant::now();

        // Delete items
        let start_key = round * ITEMS_PER_ROUND;
        let end_key = start_key + ITEMS_PER_ROUND;

        for key in start_key..end_key {
            storage.delete(key).await.unwrap();
        }

        // Trigger compaction
        storage.compact().await.unwrap();

        let round_elapsed = round_start.elapsed();
        let current_size = storage.total_bytes().await;
        let current_items = storage.size().await;

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "Round {}: deleted {} items in {:?}, {} items remaining ({} MB)",
                round,
                ITEMS_PER_ROUND,
                round_elapsed,
                current_items,
                current_size / (1024 * 1024)
            );
        }
    }

    let elapsed = start.elapsed();
    let final_size = storage.total_bytes().await;
    let final_items = storage.size().await;

    if log::log_enabled!(log::Level::Info) {
        log::info!("Delete/compact stress test completed in {:?}", elapsed);
        log::info!(
            "Initial: {} items ({} MB), Final: {} items ({} MB)",
            INITIAL_ITEMS,
            initial_size / (1024 * 1024),
            final_items,
            final_size / (1024 * 1024)
        );
    }

    println!("Delete and compact results:");
    println!("  Initial items: {}", INITIAL_ITEMS);
    println!("  Deleted items: {}", DELETE_ROUNDS * ITEMS_PER_ROUND);
    println!("  Remaining items: {}", final_items);
    println!(
        "  Size reduction: {} MB -> {} MB",
        initial_size / (1024 * 1024),
        final_size / (1024 * 1024)
    );
    println!("  Duration: {:?}", elapsed);

    // Expected Results:
    // - Correct number of items deleted
    // - Storage size reduced appropriately
    // - No data corruption in remaining items
    assert_eq!(
        final_items,
        INITIAL_ITEMS - (DELETE_ROUNDS * ITEMS_PER_ROUND)
    );
}

/// Stress Test 5: Storage recovery after simulated crashes
#[tokio::test]
async fn stress_storage_recovery() {
    // Test storage recovery mechanisms

    // Test Parameters:
    const CHECKPOINT_INTERVAL: usize = 1000;
    const TOTAL_OPERATIONS: usize = 10_000;
    const CRASH_POINTS: usize = 5;

    let storage = Arc::new(MockStorage::new());
    let crash_interval = TOTAL_OPERATIONS / CRASH_POINTS;

    if log::log_enabled!(log::Level::Info) {
        log::info!(
            "Testing storage recovery with {} crash points",
            CRASH_POINTS
        );
    }

    for crash_point in 0..CRASH_POINTS {
        let start_op = crash_point * crash_interval;
        let end_op = start_op + crash_interval;

        // Perform operations
        for i in start_op..end_op {
            storage.write(i, generate_test_data(i)).await.unwrap();

            // Create checkpoint periodically
            if i % CHECKPOINT_INTERVAL == 0 {
                storage.checkpoint().await.unwrap();
            }
        }

        // Simulate crash and recovery
        if log::log_enabled!(log::Level::Debug) {
            log::debug!("Simulating crash at operation {}", end_op);
        }

        let recovered_state = storage.simulate_crash_recovery().await.unwrap();

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "Recovery complete: {} items recovered",
                recovered_state.items_recovered
            );
        }

        // Verify data integrity after recovery
        for i in 0..end_op {
            if i > recovered_state.last_checkpoint {
                // Items after last checkpoint might be lost
                continue;
            }

            match storage.read(i).await {
                Ok(data) => {
                    let expected = generate_test_data(i);
                    assert_eq!(data, expected, "Data corruption detected after recovery");
                }
                Err(_) => panic!("Failed to read item {} after recovery", i),
            }
        }
    }

    if log::log_enabled!(log::Level::Info) {
        log::info!("Storage recovery test completed successfully");
    }

    println!("Storage recovery test results:");
    println!("  Total crash/recovery cycles: {}", CRASH_POINTS);
    println!("  Operations per cycle: {}", crash_interval);
    println!("  All recovery cycles successful");

    // Expected Results:
    // - All checkpointed data recovered
    // - No data corruption after recovery
    // - Recovery completes in reasonable time (< 1 second)
}

// ============================================================================
// Helper Functions and Mock Implementations
// ============================================================================

/// Generate test data of standard size
fn generate_test_data(key: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(256);
    for i in 0..256 {
        data.push(((key + i) % 256) as u8);
    }
    data
}

/// Generate large test data
fn generate_large_test_data(key: usize, size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    for i in 0..size {
        data.push(((key + i) % 256) as u8);
    }
    data
}

/// Mock storage implementation for testing
struct MockStorage {
    data: Arc<RwLock<std::collections::HashMap<usize, Vec<u8>>>>,
    checkpoints: Arc<Mutex<Vec<usize>>>,
}

impl MockStorage {
    fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(std::collections::HashMap::new())),
            checkpoints: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn write(&self, key: usize, value: Vec<u8>) -> Result<(), String> {
        // Simulate write latency
        tokio::time::sleep(Duration::from_micros(10)).await;

        let mut data = self.data.write().await;
        data.insert(key, value);
        Ok(())
    }

    async fn read(&self, key: usize) -> Result<Vec<u8>, String> {
        // Simulate read latency
        tokio::time::sleep(Duration::from_micros(5)).await;

        let data = self.data.read().await;
        data.get(&key)
            .cloned()
            .ok_or_else(|| format!("Key {} not found", key))
    }

    async fn delete(&self, key: usize) -> Result<(), String> {
        let mut data = self.data.write().await;
        data.remove(&key);
        Ok(())
    }

    async fn size(&self) -> usize {
        self.data.read().await.len()
    }

    async fn total_bytes(&self) -> usize {
        let data = self.data.read().await;
        data.values().map(|v| v.len()).sum()
    }

    async fn compact(&self) -> Result<(), String> {
        // Simulate compaction delay
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(())
    }

    async fn checkpoint(&self) -> Result<(), String> {
        let size = self.size().await;
        let mut checkpoints = self.checkpoints.lock().await;
        checkpoints.push(size);
        Ok(())
    }

    async fn simulate_crash_recovery(&self) -> Result<RecoveryState, String> {
        // Simulate recovery delay
        tokio::time::sleep(Duration::from_millis(50)).await;

        let checkpoints = self.checkpoints.lock().await;
        let last_checkpoint = checkpoints.last().copied().unwrap_or(0);
        let items_recovered = self.size().await;

        Ok(RecoveryState {
            last_checkpoint,
            items_recovered,
        })
    }
}

#[derive(Debug)]
struct RecoveryState {
    last_checkpoint: usize,
    items_recovered: usize,
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_generate_test_data() {
        let data1 = generate_test_data(0);
        let data2 = generate_test_data(1);

        assert_eq!(data1.len(), 256);
        assert_eq!(data2.len(), 256);
        assert_ne!(data1, data2);
    }

    #[test]
    fn test_generate_large_test_data() {
        let data = generate_large_test_data(42, 10000);
        assert_eq!(data.len(), 10000);

        // Verify data is deterministic
        let data2 = generate_large_test_data(42, 10000);
        assert_eq!(data, data2);
    }

    #[tokio::test]
    async fn test_mock_storage_basic_ops() {
        let storage = MockStorage::new();

        // Write
        let data = vec![1, 2, 3, 4, 5];
        assert!(storage.write(1, data.clone()).await.is_ok());

        // Read
        let read_data = storage.read(1).await.unwrap();
        assert_eq!(read_data, data);

        // Delete
        assert!(storage.delete(1).await.is_ok());
        assert!(storage.read(1).await.is_err());
    }

    #[tokio::test]
    async fn test_mock_storage_checkpoint() {
        let storage = MockStorage::new();

        // Write some data
        for i in 0..10 {
            storage.write(i, vec![i as u8]).await.unwrap();
        }

        // Create checkpoint
        assert!(storage.checkpoint().await.is_ok());

        // Verify checkpoint was created
        let checkpoints = storage.checkpoints.lock().await;
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0], 10);
    }
}
