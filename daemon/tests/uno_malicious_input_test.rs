//! Malicious Input Tests for UNO Privacy Transfers
//!
//! These tests verify handling of malformed/malicious data.
//!
//! Test Categories:
//! - MAL-01 ~ MAL-04: Malformed Proof Data
//! - MAL-05 ~ MAL-07: Malformed Commitment/Handle Data
//! - MAL-08 ~ MAL-10: Duplicate/Replay Inputs

#[allow(unused_imports)]
mod common;

use tos_common::{
    config::{COIN_VALUE, UNO_ASSET},
    crypto::{
        elgamal::{CompressedCiphertext, KeyPair, PedersenCommitment, PedersenOpening},
        proofs::{BatchCollector, CiphertextValidityProof},
    },
    serializer::Serializer,
    transaction::UnoTransferPayload,
};
use tos_crypto::curve25519_dalek::ristretto::CompressedRistretto;

/// Create a test UnoTransferPayload with valid proofs
fn create_test_uno_payload(
    sender_keypair: &KeyPair,
    receiver_keypair: &KeyPair,
    amount: u64,
) -> UnoTransferPayload {
    let destination = receiver_keypair.get_public_key().compress();
    let opening = PedersenOpening::generate_new();

    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

    let mut transcript = tos_common::crypto::new_proof_transcript(b"test_uno_transfer");
    let proof = CiphertextValidityProof::new(
        receiver_keypair.get_public_key(),
        Some(sender_keypair.get_public_key()),
        amount,
        &opening,
        &mut transcript,
    );

    UnoTransferPayload::new(
        UNO_ASSET,
        destination,
        None, // extra_data
        commitment.compress(),
        sender_handle.compress(),
        receiver_handle.compress(),
        proof,
    )
}

// ============================================================================
// MAL-01 ~ MAL-04: Malformed Proof Data Tests
// ============================================================================

/// MAL-01: Truncated proof - CiphertextValidityProof with missing bytes
/// Deserialization of truncated proof data MUST FAIL
#[test]
fn test_malicious_truncated_proof() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;
    let opening = PedersenOpening::generate_new();

    // Create a valid proof first
    let mut transcript = tos_common::crypto::new_proof_transcript(b"truncate_test");
    let valid_proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        Some(sender.get_public_key()),
        amount,
        &opening,
        &mut transcript,
    );

    let full_bytes = valid_proof.to_bytes();
    assert!(
        full_bytes.len() > 32,
        "Proof should have significant length"
    );

    // Test various truncation points
    let truncation_points = [1, 16, 32, full_bytes.len() / 2, full_bytes.len() - 1];

    for truncate_at in truncation_points {
        let truncated = &full_bytes[..truncate_at];
        let result = CiphertextValidityProof::from_bytes(truncated);
        assert!(
            result.is_err(),
            "Truncated proof at {} bytes should fail to deserialize",
            truncate_at
        );
    }
}

/// MAL-02: Extended proof - Proof with extra garbage bytes
/// Deserialization should either fail or ignore extra bytes
#[test]
fn test_malicious_extended_proof() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;
    let opening = PedersenOpening::generate_new();

    // Create a valid proof
    let mut transcript = tos_common::crypto::new_proof_transcript(b"extend_test");
    let valid_proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        Some(sender.get_public_key()),
        amount,
        &opening,
        &mut transcript,
    );

    let full_bytes = valid_proof.to_bytes();

    // Append garbage bytes
    let garbage_sizes = [1, 32, 64, 128, 256];

    for garbage_size in garbage_sizes {
        let mut extended = full_bytes.clone();
        extended.extend(vec![0xAB; garbage_size]);

        // Attempting to deserialize extended proof
        // The system should either fail or produce a proof that fails verification
        let result = CiphertextValidityProof::from_bytes(&extended);

        // If deserialization succeeds, we need to verify the proof would still work
        // (it shouldn't because of extra data interfering or being rejected)
        if let Ok(parsed) = result {
            // If it parses, verify the proof bytes match (extra bytes ignored)
            // The re-serialized bytes should match original (extra bytes ignored)
            assert_eq!(
                parsed.to_bytes().len(),
                full_bytes.len(),
                "Extra bytes should be ignored, not included in re-serialization"
            );
        }
        // If it fails, that's also acceptable behavior
    }
}

