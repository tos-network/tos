// Generate UNO Proof test vectors (ShieldCommitmentProof, CiphertextValidityProof)
// Run: cd ~/tos/tck/uno && cargo run --release --bin gen_uno_proof_vectors
//
// These vectors are used to verify Avatar C implementation matches TOS Rust.
//
// Note: TOS Rust currently only supports TxVersion::T1 (160-byte CT proofs).
// T0 (128-byte) is legacy and not generated here.

use serde::Serialize;
use std::fs::File;
use std::io::Write;
use tos_common::{
    crypto::{
        elgamal::{PedersenCommitment, PedersenOpening, PublicKey},
        proofs::{CiphertextValidityProof, ShieldCommitmentProof},
        KeyPair, Transcript,
    },
    serializer::Serializer,
    transaction::TxVersion,
};

#[derive(Serialize)]
struct ShieldProofVector {
    name: String,
    description: String,
    // Transcript label used for proof generation/verification
    transcript_label: String,
    // Input data
    amount: u64,
    receiver_pubkey_hex: String,
    // Computed values (for verification)
    commitment_hex: String,
    receiver_handle_hex: String,
    // Proof bytes (96 bytes: Y_H + Y_P + z)
    proof_hex: String,
    // Expected result
    should_verify: bool,
    skip: bool,
}

#[derive(Serialize)]
struct CtValidityProofVector {
    name: String,
    description: String,
    // Transcript label used for proof generation/verification
    transcript_label: String,
    // Input data
    sender_pubkey_hex: String,
    receiver_pubkey_hex: String,
    // Computed values
    commitment_hex: String,
    sender_handle_hex: String,
    receiver_handle_hex: String,
    // Version flag (T1 = true, T0 = false)
    tx_version_t1: bool,
    // Proof bytes (T1: 160 bytes = Y_0 + Y_1 + Y_2 + z_r + z_x)
    proof_hex: String,
    // Expected result
    should_verify: bool,
    skip: bool,
}

#[derive(Serialize)]
struct UnoTestFile {
    algorithm: String,
    version: u32,
    description: String,
    // Shield proof vectors
    shield_proof_vectors: Vec<ShieldProofVector>,
    // CiphertextValidity proof vectors
    ct_validity_proof_vectors: Vec<CtValidityProofVector>,
}

fn pubkey_to_hex(pk: &PublicKey) -> String {
    hex::encode(pk.compress().as_bytes())
}

