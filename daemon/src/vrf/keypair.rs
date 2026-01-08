//! VRF Key Management for Block Producers
//!
//! This module provides VRF key generation, loading, and signing functionality
//! for block producers in the TOS network.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use tos_common::block::{compute_vrf_binding_message, compute_vrf_input};
use tos_common::crypto::elgamal::CompressedPublicKey;
use tos_common::crypto::{KeyPair, PrivateKey, Signature};
use tos_common::serializer::Serializer;
use tos_crypto::vrf::{
    VrfError, VrfKeypair, VrfOutput, VrfProof, VrfPublicKey, VrfSecretKey, VRF_SECRET_KEY_SIZE,
};

/// Size of miner private key in bytes (32 bytes for Ristretto scalar)
pub const MINER_SECRET_KEY_SIZE: usize = 32;

/// VRF data produced by signing a block hash
///
/// Contains all the data needed to inject into InvokeContext for
/// smart contract VRF syscalls, plus the binding signature.
#[derive(Clone, Debug)]
pub struct VrfData {
    /// The block producer's VRF public key
    pub public_key: VrfPublicKey,
    /// The VRF output (pre-output, verifiable)
    pub output: VrfOutput,
    /// The VRF proof for verification
    pub proof: VrfProof,
    /// Miner's signature binding VRF key to this block
    pub binding_signature: Signature,
}

/// Wrapped VRF secret key for clap and serde support
///
/// This wrapper provides hex serialization/deserialization and
/// implements the necessary traits for use with clap command line parsing.
#[derive(Clone)]
pub struct WrappedVrfSecret(VrfSecretKey);

impl fmt::Debug for WrappedVrfSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WrappedVrfSecret([REDACTED])")
    }
}

impl FromStr for WrappedVrfSecret {
    type Err = VrfError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let decoded = hex::decode(s)
            .map_err(|e| VrfError::InvalidSecretKey(format!("Invalid hex: {}", e)))?;

        let decoded_len = decoded.len();
        let decoded_array: [u8; VRF_SECRET_KEY_SIZE] =
            decoded.try_into().map_err(|_| VrfError::InvalidLength {
                expected: VRF_SECRET_KEY_SIZE,
                actual: decoded_len,
            })?;

        let secret = VrfSecretKey::from_bytes(&decoded_array)?;
        Ok(Self(secret))
    }
}

impl Serialize for WrappedVrfSecret {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // SECURITY: Never serialize the actual secret key to prevent leakage
        // in config dumps, logs, or telemetry. Use to_hex() explicitly if needed.
        serializer.serialize_str("[REDACTED]")
    }
}

impl<'a> Deserialize<'a> for WrappedVrfSecret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let s = String::deserialize(deserializer)?;

        // SECURITY: Detect serialized redacted values and provide clear error
        // This prevents confusion when loading configs that were dumped with redacted keys
        if s == "[REDACTED]" {
            return Err(serde::de::Error::custom(
                "Cannot deserialize redacted VRF secret key. \
                 Provide the actual hex-encoded secret key (64 hex chars = 32 bytes).",
            ));
        }

        WrappedVrfSecret::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl WrappedVrfSecret {
    /// Get the inner secret key
    pub fn inner(&self) -> &VrfSecretKey {
        &self.0
    }

    /// Export the secret key as a hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    /// Create a keypair from this secret
    pub fn to_keypair(&self) -> Result<VrfKeypair, VrfError> {
        VrfKeypair::from_secret_key(&self.0)
    }
}

/// Error type for miner key operations
#[derive(Debug, Clone)]
pub enum MinerKeyError {
    /// Invalid hex encoding
    InvalidHex(String),
    /// Invalid key length
    InvalidLength { expected: usize, actual: usize },
    /// Invalid private key
    InvalidPrivateKey(String),
}

