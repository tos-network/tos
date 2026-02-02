// gen_uno_vectors.rs - Generate TCK test vectors for UNO Privacy transactions
//
// Type 18: UnoTransfer - Private transfer with encrypted amount
// Type 19: Shield - TOS -> UNO (plaintext to encrypted)
// Type 20: Unshield - UNO -> TOS (encrypted to plaintext)
//
// Wire formats (Big-Endian):
//
// Shield (Type 19):
//   asset:           32 bytes (Hash)
//   destination:     32 bytes (CompressedPublicKey)
//   amount:          8 bytes BE (u64)
//   extra_data:      Option<bytes> (0x00 for None, 0x01 + u16 len + data for Some)
//   commitment:      32 bytes (CompressedCommitment)
//   receiver_handle: 32 bytes (CompressedHandle)
//   proof:           96 bytes (ShieldCommitmentProof: Y_H + Y_P + z)
//
// Unshield (Type 20):
//   asset:           32 bytes
//   destination:     32 bytes
//   amount:          8 bytes BE
//   extra_data:      Option<bytes>
//   commitment:      32 bytes
//   sender_handle:   32 bytes
//   ct_proof:        160 bytes (CiphertextValidityProof T1: Y_0 + Y_1 + Y_2 + z_r + z_x)
//
// UnoTransfer (Type 18):
//   asset:           32 bytes
//   destination:     32 bytes
//   extra_data:      Option<bytes>
//   commitment:      32 bytes
//   sender_handle:   32 bytes
//   receiver_handle: 32 bytes
//   ct_proof:        160 bytes (CiphertextValidityProof T1)

use serde::Serialize;
use std::fs::File;
use std::io::Write;
use tos_common::crypto::elgamal::{KeyPair, PedersenCommitment, PedersenOpening};
use tos_common::crypto::proofs::{CiphertextValidityProof, ShieldCommitmentProof};
use tos_common::crypto::Hash;
use tos_common::serializer::Serializer;
use tos_common::transaction::{
    ShieldTransferPayload, TxVersion, UnshieldTransferPayload, UnoTransferPayload,
};
use tos_common::crypto::Transcript;

