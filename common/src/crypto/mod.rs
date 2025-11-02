mod address;
mod hash;
// Balance simplification: transcript module removed (merlin dependency removed)
// pub mod transcript;
// mod human_readable_proof;
mod blue_work;

pub mod bech32;
pub mod elgamal;
pub mod proofs;

pub use address::*;
pub use hash::*;
// Balance simplification: transcript removed (merlin dependency removed)
// pub use transcript::*;
// pub use human_readable_proof::*;
pub use blue_work::*;

pub use elgamal::{KeyPair, PrivateKey, Signature, SIGNATURE_SIZE};

/// Re-export the curve25519-dalek ecdlp module
pub use curve25519_dalek::ecdlp;

/// Public Key type used in the system
pub type PublicKey = elgamal::CompressedPublicKey;
