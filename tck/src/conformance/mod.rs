//! # TCK Conformance Testing Module
//!
//! Provides specification-driven testing for TOS syscalls and APIs.
//! Inspired by Java TCK (Technology Compatibility Kit).
//!
//! ## Overview
//!
//! Conformance tests verify that TOS implementation correctly follows
//! the specification. Tests are defined in YAML format for readability.
//!
//! ## Usage
//!
//! ```ignore
//! use tos_tck::conformance::{ConformanceRunner, Category};
//!
//! let runner = ConformanceRunner::load_from_dir("specs/syscalls")?;
//! let report = runner.run_category(Category::Syscalls).await;
//! assert_eq!(report.failed, 0);
//! ```

mod runner;
mod spec;

pub use runner::*;
pub use spec::*;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Conformance test specification
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConformanceSpec {
    /// Specification metadata
    pub spec: SpecMetadata,
    /// Human-readable description
    pub description: Option<String>,
    /// Preconditions that must be met before test
    pub preconditions: Vec<Condition>,
    /// Action to perform
    pub action: Option<Action>,
    /// Expected outcome
    pub expected: Expected,
    /// Postconditions to verify after test
    pub postconditions: Vec<Condition>,
    /// Multiple test cases (alternative to single action)
    pub test_cases: Option<Vec<TestCase>>,
}

/// Specification metadata
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SpecMetadata {
    /// Unique test name
    pub name: String,
    /// Specification version
    pub version: String,
    /// Test category
    pub category: Category,
    /// Optional subcategory
    pub subcategory: Option<String>,
}

/// Test categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Syscall behavior tests
    Syscalls,
    /// Consensus rule tests
    Consensus,
    /// API conformance tests
    Api,
    /// P2P protocol tests
    P2p,
    /// Security tests
    Security,
}

/// Test condition (pre or post)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Condition {
    /// Account identifier
    pub account: Option<String>,
    /// Expected balance
    pub balance: Option<u64>,
    /// Expected nonce
    pub nonce: Option<u64>,
    /// Expected storage values
    pub storage: Option<HashMap<String, String>>,
    /// Custom assertion expression
    pub assertion: Option<String>,
}

/// Test action
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[allow(missing_docs)]
pub enum Action {
    /// Transfer tokens
    Transfer {
        from: String,
        to: String,
        amount: u64,
    },
    /// Deploy contract
    Deploy { code: String, args: Vec<String> },
    /// Call contract function
    Call {
        contract: String,
        function: String,
        args: Vec<String>,
    },
    /// Execute syscall directly
    Syscall { name: String, args: Vec<String> },
}

/// Expected outcome
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Expected {
    /// Expected status
    #[serde(default)]
    pub status: ExpectedStatus,
    /// Expected error code (if status is error)
    pub error_code: Option<String>,
    /// Expected return value
    pub return_value: Option<serde_yaml::Value>,
    /// Expected gas usage (e.g., "<= 20000")
    pub gas_used: Option<String>,
}

/// Expected status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExpectedStatus {
    /// Operation succeeded (default)
    #[default]
    Success,
    /// Operation failed with error
    Error,
    /// Contract reverted
    Revert,
}

/// Individual test case within a spec
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TestCase {
    /// Test case name
    pub name: String,
    /// Test steps
    pub steps: Vec<TestStep>,
}

/// Single test step
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TestStep {
    /// Action to perform
    pub call: Option<Action>,
    /// Expected outcome for this step
    pub expected: Option<Expected>,
}

/// TCK version constant
pub const TCK_VERSION: &str = "1.0.0";
