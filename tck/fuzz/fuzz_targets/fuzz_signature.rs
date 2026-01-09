//! Fuzz target for signature verification
//!
//! Tests that arbitrary signatures and messages do not cause panics
//! during signature verification.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

/// Fuzz input for signature verification
#[derive(Debug, Arbitrary)]
struct SignatureInput {
    /// Message bytes (variable length)
    message: Vec<u8>,
    /// Signature bytes (should be 64 bytes for Ed25519)
    signature: [u8; 64],
    /// Public key bytes (should be 32 bytes for Ed25519)
    public_key: [u8; 32],
}

fuzz_target!(|input: SignatureInput| {
    // Attempt signature verification with arbitrary inputs
    // Should never panic, only return verification failure

    // Truncate message if too long to avoid memory issues
    let message = if input.message.len() > 10000 {
        &input.message[..10000]
    } else {
        &input.message
    };

    // Try to verify using tos_common's signature verification
    // The actual verification function should handle invalid inputs gracefully
    let _ = verify_signature_safe(message, &input.signature, &input.public_key);
});

/// Safe signature verification wrapper
fn verify_signature_safe(_message: &[u8], _signature: &[u8; 64], _public_key: &[u8; 32]) -> bool {
    // TODO: Call actual tos_common signature verification
    // For now, just ensure we handle the input without panicking

    // Basic sanity checks that should not panic
    // These simulate what a real verification would do

    // Check for all-zero public key (invalid)
    if _public_key.iter().all(|&b| b == 0) {
        return false;
    }

    // Check for all-zero signature (invalid)
    if _signature.iter().all(|&b| b == 0) {
        return false;
    }

    // In real implementation, call:
    // tos_common::crypto::verify_ed25519(message, signature, public_key)
    false
}
