mod address;
mod hash;
mod human_readable_proof;
mod transcript;

pub mod bech32;
pub mod elgamal;
pub mod error;
pub mod proofs;
pub mod random;

pub use address::*;
pub use error::CryptoError;
pub use hash::*;
pub use human_readable_proof::*;
pub use transcript::*;

pub use elgamal::{KeyPair, PrivateKey, Signature, SIGNATURE_SIZE};

/// Re-export the curve25519-dalek ecdlp module
pub use tos_crypto::curve25519_dalek::ecdlp;

/// Re-export Transcript for external crates that need to create ZK proofs
pub use tos_crypto::merlin::Transcript;

/// Public Key type used in the system
pub type PublicKey = elgamal::CompressedPublicKey;

/// Create a new Merlin transcript for proof generation
/// This is a convenience wrapper for external crates
#[inline]
pub fn new_proof_transcript(label: &'static [u8]) -> Transcript {
    Transcript::new(label)
}