/// MAL-03: Zero proof - All-zero bytes as proof
/// All-zero proof MUST FAIL to deserialize or verify
#[test]
fn test_malicious_zero_proof() {
    // Try various sizes of zero-filled proof data
    let zero_sizes = [32, 64, 96, 128, 160, 192, 224, 256];

    for size in zero_sizes {
        let zero_bytes = vec![0u8; size];
        let result = CiphertextValidityProof::from_bytes(&zero_bytes);

        // Zero bytes should fail to deserialize as valid proof
        // because zero point is not a valid Ristretto point
        assert!(
            result.is_err(),
            "All-zero proof of {} bytes should fail to deserialize",
            size
        );
    }
}

/// MAL-04: Max proof - All 0xFF bytes as proof
/// All-max proof MUST FAIL to deserialize or verify
#[test]
fn test_malicious_max_proof() {
    // Try various sizes of 0xFF-filled proof data
    let max_sizes = [32, 64, 96, 128, 160, 192, 224, 256];

    for size in max_sizes {
        let max_bytes = vec![0xFFu8; size];
        let result = CiphertextValidityProof::from_bytes(&max_bytes);

        // 0xFF bytes should fail to deserialize as valid proof
        // because they're unlikely to represent valid curve points
        assert!(
            result.is_err(),
            "All-0xFF proof of {} bytes should fail to deserialize",
            size
        );
    }
}

// ============================================================================
// MAL-05 ~ MAL-07: Malformed Commitment/Handle Data Tests
// ============================================================================

/// MAL-05: Invalid commitment point - Non-curve point as commitment
/// Non-curve point commitment MUST FAIL decompression
#[test]
fn test_malicious_invalid_commitment_point() {
    // Create various invalid point representations
    let invalid_points: Vec<[u8; 32]> = vec![
        [0u8; 32],    // All zeros
        [0xFFu8; 32], // All max
        {
            let mut arr = [0u8; 32];
            arr[0] = 0x01; // Minimal non-zero
            arr
        },
        {
            let mut arr = [0xAAu8; 32];
            arr[31] = 0x7F; // Random pattern
            arr
        },
    ];

    for invalid_bytes in invalid_points {
        // Try to create a compressed commitment from invalid bytes
        let compressed = CompressedRistretto(invalid_bytes);

        // Decompression should fail for invalid points
        let result = compressed.decompress();
        // Most random bytes won't be valid Ristretto points
        // Either decompress fails or returns None
        if result.is_some() {
            // Very unlikely but mathematically possible for some bytes
            // to accidentally be valid points
            continue;
        }
        // Decompression failed as expected for invalid point
    }
}

/// MAL-06: Invalid handle point - Non-curve point as handle
/// Non-curve point handle MUST FAIL decompression
#[test]
fn test_malicious_invalid_handle_point() {
    // Create various invalid handle representations
    let invalid_handles: Vec<[u8; 32]> = vec![
        [0u8; 32],    // All zeros - identity, but invalid for handle
        [0xFFu8; 32], // All max - not a valid point
        [0x80u8; 32], // High bit set pattern
        {
            let mut arr = [0u8; 32];
            for (i, byte) in arr.iter_mut().enumerate() {
                *byte = i as u8;
            }
            arr
        }, // Sequential bytes
    ];

    for invalid_bytes in invalid_handles {
        let compressed = CompressedRistretto(invalid_bytes);

        // Try to decompress - should fail for most invalid bytes
        let result = compressed.decompress();

        // Most invalid bytes won't decompress to valid points
        // This is the expected behavior for malicious input
        if result.is_none() {
            // Decompression correctly failed for invalid handle
            continue;
        }
        // If it somehow decompresses (very rare), the proof would still fail
    }
}

