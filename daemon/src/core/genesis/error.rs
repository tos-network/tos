use thiserror::Error;
use tos_common::crypto::Hash;

/// Errors that can occur during genesis state loading and validation
#[derive(Error, Debug)]
pub enum GenesisError {
    #[error("Invalid format version: expected 1, got {0}")]
    InvalidFormatVersion(String),

    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),

    #[error("Invalid balance value: {0}")]
    InvalidBalance(String),

    #[error("Invalid nonce value: {0}")]
    InvalidNonce(String),

    #[error("Balance overflow: total allocations would exceed maximum supply")]
    BalanceOverflow,

    #[error("Invalid network: {0}")]
    InvalidNetwork(String),

    #[error("State hash mismatch: expected {expected}, computed {computed}")]
    StateHashMismatch { expected: Hash, computed: Hash },

    #[error("Address mismatch for public key {public_key}: expected {expected}, got {provided}")]
    AddressMismatch {
        public_key: String,
        expected: String,
        provided: String,
    },

    #[error("Genesis state file not found: {0}")]
    FileNotFound(String),

    #[error("JSON parse error: {0}")]
    JsonParseError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Invalid chain ID: {0}")]
    InvalidChainId(String),

    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(String),

    #[error("Missing required asset: {0}")]
    MissingRequiredAsset(String),

    #[error("Invalid fork height for {fork}: {value}")]
    InvalidForkHeight { fork: String, value: String },

    #[error("Invalid energy value: {0}")]
    InvalidEnergy(String),

    #[error("Duplicate public key in allocations: {0}")]
    DuplicatePublicKey(String),
}
