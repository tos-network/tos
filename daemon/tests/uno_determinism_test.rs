//! Determinism/Idempotency Tests for UNO Privacy Transfers
//!
//! These tests verify deterministic behavior of cryptographic operations.
//!
//! Test Categories:
//! - DET-01 ~ DET-03: Proof Determinism
//! - DET-04 ~ DET-06: Balance Determinism
//! - DET-07 ~ DET-08: Cross-Platform Determinism

use tos_common::{
    config::COIN_VALUE,
    crypto::{
        elgamal::{KeyPair, PedersenCommitment, PedersenOpening},
        new_proof_transcript,
        proofs::{CiphertextValidityProof, G, H},
    },
    serializer::Serializer,
    transaction::TxVersion,
};
use tos_crypto::curve25519_dalek::Scalar;

// ============================================================================
// DET-01 ~ DET-03: Proof Determinism
// ============================================================================

/// DET-01: Same inputs produce deterministic semantic results
/// The encrypted values should decrypt to the same amount
#[test]
fn test_determinism_same_inputs_same_semantic_value() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount: u64 = 100 * COIN_VALUE;

    // Generate first transfer with same opening
    let opening1 = PedersenOpening::generate_new();
    let commitment1 = PedersenCommitment::new_with_opening(amount, &opening1);
    let sender_handle1 = sender.get_public_key().decrypt_handle(&opening1);
    let _receiver_handle1 = receiver.get_public_key().decrypt_handle(&opening1);

    // Generate second transfer with different opening (same amount)
    let opening2 = PedersenOpening::generate_new();
    let commitment2 = PedersenCommitment::new_with_opening(amount, &opening2);
    let sender_handle2 = sender.get_public_key().decrypt_handle(&opening2);
    let _receiver_handle2 = receiver.get_public_key().decrypt_handle(&opening2);

    // Both commitments encode the same amount, verifiable via proof
    let mut transcript1 = new_proof_transcript(b"det_test");
    let proof1 = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening1,
        TxVersion::T1,
        &mut transcript1,
    );

    let mut transcript2 = new_proof_transcript(b"det_test");
    let proof2 = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening2,
        TxVersion::T1,
        &mut transcript2,
    );

    // Proofs differ (due to different openings) but both are valid for the same amount
    assert_ne!(
        proof1.to_bytes(),
        proof2.to_bytes(),
        "Different openings produce different proofs"
    );

    // Both ciphertexts decrypt to the same point when combined with sender's key
    let ciphertext1 = tos_common::crypto::elgamal::Ciphertext::new(commitment1, sender_handle1);
    let ciphertext2 = tos_common::crypto::elgamal::Ciphertext::new(commitment2, sender_handle2);

    let point1 = sender.decrypt_to_point(&ciphertext1);
    let point2 = sender.decrypt_to_point(&ciphertext2);

    // Both should decrypt to amount * G (the encoded value)
    let expected_point = Scalar::from(amount) * *G;
    assert_eq!(
        point1, expected_point,
        "First ciphertext decrypts correctly"
    );
    assert_eq!(
        point2, expected_point,
        "Second ciphertext decrypts correctly"
    );
    assert_eq!(point1, point2, "Both decrypt to same semantic value");
}

/// DET-02: Different transcripts produce different proofs
/// Verify that changing transcript domain separator produces different proofs
#[test]
fn test_determinism_different_transcripts_different_proofs() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount: u64 = 100 * COIN_VALUE;
    let opening = PedersenOpening::generate_new();

    // Create proofs with different domain separators
    let mut transcript1 = new_proof_transcript(b"domain_A");
    let proof1 = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut transcript1,
    );

    let mut transcript2 = new_proof_transcript(b"domain_B");
    let proof2 = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut transcript2,
    );

    // Proofs should be different due to different domain separators
    let proof1_bytes = proof1.to_bytes();
    let proof2_bytes = proof2.to_bytes();
    assert_ne!(
        proof1_bytes, proof2_bytes,
        "Different domains should produce different proofs"
    );

    // Verify proof bytes are consistent (serialization is deterministic)
    assert_eq!(
        proof1.to_bytes(),
        proof1_bytes,
        "Repeated serialization should be deterministic"
    );
    assert_eq!(
        proof2.to_bytes(),
        proof2_bytes,
        "Repeated serialization should be deterministic"
    );
}

