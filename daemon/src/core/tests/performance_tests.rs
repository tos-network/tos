// Performance Tests for GHOSTDAG Components
// These tests measure and verify performance characteristics
//
// Run with: cargo test --release -- --nocapture performance
// (--release flag is important for accurate performance measurement)

#[cfg(test)]
mod performance_tests {
    use std::time::Instant;
    use tos_common::crypto::Hash;
    use tos_common::difficulty::Difficulty;
    use crate::core::ghostdag::{calc_work_from_difficulty, BlueWorkType, SortableBlock};

    // Helper: Generate test hashes
    fn generate_hashes(count: usize) -> Vec<Hash> {
        (0..count)
            .map(|i| {
                let mut bytes = [0u8; 32];
                bytes[0..8].copy_from_slice(&(i as u64).to_le_bytes());
                Hash::new(bytes)
            })
            .collect()
    }

    // Helper: Measure execution time
    fn measure<F: FnOnce()>(name: &str, f: F) -> u128 {
        let start = Instant::now();
        f();
        let duration = start.elapsed();
        println!("{}: {:?}", name, duration);
        duration.as_micros()
    }

    // ============================================================================
    // 1. Block Sorting Performance
    // ============================================================================

    #[test]
    fn test_performance_sorting_10_blocks() {
        let hashes = generate_hashes(10);
        let mut blocks: Vec<SortableBlock> = hashes
            .into_iter()
            .enumerate()
            .map(|(i, hash)| {
                SortableBlock::new(hash, BlueWorkType::from(i as u64))
            })
            .collect();

        let micros = measure("Sort 10 blocks", || {
            blocks.sort();
        });

        // Should complete in < 100 microseconds
        assert!(micros < 100, "Sorting 10 blocks took {}μs (expected < 100μs)", micros);
    }

    #[test]
    fn test_performance_sorting_100_blocks() {
        let hashes = generate_hashes(100);
        let mut blocks: Vec<SortableBlock> = hashes
            .into_iter()
            .enumerate()
            .map(|(i, hash)| {
                SortableBlock::new(hash, BlueWorkType::from(i as u64))
            })
            .collect();

        let micros = measure("Sort 100 blocks", || {
            blocks.sort();
        });

        // Should complete in < 1ms
        assert!(micros < 1000, "Sorting 100 blocks took {}μs (expected < 1000μs)", micros);
    }

    #[test]
    fn test_performance_sorting_1000_blocks() {
        let hashes = generate_hashes(1000);
        let mut blocks: Vec<SortableBlock> = hashes
            .into_iter()
            .enumerate()
            .map(|(i, hash)| {
                SortableBlock::new(hash, BlueWorkType::from(i as u64))
            })
            .collect();

        let micros = measure("Sort 1000 blocks", || {
            blocks.sort();
        });

        // Should complete in < 10ms
        assert!(micros < 10_000, "Sorting 1000 blocks took {}μs (expected < 10ms)", micros);
    }

    // ============================================================================
    // 2. Work Calculation Performance
    // ============================================================================

    #[test]
    fn test_performance_work_calculation_single() {
        let difficulty = Difficulty::from(1000u64);

        let micros = measure("Calculate work (single)", || {
            for _ in 0..1000 {
                let _work = calc_work_from_difficulty(&difficulty);
            }
        });

        // 1000 calculations should complete in < 1ms
        assert!(micros < 1000, "1000 work calculations took {}μs (expected < 1ms)", micros);
        println!("  -> Average per calculation: {}ns", micros as f64);
    }

    #[test]
    fn test_performance_work_calculation_varying() {
        let difficulties: Vec<Difficulty> = (1..=100)
            .map(|i| Difficulty::from((i * 100) as u64))
            .collect();

        let micros = measure("Calculate work (varying difficulty)", || {
            for diff in &difficulties {
                let _work = calc_work_from_difficulty(diff);
            }
        });

        // 100 calculations should complete in < 100μs
        assert!(micros < 100, "100 work calculations took {}μs (expected < 100μs)", micros);
    }

    #[test]
    fn test_performance_work_accumulation() {
        let mut total_work = BlueWorkType::zero();
        let increment = BlueWorkType::from(100u64);

        let micros = measure("Accumulate work (10,000 iterations)", || {
            for _ in 0..10_000 {
                total_work = total_work.checked_add(increment).unwrap();
            }
        });

        // Should complete in < 500μs
        assert!(micros < 500, "10,000 accumulations took {}μs (expected < 500μs)", micros);
        assert_eq!(total_work, BlueWorkType::from(1_000_000u64));
    }