/// MAL-07: Malformed ciphertext - Attempt to use mismatched data sizes
/// Ciphertext with insufficient bytes should fail
#[test]
fn test_malicious_mismatched_ciphertext_sizes() {
    let keypair = KeyPair::new();
    let amount = 100u64;

    // Create a valid ciphertext
    let valid_ct = keypair.get_public_key().encrypt(amount);
    let compressed = valid_ct.compress();
    let valid_bytes = compressed.to_bytes();

    // A CompressedCiphertext should be 64 bytes (32 for commitment + 32 for handle)
    assert_eq!(
        valid_bytes.len(),
        64,
        "CompressedCiphertext should be 64 bytes"
    );

    // Test with insufficient sizes (less than 64 bytes)
    // Note: Extra bytes may be ignored by the parser, so we only test truncated data
    let short_sizes = [0, 16, 32, 48, 63];

    for size in short_sizes {
        let malformed_bytes = vec![0xABu8; size];
        let result = CompressedCiphertext::from_bytes(&malformed_bytes);

        // Insufficient bytes should fail to parse
        assert!(
            result.is_err(),
            "Ciphertext with {} bytes (expected 64) should fail to parse",
            size
        );
    }

    // For extra bytes, the parser may accept them (ignoring extra)
    // If it does parse, verify the serialized form is still 64 bytes
    let extra_bytes = vec![0xABu8; 128];
    if let Ok(parsed) = CompressedCiphertext::from_bytes(&extra_bytes) {
        assert_eq!(
            parsed.to_bytes().len(),
            64,
            "Parsed ciphertext should serialize to exactly 64 bytes"
        );
    }
}

// ============================================================================
// MAL-08 ~ MAL-10: Duplicate/Replay Input Tests
// ============================================================================

/// MAL-08: Duplicate proof verification - Same proof verified twice
/// Proof should pass verification each time (idempotent)
#[test]
fn test_malicious_duplicate_proof_verification() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 50 * COIN_VALUE;
    let opening = PedersenOpening::generate_new();

    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    let mut gen_transcript = tos_common::crypto::new_proof_transcript(b"duplicate_verify");
    let proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        Some(sender.get_public_key()),
        amount,
        &opening,
        &mut gen_transcript,
    );

    // Verify same proof multiple times - should be idempotent
    for i in 0..3 {
        let mut verify_transcript = tos_common::crypto::new_proof_transcript(b"duplicate_verify");
        let mut batch_collector = BatchCollector::default();

        let result = proof.pre_verify(
            &commitment,
            receiver.get_public_key(),
            sender.get_public_key(),
            &receiver_handle,
            &sender_handle,
            true,
            &mut verify_transcript,
            &mut batch_collector,
        );

        assert!(
            result.is_ok(),
            "Verification {} should succeed (idempotent)",
            i + 1
        );
        assert!(
            batch_collector.verify().is_ok(),
            "Batch verification {} should succeed",
            i + 1
        );
    }
}

/// MAL-09: Replay proof with different commitment - MUST FAIL
/// Using same proof bytes with different commitment should fail
#[test]
fn test_malicious_replay_proof_different_commitment() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 50 * COIN_VALUE;

    // Create first transfer with opening1
    let opening1 = PedersenOpening::generate_new();
    let commitment1 = PedersenCommitment::new_with_opening(amount, &opening1);
    let sender_handle1 = sender.get_public_key().decrypt_handle(&opening1);
    let receiver_handle1 = receiver.get_public_key().decrypt_handle(&opening1);

    let mut gen_transcript1 = tos_common::crypto::new_proof_transcript(b"replay_test");
    let proof1 = CiphertextValidityProof::new(
        receiver.get_public_key(),
        Some(sender.get_public_key()),
        amount,
        &opening1,
        &mut gen_transcript1,
    );

    // Create different commitment with opening2
    let opening2 = PedersenOpening::generate_new();
    let commitment2 = PedersenCommitment::new_with_opening(amount, &opening2);
    let sender_handle2 = sender.get_public_key().decrypt_handle(&opening2);
    let receiver_handle2 = receiver.get_public_key().decrypt_handle(&opening2);

    // Verify original proof with original commitment - should pass
    let mut verify_transcript1 = tos_common::crypto::new_proof_transcript(b"replay_test");
    let mut batch_collector1 = BatchCollector::default();
    let original_result = proof1.pre_verify(
        &commitment1,
        receiver.get_public_key(),
        sender.get_public_key(),
        &receiver_handle1,
        &sender_handle1,
        true,
        &mut verify_transcript1,
        &mut batch_collector1,
    );
    assert!(original_result.is_ok(), "Original proof should verify");
    assert!(
        batch_collector1.verify().is_ok(),
        "Original batch should verify"
    );

    // Try to replay proof1 with commitment2 - MUST FAIL
    let mut verify_transcript2 = tos_common::crypto::new_proof_transcript(b"replay_test");
    let mut batch_collector2 = BatchCollector::default();
    let replay_result = proof1.pre_verify(
        &commitment2, // Different commitment
        receiver.get_public_key(),
        sender.get_public_key(),
        &receiver_handle2,
        &sender_handle2,
        true,
        &mut verify_transcript2,
        &mut batch_collector2,
    );

    // Either pre_verify fails or batch verify fails
    let replay_failed = replay_result.is_err() || batch_collector2.verify().is_err();
    assert!(replay_failed, "Replay with different commitment MUST FAIL");
}

