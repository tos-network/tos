//! VRF Key Management for Block Producers
//!
//! This module provides VRF key generation, loading, and signing functionality
//! for block producers in the TOS network.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use tos_crypto::vrf::{
    VrfError, VrfKeypair, VrfOutput, VrfProof, VrfPublicKey, VrfSecretKey, VRF_SECRET_KEY_SIZE,
};

/// VRF data produced by signing a block hash
///
/// Contains all the data needed to inject into InvokeContext for
/// smart contract VRF syscalls.
#[derive(Clone, Debug)]
pub struct VrfData {
    /// The block producer's VRF public key
    pub public_key: VrfPublicKey,
    /// The VRF output (pre-output, verifiable)
    pub output: VrfOutput,
    /// The VRF proof for verification
    pub proof: VrfProof,
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
        let decoded: [u8; VRF_SECRET_KEY_SIZE] = hex::decode(s)
            .map_err(|e| VrfError::InvalidSecretKey(format!("Invalid hex: {}", e)))?
            .try_into()
            .map_err(|_| VrfError::InvalidLength {
                expected: VRF_SECRET_KEY_SIZE,
                actual: 0,
            })?;

        let secret = VrfSecretKey::from_bytes(&decoded)?;
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

    /// Sign a block hash to produce VRF data
    ///
    /// # Arguments
    ///
    /// * `block_hash` - The 32-byte block hash to sign
    ///
    /// # Returns
    ///
    /// VRF data containing public key, output, and proof
    pub fn sign(&self, block_hash: &[u8; 32]) -> Result<VrfData, VrfError> {
        let (output, proof) = self.keypair.sign(block_hash)?;

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "VRF signed block_hash={}, output={}",
                hex::encode(block_hash),
                hex::encode(output.as_bytes())
            );
        }

        Ok(VrfData {
            public_key: self.keypair.public_key(),
            output,
            proof,
        })
    }

    /// Verify VRF data against a block hash
    ///
    /// This is mainly for testing/debugging purposes.
    /// In production, verification happens in InvokeContext::validate_vrf().
    pub fn verify(&self, block_hash: &[u8; 32], data: &VrfData) -> Result<(), VrfError> {
        data.public_key
            .verify(block_hash, &data.output, &data.proof)
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

        let data = manager.sign(&block_hash).unwrap();

        // Verify the signature
        assert!(manager.verify(&block_hash, &data).is_ok());

        // Verify with public key directly
        assert!(data
            .public_key
            .verify(&block_hash, &data.output, &data.proof)
            .is_ok());
    }

    #[test]
    fn test_deterministic_output() {
        let manager = VrfKeyManager::new();
        let block_hash = [0x42u8; 32];

        let data1 = manager.sign(&block_hash).unwrap();
        let data2 = manager.sign(&block_hash).unwrap();

        // Same block hash should produce same output
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
        let data1 = manager1.sign(&block_hash).unwrap();
        let data2 = manager2.sign(&block_hash).unwrap();
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
}