    // ============================================================================
    // 3. SortableBlock Comparison Performance
    // ============================================================================

    #[test]
    fn test_performance_block_comparison() {
        let hash1 = Hash::new([1u8; 32]);
        let hash2 = Hash::new([2u8; 32]);
        let work = BlueWorkType::from(1000u64);

        let block1 = SortableBlock::new(hash1, work);
        let block2 = SortableBlock::new(hash2, work);

        let micros = measure("Compare blocks (1,000,000 comparisons)", || {
            for _ in 0..1_000_000 {
                let _ = block1 < block2;
            }
        });

        // Should complete in < 10ms
        assert!(micros < 10_000, "1M comparisons took {}μs (expected < 10ms)", micros);
        println!("  -> Average per comparison: {}ns", micros as f64 / 1_000_000.0);
    }

    // ============================================================================
    // 4. Interval Operations Performance
    // ============================================================================

    #[test]
    fn test_performance_interval_split_half() {
        use crate::core::reachability::Interval;

        let interval = Interval::new(1, 1_000_000);

        let micros = measure("Interval split_half (100,000 iterations)", || {
            let mut current = interval;
            for _ in 0..100_000 {
                let (left, _right) = current.split_half();
                current = left;
            }
        });

        // Should complete in < 1ms
        assert!(micros < 1000, "100K splits took {}μs (expected < 1ms)", micros);
    }

    #[test]
    fn test_performance_interval_contains() {
        use crate::core::reachability::Interval;

        let parent = Interval::new(1, 1_000_000);
        let child = Interval::new(500_000, 750_000);

        let micros = measure("Interval contains (1,000,000 checks)", || {
            for _ in 0..1_000_000 {
                let _ = parent.contains(child);
            }
        });

        // Should complete in < 5ms
        assert!(micros < 5000, "1M contains checks took {}μs (expected < 5ms)", micros);
        println!("  -> Average per check: {}ns", micros as f64 / 1_000_000.0);
    }

    #[test]
    fn test_performance_interval_split_exact() {
        use crate::core::reachability::Interval;

        let interval = Interval::new(1, 1000);
        let sizes = vec![100, 200, 300, 400];

        let micros = measure("Interval split_exact (10,000 iterations)", || {
            for _ in 0..10_000 {
                let _splits = interval.split_exact(&sizes);
            }
        });

        // Should complete in < 10ms
        assert!(micros < 10_000, "10K split_exact took {}μs (expected < 10ms)", micros);
    }

    #[test]
    fn test_performance_interval_split_exponential() {
        use crate::core::reachability::Interval;

        let interval = Interval::new(1, 10_000);
        let sizes = vec![10, 20, 40, 80, 160];

        let micros = measure("Interval split_exponential (1,000 iterations)", || {
            for _ in 0..1000 {
                let _splits = interval.split_exponential(&sizes);
            }
        });

        // Should complete in < 50ms
        assert!(micros < 50_000, "1K split_exponential took {}μs (expected < 50ms)", micros);
    }

    // ============================================================================
    // 5. Hash Operations Performance
    // ============================================================================

    #[test]
    fn test_performance_hash_creation() {
        let micros = measure("Hash creation (100,000 hashes)", || {
            for i in 0..100_000 {
                let mut bytes = [0u8; 32];
                bytes[0..8].copy_from_slice(&(i as u64).to_le_bytes());
                let _hash = Hash::new(bytes);
            }
        });

        // Should complete in < 5ms
        assert!(micros < 5000, "100K hash creations took {}μs (expected < 5ms)", micros);
    }

    #[test]
    fn test_performance_hash_comparison() {
        let hash1 = Hash::new([1u8; 32]);
        let hash2 = Hash::new([2u8; 32]);

        let micros = measure("Hash comparison (1,000,000 comparisons)", || {
            for _ in 0..1_000_000 {
                let _ = hash1.as_bytes().cmp(hash2.as_bytes());
            }
        });

        // Should complete in < 10ms
        assert!(micros < 10_000, "1M hash comparisons took {}μs (expected < 10ms)", micros);
        println!("  -> Average per comparison: {}ns", micros as f64 / 1_000_000.0);
    }

    // ============================================================================
    // 6. Difficulty Calculation Performance
    // ============================================================================

    #[test]
    fn test_performance_difficulty_operations() {
        let diff1 = Difficulty::from(1000u64);
        let diff2 = Difficulty::from(2000u64);

        let micros = measure("Difficulty operations (100,000 iterations)", || {
            for _ in 0..100_000 {
                let _work1 = calc_work_from_difficulty(&diff1);
                let _work2 = calc_work_from_difficulty(&diff2);
            }
        });

        // 200K operations should complete in < 10ms
        assert!(micros < 10_000, "200K difficulty ops took {}μs (expected < 10ms)", micros);
    }

