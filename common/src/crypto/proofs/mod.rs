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

// Stub types for backward compatibility
use crate::serializer::{Reader, ReaderError, Serializer, Writer};

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct OwnershipProof;

impl Serializer for OwnershipProof {
    fn write(&self, _writer: &mut Writer) {
        // Stub implementation - should never be called
        panic!("OwnershipProof removed in plaintext balance system");
    }

    fn read(_reader: &mut Reader) -> Result<Self, ReaderError> {
        // Stub implementation - should never be called
        Err(ReaderError::InvalidValue)
    }

    fn size(&self) -> usize {
        0
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
pub struct CiphertextValidityProof;

impl Serializer for CiphertextValidityProof {
    fn write(&self, _writer: &mut Writer) {
        // Stub implementation - should never be called
        panic!("CiphertextValidityProof removed in plaintext balance system");
    }

    fn read(_reader: &mut Reader) -> Result<Self, ReaderError> {
        // Stub implementation - should never be called
        Err(ReaderError::InvalidValue)
    }

    fn size(&self) -> usize {
        0
    }
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
