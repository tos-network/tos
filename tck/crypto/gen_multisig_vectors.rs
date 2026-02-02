// MultiSig (Type 2) Transaction Test Vector Generator
// Generates test vectors for cross-language verification between TOS Rust and Avatar C
//
// Wire Format:
//   threshold: 1 byte (u8)
//   participants_count: 1 byte (u8) - only if threshold != 0
//   participants: N * 32 bytes (CompressedPublicKey)
//
// Special case: threshold=0 means "delete multisig" (no participants)

use hex;
use indexmap::IndexSet;
use serde::Serialize;
use std::fs::File;
use std::io::Write;
use tos_common::crypto::elgamal::CompressedPublicKey;
use tos_common::serializer::Serializer;
use tos_common::transaction::MultiSigPayload;

#[derive(Serialize)]
struct MultiSigVector {
    name: String,
    description: String,
    threshold: u8,
    participants_count: u8,
    participants_hex: Vec<String>,
    wire_hex: String,
}

#[derive(Serialize)]
struct MultiSigTestVectors {
    algorithm: String,
    version: u32,
    multisig_vectors: Vec<MultiSigVector>,
}

fn test_pubkey(seed: u8) -> CompressedPublicKey {
    CompressedPublicKey::from_bytes(&[seed; 32]).expect("Valid pubkey bytes")
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: Delete multisig (threshold=0)
    {
        let payload = MultiSigPayload {
            threshold: 0,
            participants: IndexSet::new(),
        };
        vectors.push(MultiSigVector {
            name: "multisig_delete".to_string(),
            description: "Delete multisig (threshold=0, no participants)".to_string(),
            threshold: 0,
            participants_count: 0,
            participants_hex: vec![],
            wire_hex: payload.to_hex(),
        });
    }

    // Test 2: Simple 1-of-1 multisig
    {
        let pubkey1 = test_pubkey(0x11);
        let mut participants = IndexSet::new();
        participants.insert(pubkey1.clone());

        let payload = MultiSigPayload {
            threshold: 1,
            participants: participants.clone(),
        };
        vectors.push(MultiSigVector {
            name: "multisig_1_of_1".to_string(),
            description: "1-of-1 multisig (single signer)".to_string(),
            threshold: 1,
            participants_count: 1,
            participants_hex: vec![hex::encode(pubkey1.as_bytes())],
            wire_hex: payload.to_hex(),
                    });
    }

    // Test 3: 2-of-2 multisig
    {
        let pubkey1 = test_pubkey(0x22);
        let pubkey2 = test_pubkey(0x33);
        let mut participants = IndexSet::new();
        participants.insert(pubkey1.clone());
        participants.insert(pubkey2.clone());

        let payload = MultiSigPayload {
            threshold: 2,
            participants: participants.clone(),
        };
        vectors.push(MultiSigVector {
            name: "multisig_2_of_2".to_string(),
            description: "2-of-2 multisig (both signers required)".to_string(),
            threshold: 2,
            participants_count: 2,
            participants_hex: vec![
                hex::encode(pubkey1.as_bytes()),
                hex::encode(pubkey2.as_bytes()),
            ],
            wire_hex: payload.to_hex(),
                    });
    }

    // Test 4: 2-of-3 multisig
    {
        let pubkey1 = test_pubkey(0x44);
        let pubkey2 = test_pubkey(0x55);
        let pubkey3 = test_pubkey(0x66);
        let mut participants = IndexSet::new();
        participants.insert(pubkey1.clone());
        participants.insert(pubkey2.clone());
        participants.insert(pubkey3.clone());

        let payload = MultiSigPayload {
            threshold: 2,
            participants: participants.clone(),
        };
        vectors.push(MultiSigVector {
            name: "multisig_2_of_3".to_string(),
            description: "2-of-3 multisig (any 2 of 3 signers)".to_string(),
            threshold: 2,
            participants_count: 3,
            participants_hex: vec![
                hex::encode(pubkey1.as_bytes()),
                hex::encode(pubkey2.as_bytes()),
                hex::encode(pubkey3.as_bytes()),
            ],
            wire_hex: payload.to_hex(),
                    });
    }

    // Test 5: 3-of-5 multisig
    {
        let pubkeys: Vec<CompressedPublicKey> = (0x77..0x7C)
            .map(|seed| test_pubkey(seed))
            .collect();
        let mut participants = IndexSet::new();
        for pk in &pubkeys {
            participants.insert(pk.clone());
        }

        let payload = MultiSigPayload {
            threshold: 3,
            participants: participants.clone(),
        };
        vectors.push(MultiSigVector {
            name: "multisig_3_of_5".to_string(),
            description: "3-of-5 multisig (any 3 of 5 signers)".to_string(),
            threshold: 3,
            participants_count: 5,
            participants_hex: pubkeys.iter().map(|pk| hex::encode(pk.as_bytes())).collect(),
            wire_hex: payload.to_hex(),
                    });
    }

    // Write output
    let test_file = MultiSigTestVectors {
        algorithm: "MultiSig-Transactions".to_string(),
        version: 1,
        multisig_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).expect("YAML serialization failed");

    let header = r#"# MultiSig Transaction Test Vectors (Type 2)
# Generated by TOS Rust - gen_multisig_vectors
# Cross-language verification between TOS Rust and Avatar C
#
# Wire Format:
#   threshold: 1 byte (u8)
#   participants_count: 1 byte (u8) - only if threshold != 0
#   participants: N * 32 bytes (CompressedPublicKey)
#
# Special case: threshold=0 means "delete multisig" (no participants)

"#;

    let full_yaml = format!("{}{}", header, yaml);
    println!("{}", full_yaml);

    let mut file = File::create("multisig.yaml").expect("Failed to create file");
    file.write_all(full_yaml.as_bytes())
        .expect("Failed to write file");
    eprintln!("Written to multisig.yaml");
}
