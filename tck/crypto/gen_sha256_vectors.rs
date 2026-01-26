// Generate SHA256 test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_sha256_vectors

use sha2::{Digest, Sha256};
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

fn sha256(input: &[u8]) -> String {
    hex::encode(Sha256::digest(input))
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
        expected_hex: sha256(b""),
    });

    // Test 2: "abc"
    vectors.push(TestVector {
        name: "abc".to_string(),
        description: None,
        input_hex: hex::encode(b"abc"),
        input_ascii: Some("abc".to_string()),
        input_length: 3,
        expected_hex: sha256(b"abc"),
    });

    // Test 3: "Hello, world!"
    vectors.push(TestVector {
        name: "hello_world".to_string(),
        description: None,
        input_hex: hex::encode(b"Hello, world!"),
        input_ascii: Some("Hello, world!".to_string()),
        input_length: 13,
        expected_hex: sha256(b"Hello, world!"),
    });

    // Test 4: 55 bytes (one less than single block after padding)
    let input = vec![0x61u8; 55];
    vectors.push(TestVector {
        name: "55_bytes_a".to_string(),
        description: Some("Max single block input (55 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 55,
        expected_hex: sha256(&input),
    });

    // Test 5: 56 bytes (requires two blocks)
    let input = vec![0x61u8; 56];
    vectors.push(TestVector {
        name: "56_bytes_a".to_string(),
        description: Some("Requires two blocks (56 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 56,
        expected_hex: sha256(&input),
    });

    // Test 6: 64 bytes (exactly one block)
    let input = vec![0x61u8; 64];
    vectors.push(TestVector {
        name: "64_bytes_a".to_string(),
        description: Some("Exactly one SHA256 block (64 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 64,
        expected_hex: sha256(&input),
    });

    // Test 7: 128 bytes (exactly two blocks)
    let input = vec![0x61u8; 128];
    vectors.push(TestVector {
        name: "128_bytes_a".to_string(),
        description: Some("Exactly two SHA256 blocks (128 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 128,
        expected_hex: sha256(&input),
    });

    // Test 8: NIST test vector
    let input = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    vectors.push(TestVector {
        name: "nist_vector".to_string(),
        description: Some("NIST FIPS 180-4 test vector".to_string()),
        input_hex: hex::encode(input),
        input_ascii: Some(String::from_utf8_lossy(input).to_string()),
        input_length: input.len(),
        expected_hex: sha256(input),
    });

    // Test 9: Binary data with all byte values
    let input: Vec<u8> = (0u8..=255).collect();
    vectors.push(TestVector {
        name: "all_bytes".to_string(),
        description: Some("All byte values 0x00-0xFF".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 256,
        expected_hex: sha256(&input),
    });

    let test_file = HashTestFile {
        algorithm: "SHA256".to_string(),
        output_size: 32,
        block_size: 64,
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("sha256.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to sha256.yaml");
}
