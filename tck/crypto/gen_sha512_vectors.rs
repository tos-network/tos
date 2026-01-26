// Generate SHA512 test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_sha512_vectors

use sha2::{Digest, Sha512};
use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    input_ascii: Option<String>,
    input_length: usize,
    expected_hex: String,
}

#[derive(Serialize)]
struct HashTestFile {
    algorithm: String,
    output_size: usize,
    block_size: usize,
    test_vectors: Vec<TestVector>,
}

fn sha512(input: &[u8]) -> String {
    hex::encode(Sha512::digest(input))
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: Empty string
    vectors.push(TestVector {
        name: "empty_string".to_string(),
        description: None,
        input_hex: "".to_string(),
        input_ascii: Some("".to_string()),
        input_length: 0,
        expected_hex: sha512(b""),
    });

    // Test 2: "abc"
    vectors.push(TestVector {
        name: "abc".to_string(),
        description: None,
        input_hex: hex::encode(b"abc"),
        input_ascii: Some("abc".to_string()),
        input_length: 3,
        expected_hex: sha512(b"abc"),
    });

    // Test 3: "Hello, world!"
    vectors.push(TestVector {
        name: "hello_world".to_string(),
        description: None,
        input_hex: hex::encode(b"Hello, world!"),
        input_ascii: Some("Hello, world!".to_string()),
        input_length: 13,
        expected_hex: sha512(b"Hello, world!"),
    });

    // Test 4: 111 bytes (max single block after padding)
    let input = vec![0x61u8; 111];
    vectors.push(TestVector {
        name: "111_bytes_a".to_string(),
        description: Some("Max single block input (111 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 111,
        expected_hex: sha512(&input),
    });

    // Test 5: 112 bytes (requires two blocks)
    let input = vec![0x61u8; 112];
    vectors.push(TestVector {
        name: "112_bytes_a".to_string(),
        description: Some("Requires two blocks (112 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 112,
        expected_hex: sha512(&input),
    });

    // Test 6: 128 bytes (exactly one block)
    let input = vec![0x61u8; 128];
    vectors.push(TestVector {
        name: "128_bytes_a".to_string(),
        description: Some("Exactly one SHA512 block (128 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 128,
        expected_hex: sha512(&input),
    });

    // Test 7: 256 bytes (exactly two blocks)
    let input = vec![0x61u8; 256];
    vectors.push(TestVector {
        name: "256_bytes_a".to_string(),
        description: Some("Exactly two SHA512 blocks (256 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 256,
        expected_hex: sha512(&input),
    });

    // Test 8: NIST test vector
    let input = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";
    vectors.push(TestVector {
        name: "nist_vector".to_string(),
        description: Some("NIST FIPS 180-4 test vector".to_string()),
        input_hex: hex::encode(input),
        input_ascii: Some(String::from_utf8_lossy(input).to_string()),
        input_length: input.len(),
        expected_hex: sha512(input),
    });

    // Test 9: Binary data with all byte values
    let input: Vec<u8> = (0u8..=255).collect();
    vectors.push(TestVector {
        name: "all_bytes".to_string(),
        description: Some("All byte values 0x00-0xFF".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 256,
        expected_hex: sha512(&input),
    });

    // Test 10: Ed25519 key expansion style (32 byte seed)
    let input = [0x01u8; 32];
    vectors.push(TestVector {
        name: "ed25519_seed".to_string(),
        description: Some("32-byte seed for Ed25519 key expansion".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 32,
        expected_hex: sha512(&input),
    });

    let test_file = HashTestFile {
        algorithm: "SHA512".to_string(),
        output_size: 64,
        block_size: 128,
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("sha512.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to sha512.yaml");
}