impl std::fmt::Display for MinerKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHex(e) => write!(f, "Invalid hex: {}", e),
            Self::InvalidLength { expected, actual } => {
                write!(
                    f,
                    "Invalid key length: expected {}, got {}",
                    expected, actual
                )
            }
            Self::InvalidPrivateKey(e) => write!(f, "Invalid private key: {}", e),
        }
    }
}

impl std::error::Error for MinerKeyError {}

/// Wrapped miner secret key for clap and serde support
///
/// This wrapper provides hex serialization/deserialization for the miner's
/// private key, which is used to sign VRF binding messages.
#[derive(Clone)]
pub struct WrappedMinerSecret(KeyPair);

impl fmt::Debug for WrappedMinerSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WrappedMinerSecret([REDACTED])")
    }
}

impl FromStr for WrappedMinerSecret {
    type Err = MinerKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let decoded = hex::decode(s).map_err(|e| MinerKeyError::InvalidHex(e.to_string()))?;

        let decoded_len = decoded.len();
        let decoded_array: [u8; MINER_SECRET_KEY_SIZE] =
            decoded
                .try_into()
                .map_err(|_| MinerKeyError::InvalidLength {
                    expected: MINER_SECRET_KEY_SIZE,
                    actual: decoded_len,
                })?;

        let private_key = PrivateKey::from_bytes(&decoded_array)
            .map_err(|e| MinerKeyError::InvalidPrivateKey(e.to_string()))?;
        Ok(Self(KeyPair::from_private_key(private_key)))
    }
}

impl Serialize for WrappedMinerSecret {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // SECURITY: Never serialize the actual secret key to prevent leakage
        serializer.serialize_str("[REDACTED]")
    }
}

impl<'de> Deserialize<'de> for WrappedMinerSecret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        // SECURITY: Detect serialized redacted values
        if s == "[REDACTED]" {
            return Err(serde::de::Error::custom(
                "Cannot deserialize redacted miner secret key. \
                 Provide the actual hex-encoded secret key (64 hex chars = 32 bytes).",
            ));
        }

        WrappedMinerSecret::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl WrappedMinerSecret {
    /// Get the inner keypair
    pub fn keypair(&self) -> &KeyPair {
        &self.0
    }

    /// Export the secret key as a hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.get_private_key().to_bytes())
    }
}

/// VRF Key Manager for block producers
///
/// Manages VRF keypair generation, loading, and signing operations.
/// The keypair is used to sign block hashes and produce verifiable
/// random outputs for smart contracts.
pub struct VrfKeyManager {
    keypair: VrfKeypair,
}

