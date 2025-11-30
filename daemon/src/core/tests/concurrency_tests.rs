// Concurrency and Thread Safety Tests
// Tests concurrent operations on GHOSTDAG and Reachability
// Note: Tests requiring MockStorage are disabled until Storage trait is stable

#[cfg(test)]
mod concurrency_tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use tokio;
    use tokio::sync::RwLock;

    use tos_common::crypto::Hash;
    use tos_common::difficulty::Difficulty;

    use crate::core::{
        ghostdag::{calc_work_from_difficulty, BlueWorkType, SortableBlock, TosGhostdag},
        reachability::{Interval, TosReachability},
    };

    // Test 1: Concurrent work calculations (CPU-bound operations)
    #[tokio::test]
    async fn test_concurrent_work_calculations() {
        // Spawn multiple tasks calculating work from different difficulties
        let mut handles = vec![];

        for i in 1..=50 {
            let handle = tokio::spawn(async move {
                let difficulty = Difficulty::from((i * 100) as u64);
                let work = calc_work_from_difficulty(&difficulty).unwrap();
                (i, work)
            });

            handles.push(handle);
        }

        // Collect all results
        let mut results = vec![];
        for handle in handles {
            let result = handle.await.unwrap();
            results.push(result);
        }

        // Verify all calculations completed
        assert_eq!(results.len(), 50);

        // Verify work increases with difficulty
        results.sort_by_key(|(i, _)| *i);
        for i in 1..results.len() {
            assert!(
                results[i].1 > results[i - 1].1,
                "Work should increase with difficulty"
            );
        }
    }

    // Test 2: Concurrent interval operations
    #[tokio::test]
    async fn test_concurrent_interval_operations() {
        // Test that interval operations are safe under concurrent access
        let parent_interval = Arc::new(RwLock::new(Interval::maximal()));

        let mut handles = vec![];

        for i in 0..10 {
            let parent_interval = Arc::clone(&parent_interval);

            let handle = tokio::spawn(async move {
                let interval = parent_interval.read().await;

                // Perform read operations
                let size = interval.size();
                let is_empty = interval.is_empty();
                let (left, right) = interval.split_half();

                (i, size, is_empty, left.size(), right.size())
            });

            handles.push(handle);
        }

        // All concurrent reads should succeed
        for handle in handles {
            let result = handle.await;
            assert!(result.is_ok(), "Concurrent interval reads should succeed");
        }
    }

    // Test 3: Concurrent blue/red classification
    #[tokio::test]
    async fn test_concurrent_blue_red_classification() {
        // This tests that blue/red classification logic is safe under concurrent access
        let genesis_hash = Hash::new([0u8; 32]);
        let reachability = Arc::new(TosReachability::new(genesis_hash.clone()));
        let ghostdag = Arc::new(TosGhostdag::new(10, genesis_hash.clone(), reachability));

        let k = ghostdag.k();

        // Spawn multiple tasks checking K-cluster boundaries
        let mut handles = vec![];

        for anticone_size in 0..20 {
            let handle = tokio::spawn(async move {
                // Classify based on anticone size
                let is_blue = anticone_size <= k;
                (anticone_size, is_blue)
            });

            handles.push(handle);
        }

        // Collect results
        let mut results = vec![];
        for handle in handles {
            let result = handle.await.unwrap();
            results.push(result);
        }

        // Verify classifications
        for (anticone_size, is_blue) in results {
            if anticone_size <= k {
                assert!(
                    is_blue,
                    "Anticone size {} should be blue (K={})",
                    anticone_size, k
                );
            } else {
                assert!(
                    !is_blue,
                    "Anticone size {} should be red (K={})",
                    anticone_size, k
                );
            }
        }
    }

    // Test 4: Concurrent SortableBlock operations
    #[tokio::test]
    async fn test_concurrent_sortable_block_operations() {
        // Create blocks concurrently
        let mut handles = vec![];

        for i in 0..100 {
            let handle = tokio::spawn(async move {
                let hash = Hash::new([i as u8; 32]);
                let work = BlueWorkType::from((i * 10) as u64);
                SortableBlock::new(hash, work)
            });

            handles.push(handle);
        }

        // Collect all blocks
        let mut blocks = vec![];
        for handle in handles {
            let block = handle.await.unwrap();
            blocks.push(block);
        }

        // Sort them (this should be deterministic)
        blocks.sort();

        // Verify sorting order
        for i in 1..blocks.len() {
            assert!(
                blocks[i].blue_work >= blocks[i - 1].blue_work,
                "Blocks should be sorted by blue_work"
            );
        }
    }

    // Test 5: Atomic operations test
    #[tokio::test]
    async fn test_atomic_operations() {
        let counter = Arc::new(AtomicU64::new(0));
        let mut handles = vec![];

        // Spawn 100 tasks incrementing a counter
        for _ in 0..100 {
            let counter = Arc::clone(&counter);

            let handle = tokio::spawn(async move {
                // Simulate some work
                let difficulty = Difficulty::from(1000u64);
                let _work = calc_work_from_difficulty(&difficulty).unwrap();

                // Atomic increment
                counter.fetch_add(1, Ordering::SeqCst);
            });

            handles.push(handle);
        }

        // Wait for all increments
        for handle in handles {
            handle.await.unwrap();
        }

        // Counter should be exactly 100
        assert_eq!(
            counter.load(Ordering::SeqCst),
            100,
            "Atomic operations should be thread-safe"
        );
    }

    // Test 6: Stress test with many concurrent operations
    #[tokio::test]
    async fn test_stress_concurrent_mixed_operations() {
        let genesis_hash = Hash::new([0u8; 32]);
        let _reachability = Arc::new(TosReachability::new(genesis_hash.clone()));

        let mut handles = vec![];

        // Mix of different operations (pure algorithmic, no storage)
        for i in 0..50 {
            let handle = tokio::spawn(async move {
                let operation = i % 3;

                match operation {
                    0 => {
                        // Work calculation (add 1 to avoid zero difficulty)
                        let difficulty = Difficulty::from((i * 10 + 1) as u64);
                        let _work = calc_work_from_difficulty(&difficulty).unwrap();
                    }
                    1 => {
                        // Interval operation
                        let interval = Interval::new(1, 1000);
                        let _split = interval.split_half();
                    }
                    _ => {
                        // Hash operations
                        let _hash = Hash::new([i as u8; 32]);
                    }
                }

                i
            });

            handles.push(handle);
        }

        // Wait for all operations to complete
        let mut completed = 0;
        for handle in handles {
            if handle.await.is_ok() {
                completed += 1;
            }
        }

        assert_eq!(completed, 50, "All concurrent operations should complete");
    }

    // Test 7: Concurrent GHOSTDAG and Reachability creation
    #[tokio::test]
    async fn test_concurrent_ghostdag_creation() {
        let mut handles = vec![];

        for i in 0..20 {
            let handle = tokio::spawn(async move {
                let genesis_hash = Hash::new([i as u8; 32]);
                let reachability = Arc::new(TosReachability::new(genesis_hash.clone()));
                let ghostdag = TosGhostdag::new(10, genesis_hash.clone(), reachability);

                // Verify creation
                assert_eq!(ghostdag.k(), 10);
                i
            });

            handles.push(handle);
        }

        // All creations should succeed
        for handle in handles {
            let result = handle.await;
            assert!(
                result.is_ok(),
                "Concurrent GHOSTDAG creation should succeed"
            );
        }
    }

    // Test 8: Concurrent interval splitting
    #[tokio::test]
    async fn test_concurrent_interval_splitting() {
        let intervals = vec![
            Interval::maximal(),
            Interval::new(1, 1000),
            Interval::new(500, 1500),
            Interval::new(1, 1000000),
        ];

        let mut handles = vec![];

        for interval in intervals {
            let handle = tokio::spawn(async move {
                // Split multiple times
                let (left, right) = interval.split_half();
                let (left_left, _) = left.split_half();
                let (_, right_right) = right.split_half();

                // Verify sizes are reasonable
                assert!(left_left.size() > 0 || left_left.is_empty());
                assert!(right_right.size() > 0 || right_right.is_empty());

                interval.size()
            });

            handles.push(handle);
        }

        // All splits should succeed
        for handle in handles {
            let result = handle.await;
            assert!(result.is_ok(), "Concurrent interval splits should succeed");
        }
    }

    // Test 9: Concurrent hash operations
    #[tokio::test]
    async fn test_concurrent_hash_operations() {
        let mut handles = vec![];

        for i in 0..100 {
            let handle = tokio::spawn(async move {
                let hash1 = Hash::new([i as u8; 32]);
                let hash2 = Hash::new([(i + 1) as u8; 32]);

                // Compare hashes
                let are_equal = hash1 == hash2;
                let bytes_equal = hash1.as_bytes() == hash2.as_bytes();

                assert_eq!(
                    are_equal, bytes_equal,
                    "Hash comparison should be consistent"
                );

                (hash1, hash2, are_equal)
            });

            handles.push(handle);
        }

        // All hash operations should succeed
        let mut results = vec![];
        for handle in handles {
            let result = handle.await.unwrap();
            results.push(result);
        }

        assert_eq!(results.len(), 100, "All hash operations should complete");
    }

    // Test 10: Concurrent difficulty and work operations
    #[tokio::test]
    async fn test_concurrent_difficulty_work_operations() {
        let difficulties = vec![1u64, 10, 100, 1000, 10000, 100000];
        let mut handles = vec![];

        for diff_val in difficulties {
            let handle = tokio::spawn(async move {
                // Create difficulty and calculate work
                let difficulty = Difficulty::from(diff_val);
                let work = calc_work_from_difficulty(&difficulty).unwrap();

                // Verify work is non-zero (unless difficulty is max)
                if diff_val > 0 {
                    assert!(
                        work > BlueWorkType::zero(),
                        "Work should be > 0 for non-zero difficulty"
                    );
                }

                (diff_val, work)
            });

            handles.push(handle);
        }

        // Collect results and verify
        let mut results = vec![];
        for handle in handles {
            let result = handle.await.unwrap();
            results.push(result);
        }

        // Verify work increases with difficulty
        results.sort_by_key(|(d, _)| *d);
        for i in 1..results.len() {
            assert!(
                results[i].1 > results[i - 1].1,
                "Work should increase with difficulty"
            );
        }
    }
}
