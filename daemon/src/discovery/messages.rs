//! Protocol messages for the discv6-based discovery protocol.
//!
//! Message types:
//! - PING (0x01): Liveness check and node info exchange
//! - PONG (0x02): Response to PING
//! - FINDNODE (0x03): Request nodes close to a target ID
//! - NEIGHBORS (0x04): Response with node list

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use tos_common::crypto::{self, Hash, Signature, SIGNATURE_SIZE};
use tos_common::serializer::{Reader, ReaderError, Serializer, Writer};
use tos_common::time::get_current_time_in_seconds;

use super::error::{DiscoveryError, DiscoveryResult};
use super::identity::{CompressedPublicKey, NodeId};

/// Public key size in bytes (compressed Ristretto point).
pub const PUBLIC_KEY_SIZE: usize = 32;

/// Message type identifiers.
pub mod message_type {
    pub const PING: u8 = 0x01;
    pub const PONG: u8 = 0x02;
    pub const FINDNODE: u8 = 0x03;
    pub const NEIGHBORS: u8 = 0x04;
}

/// Maximum packet size in bytes.
pub const MAX_PACKET_SIZE: usize = 1280;

/// Expiration window in seconds for message validity.
pub const EXPIRATION_WINDOW: u64 = 20;

/// Maximum acceptable clock drift in seconds (2x expiration window).
/// Messages with expiration beyond this are rejected to prevent long-lived replay.
pub const MAX_CLOCK_DRIFT: u64 = EXPIRATION_WINDOW * 2;

/// Maximum number of neighbors in a NEIGHBORS response.
pub const MAX_NEIGHBORS: usize = 16;

/// Information about a discovery node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Node ID (SHA3-256 hash of public key).
    pub node_id: NodeId,
    /// Network address.
    pub address: SocketAddr,
    /// Schnorr public key (compressed Ristretto point, 32 bytes).
    pub public_key: CompressedPublicKey,
}

impl NodeInfo {
    /// Create a new NodeInfo.
    pub fn new(node_id: NodeId, address: SocketAddr, public_key: CompressedPublicKey) -> Self {
        Self {
            node_id,
            address,
            public_key,
        }
    }

    /// Verify that the node_id matches the public key.
    pub fn verify_node_id(&self) -> bool {
        let expected = crypto::hash(self.public_key.as_bytes());
        self.node_id == expected
    }
}

impl Serializer for NodeInfo {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        // Read node_id (32 bytes)
        let node_id = Hash::read(reader)?;

        // Read address (1 byte version + ip + 2 bytes port)
        let addr_version = reader.read_u8()?;
        let address = match addr_version {
            4 => {
                let mut ip_bytes = [0u8; 4];
                for byte in &mut ip_bytes {
                    *byte = reader.read_u8()?;
                }
                let port = reader.read_u16()?;
                SocketAddr::new(
                    std::net::IpAddr::V4(std::net::Ipv4Addr::from(ip_bytes)),
                    port,
                )
            }
            6 => {
                let mut ip_bytes = [0u8; 16];
                for byte in &mut ip_bytes {
                    *byte = reader.read_u8()?;
                }
                let port = reader.read_u16()?;
                SocketAddr::new(
                    std::net::IpAddr::V6(std::net::Ipv6Addr::from(ip_bytes)),
                    port,
                )
            }
            _ => return Err(ReaderError::InvalidValue),
        };

        // Read public_key (32 bytes) using Serializer trait
        let public_key = CompressedPublicKey::read(reader)?;