impl VrfKeyManager {
    /// Create a new VRF key manager with a randomly generated keypair
    pub fn new() -> Self {
        let keypair = VrfKeypair::generate();
        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Generated new VRF keypair, public key: {}",
                hex::encode(keypair.public_key().as_bytes())
            );
        }
        Self { keypair }
    }

    /// Create from a hex-encoded secret key
    pub fn from_hex(hex_secret: &str) -> Result<Self, VrfError> {
        let wrapped = WrappedVrfSecret::from_str(hex_secret)?;
        let keypair = wrapped.to_keypair()?;
        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Loaded VRF keypair from secret, public key: {}",
                hex::encode(keypair.public_key().as_bytes())
            );
        }
        Ok(Self { keypair })
    }

    /// Create from a wrapped secret key
    pub fn from_secret(secret: &WrappedVrfSecret) -> Result<Self, VrfError> {
        let keypair = secret.to_keypair()?;
        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Loaded VRF keypair from wrapped secret, public key: {}",
                hex::encode(keypair.public_key().as_bytes())
            );
        }
        Ok(Self { keypair })
    }

    /// Create from a VrfSecretKey
    pub fn from_secret_key(secret: &VrfSecretKey) -> Result<Self, VrfError> {
        let keypair = VrfKeypair::from_secret_key(secret)?;
        if log::log_enabled!(log::Level::Info) {
            log::info!(
                "Loaded VRF keypair from secret key, public key: {}",
                hex::encode(keypair.public_key().as_bytes())
            );
        }
        Ok(Self { keypair })
    }

    /// Get the VRF public key
    pub fn public_key(&self) -> VrfPublicKey {
        self.keypair.public_key()
    }

    /// Get the VRF secret key (hex encoded)
    pub fn secret_key_hex(&self) -> String {
        hex::encode(self.keypair.secret_key().to_bytes())
    }

    /// Sign a block hash to produce VRF data bound to the miner's identity.
    ///
    /// # Security
    ///
    /// Two bindings are created:
    /// 1. **VRF input binding**: `vrf_input = BLAKE3("TOS-VRF-INPUT-v1" || block_hash || miner_public_key)`
    ///    - Ensures VRF output is unique per miner
    /// 2. **Key ownership binding**: `miner.sign(BLAKE3("TOS-VRF-BINDING-v1" || chain_id || vrf_public_key || block_hash))`
    ///    - Proves the VRF key belongs to this miner
    ///    - Chain ID prevents cross-chain replay attacks
    ///
    /// Together, these prevent VRF proof substitution attacks because:
    /// - An attacker cannot create a valid binding signature without miner's private key
    /// - Even with a whitelisted VRF key, the attacker cannot prove ownership
    /// - Signatures from devnet/testnet cannot be replayed on mainnet
    ///
    /// # Arguments
    ///
    /// * `chain_id` - Network::chain_id() value (0=Mainnet, 1=Testnet, 2=Stagenet, 3=Devnet)
    /// * `block_hash` - The 32-byte block hash (excludes VRF fields)
    /// * `miner` - The block producer's compressed public key
    /// * `miner_keypair` - The block producer's full keypair for signing
    ///
    /// # Returns
    ///
    /// VRF data containing public key, output, proof, and binding signature
    pub fn sign(
        &self,
        chain_id: u64,
        block_hash: &[u8; 32],
        miner: &CompressedPublicKey,
        miner_keypair: &KeyPair,
    ) -> Result<VrfData, VrfError> {
        // 1. Compute VRF input with miner identity binding
        let vrf_input = compute_vrf_input(block_hash, miner);
        let (output, proof) = self.keypair.sign(&vrf_input)?;

        // 2. Create binding signature: miner signs (chain_id || vrf_public_key || block_hash)
        let vrf_public_key_bytes = self.keypair.public_key().to_bytes();
        let binding_message =
            compute_vrf_binding_message(chain_id, &vrf_public_key_bytes, block_hash);
        let binding_signature = miner_keypair.sign(&binding_message);

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "VRF signed: chain_id={}, block_hash={}, miner={}, vrf_pk={}, binding_sig={}",
                chain_id,
                hex::encode(block_hash),
                hex::encode(miner.as_bytes()),
                hex::encode(&vrf_public_key_bytes),
                hex::encode(binding_signature.to_bytes())
            );
        }

        Ok(VrfData {
            public_key: self.keypair.public_key(),
            output,
            proof,
            binding_signature,
        })
    }

    /// Verify VRF data against a block hash and miner identity.
    ///
    /// This verifies only the VRF proof, not the binding signature.
    /// Full verification including binding signature happens in blockchain.rs verify_block_vrf_data().
    ///
    /// This is mainly for testing/debugging purposes.
    pub fn verify_vrf_proof(
        &self,
        block_hash: &[u8; 32],
        miner: &CompressedPublicKey,
        data: &VrfData,
    ) -> Result<(), VrfError> {
        let vrf_input = compute_vrf_input(block_hash, miner);
        data.public_key
            .verify(&vrf_input, &data.output, &data.proof)
    }
}

