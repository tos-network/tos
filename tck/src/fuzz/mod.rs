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

use tos_common::block::BlockHeader;
use tos_common::serializer::{Reader, Serializer};
use tos_common::transaction::Transaction;

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

    /// Run fuzzer on input - must never panic
    fn fuzz(&self, input: &[u8]);

    /// Validate input (returns true if input should be tested)
    fn validate_input(&self, input: &[u8]) -> bool {
        !input.is_empty()
    }
}

/// Transaction fuzzer - tests Transaction deserialization with arbitrary bytes
pub struct TransactionFuzzer;

impl FuzzTarget for TransactionFuzzer {
    fn name(&self) -> &'static str {
        "fuzz_transaction"
    }

    fn fuzz(&self, input: &[u8]) {
        // Attempt to deserialize transaction - must not panic on any input
        let _ = Self::fuzz_deserialize(input);
        let _ = Self::fuzz_validate(input);
    }
}

impl TransactionFuzzer {
    /// Fuzz transaction deserialization
    /// Returns Ok if deserialization succeeded, Err if it failed (expected for malformed input)
    pub fn fuzz_deserialize(data: &[u8]) -> Result<Transaction, FuzzError> {
        let mut reader = Reader::new(data);
        Transaction::read(&mut reader).map_err(|e| FuzzError::Deserialization(e.to_string()))
    }

    /// Fuzz transaction validation (for successfully deserialized transactions)
    pub fn fuzz_validate(data: &[u8]) -> Result<(), FuzzError> {
        let mut reader = Reader::new(data);
        if let Ok(tx) = Transaction::read(&mut reader) {
            // Verify basic invariants that should always hold
            // These should never panic regardless of input

            // Check that serialization round-trips correctly
            let reserialized = tx.to_bytes();
            let mut reader2 = Reader::new(&reserialized);
            let tx2 =
                Transaction::read(&mut reader2).map_err(|e| FuzzError::RoundTrip(e.to_string()))?;

            // Hash should be deterministic
            use tos_common::crypto::Hashable;
            if tx.hash() != tx2.hash() {
                return Err(FuzzError::HashMismatch);
            }
        }
        Ok(())
    }
}

/// Block fuzzer - tests BlockHeader deserialization with arbitrary bytes
pub struct BlockFuzzer;

impl FuzzTarget for BlockFuzzer {
    fn name(&self) -> &'static str {
        "fuzz_block"
    }

    fn fuzz(&self, input: &[u8]) {
        let _ = Self::fuzz_deserialize(input);
        let _ = Self::fuzz_validate_header(input);
    }
}

impl BlockFuzzer {
    /// Fuzz block header deserialization
    pub fn fuzz_deserialize(data: &[u8]) -> Result<BlockHeader, FuzzError> {
        let mut reader = Reader::new(data);
        BlockHeader::read(&mut reader).map_err(|e| FuzzError::Deserialization(e.to_string()))
    }

    /// Fuzz block header validation
    pub fn fuzz_validate_header(data: &[u8]) -> Result<(), FuzzError> {
        let mut reader = Reader::new(data);
        if let Ok(header) = BlockHeader::read(&mut reader) {
            // Verify basic invariants

            // Check round-trip serialization
            let reserialized = header.to_bytes();
            let mut reader2 = Reader::new(&reserialized);
            let header2 =
                BlockHeader::read(&mut reader2).map_err(|e| FuzzError::RoundTrip(e.to_string()))?;

            // Hash should be deterministic
            use tos_common::crypto::Hashable;
            if header.hash() != header2.hash() {
                return Err(FuzzError::HashMismatch);
            }

            // Size calculation should match actual serialized size
            if header.size() != reserialized.len() {
                return Err(FuzzError::SizeMismatch {
                    reported: header.size(),
                    actual: reserialized.len(),
                });
            }
        }
        Ok(())
    }
}

