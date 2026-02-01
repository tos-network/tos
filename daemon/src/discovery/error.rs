//! Error types for the discovery protocol.

use std::io::Error as IoError;
use std::net::AddrParseError;
use thiserror::Error;
use tos_common::serializer::ReaderError;

/// Error type for discovery protocol operations.
#[derive(Error, Debug)]
pub enum DiscoveryError {
    /// I/O error during network operations.
    #[error("I/O error: {0}")]
    Io(#[from] IoError),

    /// Address parsing error.
    #[error("Invalid address: {0}")]
    InvalidAddress(#[from] AddrParseError),

    /// Message serialization/deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] ReaderError),

    /// Invalid URL format.
    #[error("Invalid tosnode URL: {0}")]
    InvalidUrl(String),

    /// Invalid message type.
    #[error("Invalid message type: {0}")]
    InvalidMessageType(u8),

    /// Message has expired.
    #[error("Message expired: timestamp {0} is older than {1} seconds")]
    MessageExpired(u64, u64),

    /// Invalid packet size.
    #[error("Invalid packet size: expected at least {0} bytes, got {1}")]
    InvalidPacketSize(usize, usize),

    /// Signature verification failed.
    #[error("Signature verification failed")]
    InvalidSignature,

    /// Invalid node ID.
    #[error("Invalid node ID: expected {0}, got {1}")]
    InvalidNodeId(String, String),

    /// Bucket is full.
    #[error("Routing table bucket {0} is full")]
    BucketFull(usize),

    /// Node not found in routing table.
    #[error("Node not found: {0}")]
    NodeNotFound(String),

    /// Socket bind error.
    #[error("Failed to bind UDP socket on {0}: {1}")]
    BindFailed(String, IoError),

    /// Packet too large.
    #[error("Packet too large: {0} bytes exceeds maximum {1}")]
    PacketTooLarge(usize, usize),

    /// Hex decoding error.
    #[error("Hex decode error: {0}")]
    HexError(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Channel send error.
    #[error("Channel send error: {0}")]
    ChannelError(String),

    /// Self-referential operation (e.g., adding self to routing table).
    #[error("Cannot perform operation on self")]
    SelfOperation,
}

/// Result type alias for discovery operations.
pub type DiscoveryResult<T> = Result<T, DiscoveryError>;
