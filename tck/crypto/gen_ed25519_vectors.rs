// Generate Ed25519 test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_ed25519_vectors

use ed25519_dalek::{SigningKey, Signer, Verifier};
use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct KeypairVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    seed_hex: String,
    public_key_hex: String,
}

#[derive(Serialize)]
struct SignatureVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    seed_hex: String,
    public_key_hex: String,
    message_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_ascii: Option<String>,
    signature_hex: String,
}

#[derive(Serialize)]
struct Ed25519TestFile {
    algorithm: String,
    public_key_size: usize,
    secret_key_size: usize,
    signature_size: usize,
    keypair_vectors: Vec<KeypairVector>,
    signature_vectors: Vec<SignatureVector>,
}

fn main() {
    let mut keypair_vectors = Vec::new();
    let mut signature_vectors = Vec::new();

    // Keypair test 1: All zeros seed
    let seed = [0u8; 32];
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key();
    keypair_vectors.push(KeypairVector {
        name: "zero_seed".to_string(),
        description: Some("All zeros seed".to_string()),
        seed_hex: hex::encode(&seed),
        public_key_hex: hex::encode(public_key.as_bytes()),
    });

    // Keypair test 2: All ones seed
    let seed = [0x01u8; 32];
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key();
    keypair_vectors.push(KeypairVector {
        name: "ones_seed".to_string(),
        description: Some("All 0x01 seed".to_string()),
        seed_hex: hex::encode(&seed),
        public_key_hex: hex::encode(public_key.as_bytes()),
    });

    // Keypair test 3: All 0xFF seed
    let seed = [0xffu8; 32];
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key();
    keypair_vectors.push(KeypairVector {
        name: "ff_seed".to_string(),
        description: Some("All 0xFF seed".to_string()),
        seed_hex: hex::encode(&seed),
        public_key_hex: hex::encode(public_key.as_bytes()),
    });

    // Keypair test 4: Sequential bytes
    let seed: [u8; 32] = core::array::from_fn(|i| i as u8);
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key();
    keypair_vectors.push(KeypairVector {
        name: "sequential_seed".to_string(),
        description: Some("Bytes 0x00-0x1F".to_string()),
        seed_hex: hex::encode(&seed),
        public_key_hex: hex::encode(public_key.as_bytes()),
    });

    // Signature test 1: Empty message
    let seed = [0x42u8; 32];
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key();
    let message = b"";
    let signature = signing_key.sign(message);
    assert!(public_key.verify(message, &signature).is_ok());
    signature_vectors.push(SignatureVector {
        name: "empty_message".to_string(),
        description: Some("Empty message signature".to_string()),
        seed_hex: hex::encode(&seed),
        public_key_hex: hex::encode(public_key.as_bytes()),
        message_hex: "".to_string(),
        message_ascii: Some("".to_string()),
        signature_hex: hex::encode(signature.to_bytes()),
    });

    // Signature test 2: "Hello, world!"
    let message = b"Hello, world!";
    let signature = signing_key.sign(message);
    assert!(public_key.verify(message, &signature).is_ok());
    signature_vectors.push(SignatureVector {
        name: "hello_world".to_string(),
        description: None,
        seed_hex: hex::encode(&seed),
        public_key_hex: hex::encode(public_key.as_bytes()),
        message_hex: hex::encode(message),
        message_ascii: Some("Hello, world!".to_string()),
        signature_hex: hex::encode(signature.to_bytes()),
    });

    // Signature test 3: 32-byte message (typical hash)
    let message = [0xabu8; 32];
    let signature = signing_key.sign(&message);
    assert!(public_key.verify(&message, &signature).is_ok());
    signature_vectors.push(SignatureVector {
        name: "32byte_message".to_string(),
        description: Some("32-byte hash-like message".to_string()),
        seed_hex: hex::encode(&seed),
        public_key_hex: hex::encode(public_key.as_bytes()),
        message_hex: hex::encode(&message),
        message_ascii: None,
        signature_hex: hex::encode(signature.to_bytes()),
    });

    // Signature test 4: Long message
    let message = vec![0x61u8; 1000];
    let signature = signing_key.sign(&message);
    assert!(public_key.verify(&message, &signature).is_ok());
    signature_vectors.push(SignatureVector {
        name: "long_message".to_string(),
        description: Some("1000-byte message".to_string()),
        seed_hex: hex::encode(&seed),
        public_key_hex: hex::encode(public_key.as_bytes()),
        message_hex: hex::encode(&message),
        message_ascii: None,
        signature_hex: hex::encode(signature.to_bytes()),
    });

    // Signature test 5: Different seed
    let seed = [0x00u8; 32];
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key();
    let message = b"test message";
    let signature = signing_key.sign(message);
    assert!(public_key.verify(message, &signature).is_ok());
    signature_vectors.push(SignatureVector {
        name: "zero_seed_sign".to_string(),
        description: Some("Zero seed signature".to_string()),
        seed_hex: hex::encode(&seed),
        public_key_hex: hex::encode(public_key.as_bytes()),
        message_hex: hex::encode(message),
        message_ascii: Some("test message".to_string()),
        signature_hex: hex::encode(signature.to_bytes()),
    });

    // RFC 8032 test vector (from the spec)
    let seed = hex::decode("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60").unwrap();
    let seed: [u8; 32] = seed.try_into().unwrap();
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key();
    let message = b"";
    let signature = signing_key.sign(message);
    keypair_vectors.push(KeypairVector {
        name: "rfc8032_test1".to_string(),
        description: Some("RFC 8032 test vector 1".to_string()),
        seed_hex: hex::encode(&seed),
        public_key_hex: hex::encode(public_key.as_bytes()),
    });
    signature_vectors.push(SignatureVector {
        name: "rfc8032_test1".to_string(),
        description: Some("RFC 8032 test vector 1".to_string()),
        seed_hex: hex::encode(&seed),
        public_key_hex: hex::encode(public_key.as_bytes()),
        message_hex: "".to_string(),
        message_ascii: Some("".to_string()),
        signature_hex: hex::encode(signature.to_bytes()),
    });

    let test_file = Ed25519TestFile {
        algorithm: "Ed25519".to_string(),
        public_key_size: 32,
        secret_key_size: 32,
        signature_size: 64,
        keypair_vectors,
        signature_vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("ed25519.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to ed25519.yaml");
}
