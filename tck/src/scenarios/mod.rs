//! DSL scenario parser and executor
//!
//! Parses YAML scenario files with V3.0 format:
//! - `within {target, tolerance}` assertions
//! - `compare {gte|lte|gt|lt}` assertions
//! - No `_` or `~` in numbers (use strings: "1000000000000")
//!
//! ## Example Scenario
//!
//! ```yaml
//! name: "Simple Transfer"
//! description: "Alice transfers to Bob"
//! genesis:
//!   network: "devnet"
//!   accounts:
//!     - name: "alice"
//!       balance: "1000000000000"
//!     - name: "bob"
//!       balance: "0"
//! steps:
//!   - action: "transfer"
//!     from: "alice"
//!     to: "bob"
//!     amount: "100000000000"
//!     fee: "50"
//!   - action: "mine_block"
//!   - action: "assert_balance"
//!     account: "bob"
//!     eq: "100000000000"
//!   - action: "assert_balance"
//!     account: "alice"
//!     within:
//!       target: "899999999950"
//!       tolerance: "100"
//! invariants:
//!   - "balance_conservation"
//!   - "nonce_monotonicity"
//! ```

pub mod executor;
pub mod parser;

pub use executor::ScenarioExecutor;
pub use parser::{parse_scenario, TestScenario};
