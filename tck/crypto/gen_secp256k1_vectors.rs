// Generate secp256k1 test vectors for cross-language verification
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_secp256k1_vectors > secp256k1.yaml

use k256::{
    ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey, signature::Signer},
    elliptic_curve::sec1::ToEncodedPoint,
};
use serde::Serialize;
use sha2::{Sha256, Digest};

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    msg_hash_hex: String,
    signature_hex: String,
    recovery_id: u8,
    public_key_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    should_recover: Option<bool>,
}

#[derive(Serialize)]
struct TestVectors {
    algorithm: String,
    description: String,
    test_vectors: Vec<TestVector>,
}

fn hash_message(msg: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(msg);
    hasher.finalize().into()
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: Simple message
    {
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let verifying_key = VerifyingKey::from(&signing_key);
        let msg = b"hello world";
        let msg_hash = hash_message(msg);

        let (signature, recovery_id) = signing_key
            .sign_prehash_recoverable(&msg_hash)
            .expect("signing failed");

        let public_key_bytes = verifying_key.to_encoded_point(false);
        // Skip the 0x04 prefix byte for uncompressed format
        let pk_bytes = &public_key_bytes.as_bytes()[1..];

        vectors.push(TestVector {
            name: "hello_world".to_string(),
            description: Some("Simple message signing".to_string()),
            msg_hash_hex: hex::encode(msg_hash),
            signature_hex: hex::encode(signature.to_bytes()),
            recovery_id: recovery_id.to_byte(),
            public_key_hex: hex::encode(pk_bytes),
            should_recover: Some(true),
        });
    }

    // Test 2: Empty message hash (all zeros)
    {
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let verifying_key = VerifyingKey::from(&signing_key);
        let msg_hash = [0u8; 32];

        let (signature, recovery_id) = signing_key
            .sign_prehash_recoverable(&msg_hash)
            .expect("signing failed");

        let public_key_bytes = verifying_key.to_encoded_point(false);
        let pk_bytes = &public_key_bytes.as_bytes()[1..];

        vectors.push(TestVector {
            name: "zero_hash".to_string(),
            description: Some("All-zero message hash".to_string()),
            msg_hash_hex: hex::encode(msg_hash),
            signature_hex: hex::encode(signature.to_bytes()),
            recovery_id: recovery_id.to_byte(),
            public_key_hex: hex::encode(pk_bytes),
            should_recover: Some(true),
        });
    }

    // Test 3: Max hash (all 0xFF)
    {
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let verifying_key = VerifyingKey::from(&signing_key);
        let msg_hash = [0xffu8; 32];

        let (signature, recovery_id) = signing_key
            .sign_prehash_recoverable(&msg_hash)
            .expect("signing failed");

        let public_key_bytes = verifying_key.to_encoded_point(false);
        let pk_bytes = &public_key_bytes.as_bytes()[1..];

        vectors.push(TestVector {
            name: "max_hash".to_string(),
            description: Some("All-0xFF message hash".to_string()),
            msg_hash_hex: hex::encode(msg_hash),
            signature_hex: hex::encode(signature.to_bytes()),
            recovery_id: recovery_id.to_byte(),
            public_key_hex: hex::encode(pk_bytes),
            should_recover: Some(true),
        });
    }

    // Test 4: Known test vector (deterministic)
    {
        let private_key_bytes: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
        ];
        let signing_key = SigningKey::from_bytes(&private_key_bytes.into()).unwrap();
        let verifying_key = VerifyingKey::from(&signing_key);

        let msg_hash: [u8; 32] = [
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11,
            0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11,
            0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
        ];

        let (signature, recovery_id) = signing_key
            .sign_prehash_recoverable(&msg_hash)
            .expect("signing failed");

        let public_key_bytes = verifying_key.to_encoded_point(false);
        let pk_bytes = &public_key_bytes.as_bytes()[1..];

        vectors.push(TestVector {
            name: "deterministic".to_string(),
            description: Some("Deterministic test with known private key".to_string()),
            msg_hash_hex: hex::encode(msg_hash),
            signature_hex: hex::encode(signature.to_bytes()),
            recovery_id: recovery_id.to_byte(),
            public_key_hex: hex::encode(pk_bytes),
            should_recover: Some(true),
        });
    }

    // Test 5: Ethereum-style transaction hash
    {
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let verifying_key = VerifyingKey::from(&signing_key);
        // Simulate keccak256 hash of transaction
        let msg_hash: [u8; 32] = [
            0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0xba, 0xbe,
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
            0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
        ];

        let (signature, recovery_id) = signing_key
            .sign_prehash_recoverable(&msg_hash)
            .expect("signing failed");

        let public_key_bytes = verifying_key.to_encoded_point(false);
        let pk_bytes = &public_key_bytes.as_bytes()[1..];

        vectors.push(TestVector {
            name: "eth_style".to_string(),
            description: Some("Ethereum-style transaction hash".to_string()),
            msg_hash_hex: hex::encode(msg_hash),
            signature_hex: hex::encode(signature.to_bytes()),
            recovery_id: recovery_id.to_byte(),
            public_key_hex: hex::encode(pk_bytes),
            should_recover: Some(true),
        });
    }

    let test_vectors = TestVectors {
        algorithm: "secp256k1".to_string(),
        description: "secp256k1 ECDSA recoverable signature test vectors".to_string(),
        test_vectors: vectors,
    };

    println!("{}", serde_yaml::to_string(&test_vectors).unwrap());
}
