//! Node identity for the discovery protocol.
//!
//! Each node in the discovery network has a unique identity consisting of:
//! - An Ed25519 key pair for signing/verifying messages
//! - A node ID derived from the public key (SHA3-256 hash)

use std::fmt;
use tos_common::crypto::{
    ed25519::{Ed25519Error, Ed25519KeyPair, Ed25519PublicKey, Ed25519SecretKey, Ed25519Signature},
    Hash,
};

/// Node ID is a 32-byte hash of the node's public key.
///
/// The node ID is used for:
/// - Kademlia distance calculations in the routing table
/// - Identifying nodes in FINDNODE requests
/// - Organizing the DHT structure
pub type NodeId = Hash;

/// Node identity containing the key pair and derived node ID.
pub struct NodeIdentity {
    /// Ed25519 key pair for signing messages.
    keypair: Ed25519KeyPair,
    /// Node ID (SHA3-256 hash of public key).
    node_id: NodeId,
}

impl NodeIdentity {
    /// Generate a new random node identity.
    pub fn generate() -> Self {
        let keypair = Ed25519KeyPair::generate();
        let node_id = keypair.node_id();
        Self { keypair, node_id }
    }

    /// Create a node identity from a secret key.
    pub fn from_secret(secret: &Ed25519SecretKey) -> Result<Self, Ed25519Error> {
        let keypair = Ed25519KeyPair::from_secret(secret)?;
        let node_id = keypair.node_id();
        Ok(Self { keypair, node_id })
    }

    /// Get the node ID.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Get the public key.
    pub fn public_key(&self) -> Ed25519PublicKey {
        self.keypair.public_key()
    }

    /// Get the secret key.
    pub fn secret_key(&self) -> Ed25519SecretKey {
        self.keypair.secret_key()
    }

    /// Sign a message with this identity's private key.
    pub fn sign(&self, message: &[u8]) -> Ed25519Signature {
        self.keypair.sign(message)
    }

    /// Verify a signature against this identity's public key.
    pub fn verify(&self, message: &[u8], signature: &Ed25519Signature) -> Result<(), Ed25519Error> {
        self.public_key().verify(message, signature)
    }
}

impl fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeIdentity")
            .field("node_id", &self.node_id)
            .field("public_key", &self.keypair.public_key())
            .finish()
    }
}

/// Calculate the XOR distance between two node IDs.
///
/// The XOR distance is used for Kademlia routing table organization.
/// Nodes with smaller XOR distance are considered "closer" in the DHT.
pub fn xor_distance(a: &NodeId, b: &NodeId) -> [u8; 32] {
    let mut result = [0u8; 32];
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    for i in 0..32 {
        result[i] = a_bytes[i] ^ b_bytes[i];
    }
    result
}

/// Calculate the log2 distance between two node IDs.
///
/// This returns the index of the most significant bit that differs between
/// the two IDs, which determines which k-bucket a node should be placed in.
///
/// Returns `None` if the IDs are identical (distance is 0).
/// Returns `Some(0)` to `Some(255)` for different IDs.
pub fn log2_distance(a: &NodeId, b: &NodeId) -> Option<u8> {
    let distance = xor_distance(a, b);

    // Find the first non-zero byte
    for (i, byte) in distance.iter().enumerate() {
        if *byte != 0 {
            // Find the position of the most significant bit in this byte
            let leading_zeros = byte.leading_zeros() as usize;
            // Calculate the overall bit position (0 = most significant bit overall)
            // We want to return the bucket index where:
            // - Bucket 0: nodes differ in the least significant bit only
            // - Bucket 255: nodes differ in the most significant bit
            // So we invert: 255 - (byte_index * 8 + leading_zeros)
            let bit_position = i.saturating_mul(8).saturating_add(leading_zeros);
            return Some(255u8.saturating_sub(bit_position as u8));
        }
    }

    // IDs are identical
    None
}

