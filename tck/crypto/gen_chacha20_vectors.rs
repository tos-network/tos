// Generate ChaCha20 test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_chacha20_vectors

use chacha20::cipher::{KeyIvInit, StreamCipher};
use chacha20::ChaCha20;
use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    key_hex: String,
    nonce_hex: String,
    plaintext_hex: String,
    plaintext_length: usize,
    ciphertext_hex: String,
}

#[derive(Serialize)]
struct ChaCha20TestFile {
    algorithm: String,
    key_size: usize,
    nonce_size: usize,
    test_vectors: Vec<TestVector>,
}

fn chacha20_encrypt(key: &[u8; 32], nonce: &[u8; 12], plaintext: &[u8]) -> Vec<u8> {
    let mut cipher = ChaCha20::new(key.into(), nonce.into());
    let mut ciphertext = plaintext.to_vec();
    cipher.apply_keystream(&mut ciphertext);
    ciphertext
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: RFC 8439 test vector
    let key = hex::decode("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f").unwrap();
    let key: [u8; 32] = key.try_into().unwrap();
    let nonce = hex::decode("000000000000004a00000000").unwrap();
    let nonce: [u8; 12] = nonce.try_into().unwrap();
    let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let ciphertext = chacha20_encrypt(&key, &nonce, plaintext);
    vectors.push(TestVector {
        name: "rfc8439_test".to_string(),
        description: Some("RFC 8439 test vector".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext),
    });

    // Test 2: Empty plaintext
    let key = [0x42u8; 32];
    let nonce = [0x00u8; 12];
    let plaintext = b"";
    let ciphertext = chacha20_encrypt(&key, &nonce, plaintext);
    vectors.push(TestVector {
        name: "empty_plaintext".to_string(),
        description: Some("Empty plaintext".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        plaintext_hex: "".to_string(),
        plaintext_length: 0,
        ciphertext_hex: hex::encode(&ciphertext),
    });

    // Test 3: Single byte
    let plaintext = [0xabu8];
    let ciphertext = chacha20_encrypt(&key, &nonce, &plaintext);
    vectors.push(TestVector {
        name: "single_byte".to_string(),
        description: None,
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        plaintext_hex: hex::encode(&plaintext),
        plaintext_length: 1,
        ciphertext_hex: hex::encode(&ciphertext),
    });

    // Test 4: 64 bytes (one block)
    let key = [0x01u8; 32];
    let nonce = [0x02u8; 12];
    let plaintext = [0x00u8; 64];
    let ciphertext = chacha20_encrypt(&key, &nonce, &plaintext);
    vectors.push(TestVector {
        name: "one_block".to_string(),
        description: Some("64-byte (one block) plaintext".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        plaintext_hex: hex::encode(&plaintext),
        plaintext_length: 64,
        ciphertext_hex: hex::encode(&ciphertext),
    });

    // Test 5: 128 bytes (two blocks)
    let plaintext = [0x00u8; 128];
    let ciphertext = chacha20_encrypt(&key, &nonce, &plaintext);
    vectors.push(TestVector {
        name: "two_blocks".to_string(),
        description: Some("128-byte (two blocks) plaintext".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        plaintext_hex: hex::encode(&plaintext),
        plaintext_length: 128,
        ciphertext_hex: hex::encode(&ciphertext),
    });

    // Test 6: 65 bytes (one block + 1)
    let plaintext = [0xffu8; 65];
    let ciphertext = chacha20_encrypt(&key, &nonce, &plaintext);
    vectors.push(TestVector {
        name: "one_block_plus_one".to_string(),
        description: Some("65-byte plaintext".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        plaintext_hex: hex::encode(&plaintext),
        plaintext_length: 65,
        ciphertext_hex: hex::encode(&ciphertext),
    });

    // Test 7: All zeros key and nonce
    let key = [0x00u8; 32];
    let nonce = [0x00u8; 12];
    let plaintext = b"Hello, world!";
    let ciphertext = chacha20_encrypt(&key, &nonce, plaintext);
    vectors.push(TestVector {
        name: "zero_key_nonce".to_string(),
        description: Some("All zero key and nonce".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext),
    });

    // Test 8: Sequential nonce
    let key = [0x42u8; 32];
    let nonce: [u8; 12] = core::array::from_fn(|i| i as u8);
    let plaintext = [0x61u8; 100];
    let ciphertext = chacha20_encrypt(&key, &nonce, &plaintext);
    vectors.push(TestVector {
        name: "sequential_nonce".to_string(),
        description: Some("Sequential nonce bytes".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        plaintext_hex: hex::encode(&plaintext),
        plaintext_length: 100,
        ciphertext_hex: hex::encode(&ciphertext),
    });

    let test_file = ChaCha20TestFile {
        algorithm: "ChaCha20".to_string(),
        key_size: 32,
        nonce_size: 12,
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("chacha20.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to chacha20.yaml");
}
