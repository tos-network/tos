//! # TOS-TCK: Technology Compatibility Kit
//!
//! Comprehensive testing framework for the TOS blockchain, inspired by Java's TCK.
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
//! Plus TCK-specific modules:
//! - **Conformance**: Specification-driven testing (like Java TCK)
//! - **Fuzzing**: Security testing with cargo-fuzz
//! - **Formal**: Mathematical proofs with Kani
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use tos_tck::prelude::*;
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
//! - **default**: Tier 1 + Tier 2 tests
//! - **tier3**: E2E multi-node tests
//! - **tier4/chaos**: Property-based testing
//! - **conformance**: TCK conformance test runner
//! - **fuzz**: Fuzzing infrastructure
//! - **formal**: Formal verification with Kani
//! - **full**: All features enabled
//!
//! ## Design Principles
//!
//! 1. **Deterministic**: Clock abstraction + seeded RNG
//! 2. **State Equivalence**: Parallel ≡ Sequential execution
//! 3. **Production Consistency**: Real RocksDB, minimal mocks
//! 4. **Fast Feedback**: 80% of tests < 1s
//! 5. **Comprehensive Coverage**: All layers from unit to chaos
//! 6. **Security First**: Fuzzing and formal verification

#![warn(missing_docs)]
#![warn(clippy::all)]
#![allow(unexpected_cfgs)]

// =============================================================================
// Core Testing Infrastructure (from testing-framework)
// =============================================================================

/// Core orchestration - provides Clock, RNG, deterministic environment
pub mod orchestrator;

/// Tier 1: Component-level testing (in-process, no RPC/P2P)
pub mod tier1_component;

/// Tier 2: Integration testing (single daemon + RPC)
pub mod tier2_integration;

/// Tier 3: E2E testing (multi-node networks)
pub mod tier3_e2e;

/// Tier 4: Chaos & property-based testing
#[cfg(feature = "chaos")]
pub mod tier4_chaos;

/// Shared utilities across all tiers
pub mod utilities;

/// Core invariant checkers (balance conservation, nonce monotonicity, etc.)
pub mod invariants;

/// DSL scenario parser and executor
pub mod scenarios;

/// Convenient re-exports for common usage
pub mod prelude;

/// Doc-test helpers (always available since this is a testing framework)
pub mod doc_test_helpers;

/// Test utilities for AI-generated tests (TestEnv, helpers)
pub mod test_utils;

// =============================================================================
// TCK-Specific Modules (New)
// =============================================================================

/// TCK Conformance Testing - Specification-driven tests like Java TCK
///
/// Verifies that TOS implementation correctly follows the specification.
/// Tests are defined in YAML format for readability.
///
/// ```ignore
/// use tos_tck::conformance::{ConformanceRunner, Category};
///
/// let runner = ConformanceRunner::load_from_dir("specs/syscalls")?;
/// let report = runner.run_category(Category::Syscalls).await;
/// assert_eq!(report.failed, 0);
/// ```
pub mod conformance;

/// TCK Fuzzing Infrastructure - Security testing with cargo-fuzz
///
/// Provides fuzzing targets for finding edge cases and vulnerabilities.
///
/// ```bash
/// cargo +nightly fuzz run fuzz_transaction
/// ```
pub mod fuzz;

/// TCK Formal Verification - Mathematical proofs with Kani
///
/// Provides Kani proofs for critical properties like balance conservation.
///
/// ```bash
/// cargo kani --features formal
/// ```
pub mod formal;

// =============================================================================
// Re-exports
// =============================================================================

// Re-export commonly used types at crate root
pub use orchestrator::{Clock, DeterministicTestEnv, PausedClock, SystemClock, TestRng};
pub use tier1_component::{TestBlockchain, TestBlockchainBuilder};

// Re-export conformance types
pub use conformance::{Category, ConformanceRunner, ConformanceSpec, TestReport, TestStatus};

// Re-export fuzz types
pub use fuzz::{FuzzConfig, FuzzTarget};

// Re-export formal types
pub use formal::{InvariantViolation, VerifiableProperty};

// =============================================================================
// Version Information
// =============================================================================

/// TCK version from Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Framework version descriptor
pub const TCK_VERSION: &str = "TOS-TCK V1.0";

/// Framework version (alias for backward compatibility)
pub const FRAMEWORK_VERSION: &str = TCK_VERSION;

/// Previous framework version (for compatibility reference)
pub const LEGACY_FRAMEWORK_VERSION: &str = "TOS Testing Framework V3.0";