impl Default for VrfKeyManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::crypto::KeyPair;

    /// Test chain ID (devnet)
    const TEST_CHAIN_ID: u64 = 3;

    /// Create a test miner keypair and compressed public key
    fn test_miner_keypair() -> (KeyPair, CompressedPublicKey) {
        let keypair = KeyPair::new();
        let compressed = keypair.get_public_key().compress();
        (keypair, compressed)
    }

    #[test]
    fn test_new_key_manager() {
        let manager = VrfKeyManager::new();
        let pk = manager.public_key();
        assert_eq!(pk.as_bytes().len(), 32);
    }

    #[test]
    fn test_sign_and_verify() {
        let manager = VrfKeyManager::new();
        let block_hash = [0x42u8; 32];
        let (miner_keypair, miner) = test_miner_keypair();

        let data = manager
            .sign(TEST_CHAIN_ID, &block_hash, &miner, &miner_keypair)
            .unwrap();

        // Verify the VRF proof
        assert!(manager.verify_vrf_proof(&block_hash, &miner, &data).is_ok());

        // Verify with public key directly using vrf_input
        let vrf_input = compute_vrf_input(&block_hash, &miner);
        assert!(data
            .public_key
            .verify(&vrf_input, &data.output, &data.proof)
            .is_ok());

        // Verify binding signature
        let vrf_public_key_bytes = data.public_key.to_bytes();
        let binding_message =
            compute_vrf_binding_message(TEST_CHAIN_ID, &vrf_public_key_bytes, &block_hash);
        assert!(data
            .binding_signature
            .verify(&binding_message, &miner_keypair.get_public_key()));
    }

    #[test]
    fn test_deterministic_output() {
        let manager = VrfKeyManager::new();
        let block_hash = [0x42u8; 32];
        let (miner_keypair, miner) = test_miner_keypair();

        let data1 = manager
            .sign(TEST_CHAIN_ID, &block_hash, &miner, &miner_keypair)
            .unwrap();
        let data2 = manager
            .sign(TEST_CHAIN_ID, &block_hash, &miner, &miner_keypair)
            .unwrap();

        // Same block hash + miner should produce same VRF output
        assert_eq!(data1.output.as_bytes(), data2.output.as_bytes());
    }

    #[test]
    fn test_from_hex() {
        // Generate a keypair and export the secret
        let manager1 = VrfKeyManager::new();
        let hex_secret = manager1.secret_key_hex();

        // Recreate from hex
        let manager2 = VrfKeyManager::from_hex(&hex_secret).unwrap();

        // Public keys should match
        assert_eq!(
            manager1.public_key().as_bytes(),
            manager2.public_key().as_bytes()
        );

        // Signatures should produce same output
        let block_hash = [0x42u8; 32];
        let (miner_keypair, miner) = test_miner_keypair();
        let data1 = manager1
            .sign(TEST_CHAIN_ID, &block_hash, &miner, &miner_keypair)
            .unwrap();
        let data2 = manager2
            .sign(TEST_CHAIN_ID, &block_hash, &miner, &miner_keypair)
            .unwrap();
        assert_eq!(data1.output.as_bytes(), data2.output.as_bytes());
    }

    #[test]
    fn test_wrapped_vrf_secret() {
        let manager = VrfKeyManager::new();
        let hex_secret = manager.secret_key_hex();

        let wrapped: WrappedVrfSecret = hex_secret.parse().unwrap();
        let manager2 = VrfKeyManager::from_secret(&wrapped).unwrap();

        assert_eq!(
            manager.public_key().as_bytes(),
            manager2.public_key().as_bytes()
        );
    }

    #[test]
    fn test_vrf_bound_to_miner() {
        // Setup: Two different miners
        let (miner_a_keypair, miner_a) = test_miner_keypair();
        let (_, miner_b) = test_miner_keypair();
        let manager = VrfKeyManager::new();
        let block_hash = [0x42u8; 32];

        // Sign VRF for miner A
        let vrf_data = manager
            .sign(TEST_CHAIN_ID, &block_hash, &miner_a, &miner_a_keypair)
            .unwrap();

        // VRF proof verification succeeds for miner A
        assert!(manager
            .verify_vrf_proof(&block_hash, &miner_a, &vrf_data)
            .is_ok());

        // VRF proof verification FAILS for miner B (different miner, same VRF proof)
        assert!(manager
            .verify_vrf_proof(&block_hash, &miner_b, &vrf_data)
            .is_err());
    }

    #[test]
    fn test_vrf_different_blocks_same_miner() {
        let (miner_keypair, miner) = test_miner_keypair();
        let manager = VrfKeyManager::new();

        let block_hash_1 = [0x01u8; 32];
        let block_hash_2 = [0x02u8; 32];

        let vrf_1 = manager
            .sign(TEST_CHAIN_ID, &block_hash_1, &miner, &miner_keypair)
            .unwrap();
        let vrf_2 = manager
            .sign(TEST_CHAIN_ID, &block_hash_2, &miner, &miner_keypair)
            .unwrap();

        // Different blocks produce different VRF outputs
        assert_ne!(vrf_1.output.as_bytes(), vrf_2.output.as_bytes());

        // Each proof only valid for its own block
        assert!(manager
            .verify_vrf_proof(&block_hash_1, &miner, &vrf_1)
            .is_ok());
        assert!(manager
            .verify_vrf_proof(&block_hash_2, &miner, &vrf_2)
            .is_ok());

        // Cross-verification fails
        assert!(manager
            .verify_vrf_proof(&block_hash_1, &miner, &vrf_2)
            .is_err());
        assert!(manager
            .verify_vrf_proof(&block_hash_2, &miner, &vrf_1)
            .is_err());
    }

    #[test]
    fn test_different_miners_different_vrf_input() {
        let (_, miner_a) = test_miner_keypair();
        let (_, miner_b) = test_miner_keypair();
        let block_hash = [0x42u8; 32];

        let vrf_input_a = compute_vrf_input(&block_hash, &miner_a);
        let vrf_input_b = compute_vrf_input(&block_hash, &miner_b);

        // Same block hash but different miners should produce different VRF inputs
        assert_ne!(vrf_input_a, vrf_input_b);
    }

    #[test]
    fn test_binding_signature_chain_id() {
        // Binding signature should be chain-specific
        let manager = VrfKeyManager::new();
        let block_hash = [0x42u8; 32];
        let (miner_keypair, miner) = test_miner_keypair();

        let chain_id_mainnet: u64 = 0;
        let chain_id_testnet: u64 = 1;

        let data_mainnet = manager
            .sign(chain_id_mainnet, &block_hash, &miner, &miner_keypair)
            .unwrap();
        let data_testnet = manager
            .sign(chain_id_testnet, &block_hash, &miner, &miner_keypair)
            .unwrap();

        // VRF outputs should be the same (VRF input doesn't include chain_id)
        assert_eq!(
            data_mainnet.output.as_bytes(),
            data_testnet.output.as_bytes()
        );

        // Binding signatures should be different (chain_id is included)
        assert_ne!(
            data_mainnet.binding_signature.to_bytes(),
            data_testnet.binding_signature.to_bytes()
        );

        // Verify mainnet signature against mainnet binding message
        let vrf_public_key_bytes = data_mainnet.public_key.to_bytes();
        let mainnet_binding =
            compute_vrf_binding_message(chain_id_mainnet, &vrf_public_key_bytes, &block_hash);
        assert!(data_mainnet
            .binding_signature
            .verify(&mainnet_binding, &miner_keypair.get_public_key()));

        // Mainnet signature should NOT verify against testnet binding message
        let testnet_binding =
            compute_vrf_binding_message(chain_id_testnet, &vrf_public_key_bytes, &block_hash);
        assert!(!data_mainnet
            .binding_signature
            .verify(&testnet_binding, &miner_keypair.get_public_key()));
    }
}