        Ok(Self {
            node_id,
            address,
            public_key,
        })
    }

    fn write(&self, writer: &mut Writer) {
        // Write node_id
        self.node_id.write(writer);

        // Write address
        match self.address {
            SocketAddr::V4(addr) => {
                writer.write_u8(4);
                for byte in &addr.ip().octets() {
                    writer.write_u8(*byte);
                }
                writer.write_u16(addr.port());
            }
            SocketAddr::V6(addr) => {
                writer.write_u8(6);
                for byte in &addr.ip().octets() {
                    writer.write_u8(*byte);
                }
                writer.write_u16(addr.port());
            }
        }

        // Write public_key
        for byte in self.public_key.as_bytes() {
            writer.write_u8(*byte);
        }
    }

    fn size(&self) -> usize {
        32 // node_id
            + 1 // address version
            + if self.address.is_ipv4() { 4 } else { 16 } // IP bytes
            + 2 // port
            + PUBLIC_KEY_SIZE // public_key
    }
}

/// PING message for liveness check and node info exchange.
#[derive(Debug, Clone)]
pub struct Ping {
    /// Source node information.
    pub source: NodeInfo,
    /// Message expiration timestamp (Unix seconds).
    pub expiration: u64,
    /// Sequence number for request/response matching.
    pub seq: u64,
}

impl Ping {
    /// Create a new PING message.
    pub fn new(source: NodeInfo, seq: u64) -> Self {
        let expiration = get_current_time_in_seconds().saturating_add(EXPIRATION_WINDOW);
        Self {
            source,
            expiration,
            seq,
        }
    }

    /// Check if the message has expired (past expiration time).
    pub fn is_expired(&self) -> bool {
        get_current_time_in_seconds() > self.expiration
    }

    /// Check if the expiration timestamp is valid.
    ///
    /// Returns true if the message is not expired AND the expiration is not
    /// too far in the future (within MAX_CLOCK_DRIFT). This prevents long-lived
    /// replay attacks using messages with far-future expirations.
    pub fn is_expiration_valid(&self) -> bool {
        let now = get_current_time_in_seconds();
        // Not expired AND not too far in the future
        self.expiration > now && self.expiration <= now.saturating_add(MAX_CLOCK_DRIFT)
    }
}

impl Serializer for Ping {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let source = NodeInfo::read(reader)?;
        let expiration = reader.read_u64()?;
        let seq = reader.read_u64()?;
        Ok(Self {
            source,
            expiration,
            seq,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.source.write(writer);
        writer.write_u64(&self.expiration);
        writer.write_u64(&self.seq);
    }

    fn size(&self) -> usize {
        self.source.size() + 8 + 8 // expiration + seq
    }
}

/// PONG message as response to PING.
#[derive(Debug, Clone)]
pub struct Pong {
    /// Hash of the PING message this responds to.
    pub ping_hash: Hash,
    /// Source node information.
    pub source: NodeInfo,
    /// Message expiration timestamp (Unix seconds).
    pub expiration: u64,
}

impl Pong {
    /// Create a new PONG message.
    pub fn new(ping_hash: Hash, source: NodeInfo) -> Self {
        let expiration = get_current_time_in_seconds().saturating_add(EXPIRATION_WINDOW);
        Self {
            ping_hash,
            source,
            expiration,
        }
    }

    /// Check if the message has expired.
    pub fn is_expired(&self) -> bool {
        get_current_time_in_seconds() > self.expiration
    }

    /// Check if the expiration timestamp is valid.
    pub fn is_expiration_valid(&self) -> bool {
        let now = get_current_time_in_seconds();
        self.expiration > now && self.expiration <= now.saturating_add(MAX_CLOCK_DRIFT)
    }
}

impl Serializer for Pong {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let ping_hash = Hash::read(reader)?;
        let source = NodeInfo::read(reader)?;
        let expiration = reader.read_u64()?;
        Ok(Self {
            ping_hash,
            source,
            expiration,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.ping_hash.write(writer);
        self.source.write(writer);
        writer.write_u64(&self.expiration);
    }

    fn size(&self) -> usize {
        32 + self.source.size() + 8 // ping_hash + source + expiration
    }
}

/// FINDNODE message to request nodes close to a target.
#[derive(Debug, Clone)]
pub struct FindNode {
    /// Source node information (sender).
    pub source: NodeInfo,
    /// Target node ID to find nodes close to.
    pub target: NodeId,
    /// Message expiration timestamp (Unix seconds).
    pub expiration: u64,
}

impl FindNode {
    /// Create a new FINDNODE message.
    pub fn new(source: NodeInfo, target: NodeId) -> Self {
        let expiration = get_current_time_in_seconds().saturating_add(EXPIRATION_WINDOW);
        Self {
            source,
            target,
            expiration,
        }
    }

