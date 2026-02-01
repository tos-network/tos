//! Ed25519 cryptographic primitives for node identity in the discovery protocol.
//!
//! This module provides Ed25519 key types used for node authentication
//! in the discv6-based peer discovery protocol.

use ed25519_dalek::{
    Signature as DalekSignature, Signer, SigningKey, Verifier, VerifyingKey, SECRET_KEY_LENGTH,
    SIGNATURE_LENGTH,
};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::fmt;
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::Hash;

/// Size of Ed25519 secret key in bytes.
pub const ED25519_SECRET_KEY_SIZE: usize = SECRET_KEY_LENGTH;

/// Size of Ed25519 public key in bytes.
pub const ED25519_PUBLIC_KEY_SIZE: usize = 32;

/// Size of Ed25519 signature in bytes.
pub const ED25519_SIGNATURE_SIZE: usize = SIGNATURE_LENGTH;

/// Error types for Ed25519 operations.
#[derive(Error, Debug, Clone)]
pub enum Ed25519Error {
    /// Invalid secret key length.
    #[error(
        "Invalid secret key length: expected {}, got {}",
        ED25519_SECRET_KEY_SIZE,
        _0
    )]
    InvalidSecretKeyLength(usize),

    /// Invalid public key length.
    #[error(
        "Invalid public key length: expected {}, got {}",
        ED25519_PUBLIC_KEY_SIZE,
        _0
    )]
    InvalidPublicKeyLength(usize),

    /// Invalid signature length.
    #[error(
        "Invalid signature length: expected {}, got {}",
        ED25519_SIGNATURE_SIZE,
        _0
    )]
    InvalidSignatureLength(usize),

    /// Failed to parse secret key bytes.
    #[error("Failed to parse secret key")]
    InvalidSecretKey,

    /// Failed to parse public key bytes.
    #[error("Failed to parse public key")]
    InvalidPublicKey,

    /// Signature verification failed.
    #[error("Signature verification failed")]
    VerificationFailed,

    /// Hex decoding error.
    #[error("Invalid hex string: {0}")]
    HexError(String),
}

/// Ed25519 secret key (32 bytes).
///
/// The secret key is zeroized on drop for security.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Ed25519SecretKey([u8; ED25519_SECRET_KEY_SIZE]);

impl Ed25519SecretKey {
    /// Create a secret key from raw bytes.
    pub fn from_bytes(bytes: [u8; ED25519_SECRET_KEY_SIZE]) -> Self {
        Self(bytes)
    }

    /// Create a secret key from a slice.
    pub fn from_slice(slice: &[u8]) -> Result<Self, Ed25519Error> {
        if slice.len() != ED25519_SECRET_KEY_SIZE {
            return Err(Ed25519Error::InvalidSecretKeyLength(slice.len()));
        }
        let mut bytes = [0u8; ED25519_SECRET_KEY_SIZE];
        bytes.copy_from_slice(slice);
        Ok(Self(bytes))
    }

    /// Create a secret key from a hex string.
    pub fn from_hex(hex: &str) -> Result<Self, Ed25519Error> {
        let bytes = hex::decode(hex).map_err(|e| Ed25519Error::HexError(e.to_string()))?;
        Self::from_slice(&bytes)
    }

    /// Get the raw bytes of the secret key.
    pub fn as_bytes(&self) -> &[u8; ED25519_SECRET_KEY_SIZE] {
        &self.0
    }

    /// Convert to hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for Ed25519SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ed25519SecretKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

/// Ed25519 public key (32 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ed25519PublicKey([u8; ED25519_PUBLIC_KEY_SIZE]);

impl Ed25519PublicKey {
    /// Create a public key from raw bytes.
    pub fn from_bytes(bytes: [u8; ED25519_PUBLIC_KEY_SIZE]) -> Self {
        Self(bytes)
    }

    /// Create a public key from a slice.
    pub fn from_slice(slice: &[u8]) -> Result<Self, Ed25519Error> {
        if slice.len() != ED25519_PUBLIC_KEY_SIZE {
            return Err(Ed25519Error::InvalidPublicKeyLength(slice.len()));
        }
        let mut bytes = [0u8; ED25519_PUBLIC_KEY_SIZE];
        bytes.copy_from_slice(slice);
        Ok(Self(bytes))
    }

    /// Create a public key from a hex string.
    pub fn from_hex(hex: &str) -> Result<Self, Ed25519Error> {
        let bytes = hex::decode(hex).map_err(|e| Ed25519Error::HexError(e.to_string()))?;
        Self::from_slice(&bytes)
    }

    /// Get the raw bytes of the public key.
    pub fn as_bytes(&self) -> &[u8; ED25519_PUBLIC_KEY_SIZE] {
        &self.0
    }

