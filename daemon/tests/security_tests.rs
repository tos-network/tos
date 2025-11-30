//! Security Test Runner
//!
//! This file exposes the security/ module tests as integration tests.
//! Run with: cargo test --package tos_daemon --test security_tests

// Allow certain clippy lints for test code:
// - useless_vec: Tests often create vecs that could be arrays for clarity
// - assertions_on_constants: Meta-tests intentionally assert on constants
// - disallowed_methods: Tests can use .expect() and .unwrap() freely
// - useless_format: Tests may use format! for consistency
// - dead_code: Test utilities may have unused helper functions
// - unused: Test code may have unused variables/imports
#![allow(clippy::useless_vec)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::useless_format)]
#![allow(clippy::explicit_counter_loop)]
#![allow(clippy::absurd_extreme_comparisons)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::cloned_ref_to_slice_refs)]
#![allow(dead_code)]
#![allow(unused)]

mod security;
