// Example: Deterministic Testing Infrastructure Demo
//
// This example demonstrates the V3.0 deterministic testing components:
// - Clock abstraction (SystemClock, PausedClock)
// - Seeded RNG (TestRng)
// - DeterministicTestEnv (unified environment)
//
// Run this example with:
//   cargo run --example deterministic_demo
//
// To reproduce with a specific seed:
//   TOS_TEST_SEED=0x1234567890abcdef cargo run --example deterministic_demo

use std::sync::Arc;
use tokio::time::Duration;
use tos_testing_framework::orchestrator::{Clock, DeterministicTestEnv, PausedClock, SystemClock};

#[tokio::main]
async fn main() {
    println!("========================================");
    println!("TOS Testing Framework V3.0");
    println!("Deterministic Infrastructure Demo");
    println!("========================================\n");

    demo_system_clock().await;
    demo_paused_clock().await;
    demo_test_rng();
    demo_deterministic_env().await;
    demo_seed_replay().await;

    println!("\n========================================");
    println!("All demos completed successfully!");
    println!("========================================");
}

/// Demo 1: SystemClock (Production)
async fn demo_system_clock() {
    println!("Demo 1: SystemClock (Production Use)");
    println!("--------------------------------------");

    let clock: Arc<dyn Clock> = Arc::new(SystemClock);

    let start = clock.now();
    println!("Starting real sleep for 100ms...");

    clock.sleep(Duration::from_millis(100)).await;

    let elapsed = clock.now() - start;
    println!("Elapsed: {:?} (actual real time)", elapsed);
    println!();
}

/// Demo 2: PausedClock (Testing)
async fn demo_paused_clock() {
    println!("Demo 2: PausedClock (Test Use)");
    println!("-------------------------------");

    // Pause time at runtime start
    tokio::time::pause();

    let clock = Arc::new(PausedClock::new());

    let start = clock.now();
    println!("Time is paused. Advancing by 1 hour instantly...");

    // Advance time by 1 hour (instant, no real delay)
    clock.advance(Duration::from_secs(3600)).await;

    let elapsed = clock.now() - start;
    println!("Elapsed: {:?} (simulated time)", elapsed);
    println!("Real elapsed: ~0ms (instant advancement)");

    // Resume time for other demos
    tokio::time::resume();
    println!();
}

/// Demo 3: TestRng (Seeded Randomness)
fn demo_test_rng() {
    println!("Demo 3: TestRng (Seeded Random)");
    println!("--------------------------------");

    // Create RNG with specific seed
    use tos_testing_framework::orchestrator::TestRng;

    let rng1 = TestRng::with_seed(42);
    let rng2 = TestRng::with_seed(42);

    // Same seed produces same sequence
    let values1: Vec<u64> = (0..5).map(|_| rng1.gen()).collect();
    let values2: Vec<u64> = (0..5).map(|_| rng2.gen()).collect();

    println!("Seed: 0x{:016x}", rng1.seed());
    println!("RNG1 values: {:?}", values1);
    println!("RNG2 values: {:?}", values2);
    println!("Values match: {}", values1 == values2);

    // Different seed produces different sequence
    let rng3 = TestRng::with_seed(43);
    let values3: Vec<u64> = (0..5).map(|_| rng3.gen()).collect();
    println!("RNG3 (seed 43): {:?}", values3);
    println!("Different from seed 42: {}", values1 != values3);
    println!();
}

/// Demo 4: DeterministicTestEnv (Unified Environment)
async fn demo_deterministic_env() {
    println!("Demo 4: DeterministicTestEnv (Unified)");
    println!("---------------------------------------");

    // Pause time for this demo
    tokio::time::pause();

    let env = DeterministicTestEnv::with_seed(12345);

    println!("Environment seed: 0x{:016x}", env.seed());

    // Use clock for time control
    let start = env.clock.now();
    println!("Advancing time by 10 seconds...");
    env.advance_time(Duration::from_secs(10)).await;

    let elapsed = env.clock.now() - start;
    println!("Time advanced: {:?}", elapsed);

    // Use RNG for random data
    let random_value: u64 = env.rng.gen();
    println!("Random value: {}", random_value);

    // Use RNG for range
    let random_delay = env.rng.gen_range(1..10);
    println!("Random delay (1-10s): {}s", random_delay);

    // Resume time
    tokio::time::resume();
    println!();
}

/// Demo 5: Seed Replay (Failure Reproduction)
async fn demo_seed_replay() {
    println!("Demo 5: Seed Replay (Reproduce Failures)");
    println!("-----------------------------------------");

    // Pause time
    tokio::time::pause();

    // Create environment from env var or random
    let env = DeterministicTestEnv::new_time_paused();

    println!(
        "Current seed: 0x{:016x} (check stderr for replay command)",
        env.seed()
    );

    // Simulate test logic
    let random_values: Vec<u64> = (0..5).map(|_| env.rng.gen()).collect();
    println!("Random test data: {:?}", random_values);

    // Simulate time-dependent operation
    env.advance_time(Duration::from_secs(100)).await;

    // If test fails, call on_failure() to print replay instructions
    println!("\nIf this test failed, call env.on_failure():");
    env.on_failure();

    // Resume time
    tokio::time::resume();
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_testing_framework::orchestrator::TestRng;

    #[tokio::test(start_paused = true)]
    async fn test_paused_clock_in_test() {
        let clock = Arc::new(PausedClock::new());
        let start = clock.now();

        // Advance 1 hour
        clock.advance(Duration::from_secs(3600)).await;

        let elapsed = clock.now() - start;
        assert_eq!(elapsed, Duration::from_secs(3600));
    }

    #[test]
    fn test_deterministic_rng() {
        let rng1 = TestRng::with_seed(42);
        let rng2 = TestRng::with_seed(42);

        let v1: u64 = rng1.gen();
        let v2: u64 = rng2.gen();

        assert_eq!(v1, v2);
    }

    #[tokio::test(start_paused = true)]
    async fn test_deterministic_env_usage() {
        let env = DeterministicTestEnv::with_seed(999);

        // Time control
        let start = env.clock.now();
        env.advance_time(Duration::from_secs(60)).await;
        assert_eq!(env.clock.now() - start, Duration::from_secs(60));

        // Random control
        let value = env.rng.gen_range(1..100);
        assert!(value >= 1 && value < 100);
    }

    #[tokio::test(start_paused = true)]
    async fn test_reproducible_random_sequence() {
        let env1 = DeterministicTestEnv::with_seed(777);
        let env2 = DeterministicTestEnv::with_seed(777);

        let values1: Vec<u64> = (0..10).map(|_| env1.rng.gen()).collect();
        let values2: Vec<u64> = (0..10).map(|_| env2.rng.gen()).collect();

        assert_eq!(values1, values2);
    }
}