    /// Convert to hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Compute the node ID from this public key.
    ///
    /// The node ID is the SHA3-256 hash of the public key bytes.
    /// This is used for Kademlia routing table organization.
    pub fn node_id(&self) -> Hash {
        let mut hasher = Sha3_256::new();
        hasher.update(self.0);
        let result = hasher.finalize();
        Hash::new(result.into())
    }

    /// Verify a signature on a message.
    pub fn verify(&self, message: &[u8], signature: &Ed25519Signature) -> Result<(), Ed25519Error> {
        let verifying_key =
            VerifyingKey::from_bytes(&self.0).map_err(|_| Ed25519Error::InvalidPublicKey)?;
        let dalek_sig = DalekSignature::from_bytes(&signature.0);
        verifying_key
            .verify(message, &dalek_sig)
            .map_err(|_| Ed25519Error::VerificationFailed)
    }
}

impl fmt::Debug for Ed25519PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ed25519PublicKey({})", self.to_hex())
    }
}

impl fmt::Display for Ed25519PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Ed25519 signature (64 bytes).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Ed25519Signature([u8; ED25519_SIGNATURE_SIZE]);

impl Serialize for Ed25519Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Ed25519Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

impl Ed25519Signature {
    /// Create a signature from raw bytes.
    pub fn from_bytes(bytes: [u8; ED25519_SIGNATURE_SIZE]) -> Self {
        Self(bytes)
    }

    /// Create a signature from a slice.
    pub fn from_slice(slice: &[u8]) -> Result<Self, Ed25519Error> {
        if slice.len() != ED25519_SIGNATURE_SIZE {
            return Err(Ed25519Error::InvalidSignatureLength(slice.len()));
        }
        let mut bytes = [0u8; ED25519_SIGNATURE_SIZE];
        bytes.copy_from_slice(slice);
        Ok(Self(bytes))
    }

    /// Create a signature from a hex string.
    pub fn from_hex(hex: &str) -> Result<Self, Ed25519Error> {
        let bytes = hex::decode(hex).map_err(|e| Ed25519Error::HexError(e.to_string()))?;
        Self::from_slice(&bytes)
    }

    /// Get the raw bytes of the signature.
    pub fn as_bytes(&self) -> &[u8; ED25519_SIGNATURE_SIZE] {
        &self.0
    }

    /// Convert to hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for Ed25519Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ed25519Signature({}...)", &self.to_hex()[..16])
    }
}

impl fmt::Display for Ed25519Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Ed25519 key pair containing both secret and public keys.
///
/// The secret key is zeroized on drop for security.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Ed25519KeyPair {
    #[zeroize(skip)]
    signing_key: SigningKey,
}

impl Ed25519KeyPair {
    /// Generate a new random key pair using a cryptographically secure RNG.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self { signing_key }
    }

    /// Create a key pair from a secret key.
    pub fn from_secret(secret: &Ed25519SecretKey) -> Result<Self, Ed25519Error> {
        let signing_key = SigningKey::from_bytes(secret.as_bytes());
        Ok(Self { signing_key })
    }

    /// Create a key pair from secret key bytes.
    pub fn from_secret_bytes(bytes: &[u8; ED25519_SECRET_KEY_SIZE]) -> Self {
        let signing_key = SigningKey::from_bytes(bytes);
        Self { signing_key }
    }

    /// Get the secret key.
    pub fn secret_key(&self) -> Ed25519SecretKey {
        Ed25519SecretKey::from_bytes(self.signing_key.to_bytes())
    }

    /// Get the public key.
    pub fn public_key(&self) -> Ed25519PublicKey {
        Ed25519PublicKey::from_bytes(self.signing_key.verifying_key().to_bytes())
    }

    /// Sign a message and return the signature.
    pub fn sign(&self, message: &[u8]) -> Ed25519Signature {
        let signature = self.signing_key.sign(message);
        Ed25519Signature::from_bytes(signature.to_bytes())
    }

    /// Compute the node ID (SHA3-256 hash of the public key).
    pub fn node_id(&self) -> Hash {
        self.public_key().node_id()
    }
}

impl fmt::Debug for Ed25519KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ed25519KeyPair")
            .field("public_key", &self.public_key())
            .field("secret_key", &"[REDACTED]")
            .finish()
    }
}

/// Wrapper type for Ed25519 secret key that can be parsed from CLI arguments.
///
/// This wrapper provides `FromStr` implementation for use with clap.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct WrappedEd25519Secret(Ed25519SecretKey);

impl WrappedEd25519Secret {
    /// Create a new wrapped secret key.
    pub fn new(secret: Ed25519SecretKey) -> Self {
        Self(secret)
    }

    /// Get the inner secret key.
    pub fn inner(&self) -> &Ed25519SecretKey {
        &self.0
    }

