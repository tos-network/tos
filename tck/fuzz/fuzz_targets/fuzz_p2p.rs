//! Fuzz target for P2P message parsing
//!
//! Tests that arbitrary byte sequences do not cause panics
//! when parsed as P2P protocol messages.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

/// P2P message types
#[derive(Debug, Clone, Copy, Arbitrary)]
#[repr(u8)]
enum MessageType {
    Handshake = 0x00,
    Ping = 0x01,
    Pong = 0x02,
    GetBlockHeaders = 0x03,
    BlockHeaders = 0x04,
    GetBlockBodies = 0x05,
    BlockBodies = 0x06,
    NewBlock = 0x07,
    NewTransaction = 0x08,
    GetNodeData = 0x09,
    NodeData = 0x0A,
    GetReceipts = 0x0B,
    Receipts = 0x0C,
    NewPooledTransactionHashes = 0x0D,
    GetPooledTransactions = 0x0E,
    PooledTransactions = 0x0F,
}

/// P2P message structure
#[derive(Debug, Arbitrary)]
struct P2PMessage {
    /// Protocol version
    version: u8,
    /// Message type
    message_type: MessageType,
    /// Request ID (for request-response pairs)
    request_id: u64,
    /// Payload data
    payload: Vec<u8>,
}

/// Handshake message
#[derive(Debug)]
struct Handshake {
    protocol_version: u32,
    network_id: u64,
    genesis_hash: [u8; 32],
    head_hash: [u8; 32],
    head_height: u64,
}

fuzz_target!(|data: &[u8]| {
    // Limit input size
    if data.is_empty() || data.len() > 16 * 1024 * 1024 {
        return;
    }

    // Try to parse as P2P message
    let _ = parse_p2p_message(data);

    // Try to parse as handshake
    if data.len() >= 108 {
        let _ = parse_handshake(data);
    }

    // Try to parse message header
    let _ = parse_message_header(data);
});

/// Parse P2P message from bytes
fn parse_p2p_message(data: &[u8]) -> Option<(u8, u8, u64, &[u8])> {
    if data.len() < 10 {
        return None;
    }

    let version = data[0];
    let message_type = data[1];
    let request_id = u64::from_le_bytes([
        data[2], data[3], data[4], data[5], data[6], data[7], data[8], data[9],
    ]);
    let payload = &data[10..];

    Some((version, message_type, request_id, payload))
}

/// Parse handshake message
fn parse_handshake(data: &[u8]) -> Option<Handshake> {
    if data.len() < 108 {
        return None;
    }

    let protocol_version = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let network_id = u64::from_le_bytes([
        data[4], data[5], data[6], data[7], data[8], data[9], data[10], data[11],
    ]);

    let mut genesis_hash = [0u8; 32];
    genesis_hash.copy_from_slice(&data[12..44]);

    let mut head_hash = [0u8; 32];
    head_hash.copy_from_slice(&data[44..76]);

    let head_height = u64::from_le_bytes([
        data[76], data[77], data[78], data[79], data[80], data[81], data[82], data[83],
    ]);

    Some(Handshake {
        protocol_version,
        network_id,
        genesis_hash,
        head_hash,
        head_height,
    })
}

/// Parse message header (length + type)
fn parse_message_header(data: &[u8]) -> Option<(u32, u8)> {
    if data.len() < 5 {
        return None;
    }

    let length = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let msg_type = data[4];

    // Validate length is reasonable
    if length > 16 * 1024 * 1024 {
        return None;
    }

    Some((length, msg_type))
}

/// Validate message checksum
#[allow(dead_code)]
fn validate_checksum(data: &[u8], expected: [u8; 4]) -> bool {
    if data.is_empty() {
        return expected == [0, 0, 0, 0];
    }

    // Simple checksum: XOR all bytes in 4-byte chunks
    let mut checksum = [0u8; 4];
    for (i, byte) in data.iter().enumerate() {
        checksum[i % 4] ^= byte;
    }

    checksum == expected
}
