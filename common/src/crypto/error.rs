use thiserror::Error;

/// Errors that can occur during cryptographic operations
///
/// This error type provides structured error handling for all crypto module
/// operations, eliminating the need for .unwrap() calls that could cause panics.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
    /// Invalid hexadecimal string format
    #[error("Invalid hex string: {0}")]
    InvalidHex(String),

    /// Hex string exceeds maximum allowed length (DoS prevention)
    #[error("Hex string too long: {len} bytes, maximum: {max} bytes")]
    HexTooLong { len: usize, max: usize },

    /// Hash has invalid length
    #[error("Invalid hash length: {len} bytes, expected: {expected} bytes")]
    InvalidHashLength { len: usize, expected: usize },

    /// Invalid checksum in address
    #[error("Invalid checksum")]
    InvalidChecksum,

    /// Address string is malformed or invalid
    #[error("Invalid address format: {0}")]
    InvalidAddress(String),

    /// Bech32 encoding/decoding error
    #[error("Bech32 error: {0}")]
    Bech32(String),

    /// Invalid hex character detected
    #[error("Invalid hex character in input")]
    InvalidHexCharacter,

    /// Hex decode error
    #[error("Failed to decode hex: {0}")]
    DecodeError(String),

    /// Input validation error
    #[error("Input validation failed: {0}")]
    ValidationFailed(String),
}