    /// Convert to key pair.
    pub fn to_keypair(&self) -> Result<Ed25519KeyPair, Ed25519Error> {
        Ed25519KeyPair::from_secret(&self.0)
    }
}

impl std::str::FromStr for WrappedEd25519Secret {
    type Err = Ed25519Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let secret = Ed25519SecretKey::from_hex(s)?;
        Ok(Self(secret))
    }
}

impl fmt::Debug for WrappedEd25519Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WrappedEd25519Secret")
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Display for WrappedEd25519Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl Serialize for WrappedEd25519Secret {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_hex())
    }
}

impl<'de> Deserialize<'de> for WrappedEd25519Secret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let keypair = Ed25519KeyPair::generate();
        let public_key = keypair.public_key();
        let secret_key = keypair.secret_key();

        assert_eq!(public_key.as_bytes().len(), ED25519_PUBLIC_KEY_SIZE);
        assert_eq!(secret_key.as_bytes().len(), ED25519_SECRET_KEY_SIZE);
    }

    #[test]
    fn test_keypair_from_secret() {
        let keypair1 = Ed25519KeyPair::generate();
        let secret = keypair1.secret_key();

        let keypair2 = Ed25519KeyPair::from_secret(&secret).unwrap();
        assert_eq!(keypair1.public_key(), keypair2.public_key());
    }

    #[test]
    fn test_sign_and_verify() {
        let keypair = Ed25519KeyPair::generate();
        let message = b"Hello, TOS discovery!";

        let signature = keypair.sign(message);
        let result = keypair.public_key().verify(message, &signature);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_wrong_message() {
        let keypair = Ed25519KeyPair::generate();
        let message = b"Hello, TOS discovery!";
        let wrong_message = b"Wrong message";

        let signature = keypair.sign(message);
        let result = keypair.public_key().verify(wrong_message, &signature);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_wrong_key() {
        let keypair1 = Ed25519KeyPair::generate();
        let keypair2 = Ed25519KeyPair::generate();
        let message = b"Hello, TOS discovery!";

        let signature = keypair1.sign(message);
        let result = keypair2.public_key().verify(message, &signature);
        assert!(result.is_err());
    }

    #[test]
    fn test_node_id_deterministic() {
        let keypair = Ed25519KeyPair::generate();
        let node_id1 = keypair.node_id();
        let node_id2 = keypair.public_key().node_id();

        assert_eq!(node_id1, node_id2);
    }

    #[test]
    fn test_node_id_different_keys() {
        let keypair1 = Ed25519KeyPair::generate();
        let keypair2 = Ed25519KeyPair::generate();

        assert_ne!(keypair1.node_id(), keypair2.node_id());
    }

    #[test]
    fn test_hex_roundtrip() {
        let keypair = Ed25519KeyPair::generate();
        let secret = keypair.secret_key();
        let public = keypair.public_key();

        let secret_hex = secret.to_hex();
        let public_hex = public.to_hex();

        let secret_parsed = Ed25519SecretKey::from_hex(&secret_hex).unwrap();
        let public_parsed = Ed25519PublicKey::from_hex(&public_hex).unwrap();

        assert_eq!(secret.as_bytes(), secret_parsed.as_bytes());
        assert_eq!(public.as_bytes(), public_parsed.as_bytes());
    }

    #[test]
    fn test_signature_roundtrip() {
        let keypair = Ed25519KeyPair::generate();
        let message = b"Test message";
        let signature = keypair.sign(message);

        let sig_hex = signature.to_hex();
        let sig_parsed = Ed25519Signature::from_hex(&sig_hex).unwrap();

        assert_eq!(signature.as_bytes(), sig_parsed.as_bytes());
        assert!(keypair.public_key().verify(message, &sig_parsed).is_ok());
    }

    #[test]
    fn test_wrapped_secret_from_str() {
        let keypair = Ed25519KeyPair::generate();
        let secret_hex = keypair.secret_key().to_hex();

        let wrapped: WrappedEd25519Secret = secret_hex.parse().unwrap();
        let recovered_keypair = wrapped.to_keypair().unwrap();

        assert_eq!(keypair.public_key(), recovered_keypair.public_key());
    }

    #[test]
    fn test_invalid_lengths() {
        assert!(Ed25519SecretKey::from_slice(&[0u8; 16]).is_err());
        assert!(Ed25519PublicKey::from_slice(&[0u8; 16]).is_err());
        assert!(Ed25519Signature::from_slice(&[0u8; 32]).is_err());
    }

    #[test]
    fn test_invalid_hex() {
        assert!(Ed25519SecretKey::from_hex("invalid").is_err());
        assert!(Ed25519PublicKey::from_hex("zzzz").is_err());
        assert!(Ed25519Signature::from_hex("not-hex").is_err());
    }
}
