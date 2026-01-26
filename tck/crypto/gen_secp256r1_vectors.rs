// Generate secp256r1 (P-256) test vectors for cross-language verification
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_secp256r1_vectors > secp256r1.yaml

use p256::{
    ecdsa::{Signature, SigningKey, VerifyingKey, signature::Signer},
    elliptic_curve::sec1::ToEncodedPoint,
};
use serde::Serialize;
use sha2::{Sha256, Digest};

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    message_hex: String,
    message_length: usize,
    signature_hex: String,
    public_key_hex: String,
    should_verify: bool,
}

#[derive(Serialize)]
struct TestVectors {
    algorithm: String,
    description: String,
    note: String,
    test_vectors: Vec<TestVector>,
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: Simple message
    {
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let verifying_key = VerifyingKey::from(&signing_key);
        let msg = b"hello world";

        let signature: Signature = signing_key.sign(msg);

        // Compressed public key (33 bytes)
        let public_key_bytes = verifying_key.to_encoded_point(true);

        vectors.push(TestVector {
            name: "hello_world".to_string(),
            description: Some("Simple message signing".to_string()),
            message_hex: hex::encode(msg),
            message_length: msg.len(),
            signature_hex: hex::encode(signature.to_bytes()),
            public_key_hex: hex::encode(public_key_bytes.as_bytes()),
            should_verify: true,
        });
    }

    // Test 2: Empty message
    {
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let verifying_key = VerifyingKey::from(&signing_key);
        let msg: &[u8] = b"";

        let signature: Signature = signing_key.sign(msg);
        let public_key_bytes = verifying_key.to_encoded_point(true);

        vectors.push(TestVector {
            name: "empty_message".to_string(),
            description: Some("Empty message".to_string()),
            message_hex: hex::encode(msg),
            message_length: msg.len(),
            signature_hex: hex::encode(signature.to_bytes()),
            public_key_hex: hex::encode(public_key_bytes.as_bytes()),
            should_verify: true,
        });
    }

    // Test 3: Long message
    {
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let verifying_key = VerifyingKey::from(&signing_key);
        let msg = b"The quick brown fox jumps over the lazy dog. This is a longer message to test hashing.";

        let signature: Signature = signing_key.sign(msg);
        let public_key_bytes = verifying_key.to_encoded_point(true);

        vectors.push(TestVector {
            name: "long_message".to_string(),
            description: Some("Longer message for hash testing".to_string()),
            message_hex: hex::encode(msg),
            message_length: msg.len(),
            signature_hex: hex::encode(signature.to_bytes()),
            public_key_hex: hex::encode(public_key_bytes.as_bytes()),
            should_verify: true,
        });
    }

    // Test 4: Known test vector (deterministic private key)
    {
        let private_key_bytes: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
        ];
        let signing_key = SigningKey::from_bytes(&private_key_bytes.into()).unwrap();
        let verifying_key = VerifyingKey::from(&signing_key);
        let msg = b"test message";

        let signature: Signature = signing_key.sign(msg);
        let public_key_bytes = verifying_key.to_encoded_point(true);

        vectors.push(TestVector {
            name: "deterministic".to_string(),
            description: Some("Deterministic test with known private key".to_string()),
            message_hex: hex::encode(msg),
            message_length: msg.len(),
            signature_hex: hex::encode(signature.to_bytes()),
            public_key_hex: hex::encode(public_key_bytes.as_bytes()),
            should_verify: true,
        });
    }

    // Test 5: Binary data
    {
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let verifying_key = VerifyingKey::from(&signing_key);
        let msg: [u8; 32] = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
            0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88,
            0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x00,
        ];

        let signature: Signature = signing_key.sign(&msg);
        let public_key_bytes = verifying_key.to_encoded_point(true);

        vectors.push(TestVector {
            name: "binary_data".to_string(),
            description: Some("Binary data message".to_string()),
            message_hex: hex::encode(msg),
            message_length: msg.len(),
            signature_hex: hex::encode(signature.to_bytes()),
            public_key_hex: hex::encode(public_key_bytes.as_bytes()),
            should_verify: true,
        });
    }

    let test_vectors = TestVectors {
        algorithm: "secp256r1".to_string(),
        description: "secp256r1 (NIST P-256) ECDSA signature verification test vectors".to_string(),
        note: "Public key is compressed (33 bytes). Signature is (r, s) format (64 bytes). Message is hashed with SHA-256 internally.".to_string(),
        test_vectors: vectors,
    };

    println!("{}", serde_yaml::to_string(&test_vectors).unwrap());
}
