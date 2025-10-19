// Balance simplification: Proof implementations removed
// This module now keeps essential cryptographic constants and error types

use thiserror::Error;
use super::elgamal::DecompressionError;
use curve25519_dalek::{RistrettoPoint, Scalar};
use lazy_static::lazy_static;
use sha3::Sha3_512;

// Essential cryptographic constants still needed for signatures and Pedersen commitments

// G: Primary generator point (Ristretto basepoint)
pub use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT as G;

lazy_static! {
    // H: Secondary generator point for signatures (Schnorr scheme)
    // Generated deterministically from G using hash-to-point
    pub static ref H: RistrettoPoint = {
        RistrettoPoint::hash_from_bytes::<Sha3_512>(b"TOS_SIGNATURE_GENERATOR_H")
    };
}

// Pedersen commitment generators (needed for commitment arithmetic)
pub struct PedersenGens {
    pub g: RistrettoPoint,
    pub h: RistrettoPoint,
}

impl PedersenGens {
    pub fn commit(&self, value: Scalar, blinding: Scalar) -> RistrettoPoint {
        value * self.g + blinding * self.h
    }
}

lazy_static! {
    // PC_GENS: Pedersen commitment generators
    pub static ref PC_GENS: PedersenGens = {
        PedersenGens {
            g: G,  // G is already a RistrettoPoint value
            h: *H, // H is a static ref, so dereference it
        }
    };
}


// Error types kept for backward compatibility

#[derive(Error, Debug, Clone, Copy, Eq, PartialEq)]
#[error("proof generation error (proofs removed in plaintext balance system)")]
pub struct ProofGenerationError;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofVerificationError {
    #[error("invalid format: {0}")]
    Decompression(#[from] DecompressionError),
    #[error("proof verification not supported (plaintext balance system)")]
    NotSupported,
}

// Stub proof types for backward compatibility with contract system
// These types exist solely for serialization and will always fail validation

use crate::serializer::{Reader, ReaderError, Serializer, Writer};
use serde::{Deserialize, Serialize};

/// Stub: CiphertextValidityProof (proofs removed in plaintext balance system)
/// Used only for contract deposit serialization compatibility
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CiphertextValidityProof {
    // Empty stub - proofs are not validated
}

impl Serializer for CiphertextValidityProof {
    fn write(&self, writer: &mut Writer) {
        // Write a single zero byte as placeholder
        writer.write_u8(0);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        // Read the placeholder byte
        reader.read_u8()?;
        Ok(CiphertextValidityProof {})
    }

    fn size(&self) -> usize {
        1
    }
}