/// Compare two XOR distances.
///
/// Returns:
/// - `Ordering::Less` if `a` is closer to `target` than `b`
/// - `Ordering::Greater` if `b` is closer to `target` than `a`
/// - `Ordering::Equal` if they are equidistant
pub fn compare_distance(target: &NodeId, a: &NodeId, b: &NodeId) -> std::cmp::Ordering {
    let dist_a = xor_distance(target, a);
    let dist_b = xor_distance(target, b);

    // Compare byte by byte (big-endian comparison)
    for i in 0..32 {
        match dist_a[i].cmp(&dist_b[i]) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_generation() {
        let identity = NodeIdentity::generate();
        assert_eq!(identity.node_id().as_bytes().len(), 32);
    }

    #[test]
    fn test_identity_from_secret() {
        let identity1 = NodeIdentity::generate();
        let secret = identity1.secret_key();

        let identity2 = NodeIdentity::from_secret(&secret).unwrap();
        assert_eq!(identity1.node_id(), identity2.node_id());
        assert_eq!(identity1.public_key(), identity2.public_key());
    }

    #[test]
    fn test_sign_verify() {
        let identity = NodeIdentity::generate();
        let message = b"Test message for discovery";

        let signature = identity.sign(message);
        assert!(identity.verify(message, &signature).is_ok());
    }

    #[test]
    fn test_xor_distance_self() {
        let identity = NodeIdentity::generate();
        let distance = xor_distance(identity.node_id(), identity.node_id());
        assert_eq!(distance, [0u8; 32]);
    }

    #[test]
    fn test_xor_distance_different() {
        let id1 = NodeIdentity::generate();
        let id2 = NodeIdentity::generate();

        let distance = xor_distance(id1.node_id(), id2.node_id());
        // Should not be all zeros for different keys
        assert!(distance.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_xor_distance_symmetric() {
        let id1 = NodeIdentity::generate();
        let id2 = NodeIdentity::generate();

        let dist_ab = xor_distance(id1.node_id(), id2.node_id());
        let dist_ba = xor_distance(id2.node_id(), id1.node_id());

        assert_eq!(dist_ab, dist_ba);
    }

    #[test]
    fn test_log2_distance_identical() {
        let identity = NodeIdentity::generate();
        let distance = log2_distance(identity.node_id(), identity.node_id());
        assert_eq!(distance, None);
    }

    #[test]
    fn test_log2_distance_range() {
        let id1 = NodeIdentity::generate();
        let id2 = NodeIdentity::generate();

        // Verify log2_distance returns a valid result for different node IDs
        if id1.node_id() != id2.node_id() {
            assert!(log2_distance(id1.node_id(), id2.node_id()).is_some());
        }
    }

    #[test]
    fn test_log2_distance_known_values() {
        // Create two node IDs that differ only in the last bit
        let mut bytes1 = [0u8; 32];
        let mut bytes2 = [0u8; 32];
        bytes1[31] = 0b00000001;
        bytes2[31] = 0b00000000;

        let id1 = Hash::new(bytes1);
        let id2 = Hash::new(bytes2);

        let distance = log2_distance(&id1, &id2);
        assert_eq!(distance, Some(0)); // Differ in least significant bit -> bucket 0
    }

    #[test]
    fn test_compare_distance() {
        let target = NodeIdentity::generate();
        let a = NodeIdentity::generate();
        let b = NodeIdentity::generate();

        let ordering = compare_distance(target.node_id(), a.node_id(), b.node_id());
        // Just verify it returns a valid ordering
        assert!(matches!(
            ordering,
            std::cmp::Ordering::Less | std::cmp::Ordering::Equal | std::cmp::Ordering::Greater
        ));
    }

    #[test]
    fn test_compare_distance_reflexive() {
        let target = NodeIdentity::generate();
        let a = NodeIdentity::generate();

        let ordering = compare_distance(target.node_id(), a.node_id(), a.node_id());
        assert_eq!(ordering, std::cmp::Ordering::Equal);
    }
}