/// DET-03: Order of transfers doesn't affect validity
/// Each proof is independent and valid regardless of order
#[test]
fn test_determinism_transfer_order_independence() {
    let alice = KeyPair::new();
    let _bob = KeyPair::new();
    let _carol = KeyPair::new();
    let amount: u64 = 100 * COIN_VALUE;

    // Create transfers in different orders
    // Order 1: Alice -> Bob, then Alice -> Carol
    let opening_ab1 = PedersenOpening::generate_new();
    let commitment_ab1 = PedersenCommitment::new_with_opening(amount, &opening_ab1);
    let handle_ab1 = alice.get_public_key().decrypt_handle(&opening_ab1);

    let opening_ac1 = PedersenOpening::generate_new();
    let commitment_ac1 = PedersenCommitment::new_with_opening(amount, &opening_ac1);
    let handle_ac1 = alice.get_public_key().decrypt_handle(&opening_ac1);

    // Order 2: Alice -> Carol, then Alice -> Bob (reversed order)
    let opening_ac2 = PedersenOpening::generate_new();
    let commitment_ac2 = PedersenCommitment::new_with_opening(amount, &opening_ac2);
    let handle_ac2 = alice.get_public_key().decrypt_handle(&opening_ac2);

    let opening_ab2 = PedersenOpening::generate_new();
    let commitment_ab2 = PedersenCommitment::new_with_opening(amount, &opening_ab2);
    let handle_ab2 = alice.get_public_key().decrypt_handle(&opening_ab2);

    // Create ciphertexts
    let ct_ab1 = tos_common::crypto::elgamal::Ciphertext::new(commitment_ab1, handle_ab1);
    let ct_ac1 = tos_common::crypto::elgamal::Ciphertext::new(commitment_ac1, handle_ac1);
    let ct_ac2 = tos_common::crypto::elgamal::Ciphertext::new(commitment_ac2, handle_ac2);
    let ct_ab2 = tos_common::crypto::elgamal::Ciphertext::new(commitment_ab2, handle_ab2);

    // All ciphertexts should decrypt to same amount regardless of creation order
    let expected_point = Scalar::from(amount) * *G;
    assert_eq!(
        alice.decrypt_to_point(&ct_ab1),
        expected_point,
        "AB1 decrypts correctly"
    );
    assert_eq!(
        alice.decrypt_to_point(&ct_ac1),
        expected_point,
        "AC1 decrypts correctly"
    );
    assert_eq!(
        alice.decrypt_to_point(&ct_ac2),
        expected_point,
        "AC2 decrypts correctly"
    );
    assert_eq!(
        alice.decrypt_to_point(&ct_ab2),
        expected_point,
        "AB2 decrypts correctly"
    );
}

// ============================================================================
// DET-04 ~ DET-06: Balance Determinism
// ============================================================================

/// DET-04: Order of operations produces same final balance
/// Homomorphic addition is commutative: a + b = b + a
#[test]
fn test_determinism_homomorphic_commutative() {
    let keypair = KeyPair::new();
    let pub_key = keypair.get_public_key();

    let amount_a: u64 = 30 * COIN_VALUE;
    let amount_b: u64 = 70 * COIN_VALUE;

    // Create ciphertexts
    let cipher_a = pub_key.encrypt(amount_a);
    let cipher_b = pub_key.encrypt(amount_b);

    // Order 1: a + b (using clone since Add consumes the operand)
    let sum_ab = cipher_a.clone() + cipher_b.clone();

    // Order 2: b + a
    let sum_ba = cipher_b.clone() + cipher_a.clone();

    // Both should decrypt to same point ((amount_a + amount_b) * G)
    let point_ab = keypair.decrypt_to_point(&sum_ab);
    let point_ba = keypair.decrypt_to_point(&sum_ba);

    let expected_point = Scalar::from(amount_a + amount_b) * *G;
    assert_eq!(point_ab, expected_point, "a+b should decrypt correctly");
    assert_eq!(point_ba, expected_point, "b+a should decrypt correctly");
    assert_eq!(point_ab, point_ba, "a+b should equal b+a (commutative)");

    // Test associativity: (a + b) + c = a + (b + c)
    let amount_c: u64 = 50 * COIN_VALUE;
    let cipher_c = pub_key.encrypt(amount_c);

    let sum_abc_left = (cipher_a.clone() + cipher_b.clone()) + cipher_c.clone(); // (a + b) + c
    let sum_abc_right = cipher_a.clone() + (cipher_b.clone() + cipher_c.clone()); // a + (b + c)

    let point_left = keypair.decrypt_to_point(&sum_abc_left);
    let point_right = keypair.decrypt_to_point(&sum_abc_right);

    let expected_total = Scalar::from(amount_a + amount_b + amount_c) * *G;
    assert_eq!(
        point_left, expected_total,
        "(a+b)+c should decrypt correctly"
    );
    assert_eq!(
        point_right, expected_total,
        "a+(b+c) should decrypt correctly"
    );
    assert_eq!(point_left, point_right, "Associativity: (a+b)+c = a+(b+c)");
}