    /// Check if the message has expired.
    pub fn is_expired(&self) -> bool {
        get_current_time_in_seconds() > self.expiration
    }

    /// Check if the expiration timestamp is valid.
    pub fn is_expiration_valid(&self) -> bool {
        let now = get_current_time_in_seconds();
        self.expiration > now && self.expiration <= now.saturating_add(MAX_CLOCK_DRIFT)
    }
}

impl Serializer for FindNode {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let source = NodeInfo::read(reader)?;
        let target = Hash::read(reader)?;
        let expiration = reader.read_u64()?;
        Ok(Self {
            source,
            target,
            expiration,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.source.write(writer);
        self.target.write(writer);
        writer.write_u64(&self.expiration);
    }

    fn size(&self) -> usize {
        self.source.size() + 32 + 8 // source + target + expiration
    }
}

/// NEIGHBORS message containing a list of nodes.
#[derive(Debug, Clone)]
pub struct Neighbors {
    /// Source node information (sender).
    pub source: NodeInfo,
    /// List of node information (max MAX_NEIGHBORS).
    pub nodes: Vec<NodeInfo>,
    /// Message expiration timestamp (Unix seconds).
    pub expiration: u64,
}

impl Neighbors {
    /// Create a new NEIGHBORS message.
    pub fn new(source: NodeInfo, nodes: Vec<NodeInfo>) -> Self {
        let expiration = get_current_time_in_seconds().saturating_add(EXPIRATION_WINDOW);
        // Truncate to MAX_NEIGHBORS
        let nodes = if nodes.len() > MAX_NEIGHBORS {
            nodes.into_iter().take(MAX_NEIGHBORS).collect()
        } else {
            nodes
        };
        Self {
            source,
            nodes,
            expiration,
        }
    }

    /// Check if the message has expired.
    pub fn is_expired(&self) -> bool {
        get_current_time_in_seconds() > self.expiration
    }

    /// Check if the expiration timestamp is valid.
    pub fn is_expiration_valid(&self) -> bool {
        let now = get_current_time_in_seconds();
        self.expiration > now && self.expiration <= now.saturating_add(MAX_CLOCK_DRIFT)
    }
}

impl Serializer for Neighbors {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let source = NodeInfo::read(reader)?;
        let count = reader.read_u8()? as usize;
        if count > MAX_NEIGHBORS {
            return Err(ReaderError::InvalidSize);
        }

        let mut nodes = Vec::with_capacity(count);
        for _ in 0..count {
            nodes.push(NodeInfo::read(reader)?);
        }

        let expiration = reader.read_u64()?;
        Ok(Self {
            source,
            nodes,
            expiration,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.source.write(writer);
        writer.write_u8(self.nodes.len() as u8);
        for node in &self.nodes {
            node.write(writer);
        }
        writer.write_u64(&self.expiration);
    }

    fn size(&self) -> usize {
        self.source.size() + 1 + self.nodes.iter().map(|n| n.size()).sum::<usize>() + 8
        // source + count + nodes + expiration
    }
}

/// Discovery message types.
#[derive(Debug, Clone)]
pub enum Message {
    Ping(Ping),
    Pong(Pong),
    FindNode(FindNode),
    Neighbors(Neighbors),
}

impl Message {
    /// Get the message type ID.
    pub fn message_type(&self) -> u8 {
        match self {
            Message::Ping(_) => message_type::PING,
            Message::Pong(_) => message_type::PONG,
            Message::FindNode(_) => message_type::FINDNODE,
            Message::Neighbors(_) => message_type::NEIGHBORS,
        }
    }