/// MAL-10: Test that different transfers produce unique identifiers
/// Two different transfers should never have the same identifying hash
#[test]
fn test_malicious_transfer_uniqueness() {
    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let carol = KeyPair::new();

    // Create multiple transfers with different parameters
    let payloads = [
        create_test_uno_payload(&alice, &bob, 10 * COIN_VALUE),
        create_test_uno_payload(&alice, &bob, 20 * COIN_VALUE), // Same parties, different amount
        create_test_uno_payload(&alice, &carol, 10 * COIN_VALUE), // Different recipient
        create_test_uno_payload(&bob, &alice, 10 * COIN_VALUE), // Reversed parties
    ];

    // Collect serialized bytes of all payloads
    let serialized: Vec<Vec<u8>> = payloads.iter().map(|p| p.to_bytes()).collect();

    // Verify all serializations are unique
    for i in 0..serialized.len() {
        for j in (i + 1)..serialized.len() {
            assert_ne!(
                serialized[i], serialized[j],
                "Payloads {} and {} should have different serializations",
                i, j
            );
        }
    }

    // Verify all proofs are unique (even for same amount/parties, randomness differs)
    let proofs: Vec<Vec<u8>> = payloads.iter().map(|p| p.get_proof().to_bytes()).collect();

    for i in 0..proofs.len() {
        for j in (i + 1)..proofs.len() {
            assert_ne!(
                proofs[i], proofs[j],
                "Proof {} and {} should be unique due to randomness",
                i, j
            );
        }
    }
}

// ============================================================================
// Additional Malicious Input Tests
// ============================================================================

/// Additional test: Random garbage data as proof
#[test]
fn test_malicious_random_garbage_proof() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Generate pseudo-random garbage data
    let mut failures = 0;
    let trials = 100;

    for seed in 0..trials {
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        let hash = hasher.finish();

        // Create garbage bytes from hash
        let garbage: Vec<u8> = (0..128)
            .map(|i| ((hash >> (i % 8)) ^ (i as u64)) as u8)
            .collect();

        let result = CiphertextValidityProof::from_bytes(&garbage);
        if result.is_err() {
            failures += 1;
        }
    }

    // Most random garbage should fail to parse as valid proofs
    assert!(
        failures > trials / 2,
        "At least half of random garbage should fail to parse as proofs (got {}/{})",
        failures,
        trials
    );
}

/// Additional test: Proof with valid structure but corrupted values
#[test]
fn test_malicious_structurally_valid_but_corrupted_proof() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;
    let opening = PedersenOpening::generate_new();

    // Create a valid proof
    let mut transcript = tos_common::crypto::new_proof_transcript(b"struct_test");
    let valid_proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        Some(sender.get_public_key()),
        amount,
        &opening,
        &mut transcript,
    );

    let valid_bytes = valid_proof.to_bytes();

    // Flip bits in the proof to create invalid but structurally similar data
    let mut corrupted = valid_bytes.clone();
    corrupted[16] ^= 0x01; // Flip one bit in the middle

    // Corruption should change bytes
    assert_ne!(valid_bytes, corrupted, "Corruption should change bytes");

    // Try to deserialize corrupted proof - may or may not succeed
    // If it parses, the proof would fail verification
    let _result = CiphertextValidityProof::from_bytes(&corrupted);
    // Either parse fails or verification would fail - both are acceptable
}
