mod ciphertext_validity;
mod commitment_eq;
mod range_proof;
mod ownership;

use std::iter;
use curve25519_dalek::{
    traits::{Identity, MultiscalarMul},
    RistrettoPoint,
    Scalar
};
use lazy_static::lazy_static;
use subtle::ConstantTimeEq;
use thiserror::Error;
use bulletproofs::{BulletproofGens, PedersenGens};
use crate::transaction::MAX_TRANSFER_COUNT;
use super::{elgamal::DecompressionError, TranscriptError};

// Exports
pub use commitment_eq::CommitmentEqProof;
pub use ciphertext_validity::CiphertextValidityProof;
pub use ownership::OwnershipProof;
pub use range_proof::RangeProof;

// We are using 64 bits for the bulletproofs
pub const BULLET_PROOF_SIZE: usize = 64;

lazy_static! {
    // Bulletproof generators: party size is max transfers * 2 + 1
    // * 2 in case each transfer use a unique asset + 1 for tos asset as fee and + 1 to be a power of 2
    pub static ref BP_GENS: BulletproofGens = BulletproofGens::new(BULLET_PROOF_SIZE, MAX_TRANSFER_COUNT * 2 + 2);
    pub static ref PC_GENS: PedersenGens = PedersenGens::default();
    // Re-export the base points for convenience
    pub static ref G: RistrettoPoint = PC_GENS.B;
    pub static ref H: RistrettoPoint = PC_GENS.B_blinding;
}

#[derive(Error, Debug, Clone, Copy, Eq, PartialEq)]
#[error("batch multiscalar mul returned non identity point")]
pub struct MultiscalarMulVerificationError;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofGenerationError {
    #[error(transparent)]
    Decompression(#[from] DecompressionError),
    #[error("not enough funds in the account, required: {required}, available: {available}")]
    InsufficientFunds {
        required: u64,
        available: u64,
    },
    #[error("range proof generation failed: {0}")]
    RangeProof(#[from] bulletproofs::ProofError),
    #[error("invalid format")]
    Format,
}

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum ProofVerificationError {
    #[error("invalid format: {0}")]
    Decompression(#[from] DecompressionError),
    #[error("commitment equality proof verification failed")]
    CommitmentEqProof,
    #[error("ciphertext validity proof verification failed")]
    CiphertextValidityProof,
    #[error("proof verification failed")]
    GenericProof,
    #[error("range proof verification failed: {0}")]
    RangeProof(#[from] bulletproofs::ProofError),
    #[error("transcript error: {0}")]
    Transcript(#[from] TranscriptError),
    #[error("invalid format")]
    Format,
    #[error(transparent)]
    BatchVerificationError(#[from] MultiscalarMulVerificationError),
}


#[derive(Default)]
pub struct BatchCollector {
    dynamic_scalars: Vec<Scalar>,
    dynamic_points: Vec<RistrettoPoint>,
    g_scalar: Scalar,
    h_scalar: Scalar,
}

impl BatchCollector {
    pub fn verify(&self) -> Result<(), MultiscalarMulVerificationError> {
        // Use constant-time multiscalar multiplication to prevent timing attacks
        let mega_check = RistrettoPoint::multiscalar_mul(
            self.dynamic_scalars
                .iter()
                .copied()
                .chain(iter::once(self.g_scalar))
                .chain(iter::once(self.h_scalar)),
            self.dynamic_points
                .iter()
                .copied()
                .chain(iter::once(*G))
                .chain(iter::once(*H)),
        );

        // Use constant-time comparison to prevent timing leaks
        if bool::from(mega_check.ct_eq(&RistrettoPoint::identity())) {
            Ok(())
        } else {
            Err(MultiscalarMulVerificationError)
        }
    }
}
