// TOS Daemon Library
// Exposes internal modules for benchmarking and testing

// Allow some clippy lints for legacy code - to be fixed gradually
#![allow(clippy::all)]
#![warn(clippy::correctness)]
#![allow(clippy::int_plus_one)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::collapsible_else_if)]
#![allow(clippy::uninlined_format_args)]

extern crate log;

pub mod a2a;
pub mod config;

// VRF (Verifiable Random Function) module for block producers
// Must be declared before `core` since core/config.rs uses WrappedVrfSecret
pub mod vrf;

pub mod core;
pub mod escrow;
pub mod p2p;
pub mod rpc;

// TOS Kernel(TAKO) integration module
pub mod tako_integration;

// Doc-test helpers (always available for doc-tests to work)
// These are minimal mocks suitable for documentation examples
pub mod doc_test_helpers;
