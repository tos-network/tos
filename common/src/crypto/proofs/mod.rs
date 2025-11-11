// Balance simplification: Proof implementations removed
// This module now re-exports cryptographic constants from tos-crypto

use super::elgamal::DecompressionError;
use curve25519_dalek::{RistrettoPoint, Scalar};
use thiserror::Error;

// Re-export generator points from tos-crypto
pub use tos_crypto::proofs::{G, H};

// Import lazy_static for PC_GENS
use lazy_static::lazy_static;

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

// Balance simplification: All proof types have been removed
// Only cryptographic constants (G, H, PC_GENS) and error types remain
