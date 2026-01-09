// File: testing-framework/src/orchestrator/rng.rs
//
// Unified RNG - V3.0 Deterministic Infrastructure
//
// This module provides a seeded RNG for deterministic test execution with
// replay capability. All test randomness should flow through TestRng to
// enable failure reproduction via seed replay.

use parking_lot::Mutex;
use rand::{rngs::StdRng, Rng, RngCore, SeedableRng};

/// Test RNG with seed for reproducibility
///
/// TestRng is the unified source of randomness for non-proptest test scenarios.
/// It uses a seeded StdRng internally, allowing complete reproduction of test
/// behavior by re-running with the same seed.
///
/// # Design Principles
///
/// 1. **Unified RNG**: All test randomness flows through TestRng (except proptest)
/// 2. **No rand::random()**: Direct calls to `rand::random()` bypass seed control
/// 3. **Seed Replay**: Failed tests print seed for exact reproduction
/// 4. **Environment Control**: Can override seed via `TOS_TEST_SEED` env var
///
/// # Seed Format
///
/// Seeds are 64-bit hexadecimal values:
/// - Format: `0x1234567890abcdef`
/// - Generated randomly if not provided
/// - Logged to stderr for all test runs
/// - Can be replayed via environment variable
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tos_tck::orchestrator::rng::TestRng;
///
/// fn test_with_rng() {
///     let rng = TestRng::new_from_env_or_random();
///
///     // Generate random values
///     let random_u64: u64 = rng.gen();
///     let random_bool: bool = rng.gen();
///     let random_index: usize = rng.gen_range(0..100);
///
///     // Test logic using random values...
///     assert!(random_index < 100);
/// }
/// ```
///
/// ## Seed Replay (Reproduce Failed Test)
///
/// When a test fails, the output shows:
///
/// ```text
/// test result: FAILED
/// TestRng seed: 0xa3f5c8e1b2d94706
///    Replay: TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test test_name
/// ```
///
/// To reproduce exactly:
///
/// ```bash
/// TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test test_name
/// ```
///
/// ## Deterministic Seed (For Debugging)
///
/// ```rust
/// use tos_tck::orchestrator::rng::TestRng;
///
/// fn test_with_fixed_seed() {
///     let rng = TestRng::with_seed(0x1234567890abcdef);
///
///     // Same seed = same random sequence every run
///     let value: u64 = rng.gen();
///     // value will always be the same for this seed
/// }
/// ```
///
/// ## Thread-Safe Usage
///
/// ```rust
/// use std::sync::Arc;
/// use tos_tck::orchestrator::rng::TestRng;
///
/// #[tokio::test]
/// async fn test_concurrent_random() {
///     let rng = Arc::new(TestRng::new_from_env_or_random());
///
///     let rng1 = rng.clone();
///     let task1 = tokio::spawn(async move {
///         rng1.gen::<u64>()
///     });
///
///     let rng2 = rng.clone();
///     let task2 = tokio::spawn(async move {
///         rng2.gen::<u64>()
///     });
///
///     let (v1, v2) = tokio::join!(task1, task2);
///     // Both tasks can safely use the same RNG
/// }
/// ```
pub struct TestRng {
    inner: Mutex<StdRng>,
    seed: u64,
}

impl TestRng {
    /// Create a new TestRng with an explicit seed
    ///
    /// Use this when you need a deterministic seed for a specific test case.
    /// For general testing, prefer `new_from_env_or_random()` which provides
    /// automatic seed logging and replay capability.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_tck::orchestrator::rng::TestRng;
    ///
    /// let rng = TestRng::with_seed(42);
    /// let value: u64 = rng.gen();
    /// // Same seed produces same sequence
    /// ```
    pub fn with_seed(seed: u64) -> Self {
        Self {
            inner: Mutex::new(StdRng::seed_from_u64(seed)),
            seed,
        }
    }

