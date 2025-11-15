// File: testing-framework/src/orchestrator/mod.rs
//
// Orchestrator Module - V3.0 Deterministic Infrastructure
//
// This module provides the unified test orchestration components for
// deterministic, reproducible testing across all tiers of the testing framework.

/// Clock abstractions for deterministic time control in tests
pub mod clock;
/// Deterministic random number generation for reproducible tests
pub mod rng;

use std::sync::Arc;

/// Complete deterministic test environment
///
/// DeterministicTestEnv combines Clock and RNG into a single environment
/// that provides full control over time and randomness in tests. This enables
/// perfectly reproducible test execution where every run with the same seed
/// produces identical results.
///
/// # Design Goals
///
/// 1. **Determinism**: Control all sources of non-determinism (time, randomness)
/// 2. **Reproducibility**: Failed tests can be replayed exactly via seed
/// 3. **Fast Execution**: Paused time eliminates real delays in tests
/// 4. **Simple API**: Single struct provides everything needed
///
/// # Usage Patterns
///
/// ## Tier 1: Component Tests (In-Process)
///
/// ```rust
/// use tos_testing_framework::orchestrator::DeterministicTestEnv;
/// use tokio::time::Duration;
///
/// #[tokio::test(start_paused = true)]
/// async fn test_component_with_time() {
///     let env = DeterministicTestEnv::new_time_paused();
///
///     // Use the clock for time-dependent logic
///     let start = env.clock.now();
///     env.advance_time(Duration::from_secs(3600)).await;
///     let elapsed = env.clock.now() - start;
///     assert_eq!(elapsed, Duration::from_secs(3600));
///
///     // Use RNG for random test data
///     let random_amount: u64 = env.rng.gen_range(1..1000);
///     assert!(random_amount < 1000);
/// }
/// ```
///
/// ## Tier 2: Integration Tests (Single Node)
///
/// ```rust
/// use tos_testing_framework::orchestrator::DeterministicTestEnv;
///
/// #[tokio::test(start_paused = true)]
/// async fn test_daemon_timeout() {
///     let env = DeterministicTestEnv::new_time_paused();
///
///     // Create daemon with injected clock
///     // let daemon = TestDaemonBuilder::new()
///     //     .with_clock(env.clock.clone())
///     //     .build()
///     //     .await
///     //     .unwrap();
///
///     // Simulate timeout by advancing time
///     env.advance_time(Duration::from_secs(30)).await;
///
///     // Verify timeout behavior...
/// }
/// ```
///
/// ## Tier 3: E2E Tests (Multi-Node)
///
/// ```rust
/// use tos_testing_framework::orchestrator::DeterministicTestEnv;
///
/// #[tokio::test(start_paused = true)]
/// async fn test_network_consensus() {
///     let env = DeterministicTestEnv::new_time_paused();
///
///     // Create network with deterministic configuration
///     // let network = NetworkBuilder::new()
///     //     .with_nodes(5)
///     //     .with_clock(env.clock.clone())
///     //     .with_rng_seed(env.rng.seed())
///     //     .build()
///     //     .await
///     //     .unwrap();
///
///     // Run test scenario with controlled time...
/// }
/// ```
///
/// ## Error Handling and Replay
///
/// ```rust
/// use tos_testing_framework::orchestrator::DeterministicTestEnv;
///
/// #[tokio::test(start_paused = true)]
/// async fn test_with_error_handling() {
///     let env = DeterministicTestEnv::new_time_paused();
///
///     // Run test logic
///     let result = run_complex_test(&env).await;
///
///     // On failure, print replay instructions
///     if result.is_err() {
///         env.on_failure();
///         panic!("Test failed!");
///     }
/// }
///
/// # async fn run_complex_test(env: &DeterministicTestEnv) -> Result<(), ()> {
/// #     Ok(())
/// # }
/// ```
pub struct DeterministicTestEnv {
    /// Clock for time control (SystemClock in production, PausedClock in tests)
    pub clock: Arc<dyn Clock>,