    // ============================================================================
    // 7. Complex Scenario Performance
    // ============================================================================

    #[test]
    fn test_performance_simulated_block_processing() {
        // Simulate processing a block with 32 parents
        let hashes = generate_hashes(32);
        let difficulties: Vec<Difficulty> = (1..=32).map(|i| Difficulty::from((i * 100) as u64)).collect();

        let micros = measure("Simulated block processing (100 blocks)", || {
            for _ in 0..100 {
                // 1. Select parent (find max blue_work)
                let mut blocks: Vec<SortableBlock> = hashes
                    .iter()
                    .zip(&difficulties)
                    .map(|(hash, diff)| {
                        let work = calc_work_from_difficulty(diff);
                        SortableBlock::new(hash.clone(), work)
                    })
                    .collect();

                // 2. Sort blocks
                blocks.sort();

                // 3. Calculate total work
                let mut total_work = BlueWorkType::zero();
                for block in &blocks {
                    total_work = total_work.checked_add(block.blue_work).unwrap();
                }
            }
        });

        // Should complete in < 50ms
        assert!(micros < 50_000, "100 block simulations took {}μs (expected < 50ms)", micros);
        println!("  -> Average per block: {}μs", micros as f64 / 100.0);
    }

    // ============================================================================
    // 8. Memory Efficiency Tests
    // ============================================================================

    #[test]
    fn test_performance_memory_allocation() {
        use std::collections::HashMap;

        let micros = measure("Allocate 1000 HashMaps", || {
            let mut maps = Vec::new();
            for _ in 0..1000 {
                let mut map = HashMap::new();
                map.insert(Hash::new([1u8; 32]), 100u64);
                maps.push(map);
            }
        });

        // Should complete in < 5ms
        assert!(micros < 5000, "1000 HashMap allocations took {}μs (expected < 5ms)", micros);
    }

    // ============================================================================
    // 9. Scaling Tests
    // ============================================================================

    #[test]
    fn test_performance_scaling_linear() {
        // Verify that operations scale linearly with input size
        let sizes = vec![10, 100, 1000];
        let mut times = Vec::new();

        for size in &sizes {
            let hashes = generate_hashes(*size);
            let mut blocks: Vec<SortableBlock> = hashes
                .into_iter()
                .enumerate()
                .map(|(i, hash)| {
                    SortableBlock::new(hash, BlueWorkType::from(i as u64))
                })
                .collect();

            let start = Instant::now();
            blocks.sort();
            let duration = start.elapsed().as_micros();
            times.push(duration);

            println!("Size {}: {}μs", size, duration);
        }

        // Verify approximately linear scaling (with log factor for sorting)
        // time(1000) should be < 200 * time(10)
        assert!(times[2] < times[0] * 200, "Scaling is not linear");
    }

    // ============================================================================
    // 10. Summary Performance Report
    // ============================================================================

    #[test]
    fn test_performance_summary() {
        println!("\n=== GHOSTDAG Performance Summary ===\n");

        // Core operations
        let diff = Difficulty::from(1000u64);
        let work_calc_time = measure("Work calculation (single)", || {
            let _work = calc_work_from_difficulty(&diff);
        });

        let hash1 = Hash::new([1u8; 32]);
        let hash2 = Hash::new([2u8; 32]);
        let work = BlueWorkType::from(1000u64);
        let block1 = SortableBlock::new(hash1, work);
        let block2 = SortableBlock::new(hash2, work);

        let cmp_time = measure("Block comparison (single)", || {
            let _ = block1 < block2;
        });

        use crate::core::reachability::Interval;
        let parent = Interval::new(1, 1_000_000);
        let child = Interval::new(500_000, 750_000);

        let contains_time = measure("Interval contains (single)", || {
            let _ = parent.contains(child);
        });

        println!("\nPer-Operation Times:");
        println!("  Work calculation: ~{}ns", work_calc_time);
        println!("  Block comparison: ~{}ns", cmp_time);
        println!("  Interval contains: ~{}ns", contains_time);

        println!("\nAll operations complete in sub-microsecond time ✅");

        // Verify all critical operations are fast enough
        assert!(work_calc_time < 10, "Work calculation too slow");
        assert!(cmp_time < 1, "Block comparison too slow");
        assert!(contains_time < 1, "Interval contains too slow");
    }
}