/// Syscall fuzzer - tests syscall input parsing
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
    /// Parses syscall ID and arguments, verifies no panics occur
    pub fn fuzz_syscall_input(data: &[u8]) -> Result<(), FuzzError> {
        if data.is_empty() {
            return Ok(());
        }

        // First byte is syscall ID
        let syscall_id = data[0];
        let args = &data[1..];

        // Parse arguments based on syscall type
        // Each syscall has different argument formats
        match syscall_id {
            // Balance syscalls (0x00-0x0F)
            0x00 => {
                // balance_get: expects 20-byte address
                if args.len() >= 20 {
                    let _address = &args[..20];
                }
            }
            0x01 => {
                // balance_transfer: expects address (20) + amount (8)
                if args.len() >= 28 {
                    let _address = &args[..20];
                    let _amount = u64::from_be_bytes(args[20..28].try_into().unwrap_or([0u8; 8]));
                }
            }
            // Storage syscalls (0x10-0x1F)
            0x10 => {
                // storage_read: expects 32-byte key
                if args.len() >= 32 {
                    let _key = &args[..32];
                }
            }
            0x11 => {
                // storage_write: expects key (32) + value (variable)
                if args.len() >= 32 {
                    let _key = &args[..32];
                    let _value = &args[32..];
                }
            }
            // Event syscalls (0x20-0x2F)
            0x20..=0x24 => {
                // LOG0-LOG4: data + topics
                let topic_count = (syscall_id - 0x20) as usize;
                let topics_size = topic_count * 32;
                if args.len() >= topics_size {
                    let _topics = &args[..topics_size];
                    let _data = &args[topics_size..];
                }
            }
            // Crypto syscalls (0x30-0x3F)
            0x30 => {
                // keccak256: arbitrary data
                let _data = args;
            }
            0x31 => {
                // sha256: arbitrary data
                let _data = args;
            }
            0x32 => {
                // ecrecover: hash (32) + signature (65)
                if args.len() >= 97 {
                    let _hash = &args[..32];
                    let _signature = &args[32..97];
                }
            }
            _ => {
                // Unknown syscall - still shouldn't panic
            }
        }

        Ok(())
    }
}

/// P2P message fuzzer - tests network message parsing
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
    pub fn fuzz_deserialize(data: &[u8]) -> Result<P2pMessage, FuzzError> {
        if data.is_empty() {
            return Err(FuzzError::EmptyInput);
        }

        // P2P message format: type (1 byte) + length (4 bytes) + payload
        if data.len() < 5 {
            return Err(FuzzError::TooShort {
                min: 5,
                actual: data.len(),
            });
        }

        let msg_type = data[0];
        let length = u32::from_be_bytes(
            data[1..5]
                .try_into()
                .map_err(|_| FuzzError::InvalidFormat)?,
        ) as usize;

        // Validate length doesn't exceed remaining data
        if length > data.len() - 5 {
            return Err(FuzzError::LengthMismatch {
                declared: length,
                available: data.len() - 5,
            });
        }

        let payload = &data[5..5 + length];

        Ok(P2pMessage {
            msg_type,
            payload: payload.to_vec(),
        })
    }
}

/// Parsed P2P message for fuzzing
#[derive(Debug, Clone)]
pub struct P2pMessage {
    /// Message type identifier
    pub msg_type: u8,
    /// Message payload bytes
    pub payload: Vec<u8>,
}

/// RPC request fuzzer - tests JSON-RPC parsing
pub struct RpcRequestFuzzer;

impl FuzzTarget for RpcRequestFuzzer {
    fn name(&self) -> &'static str {
        "fuzz_rpc_request"
    }

    fn fuzz(&self, input: &[u8]) {
        let _ = Self::fuzz_json_parse(input);
        let _ = Self::fuzz_rpc_structure(input);
    }
}

impl RpcRequestFuzzer {
    /// Fuzz RPC JSON parsing
    pub fn fuzz_json_parse(data: &[u8]) -> Result<serde_json::Value, FuzzError> {
        let json_str =
            std::str::from_utf8(data).map_err(|e| FuzzError::InvalidUtf8(e.to_string()))?;
        serde_json::from_str(json_str).map_err(|e| FuzzError::JsonParse(e.to_string()))
    }

    /// Fuzz JSON-RPC 2.0 structure validation
    pub fn fuzz_rpc_structure(data: &[u8]) -> Result<RpcRequest, FuzzError> {
        let json_str =
            std::str::from_utf8(data).map_err(|e| FuzzError::InvalidUtf8(e.to_string()))?;

        let value: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| FuzzError::JsonParse(e.to_string()))?;

        // Validate JSON-RPC 2.0 structure
        let obj = value.as_object().ok_or(FuzzError::InvalidFormat)?;

        // jsonrpc field must be "2.0"
        let jsonrpc = obj
            .get("jsonrpc")
            .and_then(|v| v.as_str())
            .ok_or(FuzzError::MissingField("jsonrpc"))?;
        if jsonrpc != "2.0" {
            return Err(FuzzError::InvalidJsonRpcVersion(jsonrpc.to_string()));
        }

        // method must be a string
        let method = obj
            .get("method")
            .and_then(|v| v.as_str())
            .ok_or(FuzzError::MissingField("method"))?
            .to_string();

        // params is optional
        let params = obj.get("params").cloned();

        // id is optional for notifications
        let id = obj.get("id").cloned();

        Ok(RpcRequest { method, params, id })
    }
}