fn main() {
    let mut shield_vectors = Vec::new();
    let mut ct_vectors = Vec::new();

    // =========================================================================
    // Shield Commitment Proof Vectors
    // =========================================================================

    // Test 1: Basic Shield proof (amount = 1000)
    {
        let receiver_keypair = KeyPair::new();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 1000u64;
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let receiver_handle = receiver_pubkey.decrypt_handle(&opening);

        // Generate proof
        let mut transcript = Transcript::new(b"shield-test");
        let proof =
            ShieldCommitmentProof::new(receiver_pubkey, amount, &opening, &mut transcript);

        // Serialize proof
        let proof_bytes = proof.to_bytes();

        shield_vectors.push(ShieldProofVector {
            name: "shield_proof_amount_1000".to_string(),
            description: "Valid Shield proof for amount=1000".to_string(),
            transcript_label: "shield-test".to_string(),
            amount,
            receiver_pubkey_hex: pubkey_to_hex(receiver_pubkey),
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            proof_hex: hex::encode(&proof_bytes),
            should_verify: true,
            skip: false,
        });
    }

    // Test 2: Shield proof (amount = 0)
    {
        let receiver_keypair = KeyPair::new();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 0u64;
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let receiver_handle = receiver_pubkey.decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"shield-test");
        let proof =
            ShieldCommitmentProof::new(receiver_pubkey, amount, &opening, &mut transcript);

        let proof_bytes = proof.to_bytes();

        shield_vectors.push(ShieldProofVector {
            name: "shield_proof_amount_0".to_string(),
            description: "Valid Shield proof for amount=0".to_string(),
            transcript_label: "shield-test".to_string(),
            amount,
            receiver_pubkey_hex: pubkey_to_hex(receiver_pubkey),
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            proof_hex: hex::encode(&proof_bytes),
            should_verify: true,
            skip: false,
        });
    }

    // Test 3: Shield proof (large amount)
    {
        let receiver_keypair = KeyPair::new();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 1_000_000_000_000u64; // 1 trillion
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let receiver_handle = receiver_pubkey.decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"shield-test");
        let proof =
            ShieldCommitmentProof::new(receiver_pubkey, amount, &opening, &mut transcript);

        let proof_bytes = proof.to_bytes();

        shield_vectors.push(ShieldProofVector {
            name: "shield_proof_large_amount".to_string(),
            description: "Valid Shield proof for amount=1_000_000_000_000".to_string(),
            transcript_label: "shield-test".to_string(),
            amount,
            receiver_pubkey_hex: pubkey_to_hex(receiver_pubkey),
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            proof_hex: hex::encode(&proof_bytes),
            should_verify: true,
            skip: false,
        });
    }

    // Test 4: Invalid Shield proof (wrong amount for verification)
    {
        let receiver_keypair = KeyPair::new();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 1000u64;
        let wrong_amount = 2000u64; // Will be used for verification
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let receiver_handle = receiver_pubkey.decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"shield-test");
        let proof =
            ShieldCommitmentProof::new(receiver_pubkey, amount, &opening, &mut transcript);

        let proof_bytes = proof.to_bytes();

        shield_vectors.push(ShieldProofVector {
            name: "shield_proof_wrong_amount".to_string(),
            description: "Invalid Shield proof - verifier uses wrong amount (2000 instead of 1000)"
                .to_string(),
            transcript_label: "shield-test".to_string(),
            amount: wrong_amount, // Verification will use this wrong amount
            receiver_pubkey_hex: pubkey_to_hex(receiver_pubkey),
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            proof_hex: hex::encode(&proof_bytes),
            should_verify: false,
            skip: false,
        });
    }

    // =========================================================================
    // CiphertextValidity Proof Vectors (T1 format - 160 bytes)
    // Note: T0 (128 bytes) is legacy and not supported by current TOS Rust
    // =========================================================================

    // Test 1: Basic T1 proof (amount = 500)
    {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let sender_pubkey = sender_keypair.get_public_key();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 500u64;
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender_pubkey.decrypt_handle(&opening);
        let receiver_handle = receiver_pubkey.decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"ct-validity-test");
        let proof = CiphertextValidityProof::new(
            receiver_pubkey,
            sender_pubkey,
            amount,
            &opening,
            TxVersion::T1,
            &mut transcript,
        );

        let proof_bytes = proof.to_bytes();

        ct_vectors.push(CtValidityProofVector {
            name: "ct_validity_t1_amount_500".to_string(),
            description: "Valid T1 CiphertextValidity proof for amount=500".to_string(),
            transcript_label: "ct-validity-test".to_string(),
            sender_pubkey_hex: pubkey_to_hex(sender_pubkey),
            receiver_pubkey_hex: pubkey_to_hex(receiver_pubkey),
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            sender_handle_hex: hex::encode(sender_handle.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            tx_version_t1: true,
            proof_hex: hex::encode(&proof_bytes),
            should_verify: true,
            skip: false,
        });
    }

    // Test 2: T1 proof with zero amount
    {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let sender_pubkey = sender_keypair.get_public_key();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 0u64;
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender_pubkey.decrypt_handle(&opening);
        let receiver_handle = receiver_pubkey.decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"ct-validity-test");
        let proof = CiphertextValidityProof::new(
            receiver_pubkey,
            sender_pubkey,
            amount,
            &opening,
            TxVersion::T1,
            &mut transcript,
        );

        let proof_bytes = proof.to_bytes();

        ct_vectors.push(CtValidityProofVector {
            name: "ct_validity_t1_zero_amount".to_string(),
            description: "Valid T1 CiphertextValidity proof for amount=0".to_string(),
            transcript_label: "ct-validity-test".to_string(),
            sender_pubkey_hex: pubkey_to_hex(sender_pubkey),
            receiver_pubkey_hex: pubkey_to_hex(receiver_pubkey),
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            sender_handle_hex: hex::encode(sender_handle.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            tx_version_t1: true,
            proof_hex: hex::encode(&proof_bytes),
            should_verify: true,
            skip: false,
        });
    }

    // Test 3: T1 proof with amount = 1000
    {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let sender_pubkey = sender_keypair.get_public_key();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 1000u64;
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender_pubkey.decrypt_handle(&opening);
        let receiver_handle = receiver_pubkey.decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"ct-validity-test");
        let proof = CiphertextValidityProof::new(
            receiver_pubkey,
            sender_pubkey,
            amount,
            &opening,
            TxVersion::T1,
            &mut transcript,
        );

        let proof_bytes = proof.to_bytes();

        ct_vectors.push(CtValidityProofVector {
            name: "ct_validity_t1_amount_1000".to_string(),
            description: "Valid T1 CiphertextValidity proof for amount=1000".to_string(),
            transcript_label: "ct-validity-test".to_string(),
            sender_pubkey_hex: pubkey_to_hex(sender_pubkey),
            receiver_pubkey_hex: pubkey_to_hex(receiver_pubkey),
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            sender_handle_hex: hex::encode(sender_handle.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            tx_version_t1: true,
            proof_hex: hex::encode(&proof_bytes),
            should_verify: true,
            skip: false,
        });
    }

    // Test 4: T1 proof with large amount
    {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let sender_pubkey = sender_keypair.get_public_key();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 999_999_999_999u64;
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender_pubkey.decrypt_handle(&opening);
        let receiver_handle = receiver_pubkey.decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"ct-validity-test");
        let proof = CiphertextValidityProof::new(
            receiver_pubkey,
            sender_pubkey,
            amount,
            &opening,
            TxVersion::T1,
            &mut transcript,
        );

        let proof_bytes = proof.to_bytes();

        ct_vectors.push(CtValidityProofVector {
            name: "ct_validity_t1_large_amount".to_string(),
            description: "Valid T1 CiphertextValidity proof for amount=999_999_999_999".to_string(),
            transcript_label: "ct-validity-test".to_string(),
            sender_pubkey_hex: pubkey_to_hex(sender_pubkey),
            receiver_pubkey_hex: pubkey_to_hex(receiver_pubkey),
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            sender_handle_hex: hex::encode(sender_handle.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            tx_version_t1: true,
            proof_hex: hex::encode(&proof_bytes),
            should_verify: true,
            skip: false,
        });
    }

    // Build output
    let test_file = UnoTestFile {
        algorithm: "UNO-Privacy-Proofs".to_string(),
        version: 1,
        description: "UNO proof test vectors generated by TOS Rust for Avatar C verification. Note: Only T1 format (160-byte CT proofs) is supported by current TOS.".to_string(),
        shield_proof_vectors: shield_vectors,
        ct_validity_proof_vectors: ct_vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).expect("Failed to serialize YAML");
    println!("{}", yaml);

    // Write to file
    let output_path = "uno_proofs.yaml";
    let mut file = File::create(output_path).expect("Failed to create output file");
    file.write_all(yaml.as_bytes())
        .expect("Failed to write output");
    eprintln!("Written to {}", output_path);
}
