//! # TOS Testing Framework V3.0 - Production-Ready
//!
//! Comprehensive, deterministic, multi-tier testing framework for TOS blockchain.
//!
//! ## Architecture Overview
//!
//! Five-tier testing pyramid:
//! - **Tier 0**: Unit tests (pure functions, < 100ms)
//! - **Tier 1**: Component tests (TestBlockchain, < 1s)
//! - **Tier 2**: Integration tests (TestDaemon + RPC, 1-5s)
//! - **Tier 3**: E2E tests (multi-node, consensus)
//! - **Tier 4**: Chaos & property-based tests (weekly/nightly)
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use tos_testing_framework::prelude::*;
//!
//! #[tokio::test(start_paused = true)]
//! async fn test_simple_transfer() {
//!     let clock = Arc::new(PausedClock::new());
//!     let blockchain = TestBlockchainBuilder::new()
//!         .with_clock(clock)
//!         .build()
//!         .await
//!         .unwrap();
//!
//!     // Your test here...
//! }
//! ```
//!
//! ## Features
//!
//! - **default**: Core testing framework
//! - **chaos**: Chaos engineering and property-based testing (requires proptest)
//! - **full**: All features enabled
//!
//! ## Design Principles
//!
//! 1. **Deterministic**: Clock abstraction + seeded RNG
//! 2. **State Equivalence**: Parallel ≡ Sequential execution
//! 3. **Production Consistency**: Real RocksDB, minimal mocks
//! 4. **Fast Feedback**: 80% of tests < 1s
//! 5. **Comprehensive Coverage**: All layers from unit to chaos

#![warn(missing_docs)]
#![warn(clippy::all)]

/// Core orchestration - provides Clock, RNG, deterministic environment
pub mod orchestrator;

// Tier 1: Component-level testing (in-process, no RPC/P2P)
pub mod tier1_component;

/// Tier 2: Integration testing (single daemon + RPC)
pub mod tier2_integration;

/// Tier 3: E2E testing (multi-node networks)
pub mod tier3_e2e;

// Tier 4: Chaos & property-based testing
#[cfg(feature = "chaos")]
pub mod tier4_chaos;

/// Shared utilities across all tiers
pub mod utilities;

// Core invariant checkers (balance conservation, nonce monotonicity, etc.)
pub mod invariants;

// DSL scenario parser and executor
pub mod scenarios;

// Convenient re-exports for common usage
pub mod prelude;

// Re-export commonly used types at crate root
pub use orchestrator::{Clock, DeterministicTestEnv, PausedClock, SystemClock, TestRng};
pub use tier1_component::{TestBlockchain, TestBlockchainBuilder};

/// Framework version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Framework version descriptor
pub const FRAMEWORK_VERSION: &str = "TOS Testing Framework V3.0";
