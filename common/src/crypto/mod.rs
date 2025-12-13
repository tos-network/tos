mod address;
mod hash;

pub mod bech32;
pub mod elgamal;
pub mod error;
pub mod proofs;
pub mod random;

pub use address::*;
pub use error::CryptoError;
pub use hash::*;

pub use elgamal::{KeyPair, PrivateKey, Signature, SIGNATURE_SIZE};

/// Re-export the curve25519-dalek ecdlp module
pub use curve25519_dalek::ecdlp;

/// Public Key type used in the system
pub type PublicKey = elgamal::CompressedPublicKey;