#[derive(Serialize)]
struct ShieldWireVector {
    name: String,
    description: String,
    asset_hex: String,
    destination_hex: String,
    amount: u64,
    has_extra_data: bool,
    commitment_hex: String,
    receiver_handle_hex: String,
    proof_size: usize,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct UnshieldWireVector {
    name: String,
    description: String,
    asset_hex: String,
    destination_hex: String,
    amount: u64,
    has_extra_data: bool,
    commitment_hex: String,
    sender_handle_hex: String,
    proof_size: usize,
    tx_version_t1: bool,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct UnoTransferWireVector {
    name: String,
    description: String,
    asset_hex: String,
    destination_hex: String,
    has_extra_data: bool,
    commitment_hex: String,
    sender_handle_hex: String,
    receiver_handle_hex: String,
    proof_size: usize,
    tx_version_t1: bool,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct UnoVectors {
    algorithm: String,
    description: String,
    shield_wire_vectors: Vec<ShieldWireVector>,
    unshield_wire_vectors: Vec<UnshieldWireVector>,
    uno_transfer_wire_vectors: Vec<UnoTransferWireVector>,
}

fn main() {
    let mut shield_wire_vectors = Vec::new();
    let mut unshield_wire_vectors = Vec::new();
    let mut uno_transfer_wire_vectors = Vec::new();

    // ========== Shield Vectors (Type 19) ==========

    // Vector 1: Basic Shield with native TOS asset
    {
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::zero(); // TOS native asset
        let amount = 1000000u64;

        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"shield_proof");
        let proof = ShieldCommitmentProof::new(
            receiver_keypair.get_public_key(),
            amount,
            &opening,
            &mut transcript,
        );

        let payload = ShieldTransferPayload::new(
            asset.clone(),
            destination.clone(),
            amount,
            None,
            commitment.compress(),
            receiver_handle.compress(),
            proof,
        );

        let wire = payload.to_bytes();
        shield_wire_vectors.push(ShieldWireVector {
            name: "shield_basic_tos".to_string(),
            description: "Basic Shield with native TOS asset, no extra_data".to_string(),
            asset_hex: hex::encode(asset.as_bytes()),
            destination_hex: hex::encode(destination.as_bytes()),
            amount,
            has_extra_data: false,
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            proof_size: 96, // ShieldCommitmentProof: Y_H(32) + Y_P(32) + z(32)
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 2: Shield with larger amount
    {
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::zero();
        let amount = 100_000_000_000u64; // 1000 TOS

        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"shield_proof");
        let proof = ShieldCommitmentProof::new(
            receiver_keypair.get_public_key(),
            amount,
            &opening,
            &mut transcript,
        );

        let payload = ShieldTransferPayload::new(
            asset.clone(),
            destination.clone(),
            amount,
            None,
            commitment.compress(),
            receiver_handle.compress(),
            proof,
        );

        let wire = payload.to_bytes();
        shield_wire_vectors.push(ShieldWireVector {
            name: "shield_large_amount".to_string(),
            description: "Shield with large amount (1000 TOS)".to_string(),
            asset_hex: hex::encode(asset.as_bytes()),
            destination_hex: hex::encode(destination.as_bytes()),
            amount,
            has_extra_data: false,
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            proof_size: 96,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // ========== Unshield Vectors (Type 20) ==========

    // Vector 1: Basic Unshield
    {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::zero();
        let amount = 5000000u64;

        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"unshield_proof");
        let proof = CiphertextValidityProof::new(
            receiver_keypair.get_public_key(),
            sender_keypair.get_public_key(),
            amount,
            &opening,
            TxVersion::T1,
            &mut transcript,
        );

        let payload = UnshieldTransferPayload::new(
            asset.clone(),
            destination.clone(),
            amount,
            None,
            commitment.compress(),
            sender_handle.compress(),
            proof,
        );

        let wire = payload.to_bytes();
        unshield_wire_vectors.push(UnshieldWireVector {
            name: "unshield_basic".to_string(),
            description: "Basic Unshield with T1 proof format".to_string(),
            asset_hex: hex::encode(asset.as_bytes()),
            destination_hex: hex::encode(destination.as_bytes()),
            amount,
            has_extra_data: false,
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            sender_handle_hex: hex::encode(sender_handle.compress().as_bytes()),
            proof_size: 160, // T1: Y_0(32) + Y_1(32) + Y_2(32) + z_r(32) + z_x(32)
            tx_version_t1: true,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 2: Unshield with large amount
    {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::zero();
        let amount = 50_000_000_000u64; // 500 TOS

        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"unshield_proof");
        let proof = CiphertextValidityProof::new(
            receiver_keypair.get_public_key(),
            sender_keypair.get_public_key(),
            amount,
            &opening,
            TxVersion::T1,
            &mut transcript,
        );

        let payload = UnshieldTransferPayload::new(
            asset.clone(),
            destination.clone(),
            amount,
            None,
            commitment.compress(),
            sender_handle.compress(),
            proof,
        );

        let wire = payload.to_bytes();
        unshield_wire_vectors.push(UnshieldWireVector {
            name: "unshield_large_amount".to_string(),
            description: "Unshield with large amount (500 TOS)".to_string(),
            asset_hex: hex::encode(asset.as_bytes()),
            destination_hex: hex::encode(destination.as_bytes()),
            amount,
            has_extra_data: false,
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            sender_handle_hex: hex::encode(sender_handle.compress().as_bytes()),
            proof_size: 160,
            tx_version_t1: true,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // ========== UnoTransfer Vectors (Type 18) ==========

    // Vector 1: Basic UnoTransfer
    {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::zero();

        let opening = PedersenOpening::generate_new();
        let amount = 1000000u64;
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);
        let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"uno_transfer_proof");
        let proof = CiphertextValidityProof::new(
            receiver_keypair.get_public_key(),
            sender_keypair.get_public_key(),
            amount,
            &opening,
            TxVersion::T1,
            &mut transcript,
        );

        let payload = UnoTransferPayload::new(
            asset.clone(),
            destination.clone(),
            None,
            commitment.compress(),
            sender_handle.compress(),
            receiver_handle.compress(),
            proof,
        );

        let wire = payload.to_bytes();
        uno_transfer_wire_vectors.push(UnoTransferWireVector {
            name: "uno_transfer_basic".to_string(),
            description: "Basic UnoTransfer with T1 proof format".to_string(),
            asset_hex: hex::encode(asset.as_bytes()),
            destination_hex: hex::encode(destination.as_bytes()),
            has_extra_data: false,
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            sender_handle_hex: hex::encode(sender_handle.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            proof_size: 160,
            tx_version_t1: true,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Vector 2: UnoTransfer with custom asset
    {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::new([0x11u8; 32]); // Custom asset

        let opening = PedersenOpening::generate_new();
        let amount = 25_000_000u64;
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);
        let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"uno_transfer_proof");
        let proof = CiphertextValidityProof::new(
            receiver_keypair.get_public_key(),
            sender_keypair.get_public_key(),
            amount,
            &opening,
            TxVersion::T1,
            &mut transcript,
        );

        let payload = UnoTransferPayload::new(
            asset.clone(),
            destination.clone(),
            None,
            commitment.compress(),
            sender_handle.compress(),
            receiver_handle.compress(),
            proof,
        );

        let wire = payload.to_bytes();
        uno_transfer_wire_vectors.push(UnoTransferWireVector {
            name: "uno_transfer_custom_asset".to_string(),
            description: "UnoTransfer with custom asset".to_string(),
            asset_hex: hex::encode(asset.as_bytes()),
            destination_hex: hex::encode(destination.as_bytes()),
            has_extra_data: false,
            commitment_hex: hex::encode(commitment.compress().as_bytes()),
            sender_handle_hex: hex::encode(sender_handle.compress().as_bytes()),
            receiver_handle_hex: hex::encode(receiver_handle.compress().as_bytes()),
            proof_size: 160,
            tx_version_t1: true,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Build output
    let vectors = UnoVectors {
        algorithm: "UNO-Privacy-Transactions".to_string(),
        description: "TCK test vectors for UNO Privacy transactions (Types 18-20)".to_string(),
        shield_wire_vectors,
        unshield_wire_vectors,
        uno_transfer_wire_vectors,
    };

    // Write YAML output
    let yaml = serde_yaml::to_string(&vectors).expect("Failed to serialize");
    let mut file = File::create("uno.yaml").expect("Failed to create file");
    file.write_all(yaml.as_bytes())
        .expect("Failed to write file");

    println!("Generated uno.yaml with:");
    println!("  - {} Shield wire vectors", vectors.shield_wire_vectors.len());
    println!("  - {} Unshield wire vectors", vectors.unshield_wire_vectors.len());
    println!(
        "  - {} UnoTransfer wire vectors",
        vectors.uno_transfer_wire_vectors.len()
    );
}