    /// Check if the message has expired.
    pub fn is_expired(&self) -> bool {
        match self {
            Message::Ping(m) => m.is_expired(),
            Message::Pong(m) => m.is_expired(),
            Message::FindNode(m) => m.is_expired(),
            Message::Neighbors(m) => m.is_expired(),
        }
    }

    /// Check if the expiration timestamp is valid.
    ///
    /// Returns true if the message is not expired AND the expiration is not
    /// too far in the future. This prevents long-lived replay attacks.
    pub fn is_expiration_valid(&self) -> bool {
        match self {
            Message::Ping(m) => m.is_expiration_valid(),
            Message::Pong(m) => m.is_expiration_valid(),
            Message::FindNode(m) => m.is_expiration_valid(),
            Message::Neighbors(m) => m.is_expiration_valid(),
        }
    }
}

impl Serializer for Message {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let msg_type = reader.read_u8()?;
        match msg_type {
            message_type::PING => Ok(Message::Ping(Ping::read(reader)?)),
            message_type::PONG => Ok(Message::Pong(Pong::read(reader)?)),
            message_type::FINDNODE => Ok(Message::FindNode(FindNode::read(reader)?)),
            message_type::NEIGHBORS => Ok(Message::Neighbors(Neighbors::read(reader)?)),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn write(&self, writer: &mut Writer) {
        writer.write_u8(self.message_type());
        match self {
            Message::Ping(m) => m.write(writer),
            Message::Pong(m) => m.write(writer),
            Message::FindNode(m) => m.write(writer),
            Message::Neighbors(m) => m.write(writer),
        }
    }

    fn size(&self) -> usize {
        1 + match self {
            Message::Ping(m) => m.size(),
            Message::Pong(m) => m.size(),
            Message::FindNode(m) => m.size(),
            Message::Neighbors(m) => m.size(),
        }
    }
}

/// A signed packet containing a discovery message.
///
/// Packet format:
/// - signature (64 bytes)
/// - message_type (1 byte)
/// - message data (variable)
///
/// The signature is over (message_type || message_data).
#[derive(Debug, Clone)]
pub struct SignedPacket {
    /// Schnorr signature over the message.
    pub signature: Signature,
    /// The message.
    pub message: Message,
}

impl SignedPacket {
    /// Create a new signed packet (signature will be computed later).
    pub fn new(message: Message, signature: Signature) -> Self {
        Self { signature, message }
    }

    /// Encode the packet to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        // Write signature
        let sig_bytes = self.signature.to_bytes();
        for byte in &sig_bytes {
            writer.write_u8(*byte);
        }
        // Write message
        self.message.write(&mut writer);
        bytes
    }

    /// Decode a packet from bytes.
    ///
    /// Rejects packets with trailing data after the message to prevent
    /// byte-level message smuggling attacks.
    pub fn decode(data: &[u8]) -> DiscoveryResult<Self> {
        if data.len() < SIGNATURE_SIZE + 1 {
            return Err(DiscoveryError::InvalidPacketSize(
                SIGNATURE_SIZE + 1,
                data.len(),
            ));
        }

        let mut reader = Reader::new(data);

        // Read signature
        let mut sig_bytes = [0u8; SIGNATURE_SIZE];
        for byte in &mut sig_bytes {
            *byte = reader.read_u8()?;
        }
        let signature =
            Signature::from_bytes(&sig_bytes).map_err(|_| DiscoveryError::InvalidSignature)?;

        // Read message
        let message = Message::read(&mut reader)?;

        // Reject trailing data (Fix 3: prevents byte-level message smuggling)
        let remaining = reader.size();
        if remaining > 0 {
            return Err(DiscoveryError::TrailingData(remaining));
        }

        Ok(Self { signature, message })
    }