    /// Create RNG from environment variable or random seed
    ///
    /// This is the recommended constructor for most tests. It:
    /// 1. Checks for `TOS_TEST_SEED` environment variable
    /// 2. If found, uses that seed (for replay)
    /// 3. If not found, generates a random seed
    /// 4. Logs the seed to stderr for future replay
    ///
    /// # Environment Variable Format
    ///
    /// ```bash
    /// # Hexadecimal with 0x prefix
    /// export TOS_TEST_SEED=0x1234567890abcdef
    ///
    /// # Or without prefix
    /// export TOS_TEST_SEED=1234567890abcdef
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_tck::orchestrator::rng::TestRng;
    ///
    /// fn test_example() {
    ///     let rng = TestRng::new_from_env_or_random();
    ///     // Seed is logged to stderr:
    ///     // TestRng seed: 0xa3f5c8e1b2d94706
    ///     //    Replay: TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test ...
    ///
    ///     let value: u64 = rng.gen();
    ///     // Use value in test...
    /// }
    /// ```
    ///
    /// # Replay Failed Tests
    ///
    /// When test fails, copy the seed from output:
    ///
    /// ```bash
    /// TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test test_example
    /// ```
    pub fn new_from_env_or_random() -> Self {
        let seed = std::env::var("TOS_TEST_SEED")
            .ok()
            .and_then(|s| {
                // Support both "0x..." and raw hex
                let trimmed = s.trim().trim_start_matches("0x");
                u64::from_str_radix(trimmed, 16).ok()
            })
            .unwrap_or_else(|| {
                // Generate random seed using thread_rng
                let mut system_rng = rand::thread_rng();
                system_rng.gen()
            });

        eprintln!("🔁 TestRng seed: 0x{:016x}", seed);
        eprintln!("   Replay: TOS_TEST_SEED=0x{:016x} cargo test ...", seed);

        Self::with_seed(seed)
    }

    /// Get the seed used by this RNG
    ///
    /// Useful for logging or custom error messages when tests fail.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_tck::orchestrator::rng::TestRng;
    ///
    /// fn test_with_seed_logging() {
    ///     let rng = TestRng::new_from_env_or_random();
    ///
    ///     // Do test work...
    ///     let result = some_random_operation(&rng);
    ///
    ///     if result.is_err() {
    ///         eprintln!("Test failed with seed: 0x{:016x}", rng.seed());
    ///         panic!("Operation failed!");
    ///     }
    /// }
    ///
    /// # fn some_random_operation(rng: &TestRng) -> Result<(), ()> { Ok(()) }
    /// ```
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Generate a random value of type T
    ///
    /// Type T must implement `rand::distributions::Standard` distribution.
    /// Most primitive types (u8, u16, u32, u64, usize, bool, f32, f64, etc.)
    /// are supported.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_tck::orchestrator::rng::TestRng;
    ///
    /// let rng = TestRng::with_seed(42);
    ///
    /// // Generate different types
    /// let num: u64 = rng.gen();
    /// let flag: bool = rng.gen();
    /// let byte: u8 = rng.gen();
    /// ```
    pub fn gen<T>(&self) -> T
    where
        rand::distributions::Standard: rand::distributions::Distribution<T>,
    {
        self.inner.lock().gen()
    }

    /// Generate a random value in the given range
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_tck::orchestrator::rng::TestRng;
    ///
    /// let rng = TestRng::with_seed(42);
    ///
    /// // Random index in 0..100
    /// let index = rng.gen_range(0..100);
    /// assert!(index < 100);
    ///
    /// // Random amount in 1..=1000
    /// let amount = rng.gen_range(1..=1000);
    /// assert!(amount >= 1 && amount <= 1000);
    /// ```
    pub fn gen_range<T, R>(&self, range: R) -> T
    where
        T: rand::distributions::uniform::SampleUniform,
        R: rand::distributions::uniform::SampleRange<T>,
    {
        self.inner.lock().gen_range(range)
    }

    /// Fill a slice with random bytes
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_tck::orchestrator::rng::TestRng;
    ///
    /// let rng = TestRng::with_seed(42);
    ///
    /// let mut buffer = [0u8; 32];
    /// rng.fill_bytes(&mut buffer);
    /// // buffer now contains random bytes
    /// ```
    pub fn fill_bytes(&self, dest: &mut [u8]) {
        let mut rng = self.inner.lock();
        rng.fill_bytes(dest)
    }

