//! Transaction Fixture Testing Framework
//!
//! Declarative input/output testing where authors specify:
//! 1. Initial state (accounts, balances, assets)
//! 2. Transaction sequence (operations to execute)
//! 3. Expected final state (balances, nonces, energy, UNO)
//! 4. Invariants to verify
//!
//! The framework handles setup, execution, and verification automatically.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                 Transaction Fixture Testing                   │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                               │
//! │  ┌─────────────────────────────────────────────────────────┐ │
//! │  │  Fixture YAML    ──▶    Parser    ──▶    Runner          │ │
//! │  └─────────────────────────────────────────────────────────┘ │
//! │                              │                                 │
//! │              ┌───────────────┼───────────────┐                │
//! │              ▼               ▼               ▼                │
//! │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐         │
//! │  │TestBlockchain│ │  ChainClient │ │ LocalCluster │         │
//! │  │ (Tier 1)     │ │  (Tier 1.5)  │ │  (Tier 3)    │         │
//! │  └──────────────┘ └──────────────┘ └──────────────┘         │
//! │              │               │               │                │
//! │              ▼               ▼               ▼                │
//! │  ┌─────────────────────────────────────────────────────────┐ │
//! │  │  Verification: Balance | Nonce | Energy | Invariants    │ │
//! │  └─────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use tos_tck::fixtures::{run_fixture_on_backend, TestBlockchainBackend};
//!
//! #[tokio::test]
//! async fn test_basic_transfer() {
//!     let yaml = r#"
//!     fixture:
//!       name: "basic_transfer"
//!     setup:
//!       accounts:
//!         alice: { balance: "10000" }
//!         bob:   { balance: "0" }
//!     transactions:
//!       - step: 1
//!         type: transfer
//!         from: alice
//!         to: bob
//!         amount: "1000"
//!         expect_status: success
//!     expected:
//!       accounts:
//!         alice: { balance: "9000" }
//!         bob:   { balance: "1000" }
//!     "#;
//!
//!     let mut backend = TestBlockchainBackend::new();
//!     let result = run_fixture_on_backend(yaml, &mut backend).await.unwrap();
//!     assert!(result.all_passed());
//! }
//! ```

pub mod backend;
pub mod backends;
pub mod invariants;
pub mod parser;
/// Regression capture utility for generating fixture files from observed behavior
pub mod regression;
pub mod runner;
pub mod types;
pub mod verification;

// Re-export commonly used types
pub use backend::FixtureBackend;
pub use backends::tier1::TestBlockchainBackend;
pub use backends::tier1_5::ChainClientBackend;
pub use backends::tier2::TestDaemonBackend;
pub use backends::tier3::LocalClusterBackend;
pub use parser::{parse_fixture, parse_fixture_file};
pub use regression::RegressionCapture;
pub use runner::{
    create_backend, execute_fixture, run_fixture_cross_tier, run_fixture_file_on_backend,
    run_fixture_on_backend,
};
pub use types::{
    AccountState, CrossTierResult, EnergyState, ExpectStatus, Fixture, FixtureMeta, FixtureResult,
    FixtureSetup, Step, StepResult, Tier, TransactionStep, TransactionType,
};
