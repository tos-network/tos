// Generate AES-GCM test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_aes_gcm_vectors

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes128Gcm, Aes256Gcm, Nonce,
};
use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    key_hex: String,
    key_size: usize,
    nonce_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    aad_hex: Option<String>,
    plaintext_hex: String,
    plaintext_length: usize,
    ciphertext_hex: String,
    tag_hex: String,
}

#[derive(Serialize)]
struct AesGcmTestFile {
    algorithm: String,
    nonce_size: usize,
    tag_size: usize,
    test_vectors: Vec<TestVector>,
}

fn main() {
    let mut vectors = Vec::new();

    // AES-256-GCM tests

    // Test 1: Empty plaintext, no AAD
    let key = [0x42u8; 32];
    let nonce = [0x00u8; 12];
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), b"".as_ref()).unwrap();
    // ciphertext is actually just the tag for empty plaintext
    vectors.push(TestVector {
        name: "aes256_empty".to_string(),
        description: Some("AES-256-GCM empty plaintext".to_string()),
        key_hex: hex::encode(&key),
        key_size: 32,
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: "".to_string(),
        plaintext_length: 0,
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 2: Simple message
    let plaintext = b"Hello, world!";
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_ref()).unwrap();
    vectors.push(TestVector {
        name: "aes256_hello".to_string(),
        description: Some("AES-256-GCM simple message".to_string()),
        key_hex: hex::encode(&key),
        key_size: 32,
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 3: With AAD
    let key = [0x01u8; 32];
    let nonce: [u8; 12] = core::array::from_fn(|i| i as u8);
    let aad = b"additional authenticated data";
    let plaintext = b"secret message";
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let payload = Payload {
        msg: plaintext.as_ref(),
        aad: aad.as_ref(),
    };
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), payload).unwrap();
    vectors.push(TestVector {
        name: "aes256_with_aad".to_string(),
        description: Some("AES-256-GCM with AAD".to_string()),
        key_hex: hex::encode(&key),
        key_size: 32,
        nonce_hex: hex::encode(&nonce),
        aad_hex: Some(hex::encode(aad)),
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 4: 64 bytes plaintext
    let key = [0xab; 32];
    let nonce = [0xcd; 12];
    let plaintext = [0x00u8; 64];
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_ref()).unwrap();
    vectors.push(TestVector {
        name: "aes256_64bytes".to_string(),
        description: Some("AES-256-GCM 64-byte plaintext".to_string()),
        key_hex: hex::encode(&key),
        key_size: 32,
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: hex::encode(&plaintext),
        plaintext_length: 64,
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // AES-128-GCM tests

    // Test 5: AES-128-GCM empty
    let key = [0x42u8; 16];
    let nonce = [0x00u8; 12];
    let cipher = Aes128Gcm::new_from_slice(&key).unwrap();
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), b"".as_ref()).unwrap();
    vectors.push(TestVector {
        name: "aes128_empty".to_string(),
        description: Some("AES-128-GCM empty plaintext".to_string()),
        key_hex: hex::encode(&key),
        key_size: 16,
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: "".to_string(),
        plaintext_length: 0,
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 6: AES-128-GCM message
    let plaintext = b"Hello, world!";
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_ref()).unwrap();
    vectors.push(TestVector {
        name: "aes128_hello".to_string(),
        description: Some("AES-128-GCM simple message".to_string()),
        key_hex: hex::encode(&key),
        key_size: 16,
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 7: AES-128-GCM with AAD
    let key = [0x01u8; 16];
    let nonce: [u8; 12] = core::array::from_fn(|i| i as u8);
    let aad = b"additional authenticated data";
    let plaintext = b"secret message";
    let cipher = Aes128Gcm::new_from_slice(&key).unwrap();
    let payload = Payload {
        msg: plaintext.as_ref(),
        aad: aad.as_ref(),
    };
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), payload).unwrap();
    vectors.push(TestVector {
        name: "aes128_with_aad".to_string(),
        description: Some("AES-128-GCM with AAD".to_string()),
        key_hex: hex::encode(&key),
        key_size: 16,
        nonce_hex: hex::encode(&nonce),
        aad_hex: Some(hex::encode(aad)),
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // NIST test vectors (GCM-AES-256, Test Case 14)
    let key = hex::decode("0000000000000000000000000000000000000000000000000000000000000000").unwrap();
    let key: [u8; 32] = key.try_into().unwrap();
    let nonce = hex::decode("000000000000000000000000").unwrap();
    let nonce: [u8; 12] = nonce.try_into().unwrap();
    let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), b"".as_ref()).unwrap();
    vectors.push(TestVector {
        name: "nist_aes256_tc14".to_string(),
        description: Some("NIST GCM Test Case 14 (zero key/nonce, empty plaintext)".to_string()),
        key_hex: hex::encode(&key),
        key_size: 32,
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: "".to_string(),
        plaintext_length: 0,
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    let test_file = AesGcmTestFile {
        algorithm: "AES-GCM".to_string(),
        nonce_size: 12,
        tag_size: 16,
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("aes_gcm.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to aes_gcm.yaml");
}