    /// Get the signed data (message bytes that were signed).
    pub fn signed_data(&self) -> Vec<u8> {
        self.message.to_bytes()
    }

    /// Compute the hash of this packet (used for PONG reference).
    pub fn hash(&self) -> Hash {
        let data = self.encode();
        crypto::hash(&data)
    }

    /// Verify the signature against a compressed public key.
    ///
    /// The public key is decompressed before verification.
    pub fn verify(&self, public_key: &CompressedPublicKey) -> DiscoveryResult<()> {
        let signed_data = self.signed_data();
        // Decompress the public key for signature verification
        let uncompressed = public_key
            .decompress()
            .map_err(|_| DiscoveryError::InvalidSignature)?;
        if self.signature.verify(&signed_data, &uncompressed) {
            Ok(())
        } else {
            Err(DiscoveryError::InvalidSignature)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::identity::NodeIdentity;
    use std::net::{IpAddr, Ipv4Addr};
    use tos_common::crypto::KeyPair;

    fn create_test_node_info() -> NodeInfo {
        let keypair = KeyPair::new();
        let compressed = keypair.get_public_key().compress();
        let node_id = NodeIdentity::compute_node_id(&compressed);
        NodeInfo::new(
            node_id,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 2126),
            compressed,
        )
    }

    #[test]
    fn test_node_info_serialization() {
        let node_info = create_test_node_info();
        let bytes = node_info.to_bytes();
        let decoded = NodeInfo::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.node_id, node_info.node_id);
        assert_eq!(decoded.address, node_info.address);
        assert_eq!(decoded.public_key, node_info.public_key);
    }

    #[test]
    fn test_node_info_verify_node_id() {
        let node_info = create_test_node_info();
        assert!(node_info.verify_node_id());
    }

    #[test]
    fn test_ping_message() {
        let source = create_test_node_info();
        let ping = Ping::new(source.clone(), 42);

        assert!(!ping.is_expired());
        assert!(ping.is_expiration_valid()); // New message should be valid
        assert_eq!(ping.seq, 42);

        let bytes = ping.to_bytes();
        let decoded = Ping::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.source.node_id, source.node_id);
        assert_eq!(decoded.seq, 42);
    }

    #[test]
    fn test_expiration_validation() {
        // Test that far-future expirations are rejected to prevent replay attacks
        let source = create_test_node_info();
        let now = get_current_time_in_seconds();

        // Valid: expiration within acceptable window
        let mut ping = Ping::new(source.clone(), 1);
        assert!(ping.is_expiration_valid());

        // Invalid: expiration in the past
        ping.expiration = now.saturating_sub(1);
        assert!(!ping.is_expiration_valid());

        // Invalid: expiration too far in the future (beyond MAX_CLOCK_DRIFT)
        ping.expiration = now.saturating_add(MAX_CLOCK_DRIFT + 10);
        assert!(!ping.is_expiration_valid());

        // Valid: expiration exactly at MAX_CLOCK_DRIFT
        ping.expiration = now.saturating_add(MAX_CLOCK_DRIFT);
        assert!(ping.is_expiration_valid());

        // Valid: expiration at 1 second from now
        ping.expiration = now.saturating_add(1);
        assert!(ping.is_expiration_valid());
    }

    #[test]
    fn test_findnode_message() {
        let source = create_test_node_info();
        let target = Hash::new([0xaa; 32]);
        let findnode = FindNode::new(source.clone(), target.clone());

        assert!(!findnode.is_expired());
        assert_eq!(findnode.target, target);
        assert_eq!(findnode.source.node_id, source.node_id);

        let bytes = findnode.to_bytes();
        let decoded = FindNode::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.target, target);
        assert_eq!(decoded.source.node_id, source.node_id);
    }