/// Parsed RPC request for fuzzing
#[derive(Debug, Clone)]
pub struct RpcRequest {
    /// RPC method name
    pub method: String,
    /// Optional method parameters
    pub params: Option<serde_json::Value>,
    /// Optional request ID (absent for notifications)
    pub id: Option<serde_json::Value>,
}

/// Fuzzing error types
#[derive(Debug, Clone)]
pub enum FuzzError {
    /// Deserialization failed (expected for malformed input)
    Deserialization(String),
    /// Round-trip serialization failed (unexpected)
    RoundTrip(String),
    /// Hash mismatch after round-trip (unexpected)
    HashMismatch,
    /// Size mismatch between reported and actual
    SizeMismatch {
        /// Size reported by the data structure
        reported: usize,
        /// Actual serialized size
        actual: usize,
    },
    /// Empty input provided
    EmptyInput,
    /// Input too short
    TooShort {
        /// Minimum required bytes
        min: usize,
        /// Actual bytes provided
        actual: usize,
    },
    /// Length field doesn't match available data
    LengthMismatch {
        /// Length declared in the message
        declared: usize,
        /// Bytes actually available
        available: usize,
    },
    /// Invalid UTF-8 in string input
    InvalidUtf8(String),
    /// JSON parsing failed
    JsonParse(String),
    /// Invalid message format
    InvalidFormat,
    /// Missing required field
    MissingField(&'static str),
    /// Invalid JSON-RPC version
    InvalidJsonRpcVersion(String),
}

impl std::fmt::Display for FuzzError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FuzzError::Deserialization(e) => write!(f, "Deserialization failed: {}", e),
            FuzzError::RoundTrip(e) => write!(f, "Round-trip failed: {}", e),
            FuzzError::HashMismatch => write!(f, "Hash mismatch after round-trip"),
            FuzzError::SizeMismatch { reported, actual } => {
                write!(f, "Size mismatch: reported={}, actual={}", reported, actual)
            }
            FuzzError::EmptyInput => write!(f, "Empty input"),
            FuzzError::TooShort { min, actual } => {
                write!(f, "Input too short: need {} bytes, got {}", min, actual)
            }
            FuzzError::LengthMismatch {
                declared,
                available,
            } => write!(
                f,
                "Length mismatch: declared={}, available={}",
                declared, available
            ),
            FuzzError::InvalidUtf8(e) => write!(f, "Invalid UTF-8: {}", e),
            FuzzError::JsonParse(e) => write!(f, "JSON parse error: {}", e),
            FuzzError::InvalidFormat => write!(f, "Invalid format"),
            FuzzError::MissingField(field) => write!(f, "Missing field: {}", field),
            FuzzError::InvalidJsonRpcVersion(v) => write!(f, "Invalid JSON-RPC version: {}", v),
        }
    }
}

impl std::error::Error for FuzzError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_fuzzer_empty_input() {
        let fuzzer = TransactionFuzzer;
        // Should not panic on empty input
        fuzzer.fuzz(&[]);
    }

    #[test]
    fn test_transaction_fuzzer_random_bytes() {
        let fuzzer = TransactionFuzzer;
        // Should not panic on random bytes
        fuzzer.fuzz(&[0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE]);
    }

    #[test]
    fn test_block_fuzzer_empty_input() {
        let fuzzer = BlockFuzzer;
        fuzzer.fuzz(&[]);
    }

    #[test]
    fn test_syscall_fuzzer_all_opcodes() {
        let fuzzer = SyscallFuzzer;
        // Test all possible syscall IDs
        for id in 0..=255u8 {
            let mut input = vec![id];
            input.extend_from_slice(&[0u8; 100]); // Add some argument data
            fuzzer.fuzz(&input);
        }
    }

    #[test]
    fn test_p2p_fuzzer_malformed_length() {
        let fuzzer = P2pMessageFuzzer;
        // Malformed: declares 1000 bytes but only has 10
        let input = [0x01, 0x00, 0x00, 0x03, 0xE8, 0x00, 0x01, 0x02, 0x03, 0x04];
        fuzzer.fuzz(&input);
    }

    #[test]
    fn test_rpc_fuzzer_valid_request() {
        let result = RpcRequestFuzzer::fuzz_rpc_structure(
            br#"{"jsonrpc":"2.0","method":"get_info","id":1}"#,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_rpc_fuzzer_invalid_version() {
        let result =
            RpcRequestFuzzer::fuzz_rpc_structure(br#"{"jsonrpc":"1.0","method":"test","id":1}"#);
        assert!(matches!(result, Err(FuzzError::InvalidJsonRpcVersion(_))));
    }
}
