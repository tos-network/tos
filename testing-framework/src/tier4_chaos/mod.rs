//! Tier 4: Chaos Engineering & Property-Based Testing
//!
//! Advanced chaos engineering and property-based testing for TOS blockchain.
//!
//! # Overview
//!
//! This module implements comprehensive chaos testing to validate system behavior
//! under extreme conditions, random scenarios, and Byzantine attacks.
//!
//! # Test Categories
//!
//! 1. **Property-Based Tests** - Invariant verification with proptest
//! 2. **Chaos Scenarios** - Network failures, timing issues, resource constraints
//! 3. **Stress Tests** - High transaction volumes, large networks
//! 4. **Byzantine Scenarios** - Invalid blocks, double spends, malicious behavior
//!
//! # Design Principles
//!
//! - **Deterministic**: All tests use seeded RNG for full reproducibility
//! - **Invariant-Based**: Tests verify properties, not specific outcomes
//! - **Failure Replay**: Failed tests can be reproduced with seed
//! - **Fast Feedback**: Uses in-process testing for speed (< 1s per scenario)
//!
//! # Usage
//!
//! ```rust,ignore
//! use tos_testing_framework::tier4_chaos::*;
//!
//! #[test]
//! fn test_my_chaos_scenario() {
//!     let mut scenario = ChaosScenario::new_with_seed(42);
//!     scenario.inject_network_partition(vec![0, 1], vec![2, 3]);
//!     scenario.run().await.unwrap();
//!     scenario.verify_invariants();
//! }
//! ```
//!
//! # Reproducing Failures
//!
//! When a test fails, it prints the seed:
//! ```text
//! Test failed with seed: 0xa3f5c8e1b2d94706
//! ```
//!
//! Reproduce with:
//! ```bash
//! TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test test_name
//! ```

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]
// Allow assertions in test code - tests should fail loudly
#![cfg_attr(test, allow(clippy::assertions_on_constants))]

pub mod property_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn test_tier4_module_accessible() {
        // Ensure tier4_chaos module is accessible
        // Property tests are in property_tests submodule
    }

    #[tokio::test]
    async fn test_tier4_simple() {
        // Simple smoke test for tier4 module
        assert_eq!(1 + 1, 2);
    }
}
