// Generate ChaCha20-Poly1305 test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_chacha20_poly1305_vectors
//
// This generates test vectors for TOS P2P encryption compatibility.
// Uses the same chacha20poly1305 crate as TOS P2P encryption.

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Nonce,
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
    nonce_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    aad_hex: Option<String>,
    plaintext_hex: String,
    plaintext_length: usize,
    ciphertext_hex: String,
    tag_hex: String,
}

#[derive(Serialize)]
struct ChaCha20Poly1305TestFile {
    algorithm: String,
    description: String,
    key_size: usize,
    nonce_size: usize,
    tag_size: usize,
    test_vectors: Vec<TestVector>,
}

/// Build nonce in TOS format: [8-byte counter big-endian][4-byte zeros]
fn build_tos_nonce(counter: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[0] = (counter >> 56) as u8;
    nonce[1] = (counter >> 48) as u8;
    nonce[2] = (counter >> 40) as u8;
    nonce[3] = (counter >> 32) as u8;
    nonce[4] = (counter >> 24) as u8;
    nonce[5] = (counter >> 16) as u8;
    nonce[6] = (counter >> 8) as u8;
    nonce[7] = counter as u8;
    // nonce[8..12] are already zeros
    nonce
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: Empty plaintext, no AAD, counter=0
    let key = [0x42u8; 32];
    let nonce = build_tos_nonce(0);
    let cipher = ChaCha20Poly1305::new_from_slice(&key).unwrap();
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), b"".as_ref()).unwrap();
    vectors.push(TestVector {
        name: "empty_counter0".to_string(),
        description: Some("Empty plaintext, TOS nonce counter=0".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: "".to_string(),
        plaintext_length: 0,
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 2: Simple message, no AAD, counter=0
    let plaintext = b"Hello, TOS P2P!";
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_ref()).unwrap();
    vectors.push(TestVector {
        name: "hello_counter0".to_string(),
        description: Some("Simple message, TOS nonce counter=0".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 3: Message with counter=1 (simulating second message)
    let nonce = build_tos_nonce(1);
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_ref()).unwrap();
    vectors.push(TestVector {
        name: "hello_counter1".to_string(),
        description: Some("Simple message, TOS nonce counter=1".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 4: Message with counter=255 (boundary test)
    let nonce = build_tos_nonce(255);
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_ref()).unwrap();
    vectors.push(TestVector {
        name: "hello_counter255".to_string(),
        description: Some("Simple message, TOS nonce counter=255".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 5: Message with large counter (testing big-endian encoding)
    let nonce = build_tos_nonce(0x0102030405060708);
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_ref()).unwrap();
    vectors.push(TestVector {
        name: "hello_large_counter".to_string(),
        description: Some("Simple message, TOS nonce with large counter".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 6: With AAD (additional authenticated data)
    let key = [0x01u8; 32];
    let nonce = build_tos_nonce(0);
    let aad = b"TOS P2P session";
    let plaintext = b"secret message";
    let cipher = ChaCha20Poly1305::new_from_slice(&key).unwrap();
    let payload = Payload {
        msg: plaintext.as_ref(),
        aad: aad.as_ref(),
    };
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), payload).unwrap();
    vectors.push(TestVector {
        name: "with_aad".to_string(),
        description: Some("Message with AAD".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        aad_hex: Some(hex::encode(aad)),
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 7: 64 bytes plaintext (typical P2P message)
    let key = [0xab; 32];
    let nonce = build_tos_nonce(42);
    let plaintext = [0x00u8; 64];
    let cipher = ChaCha20Poly1305::new_from_slice(&key).unwrap();
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_ref()).unwrap();
    vectors.push(TestVector {
        name: "64bytes".to_string(),
        description: Some("64-byte plaintext".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: hex::encode(&plaintext),
        plaintext_length: 64,
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 8: 1024 bytes plaintext (larger P2P message)
    let plaintext: Vec<u8> = (0..1024).map(|i| (i & 0xff) as u8).collect();
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_slice()).unwrap();
    vectors.push(TestVector {
        name: "1024bytes".to_string(),
        description: Some("1024-byte plaintext".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: hex::encode(&plaintext),
        plaintext_length: 1024,
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 9: RFC 8439 test vector (Section 2.8.2)
    let key = hex::decode("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f").unwrap();
    let key: [u8; 32] = key.try_into().unwrap();
    let nonce = hex::decode("070000004041424344454647").unwrap();
    let nonce: [u8; 12] = nonce.try_into().unwrap();
    let aad = hex::decode("50515253c0c1c2c3c4c5c6c7").unwrap();
    let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let cipher = ChaCha20Poly1305::new_from_slice(&key).unwrap();
    let payload = Payload {
        msg: plaintext.as_ref(),
        aad: aad.as_slice(),
    };
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), payload).unwrap();
    vectors.push(TestVector {
        name: "rfc8439_test".to_string(),
        description: Some("RFC 8439 Section 2.8.2 test vector".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        aad_hex: Some(hex::encode(&aad)),
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    // Test 10: Zero key, zero nonce (edge case)
    let key = [0x00u8; 32];
    let nonce = [0x00u8; 12];
    let plaintext = b"test";
    let cipher = ChaCha20Poly1305::new_from_slice(&key).unwrap();
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_ref()).unwrap();
    vectors.push(TestVector {
        name: "zero_key_nonce".to_string(),
        description: Some("Zero key and nonce".to_string()),
        key_hex: hex::encode(&key),
        nonce_hex: hex::encode(&nonce),
        aad_hex: None,
        plaintext_hex: hex::encode(plaintext),
        plaintext_length: plaintext.len(),
        ciphertext_hex: hex::encode(&ciphertext[..ciphertext.len() - 16]),
        tag_hex: hex::encode(&ciphertext[ciphertext.len() - 16..]),
    });

    let test_file = ChaCha20Poly1305TestFile {
        algorithm: "ChaCha20-Poly1305".to_string(),
        description: "AEAD per RFC 8439, compatible with TOS P2P encryption".to_string(),
        key_size: 32,
        nonce_size: 12,
        tag_size: 16,
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("chacha20_poly1305.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to chacha20_poly1305.yaml");
}