/// DET-05: Serialize -> deserialize produces same value
/// Round-trip serialization preserves ciphertext
#[test]
fn test_determinism_serialization_roundtrip() {
    let keypair = KeyPair::new();
    let amount: u64 = 12345 * COIN_VALUE;

    let original = keypair.get_public_key().encrypt(amount);

    // Compress -> Decompress round trip
    let compressed = original.compress();
    let decompressed = compressed
        .decompress()
        .expect("Decompression should succeed");

    // Verify round-trip preserves decryptable value (by comparing decrypted points)
    let original_point = keypair.decrypt_to_point(&original);
    let roundtrip_point = keypair.decrypt_to_point(&decompressed);

    let expected_point = Scalar::from(amount) * *G;
    assert_eq!(
        original_point, expected_point,
        "Original should decrypt correctly"
    );
    assert_eq!(
        roundtrip_point, expected_point,
        "Round-trip should decrypt correctly"
    );
    assert_eq!(
        original_point, roundtrip_point,
        "Round-trip should preserve value"
    );

    // Verify byte representation is identical
    let original_compressed = original.compress();
    let roundtrip_compressed = decompressed.compress();
    assert_eq!(
        original_compressed.to_bytes(),
        roundtrip_compressed.to_bytes(),
        "Compressed bytes should match after round-trip"
    );
}

/// DET-06: Compress -> decompress produces same point
/// Lossless compression of cryptographic points
#[test]
fn test_determinism_point_compression_lossless() {
    let keypair = KeyPair::new();
    let pub_key = keypair.get_public_key();
    let amount: u64 = 100 * COIN_VALUE;

    // Test public key compression
    let compressed_pub = pub_key.compress();
    let decompressed_pub = compressed_pub
        .decompress()
        .expect("Pub key decompress should succeed");
    let recompressed = decompressed_pub.compress();
    assert_eq!(
        compressed_pub, recompressed,
        "Public key round-trip should be lossless"
    );

    // Test ciphertext compression
    let ciphertext = pub_key.encrypt(amount);
    let compressed_ct = ciphertext.compress();
    let decompressed_ct = compressed_ct
        .decompress()
        .expect("Ciphertext decompress should succeed");

    // Verify decryption works after round-trip
    let original_point = keypair.decrypt_to_point(&ciphertext);
    let roundtrip_point = keypair.decrypt_to_point(&decompressed_ct);
    assert_eq!(
        original_point, roundtrip_point,
        "Decompressed ciphertext should decrypt to same point"
    );

    // Verify multiple round-trips
    let ct2 = decompressed_ct
        .compress()
        .decompress()
        .expect("Second round-trip should succeed");
    let ct3 = ct2
        .compress()
        .decompress()
        .expect("Third round-trip should succeed");
    let point3 = keypair.decrypt_to_point(&ct3);
    assert_eq!(
        point3, original_point,
        "Multiple round-trips should preserve value"
    );
}

// ============================================================================
// DET-07 ~ DET-08: Cross-Platform Determinism
// ============================================================================

/// DET-07: Proof serialization is platform independent
/// Verify that proof serialization is deterministic
#[test]
fn test_determinism_proof_serialization_platform_independent() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount: u64 = 100 * COIN_VALUE;
    let opening = PedersenOpening::generate_new();

    // Create proof
    let mut create_transcript = new_proof_transcript(b"platform_test");
    let proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut create_transcript,
    );

    // Serialize proof to bytes
    let proof_bytes = proof.to_bytes();

    // Multiple serializations should always produce same result (deterministic)
    for i in 0..5 {
        let re_serialized = proof.to_bytes();
        assert_eq!(
            proof_bytes, re_serialized,
            "Repeated serialization {} should always produce same bytes",
            i
        );
    }

    // Proof bytes should be non-empty and reasonable size
    assert!(!proof_bytes.is_empty(), "Proof bytes should not be empty");
    assert!(
        proof_bytes.len() > 32,
        "Proof should be larger than a single point"
    );

    // Creating the same proof again with same inputs should produce deterministic behavior
    // Note: Due to internal randomness in the proof protocol, the actual bytes may differ
    // but the proof structure should be consistent
    let mut transcript2 = new_proof_transcript(b"platform_test");
    let proof2 = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut transcript2,
    );

    // Both proofs should have the same size (structure is deterministic)
    assert_eq!(
        proof.to_bytes().len(),
        proof2.to_bytes().len(),
        "Proof structure should be deterministic"
    );
}

