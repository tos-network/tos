// Generate X25519 key exchange test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_x25519_vectors

use x25519_dalek::{StaticSecret, PublicKey};
use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct KeypairVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    secret_key_hex: String,
    public_key_hex: String,
}

#[derive(Serialize)]
struct SharedSecretVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    alice_secret_hex: String,
    alice_public_hex: String,
    bob_secret_hex: String,
    bob_public_hex: String,
    shared_secret_hex: String,
}

#[derive(Serialize)]
struct X25519TestFile {
    algorithm: String,
    key_size: usize,
    keypair_vectors: Vec<KeypairVector>,
    shared_secret_vectors: Vec<SharedSecretVector>,
}

fn main() {
    let mut keypair_vectors = Vec::new();
    let mut shared_secret_vectors = Vec::new();

    // Keypair test 1: All zeros (clamped)
    let secret_bytes = [0u8; 32];
    let secret = StaticSecret::from(secret_bytes);
    let public = PublicKey::from(&secret);
    keypair_vectors.push(KeypairVector {
        name: "zero_secret".to_string(),
        description: Some("All zeros secret (will be clamped)".to_string()),
        secret_key_hex: hex::encode(&secret_bytes),
        public_key_hex: hex::encode(public.as_bytes()),
    });

    // Keypair test 2: All 0x01
    let secret_bytes = [0x01u8; 32];
    let secret = StaticSecret::from(secret_bytes);
    let public = PublicKey::from(&secret);
    keypair_vectors.push(KeypairVector {
        name: "ones_secret".to_string(),
        description: Some("All 0x01 secret".to_string()),
        secret_key_hex: hex::encode(&secret_bytes),
        public_key_hex: hex::encode(public.as_bytes()),
    });

    // Keypair test 3: All 0xFF
    let secret_bytes = [0xffu8; 32];
    let secret = StaticSecret::from(secret_bytes);
    let public = PublicKey::from(&secret);
    keypair_vectors.push(KeypairVector {
        name: "ff_secret".to_string(),
        description: Some("All 0xFF secret".to_string()),
        secret_key_hex: hex::encode(&secret_bytes),
        public_key_hex: hex::encode(public.as_bytes()),
    });

    // Keypair test 4: Sequential bytes
    let secret_bytes: [u8; 32] = core::array::from_fn(|i| i as u8);
    let secret = StaticSecret::from(secret_bytes);
    let public = PublicKey::from(&secret);
    keypair_vectors.push(KeypairVector {
        name: "sequential_secret".to_string(),
        description: Some("Bytes 0x00-0x1F".to_string()),
        secret_key_hex: hex::encode(&secret_bytes),
        public_key_hex: hex::encode(public.as_bytes()),
    });

    // RFC 7748 test vector
    let alice_secret_bytes = hex::decode("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a").unwrap();
    let alice_secret_bytes: [u8; 32] = alice_secret_bytes.try_into().unwrap();
    let alice_secret = StaticSecret::from(alice_secret_bytes);
    let alice_public = PublicKey::from(&alice_secret);
    keypair_vectors.push(KeypairVector {
        name: "rfc7748_alice".to_string(),
        description: Some("RFC 7748 Alice's keypair".to_string()),
        secret_key_hex: hex::encode(&alice_secret_bytes),
        public_key_hex: hex::encode(alice_public.as_bytes()),
    });

    let bob_secret_bytes = hex::decode("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb").unwrap();
    let bob_secret_bytes: [u8; 32] = bob_secret_bytes.try_into().unwrap();
    let bob_secret = StaticSecret::from(bob_secret_bytes);
    let bob_public = PublicKey::from(&bob_secret);
    keypair_vectors.push(KeypairVector {
        name: "rfc7748_bob".to_string(),
        description: Some("RFC 7748 Bob's keypair".to_string()),
        secret_key_hex: hex::encode(&bob_secret_bytes),
        public_key_hex: hex::encode(bob_public.as_bytes()),
    });

    // Shared secret: RFC 7748
    let shared_ab = alice_secret.diffie_hellman(&bob_public);
    let shared_ba = bob_secret.diffie_hellman(&alice_public);
    assert_eq!(shared_ab.as_bytes(), shared_ba.as_bytes());
    shared_secret_vectors.push(SharedSecretVector {
        name: "rfc7748_exchange".to_string(),
        description: Some("RFC 7748 key exchange".to_string()),
        alice_secret_hex: hex::encode(&alice_secret_bytes),
        alice_public_hex: hex::encode(alice_public.as_bytes()),
        bob_secret_hex: hex::encode(&bob_secret_bytes),
        bob_public_hex: hex::encode(bob_public.as_bytes()),
        shared_secret_hex: hex::encode(shared_ab.as_bytes()),
    });

    // Shared secret: Simple test
    let alice_secret_bytes = [0x42u8; 32];
    let alice_secret = StaticSecret::from(alice_secret_bytes);
    let alice_public = PublicKey::from(&alice_secret);

    let bob_secret_bytes = [0x24u8; 32];
    let bob_secret = StaticSecret::from(bob_secret_bytes);
    let bob_public = PublicKey::from(&bob_secret);

    let shared_ab = alice_secret.diffie_hellman(&bob_public);
    let shared_ba = bob_secret.diffie_hellman(&alice_public);
    assert_eq!(shared_ab.as_bytes(), shared_ba.as_bytes());
    shared_secret_vectors.push(SharedSecretVector {
        name: "simple_exchange".to_string(),
        description: Some("Simple key exchange test".to_string()),
        alice_secret_hex: hex::encode(&alice_secret_bytes),
        alice_public_hex: hex::encode(alice_public.as_bytes()),
        bob_secret_hex: hex::encode(&bob_secret_bytes),
        bob_public_hex: hex::encode(bob_public.as_bytes()),
        shared_secret_hex: hex::encode(shared_ab.as_bytes()),
    });

    // Shared secret: Sequential keys
    let alice_secret_bytes: [u8; 32] = core::array::from_fn(|i| i as u8);
    let alice_secret = StaticSecret::from(alice_secret_bytes);
    let alice_public = PublicKey::from(&alice_secret);

    let bob_secret_bytes: [u8; 32] = core::array::from_fn(|i| (i + 32) as u8);
    let bob_secret = StaticSecret::from(bob_secret_bytes);
    let bob_public = PublicKey::from(&bob_secret);

    let shared_ab = alice_secret.diffie_hellman(&bob_public);
    let shared_ba = bob_secret.diffie_hellman(&alice_public);
    assert_eq!(shared_ab.as_bytes(), shared_ba.as_bytes());
    shared_secret_vectors.push(SharedSecretVector {
        name: "sequential_exchange".to_string(),
        description: Some("Sequential bytes key exchange".to_string()),
        alice_secret_hex: hex::encode(&alice_secret_bytes),
        alice_public_hex: hex::encode(alice_public.as_bytes()),
        bob_secret_hex: hex::encode(&bob_secret_bytes),
        bob_public_hex: hex::encode(bob_public.as_bytes()),
        shared_secret_hex: hex::encode(shared_ab.as_bytes()),
    });

    let test_file = X25519TestFile {
        algorithm: "X25519".to_string(),
        key_size: 32,
        keypair_vectors,
        shared_secret_vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("x25519.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to x25519.yaml");
}
