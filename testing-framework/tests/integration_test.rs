#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::useless_vec)]

//! Basic integration test to verify framework compilation
//!
//! This smoke test ensures the framework compiles and basic
//! functionality is accessible.

use tos_testing_framework::prelude::*;

#[tokio::test]
async fn test_framework_basic_imports() {
    // Verify clock abstraction works
    // PausedClock::new() calls time::pause() internally
    let clock = Arc::new(PausedClock::new());
    let start = clock.now();

    clock.advance(Duration::from_secs(1)).await;
    let end = clock.now();

    assert!(end > start, "Clock should advance");
}

#[test]
fn test_rng_seed_creation() {
    // Verify RNG can be created with seed
    let rng = TestRng::with_seed(12345);
    assert_eq!(rng.seed(), 12345);
}

#[test]
fn test_framework_version() {
    // Verify version constants are accessible
    use tos_testing_framework::{FRAMEWORK_VERSION, VERSION};

    assert_eq!(VERSION, "0.1.4");
    assert_eq!(FRAMEWORK_VERSION, "TOS Testing Framework V3.0");
}

#[tokio::test]
async fn test_deterministic_env_creation() {
    // Verify deterministic environment can be created
    let env = DeterministicTestEnv::new_time_paused();

    // Clock should be available
    let now = env.clock.now();
    assert!(now.elapsed().as_millis() < 100);

    // RNG should be available
    let seed = env.rng.seed();
    assert!(seed > 0);
}

#[test]
fn test_system_clock() {
    // Verify SystemClock is available
    let clock = SystemClock;
    let _ = clock.now();
}

// Test that feature flags work correctly
#[cfg(feature = "chaos")]
#[test]
fn test_chaos_feature_enabled() {
    // This test only compiles if chaos feature is enabled
    // TODO: Add chaos types when they are implemented
    // let _ = std::marker::PhantomData::<FaultInjector>;
    // let _ = std::marker::PhantomData::<TimeSkew>;
    // Verify chaos feature is enabled by the fact that this test compiles
}

#[cfg(not(feature = "chaos"))]
#[test]
fn test_chaos_feature_disabled() {
    // This test only compiles if chaos feature is disabled
    // The fact that this test runs verifies default features are minimal
}