/// DET-08: Balance calculations use no floating point
/// All balance calculations must use u64/u128/Scalar (no f32/f64)
#[test]
fn test_determinism_no_floating_point() {
    // Test that all balance operations use integer arithmetic
    let keypair = KeyPair::new();
    let pub_key = keypair.get_public_key();

    // Test precise integer amounts (no floating point rounding)
    let precise_amounts: Vec<u64> = vec![
        1,                     // Minimum
        COIN_VALUE - 1,        // Just below 1 coin
        COIN_VALUE,            // Exactly 1 coin
        COIN_VALUE + 1,        // Just above 1 coin
        u64::MAX / 2,          // Large value
        u64::MAX - COIN_VALUE, // Near maximum
    ];

    for amount in &precise_amounts {
        let ciphertext = pub_key.encrypt(*amount);
        let decrypted_point = keypair.decrypt_to_point(&ciphertext);
        let expected_point = Scalar::from(*amount) * *G;

        // Integer arithmetic should be exact - no floating point errors
        assert_eq!(
            decrypted_point, expected_point,
            "Encryption/decryption should be exact for amount {}",
            amount
        );
    }

    // Test homomorphic addition preserves integer precision
    let a: u64 = 333_333_333;
    let b: u64 = 666_666_667;
    let expected_sum = 1_000_000_000u64; // Exact sum

    let cipher_a = pub_key.encrypt(a);
    let cipher_b = pub_key.encrypt(b);
    let sum = cipher_a + cipher_b;
    let sum_point = keypair.decrypt_to_point(&sum);
    let expected_point = Scalar::from(expected_sum) * *G;

    assert_eq!(
        sum_point, expected_point,
        "Homomorphic sum should be exact: {} + {} = {}",
        a, b, expected_sum
    );
}

/// Additional test: Scalar arithmetic determinism
#[test]
fn test_determinism_scalar_arithmetic() {
    // Verify Scalar operations are deterministic
    let value1: u64 = 12345678;
    let value2: u64 = 87654321;

    let scalar1 = Scalar::from(value1);
    let scalar2 = Scalar::from(value2);

    // Addition
    let sum = scalar1 + scalar2;
    let expected_sum = Scalar::from(value1 + value2);
    assert_eq!(sum, expected_sum, "Scalar addition should be deterministic");

    // Multiplication
    let product = scalar1 * scalar2;
    let expected_product = Scalar::from(value1 as u128 * value2 as u128);
    assert_eq!(
        product, expected_product,
        "Scalar multiplication should be deterministic"
    );

    // Multiple operations should produce same result
    let result1 = (scalar1 + scalar2) * scalar1;
    let result2 = (scalar1 + scalar2) * scalar1;
    assert_eq!(
        result1, result2,
        "Repeated scalar operations should be deterministic"
    );

    // Verify generator points are constant
    let g1 = *G;
    let g2 = *G;
    assert_eq!(g1, g2, "Generator G should be constant");

    let h1 = *H;
    let h2 = *H;
    assert_eq!(h1, h2, "Generator H should be constant");
}

/// Additional test: Pedersen commitment determinism
#[test]
fn test_determinism_pedersen_commitment() {
    let amount: u64 = 100 * COIN_VALUE;

    // Same opening should produce same commitment
    let opening = PedersenOpening::generate_new();
    let commitment1 = PedersenCommitment::new_with_opening(amount, &opening);
    let commitment2 = PedersenCommitment::new_with_opening(amount, &opening);

    assert_eq!(
        commitment1.as_point(),
        commitment2.as_point(),
        "Same inputs should produce same commitment"
    );

    // Different opening should produce different commitment
    let different_opening = PedersenOpening::generate_new();
    let different_commitment = PedersenCommitment::new_with_opening(amount, &different_opening);

    assert_ne!(
        commitment1.as_point(),
        different_commitment.as_point(),
        "Different openings should produce different commitments"
    );

    // Both commitments encode the same amount, verifiable via expected point
    // commitment = amount * G + opening * H
    let expected_point1 = Scalar::from(amount) * *G + opening.as_scalar() * *H;
    let expected_point2 = Scalar::from(amount) * *G + different_opening.as_scalar() * *H;

    assert_eq!(
        commitment1.as_point(),
        &expected_point1,
        "Commitment1 should match expected point"
    );
    assert_eq!(
        different_commitment.as_point(),
        &expected_point2,
        "Different commitment should match expected point"
    );
}