    /// Seeded RNG for reproducible randomness
    pub rng: TestRng,
}

impl DeterministicTestEnv {
    /// Create a new environment with time paused (for testing)
    ///
    /// This is the standard constructor for test code. It creates:
    /// - PausedClock: Time only advances when explicitly told to
    /// - TestRng: Seeded from environment or randomly (with logging)
    ///
    /// # Important Notes
    ///
    /// 1. Must use `#[tokio::test(start_paused = true)]` attribute
    /// 2. Time advancement is manual via `advance_time()`
    /// 3. Seed is logged for replay if test fails
    ///
    /// # Examples
    ///
    /// ## Basic Usage
    ///
    /// ```rust
    /// use tos_testing_framework::orchestrator::DeterministicTestEnv;
    /// use tokio::time::Duration;
    ///
    /// #[tokio::test(start_paused = true)]
    /// async fn test_example() {
    ///     let env = DeterministicTestEnv::new_time_paused();
    ///
    ///     // Time is paused, advance manually
    ///     env.advance_time(Duration::from_secs(10)).await;
    ///
    ///     // Generate random values
    ///     let random: u64 = env.rng.gen();
    /// }
    /// ```
    ///
    /// ## With Seed Replay
    ///
    /// When test fails, you'll see:
    ///
    /// ```text
    /// TestRng seed: 0xa3f5c8e1b2d94706
    ///    Replay: TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test ...
    /// ```
    ///
    /// To reproduce:
    ///
    /// ```bash
    /// TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test test_example
    /// ```
    pub fn new_time_paused() -> Self {
        Self {
            clock: Arc::new(clock::PausedClock::new()),
            rng: rng::TestRng::new_from_env_or_random(),
        }
    }

    /// Create environment with a specific seed (for debugging)
    ///
    /// Use this when you want to reproduce a specific test run with a known seed.
    /// This is useful when debugging a failed test that printed its seed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_testing_framework::orchestrator::DeterministicTestEnv;
    ///
    /// #[tokio::test(start_paused = true)]
    /// async fn test_with_specific_seed() {
    ///     // Reproduce exact behavior from failed run
    ///     let env = DeterministicTestEnv::with_seed(0xa3f5c8e1b2d94706);
    ///
    ///     // Test will use this exact seed
    ///     let random: u64 = env.rng.gen();
    ///     // ... same random values as original run
    /// }
    /// ```
    pub fn with_seed(seed: u64) -> Self {
        Self {
            clock: Arc::new(clock::SystemClock),
            rng: rng::TestRng::with_seed(seed),
        }
    }

    /// Create a new environment with paused time and specific seed
    ///
    /// This combines time control and deterministic RNG with a specific seed.
    /// Useful for tests that need both time advancement and reproducible randomness.
    ///
    /// # Arguments
    ///
    /// * `seed` - The RNG seed to use
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// #[tokio::test]
    /// async fn test_with_time_and_rng() {
    ///     let env = DeterministicTestEnv::new_time_paused_with_seed(12345);
    ///     let random_delay = env.rng.gen_range(1..10);
    ///     env.advance_time(Duration::from_secs(random_delay)).await;
    /// }
    /// ```
    pub fn new_time_paused_with_seed(seed: u64) -> Self {
        Self {
            clock: Arc::new(clock::PausedClock::new()),
            rng: rng::TestRng::with_seed(seed),
        }
    }

    /// Advance time by the specified duration
    ///
    /// This is a convenience method that forwards to the underlying PausedClock.
    /// Only works when using PausedClock (i.e., when created via `new_time_paused()`).
    ///
    /// # Panics
    ///
    /// Panics if the clock is not a PausedClock (shouldn't happen in normal usage).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_testing_framework::orchestrator::DeterministicTestEnv;
    /// use tokio::time::Duration;
    ///
    /// #[tokio::test(start_paused = true)]
    /// async fn test_time_advancement() {
    ///     let env = DeterministicTestEnv::new_time_paused();
    ///     let start = env.clock.now();
    ///
    ///     // Advance by 1 hour
    ///     env.advance_time(Duration::from_secs(3600)).await;
    ///
    ///     let elapsed = env.clock.now() - start;
    ///     assert_eq!(elapsed, Duration::from_secs(3600));
    /// }
    /// ```
    pub async fn advance_time(&self, duration: tokio::time::Duration) {
        // Note: This requires the clock to be a PausedClock
        // In practice, this is always the case when using new_time_paused()
        tokio::time::advance(duration).await
    }

