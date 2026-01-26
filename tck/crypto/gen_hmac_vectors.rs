// Generate HMAC test vectors (HMAC-SHA256 and HMAC-SHA512)
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_hmac_vectors

use hmac::{Hmac, Mac};
use sha2::{Sha256, Sha512};
use serde::Serialize;
use std::fs::File;
use std::io::Write;

type HmacSha256 = Hmac<Sha256>;
type HmacSha512 = Hmac<Sha512>;

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    key_hex: String,
    key_length: usize,
    message_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_ascii: Option<String>,
    message_length: usize,
    expected_hex: String,
}

#[derive(Serialize)]
struct HmacTestFile {
    algorithm: String,
    output_size: usize,
    test_vectors: Vec<TestVector>,
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(key).unwrap();
    mac.update(message);
    hex::encode(mac.finalize().into_bytes())
}

fn hmac_sha512(key: &[u8], message: &[u8]) -> String {
    let mut mac = HmacSha512::new_from_slice(key).unwrap();
    mac.update(message);
    hex::encode(mac.finalize().into_bytes())
}

fn generate_hmac_sha256_vectors() -> HmacTestFile {
    let mut vectors = Vec::new();

    // Test 1: RFC 4231 Test Case 1
    let key = hex::decode("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b").unwrap();
    let message = b"Hi There";
    vectors.push(TestVector {
        name: "rfc4231_test1".to_string(),
        description: Some("RFC 4231 Test Case 1".to_string()),
        key_hex: hex::encode(&key),
        key_length: key.len(),
        message_hex: hex::encode(message),
        message_ascii: Some("Hi There".to_string()),
        message_length: message.len(),
        expected_hex: hmac_sha256(&key, message),
    });

    // Test 2: RFC 4231 Test Case 2
    let key = b"Jefe";
    let message = b"what do ya want for nothing?";
    vectors.push(TestVector {
        name: "rfc4231_test2".to_string(),
        description: Some("RFC 4231 Test Case 2".to_string()),
        key_hex: hex::encode(key),
        key_length: key.len(),
        message_hex: hex::encode(message),
        message_ascii: Some("what do ya want for nothing?".to_string()),
        message_length: message.len(),
        expected_hex: hmac_sha256(key, message),
    });

    // Test 3: RFC 4231 Test Case 3
    let key = hex::decode("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let message = hex::decode("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd").unwrap();
    vectors.push(TestVector {
        name: "rfc4231_test3".to_string(),
        description: Some("RFC 4231 Test Case 3".to_string()),
        key_hex: hex::encode(&key),
        key_length: key.len(),
        message_hex: hex::encode(&message),
        message_ascii: None,
        message_length: message.len(),
        expected_hex: hmac_sha256(&key, &message),
    });

    // Test 4: Empty message
    let key = [0x01u8; 32];
    let message = b"";
    vectors.push(TestVector {
        name: "empty_message".to_string(),
        description: Some("32-byte key, empty message".to_string()),
        key_hex: hex::encode(&key),
        key_length: key.len(),
        message_hex: "".to_string(),
        message_ascii: Some("".to_string()),
        message_length: 0,
        expected_hex: hmac_sha256(&key, message),
    });

    // Test 5: Long key (> block size, will be hashed)
    let key = vec![0xaau8; 128];
    let message = b"Test with a key longer than block size";
    vectors.push(TestVector {
        name: "long_key".to_string(),
        description: Some("Key longer than block size (128 bytes)".to_string()),
        key_hex: hex::encode(&key),
        key_length: key.len(),
        message_hex: hex::encode(message),
        message_ascii: Some(String::from_utf8_lossy(message).to_string()),
        message_length: message.len(),
        expected_hex: hmac_sha256(&key, message),
    });

    // Test 6: Blockchain-style key derivation
    let key = [0x42u8; 32];
    let message = b"m/44'/501'/0'/0'";
    vectors.push(TestVector {
        name: "bip32_path".to_string(),
        description: Some("HD wallet path derivation style".to_string()),
        key_hex: hex::encode(&key),
        key_length: key.len(),
        message_hex: hex::encode(message),
        message_ascii: Some(String::from_utf8_lossy(message).to_string()),
        message_length: message.len(),
        expected_hex: hmac_sha256(&key, message),
    });

    HmacTestFile {
        algorithm: "HMAC-SHA256".to_string(),
        output_size: 32,
        test_vectors: vectors,
    }
}

fn generate_hmac_sha512_vectors() -> HmacTestFile {
    let mut vectors = Vec::new();

    // Test 1: RFC 4231 Test Case 1
    let key = hex::decode("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b").unwrap();
    let message = b"Hi There";
    vectors.push(TestVector {
        name: "rfc4231_test1".to_string(),
        description: Some("RFC 4231 Test Case 1".to_string()),
        key_hex: hex::encode(&key),
        key_length: key.len(),
        message_hex: hex::encode(message),
        message_ascii: Some("Hi There".to_string()),
        message_length: message.len(),
        expected_hex: hmac_sha512(&key, message),
    });

    // Test 2: RFC 4231 Test Case 2
    let key = b"Jefe";
    let message = b"what do ya want for nothing?";
    vectors.push(TestVector {
        name: "rfc4231_test2".to_string(),
        description: Some("RFC 4231 Test Case 2".to_string()),
        key_hex: hex::encode(key),
        key_length: key.len(),
        message_hex: hex::encode(message),
        message_ascii: Some("what do ya want for nothing?".to_string()),
        message_length: message.len(),
        expected_hex: hmac_sha512(key, message),
    });

    // Test 3: Empty message
    let key = [0x01u8; 64];
    let message = b"";
    vectors.push(TestVector {
        name: "empty_message".to_string(),
        description: Some("64-byte key, empty message".to_string()),
        key_hex: hex::encode(&key),
        key_length: key.len(),
        message_hex: "".to_string(),
        message_ascii: Some("".to_string()),
        message_length: 0,
        expected_hex: hmac_sha512(&key, message),
    });

    // Test 4: Long key
    let key = vec![0xaau8; 256];
    let message = b"Test with a key longer than block size";
    vectors.push(TestVector {
        name: "long_key".to_string(),
        description: Some("Key longer than block size (256 bytes)".to_string()),
        key_hex: hex::encode(&key),
        key_length: key.len(),
        message_hex: hex::encode(message),
        message_ascii: Some(String::from_utf8_lossy(message).to_string()),
        message_length: message.len(),
        expected_hex: hmac_sha512(&key, message),
    });

    // Test 5: BIP39 mnemonic to seed style
    let key = b"mnemonic";
    let message = b"abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    vectors.push(TestVector {
        name: "bip39_mnemonic".to_string(),
        description: Some("BIP39 mnemonic to seed derivation style".to_string()),
        key_hex: hex::encode(key),
        key_length: key.len(),
        message_hex: hex::encode(message),
        message_ascii: Some(String::from_utf8_lossy(message).to_string()),
        message_length: message.len(),
        expected_hex: hmac_sha512(key, message),
    });

    HmacTestFile {
        algorithm: "HMAC-SHA512".to_string(),
        output_size: 64,
        test_vectors: vectors,
    }
}

fn main() {
    // Generate HMAC-SHA256 vectors
    let sha256_file = generate_hmac_sha256_vectors();
    let yaml = serde_yaml::to_string(&sha256_file).unwrap();
    println!("=== HMAC-SHA256 ===\n{}", yaml);
    let mut file = File::create("hmac_sha256.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to hmac_sha256.yaml");

    // Generate HMAC-SHA512 vectors
    let sha512_file = generate_hmac_sha512_vectors();
    let yaml = serde_yaml::to_string(&sha512_file).unwrap();
    println!("\n=== HMAC-SHA512 ===\n{}", yaml);
    let mut file = File::create("hmac_sha512.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to hmac_sha512.yaml");
}