    /// Shuffle a slice in place
    ///
    /// Uses Fisher-Yates shuffle algorithm for uniform distribution.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_tck::orchestrator::rng::TestRng;
    ///
    /// let rng = TestRng::with_seed(42);
    ///
    /// let mut items = vec![1, 2, 3, 4, 5];
    /// rng.shuffle(&mut items);
    /// // items are now in random order (deterministic for seed 42)
    /// ```
    pub fn shuffle<T>(&self, slice: &mut [T]) {
        use rand::seq::SliceRandom;
        slice.shuffle(&mut *self.inner.lock());
    }

    /// Choose a random element from a slice
    ///
    /// Returns `None` if the slice is empty.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_tck::orchestrator::rng::TestRng;
    ///
    /// let rng = TestRng::with_seed(42);
    ///
    /// let items = vec![10, 20, 30, 40, 50];
    /// let chosen = rng.choose(&items);
    /// assert!(chosen.is_some());
    /// assert!(items.contains(chosen.unwrap()));
    /// ```
    pub fn choose<'a, T>(&self, slice: &'a [T]) -> Option<&'a T> {
        use rand::seq::SliceRandom;
        slice.choose(&mut *self.inner.lock())
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_generation() {
        let rng1 = TestRng::with_seed(42);
        let rng2 = TestRng::with_seed(42);

        // Same seed should produce same sequence
        let values1: Vec<u64> = (0..10).map(|_| rng1.gen()).collect();
        let values2: Vec<u64> = (0..10).map(|_| rng2.gen()).collect();

        assert_eq!(values1, values2);
    }

    #[test]
    fn test_different_seeds_produce_different_values() {
        let rng1 = TestRng::with_seed(42);
        let rng2 = TestRng::with_seed(43);

        let values1: Vec<u64> = (0..10).map(|_| rng1.gen()).collect();
        let values2: Vec<u64> = (0..10).map(|_| rng2.gen()).collect();

        // Different seeds should (almost certainly) produce different sequences
        assert_ne!(values1, values2);
    }

    #[test]
    fn test_gen_range() {
        let rng = TestRng::with_seed(42);

        // Test range generation
        for _ in 0..100 {
            let value = rng.gen_range(0..10);
            assert!(value < 10);
        }

        // Test inclusive range
        for _ in 0..100 {
            let value = rng.gen_range(1..=10);
            assert!((1..=10).contains(&value));
        }
    }

    #[test]
    fn test_shuffle() {
        let rng1 = TestRng::with_seed(42);
        let rng2 = TestRng::with_seed(42);

        let mut items1 = vec![1, 2, 3, 4, 5];
        let mut items2 = vec![1, 2, 3, 4, 5];

        rng1.shuffle(&mut items1);
        rng2.shuffle(&mut items2);

        // Same seed produces same shuffle
        assert_eq!(items1, items2);
    }

    #[test]
    fn test_choose() {
        let rng = TestRng::with_seed(42);
        let items = vec![10, 20, 30, 40, 50];

        let chosen = rng.choose(&items);
        assert!(chosen.is_some());
        assert!(items.contains(chosen.unwrap()));

        // Empty slice returns None
        let empty: Vec<i32> = vec![];
        assert!(rng.choose(&empty).is_none());
    }

    #[test]
    fn test_fill_bytes() {
        let rng1 = TestRng::with_seed(42);
        let rng2 = TestRng::with_seed(42);

        let mut buffer1 = [0u8; 32];
        let mut buffer2 = [0u8; 32];

        rng1.fill_bytes(&mut buffer1);
        rng2.fill_bytes(&mut buffer2);

        // Same seed produces same bytes
        assert_eq!(buffer1, buffer2);
    }

    #[test]
    fn test_seed_retrieval() {
        let seed = 0x1234567890abcdef;
        let rng = TestRng::with_seed(seed);
        assert_eq!(rng.seed(), seed);
    }

    #[test]
    fn test_env_seed_parsing() {
        // Test with 0x prefix
        std::env::set_var("TOS_TEST_SEED", "0xdeadbeefcafebabe");
        let rng = TestRng::new_from_env_or_random();
        assert_eq!(rng.seed(), 0xdeadbeefcafebabe);

        // Test without prefix
        std::env::set_var("TOS_TEST_SEED", "1234567890abcdef");
        let rng = TestRng::new_from_env_or_random();
        assert_eq!(rng.seed(), 0x1234567890abcdef);

        // Clean up
        std::env::remove_var("TOS_TEST_SEED");
    }

    #[test]
    fn test_thread_safety() {
        use std::sync::Arc;

        let rng = Arc::new(TestRng::with_seed(42));
        let mut handles = vec![];

        // Spawn multiple threads using the same RNG
        for _ in 0..10 {
            let rng_clone = rng.clone();
            handles.push(std::thread::spawn(move || {
                let _value: u64 = rng_clone.gen();
            }));
        }

        // All threads should complete without panic
        for handle in handles {
            handle.join().unwrap();
        }
    }

    // ============================================================================
    // Additional RNG Tests for V3.0 Coverage
    // ============================================================================

    #[test]
    fn test_gen_different_types() {
        let rng = TestRng::with_seed(42);

        // Test generation of various types
        let _u8_val: u8 = rng.gen();
        let _u16_val: u16 = rng.gen();
        let _u32_val: u32 = rng.gen();
        let _u64_val: u64 = rng.gen();
        let _usize_val: usize = rng.gen();
        let _bool_val: bool = rng.gen();
        let _i32_val: i32 = rng.gen();
        let _i64_val: i64 = rng.gen();

        // All types should generate successfully
    }

    #[test]
    fn test_gen_range_boundary_conditions() {
        let rng = TestRng::with_seed(42);

        // Test single value range
        let value = rng.gen_range(5..6);
        assert_eq!(value, 5);

        // Test inclusive range with equal bounds
        let value = rng.gen_range(7..=7);
        assert_eq!(value, 7);
    }

    #[test]
    fn test_gen_range_large_range() {
        let rng = TestRng::with_seed(42);

        // Test with very large range
        for _ in 0..100 {
            let value = rng.gen_range(0..u64::MAX);
            assert!(value < u64::MAX);
        }
    }

    #[test]
    fn test_gen_range_negative_values() {
        let rng = TestRng::with_seed(42);

        // Test with negative range
        for _ in 0..100 {
            let value = rng.gen_range(-100..100);
            assert!((-100..100).contains(&value));
        }
    }

    #[test]
    fn test_fill_bytes_empty_buffer() {
        let rng = TestRng::with_seed(42);

        let mut buffer: [u8; 0] = [];
        rng.fill_bytes(&mut buffer);
        // Should not panic
    }

    #[test]
    fn test_fill_bytes_large_buffer() {
        let rng = TestRng::with_seed(42);

        let mut buffer = vec![0u8; 1024];
        rng.fill_bytes(&mut buffer);

        // Buffer should be filled with non-zero values (statistically)
        let non_zero_count = buffer.iter().filter(|&&b| b != 0).count();
        assert!(non_zero_count > 900); // Should have mostly non-zero bytes
    }

    #[test]
    fn test_fill_bytes_deterministic() {
        let rng1 = TestRng::with_seed(12345);
        let rng2 = TestRng::with_seed(12345);

        let mut buffer1 = [0u8; 100];
        let mut buffer2 = [0u8; 100];

        rng1.fill_bytes(&mut buffer1);
        rng2.fill_bytes(&mut buffer2);

        assert_eq!(buffer1, buffer2);
    }

    #[test]
    fn test_shuffle_empty_slice() {
        let rng = TestRng::with_seed(42);

        let mut items: Vec<i32> = vec![];
        rng.shuffle(&mut items);
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_shuffle_single_element() {
        let rng = TestRng::with_seed(42);

        let mut items = vec![42];
        rng.shuffle(&mut items);
        assert_eq!(items, vec![42]);
    }

    #[test]
    fn test_shuffle_deterministic() {
        let rng1 = TestRng::with_seed(999);
        let rng2 = TestRng::with_seed(999);

        let mut items1: Vec<u32> = (0..100).collect();
        let mut items2: Vec<u32> = (0..100).collect();

        rng1.shuffle(&mut items1);
        rng2.shuffle(&mut items2);

        assert_eq!(items1, items2);
    }

    #[test]
    fn test_shuffle_actually_shuffles() {
        let rng = TestRng::with_seed(42);

        let original: Vec<u32> = (0..100).collect();
        let mut shuffled = original.clone();

        rng.shuffle(&mut shuffled);

        // Should be different from original (with extremely high probability)
        assert_ne!(original, shuffled);

        // But should contain same elements
        let mut sorted_shuffled = shuffled.clone();
        sorted_shuffled.sort();
        assert_eq!(original, sorted_shuffled);
    }

    #[test]
    fn test_choose_empty_slice() {
        let rng = TestRng::with_seed(42);

        let items: Vec<i32> = vec![];
        let chosen = rng.choose(&items);
        assert!(chosen.is_none());
    }

    #[test]
    fn test_choose_single_element() {
        let rng = TestRng::with_seed(42);

        let items = vec![42];
        let chosen = rng.choose(&items);
        assert_eq!(chosen, Some(&42));
    }

    #[test]
    fn test_choose_deterministic() {
        let rng1 = TestRng::with_seed(777);
        let rng2 = TestRng::with_seed(777);

        let items = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

        let choices1: Vec<i32> = (0..20).map(|_| *rng1.choose(&items).unwrap()).collect();

        let choices2: Vec<i32> = (0..20).map(|_| *rng2.choose(&items).unwrap()).collect();

        assert_eq!(choices1, choices2);
    }

    #[test]
    fn test_choose_distribution() {
        let rng = TestRng::with_seed(42);

        let items = vec![1, 2, 3, 4, 5];
        let mut counts = [0; 5];

        // Choose many times
        for _ in 0..1000 {
            let chosen = rng.choose(&items).unwrap();
            counts[(*chosen - 1) as usize] += 1;
        }

        // Each item should be chosen at least once (statistically)
        for count in &counts {
            assert!(*count > 0);
        }
    }

    #[test]
    fn test_seed_zero() {
        let rng = TestRng::with_seed(0);
        assert_eq!(rng.seed(), 0);

        // Should still generate values
        let _value: u64 = rng.gen();
    }

    #[test]
    fn test_seed_max_value() {
        let rng = TestRng::with_seed(u64::MAX);
        assert_eq!(rng.seed(), u64::MAX);

        // Should still generate values
        let _value: u64 = rng.gen();
    }

    #[test]
    fn test_concurrent_generation_consistency() {
        use std::sync::Arc;

        let rng = Arc::new(TestRng::with_seed(42));
        let mut handles = vec![];

        // Spawn threads that generate values
        for _ in 0..10 {
            let rng_clone = rng.clone();
            handles.push(std::thread::spawn(move || {
                let mut values = Vec::new();
                for _ in 0..10 {
                    values.push(rng_clone.gen::<u64>());
                }
                values
            }));
        }

        // Collect results
        let results: Vec<Vec<u64>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // All threads should have generated 10 values
        for result in &results {
            assert_eq!(result.len(), 10);
        }
    }

    #[test]
    fn test_bool_generation_distribution() {
        let rng = TestRng::with_seed(42);

        let mut true_count = 0;
        let mut false_count = 0;

        for _ in 0..1000 {
            if rng.gen::<bool>() {
                true_count += 1;
            } else {
                false_count += 1;
            }
        }

        // Should have roughly equal distribution (allow 30-70% range)
        assert!(true_count > 300 && true_count < 700);
        assert!(false_count > 300 && false_count < 700);
    }

    #[test]
    fn test_seed_from_env_with_invalid_value() {
        // Set invalid seed
        std::env::set_var("TOS_TEST_SEED", "invalid_hex");

        // Should fall back to random seed and not panic
        let rng = TestRng::new_from_env_or_random();

        // Should be able to generate values
        let _value: u64 = rng.gen();

        // Clean up
        std::env::remove_var("TOS_TEST_SEED");
    }

    #[test]
    fn test_reproducibility_across_sessions() {
        // Simulate two different test runs with same seed
        let seed = 0xabcdef1234567890;

        // Session 1
        let rng1 = TestRng::with_seed(seed);
        let sequence1: Vec<u64> = (0..50).map(|_| rng1.gen()).collect();

        // Session 2 (new RNG instance)
        let rng2 = TestRng::with_seed(seed);
        let sequence2: Vec<u64> = (0..50).map(|_| rng2.gen()).collect();

        assert_eq!(sequence1, sequence2);
    }

    #[test]
    fn test_gen_range_float() {
        let rng = TestRng::with_seed(42);

        for _ in 0..100 {
            let value = rng.gen_range(0.0..1.0);
            assert!((0.0..1.0).contains(&value));
        }
    }
}