    /// Get the current RNG seed
    ///
    /// Useful for custom logging or debugging messages.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_testing_framework::orchestrator::DeterministicTestEnv;
    ///
    /// #[test]
    /// fn test_seed_logging() {
    ///     let env = DeterministicTestEnv::new_time_paused();
    ///     println!("Using seed: 0x{:016x}", env.seed());
    /// }
    /// ```
    pub fn seed(&self) -> u64 {
        self.rng.seed()
    }

    /// Print failure message with replay instructions
    ///
    /// Call this when a test fails to provide the user with clear instructions
    /// on how to reproduce the exact failure using the same seed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tos_testing_framework::orchestrator::DeterministicTestEnv;
    ///
    /// #[tokio::test(start_paused = true)]
    /// async fn test_with_failure_handling() {
    ///     let env = DeterministicTestEnv::new_time_paused();
    ///
    ///     let result = complex_test_logic(&env).await;
    ///
    ///     if result.is_err() {
    ///         env.on_failure();
    ///         panic!("Test failed! See seed above for replay.");
    ///     }
    /// }
    ///
    /// # async fn complex_test_logic(env: &DeterministicTestEnv) -> Result<(), ()> {
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// Output when test fails:
    ///
    /// ```text
    /// ❌ Test failed! Replay with:
    ///    TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test ...
    /// ```
    pub fn on_failure(&self) {
        eprintln!("❌ Test failed! Replay with:");
        eprintln!("   TOS_TEST_SEED=0x{:016x} cargo test ...", self.rng.seed());
    }
}

// Re-export key types for convenience
pub use clock::{Clock, PausedClock, SystemClock};
pub use rng::TestRng;

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_deterministic_env_creation() {
        let env = DeterministicTestEnv::new_time_paused();

        // Should have a clock
        let _now = env.clock.now();

        // Should have an RNG with a seed
        let seed = env.seed();
        assert!(seed > 0);
    }

    #[tokio::test]
    async fn test_time_advancement() {
        let env = DeterministicTestEnv::new_time_paused();
        let start = env.clock.now();

        // Advance time
        env.advance_time(Duration::from_secs(100)).await;

        let elapsed = env.clock.now() - start;
        assert_eq!(elapsed, Duration::from_secs(100));
    }

    #[tokio::test]
    async fn test_deterministic_rng() {
        let env1 = DeterministicTestEnv::with_seed(42);
        let env2 = DeterministicTestEnv::with_seed(42);

        // Same seed should produce same random sequence
        let values1: Vec<u64> = (0..10).map(|_| env1.rng.gen()).collect();
        let values2: Vec<u64> = (0..10).map(|_| env2.rng.gen()).collect();

        assert_eq!(values1, values2);
    }

    #[tokio::test]
    async fn test_combined_time_and_random() {
        let env = DeterministicTestEnv::new_time_paused_with_seed(12345);

        // Use both time and randomness
        let start = env.clock.now();
        let random_delay = env.rng.gen_range(1..10);

        env.advance_time(Duration::from_secs(random_delay)).await;

        let elapsed = env.clock.now() - start;
        assert_eq!(elapsed, Duration::from_secs(random_delay));
    }

    #[tokio::test]
    async fn test_seed_retrieval() {
        let seed = 0xdeadbeefcafebabe;
        let env = DeterministicTestEnv::with_seed(seed);
        assert_eq!(env.seed(), seed);
    }

    #[tokio::test]
    async fn test_on_failure_doesnt_panic() {
        let env = DeterministicTestEnv::with_seed(42);
        // Should just print to stderr, not panic
        env.on_failure();
    }
}
