// TNS (TOS Name Service) Transaction Test Vector Generator (Types 21-22)
// Generates test vectors for cross-language verification between TOS Rust and Avatar C
//
// Transaction Types:
//   Type 21: RegisterName - Register a human-readable name (e.g., alice@tos.network)
//   Type 22: EphemeralMessage - Send encrypted message to a registered name

use hex;
use serde::Serialize;
use std::fs::File;
use std::io::Write;
use tos_common::crypto::Hash;
use tos_common::serializer::Serializer;
use tos_common::transaction::{EphemeralMessagePayload, RegisterNamePayload};

// ============================================================================
// YAML Structures
// ============================================================================

#[derive(Serialize)]
struct TnsTestVectors {
    algorithm: String,
    version: u32,
    register_name_vectors: Vec<RegisterNameVector>,
    ephemeral_message_vectors: Vec<EphemeralMessageVector>,
}

#[derive(Serialize)]
struct RegisterNameVector {
    name: String,
    description: String,
    tns_name: String,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct EphemeralMessageVector {
    name: String,
    description: String,
    sender_name_hash_hex: String,
    recipient_name_hash_hex: String,
    message_nonce: u64,
    ttl_blocks: u32,
    encrypted_content_len: usize,
    receiver_handle_hex: String,
    wire_hex: String,
    expected_size: usize,
}

// ============================================================================
// Helper Functions
// ============================================================================

fn test_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

// ============================================================================
// Vector Generation
// ============================================================================

fn gen_register_name_vectors() -> Vec<RegisterNameVector> {
    let mut vectors = Vec::new();

    // Basic name (5 chars)
    {
        let tns_name = "alice".to_string();
        let payload = RegisterNamePayload::new(tns_name.clone());
        let wire = payload.to_bytes();

        vectors.push(RegisterNameVector {
            name: "register_name_basic".to_string(),
            description: "Register basic 5-char name".to_string(),
            tns_name,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Minimum length name (3 chars)
    {
        let tns_name = "bob".to_string();
        let payload = RegisterNamePayload::new(tns_name.clone());
        let wire = payload.to_bytes();

        vectors.push(RegisterNameVector {
            name: "register_name_min".to_string(),
            description: "Register minimum length (3 char) name".to_string(),
            tns_name,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Longer name (16 chars)
    {
        let tns_name = "cryptoenthusiast".to_string();
        let payload = RegisterNamePayload::new(tns_name.clone());
        let wire = payload.to_bytes();

        vectors.push(RegisterNameVector {
            name: "register_name_16chars".to_string(),
            description: "Register 16-char name".to_string(),
            tns_name,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Name with numbers
    {
        let tns_name = "user123".to_string();
        let payload = RegisterNamePayload::new(tns_name.clone());
        let wire = payload.to_bytes();

        vectors.push(RegisterNameVector {
            name: "register_name_alphanumeric".to_string(),
            description: "Register alphanumeric name".to_string(),
            tns_name,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Name with underscore
    {
        let tns_name = "my_wallet".to_string();
        let payload = RegisterNamePayload::new(tns_name.clone());
        let wire = payload.to_bytes();

        vectors.push(RegisterNameVector {
            name: "register_name_underscore".to_string(),
            description: "Register name with underscore".to_string(),
            tns_name,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

fn gen_ephemeral_message_vectors() -> Vec<EphemeralMessageVector> {
    let mut vectors = Vec::new();

    // Basic ephemeral message
    {
        let sender_hash = test_hash(0x11);
        let recipient_hash = test_hash(0x22);
        let message_nonce = 1u64;
        let ttl_blocks = 1000u32;
        let encrypted_content = vec![0xAAu8; 64]; // 64 bytes encrypted
        let receiver_handle = [0x33u8; 32];

        let payload = EphemeralMessagePayload::new(
            sender_hash.clone(),
            recipient_hash.clone(),
            message_nonce,
            ttl_blocks,
            encrypted_content.clone(),
            receiver_handle,
        );
        let wire = payload.to_bytes();

        vectors.push(EphemeralMessageVector {
            name: "ephemeral_message_basic".to_string(),
            description: "Basic ephemeral message with 64-byte content".to_string(),
            sender_name_hash_hex: hex::encode(sender_hash.as_bytes()),
            recipient_name_hash_hex: hex::encode(recipient_hash.as_bytes()),
            message_nonce,
            ttl_blocks,
            encrypted_content_len: 64,
            receiver_handle_hex: hex::encode(&receiver_handle),
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Minimum content (1 byte)
    {
        let sender_hash = test_hash(0x44);
        let recipient_hash = test_hash(0x55);
        let message_nonce = 42u64;
        let ttl_blocks = 100u32;
        let encrypted_content = vec![0xBBu8; 1]; // 1 byte minimum
        let receiver_handle = [0x66u8; 32];

        let payload = EphemeralMessagePayload::new(
            sender_hash.clone(),
            recipient_hash.clone(),
            message_nonce,
            ttl_blocks,
            encrypted_content.clone(),
            receiver_handle,
        );
        let wire = payload.to_bytes();

        vectors.push(EphemeralMessageVector {
            name: "ephemeral_message_min_content".to_string(),
            description: "Ephemeral message with minimum 1-byte content".to_string(),
            sender_name_hash_hex: hex::encode(sender_hash.as_bytes()),
            recipient_name_hash_hex: hex::encode(recipient_hash.as_bytes()),
            message_nonce,
            ttl_blocks,
            encrypted_content_len: 1,
            receiver_handle_hex: hex::encode(&receiver_handle),
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Larger message (128 bytes)
    {
        let sender_hash = test_hash(0x77);
        let recipient_hash = test_hash(0x88);
        let message_nonce = 9999u64;
        let ttl_blocks = 8640u32; // ~1 day at 10s blocks
        let encrypted_content = vec![0xCCu8; 128]; // 128 bytes
        let receiver_handle = [0x99u8; 32];

        let payload = EphemeralMessagePayload::new(
            sender_hash.clone(),
            recipient_hash.clone(),
            message_nonce,
            ttl_blocks,
            encrypted_content.clone(),
            receiver_handle,
        );
        let wire = payload.to_bytes();

        vectors.push(EphemeralMessageVector {
            name: "ephemeral_message_128bytes".to_string(),
            description: "Ephemeral message with 128-byte content".to_string(),
            sender_name_hash_hex: hex::encode(sender_hash.as_bytes()),
            recipient_name_hash_hex: hex::encode(recipient_hash.as_bytes()),
            message_nonce,
            ttl_blocks,
            encrypted_content_len: 128,
            receiver_handle_hex: hex::encode(&receiver_handle),
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Max TTL
    {
        let sender_hash = test_hash(0xAA);
        let recipient_hash = test_hash(0xBB);
        let message_nonce = 12345u64;
        let ttl_blocks = 604800u32; // 7 days at 1s blocks (max TTL)
        let encrypted_content = vec![0xDDu8; 32];
        let receiver_handle = [0xEEu8; 32];

        let payload = EphemeralMessagePayload::new(
            sender_hash.clone(),
            recipient_hash.clone(),
            message_nonce,
            ttl_blocks,
            encrypted_content.clone(),
            receiver_handle,
        );
        let wire = payload.to_bytes();

        vectors.push(EphemeralMessageVector {
            name: "ephemeral_message_max_ttl".to_string(),
            description: "Ephemeral message with maximum TTL".to_string(),
            sender_name_hash_hex: hex::encode(sender_hash.as_bytes()),
            recipient_name_hash_hex: hex::encode(recipient_hash.as_bytes()),
            message_nonce,
            ttl_blocks,
            encrypted_content_len: 32,
            receiver_handle_hex: hex::encode(&receiver_handle),
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let vectors = TnsTestVectors {
        algorithm: "TNS-Transactions".to_string(),
        version: 1,
        register_name_vectors: gen_register_name_vectors(),
        ephemeral_message_vectors: gen_ephemeral_message_vectors(),
    };

    let yaml = serde_yaml::to_string(&vectors).expect("Failed to serialize to YAML");

    // Add header comment
    let header = r#"# TNS (TOS Name Service) Transactions Test Vectors (Types 21-22)
# Generated by TOS Rust - gen_tns_vectors
# Cross-language verification between TOS Rust and Avatar C
#
# Transaction Types:
#   Type 21: RegisterName - Register a human-readable name (e.g., alice@tos.network)
#   Type 22: EphemeralMessage - Send encrypted message to a registered name
#
# RegisterName Wire Format:
#   [name_len:1][name:1-64]
#
# EphemeralMessage Wire Format:
#   [sender_name_hash:32][recipient_name_hash:32][message_nonce:8][ttl_blocks:4]
#   [content_len:2][encrypted_content:1-188][receiver_handle:32]

"#;

    let output = format!("{}{}", header, yaml);

    // Write to file
    let output_path = "tns.yaml";
    let mut file = File::create(output_path).expect("Failed to create output file");
    file.write_all(output.as_bytes())
        .expect("Failed to write output");

    println!("Generated TNS vectors to {}", output_path);
    println!("  RegisterName: {}", vectors.register_name_vectors.len());
    println!(
        "  EphemeralMessage: {}",
        vectors.ephemeral_message_vectors.len()
    );
}
