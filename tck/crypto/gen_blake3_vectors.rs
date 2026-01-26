// Generate BLAKE3 test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_blake3_vectors

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

fn blake3_hash(input: &[u8]) -> String {
    hex::encode(blake3::hash(input).as_bytes())
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
        expected_hex: blake3_hash(b""),
    });

    // Test 2: "abc"
    vectors.push(TestVector {
        name: "abc".to_string(),
        description: None,
        input_hex: hex::encode(b"abc"),
        input_ascii: Some("abc".to_string()),
        input_length: 3,
        expected_hex: blake3_hash(b"abc"),
    });

    // Test 3: "Hello, world!"
    vectors.push(TestVector {
        name: "hello_world".to_string(),
        description: None,
        input_hex: hex::encode(b"Hello, world!"),
        input_ascii: Some("Hello, world!".to_string()),
        input_length: 13,
        expected_hex: blake3_hash(b"Hello, world!"),
    });

    // Test 4: 63 bytes (one less than chunk)
    let input = vec![0x61u8; 63];
    vectors.push(TestVector {
        name: "63_bytes_a".to_string(),
        description: Some("One byte less than BLAKE3 chunk size".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 63,
        expected_hex: blake3_hash(&input),
    });

    // Test 5: 64 bytes (exactly one chunk)
    let input = vec![0x61u8; 64];
    vectors.push(TestVector {
        name: "64_bytes_a".to_string(),
        description: Some("Exactly one BLAKE3 chunk (64 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 64,
        expected_hex: blake3_hash(&input),
    });

    // Test 6: 65 bytes (one more than chunk)
    let input = vec![0x61u8; 65];
    vectors.push(TestVector {
        name: "65_bytes_a".to_string(),
        description: Some("One byte more than BLAKE3 chunk size".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 65,
        expected_hex: blake3_hash(&input),
    });

    // Test 7: 1024 bytes (multiple chunks)
    let input = vec![0x61u8; 1024];
    vectors.push(TestVector {
        name: "1024_bytes_a".to_string(),
        description: Some("1024 bytes spanning multiple chunks".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 1024,
        expected_hex: blake3_hash(&input),
    });

    // Test 8: Binary data with all byte values
    let input: Vec<u8> = (0u8..=255).collect();
    vectors.push(TestVector {
        name: "all_bytes".to_string(),
        description: Some("All byte values 0x00-0xFF".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 256,
        expected_hex: blake3_hash(&input),
    });

    // Test 9: Transaction hash style (common blockchain use)
    let input = [0x42u8; 32];
    vectors.push(TestVector {
        name: "tx_hash".to_string(),
        description: Some("32-byte transaction data hash".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 32,
        expected_hex: blake3_hash(&input),
    });

    let test_file = HashTestFile {
        algorithm: "BLAKE3".to_string(),
        output_size: 32,
        block_size: 64,
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("blake3.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to blake3.yaml");
}
