//! VRF (Verifiable Random Function) Module for TOS Daemon
//!
//! This module provides VRF key management and signing for block producers.
//! VRF outputs are injected into contract execution context to provide
//! verifiable randomness to smart contracts.
//!
//! # Architecture
//!
//! ```text
//! Block Producer
//!     |
//!     v
//! VrfKeyManager (holds VRF keypair)
//!     |
//!     | sign(block_hash)
//!     v
//! (VrfOutput, VrfProof, VrfPublicKey)
//!     |
//!     | inject into InvokeContext
//!     v
//! Smart Contract calls tos_vrf_random()
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use tos_daemon::vrf::VrfKeyManager;
//!
//! // Create from config
//! let manager = VrfKeyManager::new()?;
//!
//! // Or load from hex private key
//! let manager = VrfKeyManager::from_hex("deadbeef...")?;
//!
//! // Sign a block hash to produce VRF data
//! let block_hash = [0u8; 32];
//! let vrf_data = manager.sign(&block_hash)?;
//!
//! // Inject into InvokeContext
//! invoke_context.vrf_public_key = Some(vrf_data.public_key);
//! invoke_context.vrf_output = Some(vrf_data.output);
//! invoke_context.vrf_proof = Some(vrf_data.proof);
//! invoke_context.validate_vrf()?;
//! ```

mod keypair;

pub use keypair::{
    MinerKeyError, VrfData, VrfKeyManager, WrappedMinerSecret, WrappedVrfSecret,
    MINER_SECRET_KEY_SIZE,
};

// Re-export tos-crypto VRF types for convenience
pub use tos_crypto::vrf::{
    VrfError, VrfKeypair, VrfOutput, VrfProof, VrfPublicKey, VrfSecretKey, VRF_OUTPUT_SIZE,
    VRF_PROOF_SIZE, VRF_PUBLIC_KEY_SIZE, VRF_SECRET_KEY_SIZE,
};
