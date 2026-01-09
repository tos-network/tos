// Allow Result<_, ()> for placeholder fuzz functions
#![allow(clippy::result_unit_err)]

//! # TCK Fuzzing Infrastructure
//!
//! Provides fuzzing targets and utilities for security testing.
//! Integrates with cargo-fuzz for continuous fuzzing.
//!
//! ## Overview
//!
//! Fuzzing generates random/malformed inputs to find edge cases,
//! crashes, and security vulnerabilities.
//!
//! ## Fuzz Targets
//!
//! - `fuzz_transaction` - Transaction parsing and validation
//! - `fuzz_block` - Block parsing and validation
//! - `fuzz_syscall` - Syscall input validation
//! - `fuzz_p2p_message` - P2P message parsing
//! - `fuzz_rpc_request` - RPC request parsing
//!
//! ## Usage
//!
//! ```bash
//! # Run transaction fuzzer
//! cargo +nightly fuzz run fuzz_transaction
//!
//! # Run with coverage
//! cargo +nightly fuzz coverage fuzz_transaction
//! ```

mod targets;

pub use targets::*;

/// Fuzzing configuration
#[derive(Debug, Clone)]
pub struct FuzzConfig {
    /// Maximum input size in bytes
    pub max_input_size: usize,
    /// Maximum execution time per input (ms)
    pub timeout_ms: u64,
    /// Seed for reproducibility (0 = random)
    pub seed: u64,
}

impl Default for FuzzConfig {
    fn default() -> Self {
        Self {
            max_input_size: 65536, // 64KB
            timeout_ms: 1000,      // 1 second
            seed: 0,
        }
    }
}

/// Result of a fuzz run
#[derive(Debug, Clone)]
pub struct FuzzResult {
    /// Number of inputs tested
    pub inputs_tested: u64,
    /// Number of crashes found
    pub crashes_found: u64,
    /// Number of timeouts
    pub timeouts: u64,
    /// Unique code paths discovered
    pub paths_discovered: u64,
    /// Total execution time
    pub duration_secs: u64,
}

/// Trait for fuzz targets
pub trait FuzzTarget {
    /// Target name
    fn name(&self) -> &'static str;

    /// Run fuzzer on input
    fn fuzz(&self, input: &[u8]);

    /// Validate input (returns true if input should be tested)
    fn validate_input(&self, input: &[u8]) -> bool {
        !input.is_empty()
    }
}

/// Transaction fuzzer
pub struct TransactionFuzzer;

impl FuzzTarget for TransactionFuzzer {
    fn name(&self) -> &'static str {
        "fuzz_transaction"
    }

    fn fuzz(&self, input: &[u8]) {
        // Attempt to deserialize transaction
        // This should never panic, always return Result
        let _ = Self::fuzz_deserialize(input);
    }
}

impl TransactionFuzzer {
    /// Fuzz transaction deserialization
    pub fn fuzz_deserialize(_data: &[u8]) -> Result<(), ()> {
        // TODO: Implement actual transaction deserialization
        // use tos_common::transaction::Transaction;
        // use tos_common::serializer::Reader;
        //
        // let mut reader = Reader::new(data);
        // let _ = Transaction::read(&mut reader);

        Ok(())
    }

    /// Fuzz transaction validation
    pub fn fuzz_validate(_data: &[u8]) -> Result<(), ()> {
        // TODO: Implement actual transaction validation
        Ok(())
    }
}

/// Block fuzzer
pub struct BlockFuzzer;

impl FuzzTarget for BlockFuzzer {
    fn name(&self) -> &'static str {
        "fuzz_block"
    }

    fn fuzz(&self, input: &[u8]) {
        let _ = Self::fuzz_deserialize(input);
    }
}

impl BlockFuzzer {
    /// Fuzz block deserialization
    pub fn fuzz_deserialize(_data: &[u8]) -> Result<(), ()> {
        // TODO: Implement actual block deserialization
        Ok(())
    }

    /// Fuzz block header validation
    pub fn fuzz_validate_header(_data: &[u8]) -> Result<(), ()> {
        // TODO: Implement actual block header validation
        Ok(())
    }
}

/// Syscall fuzzer
pub struct SyscallFuzzer;

impl FuzzTarget for SyscallFuzzer {
    fn name(&self) -> &'static str {
        "fuzz_syscall"
    }

    fn fuzz(&self, input: &[u8]) {
        let _ = Self::fuzz_syscall_input(input);
    }
}

impl SyscallFuzzer {
    /// Fuzz syscall with arbitrary input
    pub fn fuzz_syscall_input(_data: &[u8]) -> Result<(), ()> {
        // TODO: Implement actual syscall fuzzing
        // Parse syscall ID and arguments from data
        // Execute syscall with fuzzed inputs
        // Verify no panics occur
        Ok(())
    }
}

/// P2P message fuzzer
pub struct P2pMessageFuzzer;

impl FuzzTarget for P2pMessageFuzzer {
    fn name(&self) -> &'static str {
        "fuzz_p2p_message"
    }

    fn fuzz(&self, input: &[u8]) {
        let _ = Self::fuzz_deserialize(input);
    }
}

impl P2pMessageFuzzer {
    /// Fuzz P2P message deserialization
    pub fn fuzz_deserialize(_data: &[u8]) -> Result<(), ()> {
        // TODO: Implement actual P2P message deserialization
        Ok(())
    }
}

/// RPC request fuzzer
pub struct RpcRequestFuzzer;

impl FuzzTarget for RpcRequestFuzzer {
    fn name(&self) -> &'static str {
        "fuzz_rpc_request"
    }

    fn fuzz(&self, input: &[u8]) {
        let _ = Self::fuzz_json_parse(input);
    }
}

impl RpcRequestFuzzer {
    /// Fuzz RPC JSON parsing
    pub fn fuzz_json_parse(data: &[u8]) -> Result<(), ()> {
        // Attempt to parse as JSON
        if let Ok(json_str) = std::str::from_utf8(data) {
            let _ = serde_json::from_str::<serde_json::Value>(json_str);
        }
        Ok(())
    }
}
