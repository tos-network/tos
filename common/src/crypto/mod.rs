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

/// Public Key type used in the system
pub type PublicKey = elgamal::CompressedPublicKey;