    #[test]
    fn test_neighbors_message() {
        let source = create_test_node_info();
        let nodes: Vec<NodeInfo> = (0..5).map(|_| create_test_node_info()).collect();
        let neighbors = Neighbors::new(source.clone(), nodes.clone());

        assert!(!neighbors.is_expired());
        assert_eq!(neighbors.nodes.len(), 5);
        assert_eq!(neighbors.source.node_id, source.node_id);

        let bytes = neighbors.to_bytes();
        let decoded = Neighbors::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.nodes.len(), 5);
        assert_eq!(decoded.source.node_id, source.node_id);
    }

    #[test]
    fn test_neighbors_truncation() {
        let source = create_test_node_info();
        let nodes: Vec<NodeInfo> = (0..20).map(|_| create_test_node_info()).collect();
        let neighbors = Neighbors::new(source, nodes);

        assert_eq!(neighbors.nodes.len(), MAX_NEIGHBORS);
    }

    #[test]
    fn test_message_serialization() {
        let source = create_test_node_info();
        let ping = Ping::new(source, 123);
        let message = Message::Ping(ping);

        assert_eq!(message.message_type(), message_type::PING);

        let bytes = message.to_bytes();
        let decoded = Message::from_bytes(&bytes).unwrap();

        if let Message::Ping(decoded_ping) = decoded {
            assert_eq!(decoded_ping.seq, 123);
        } else {
            panic!("Expected Ping message");
        }
    }

    #[test]
    fn test_signed_packet() {
        let keypair = KeyPair::new();
        let compressed = keypair.get_public_key().compress();
        let node_id = NodeIdentity::compute_node_id(&compressed);
        let source = NodeInfo::new(
            node_id,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 2126),
            compressed.clone(),
        );

        let ping = Ping::new(source, 999);
        let message = Message::Ping(ping);

        // Sign the message
        let msg_bytes = message.to_bytes();
        let signature = keypair.sign(&msg_bytes);

        let packet = SignedPacket::new(message, signature);

        // Encode and decode
        let encoded = packet.encode();
        let decoded = SignedPacket::decode(&encoded).unwrap();

        // Verify signature
        assert!(decoded.verify(&compressed).is_ok());
    }

    #[test]
    fn test_signed_packet_invalid_signature() {
        let keypair1 = KeyPair::new();
        let keypair2 = KeyPair::new();

        let compressed1 = keypair1.get_public_key().compress();
        let node_id = NodeIdentity::compute_node_id(&compressed1);
        let source = NodeInfo::new(
            node_id,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 2126),
            compressed1,
        );

        let ping = Ping::new(source, 999);
        let message = Message::Ping(ping);

        // Sign with keypair1
        let msg_bytes = message.to_bytes();
        let signature = keypair1.sign(&msg_bytes);

        let packet = SignedPacket::new(message, signature);

        // Verify with keypair2 (should fail)
        let compressed2 = keypair2.get_public_key().compress();
        assert!(packet.verify(&compressed2).is_err());
    }

    #[test]
    fn test_signed_packet_trailing_data_rejected() {
        // Fix 3: Verify that trailing data is rejected to prevent smuggling
        let keypair = KeyPair::new();
        let compressed = keypair.get_public_key().compress();
        let node_id = NodeIdentity::compute_node_id(&compressed);
        let source = NodeInfo::new(
            node_id,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 2126),
            compressed,
        );

        let ping = Ping::new(source, 999);
        let message = Message::Ping(ping);
        let msg_bytes = message.to_bytes();
        let signature = keypair.sign(&msg_bytes);
        let packet = SignedPacket::new(message, signature);

        // Encode valid packet
        let mut encoded = packet.encode();

        // Append trailing garbage data
        encoded.extend_from_slice(b"TRAILING_GARBAGE");

        // Decode should fail with TrailingData error
        let result = SignedPacket::decode(&encoded);
        assert!(result.is_err());

        // Verify error is specifically TrailingData
        if let Err(e) = result {
            let error_msg = format!("{}", e);
            assert!(
                error_msg.contains("trailing data"),
                "Expected TrailingData error, got: {}",
                error_msg
            );
        }
    }
}
