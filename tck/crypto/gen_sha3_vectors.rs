// Generate SHA3-512 test vectors using the same sha3 crate as TOS
// Run: cd /Users/tomisetsu/tos/tck/crypto && cargo run --release --bin gen_sha3_vectors

use sha3::{Digest, Sha3_512};
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
struct Sha3TestFile {
    algorithm: String,
    output_size: usize,
    block_size: usize,
    test_vectors: Vec<TestVector>,
}

fn sha3_512(input: &[u8]) -> String {
    hex::encode(Sha3_512::digest(input))
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
        expected_hex: sha3_512(b""),
    });

    // Test 2: "abc"
    vectors.push(TestVector {
        name: "abc".to_string(),
        description: None,
        input_hex: hex::encode(b"abc"),
        input_ascii: Some("abc".to_string()),
        input_length: 3,
        expected_hex: sha3_512(b"abc"),
    });

    // Test 3: "Hello, world!" (TOS test message)
    vectors.push(TestVector {
        name: "hello_world".to_string(),
        description: Some("Message used in TOS signature tests".to_string()),
        input_hex: hex::encode(b"Hello, world!"),
        input_ascii: Some("Hello, world!".to_string()),
        input_length: 13,
        expected_hex: sha3_512(b"Hello, world!"),
    });

    // Test 4: 71 bytes (one less than block)
    let input = vec![0x61u8; 71];
    vectors.push(TestVector {
        name: "71_bytes_a".to_string(),
        description: Some("One byte less than SHA3-512 block size".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 71,
        expected_hex: sha3_512(&input),
    });

    // Test 5: 72 bytes (exactly one block)
    let input = vec![0x61u8; 72];
    vectors.push(TestVector {
        name: "72_bytes_a".to_string(),
        description: Some("Exactly one SHA3-512 block (72 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 72,
        expected_hex: sha3_512(&input),
    });

    // Test 6: 73 bytes (one more than block)
    let input = vec![0x61u8; 73];
    vectors.push(TestVector {
        name: "73_bytes_a".to_string(),
        description: Some("One byte more than SHA3-512 block size".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 73,
        expected_hex: sha3_512(&input),
    });

    // Test 7: 144 bytes (exactly two blocks)
    let input = vec![0x61u8; 144];
    vectors.push(TestVector {
        name: "144_bytes_a".to_string(),
        description: Some("Exactly two SHA3-512 blocks (144 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 144,
        expected_hex: sha3_512(&input),
    });

    // Test 8: TOS signature hash style (pubkey + message + point)
    let mut input = Vec::new();
    input.extend_from_slice(&[0u8; 32]); // pubkey (32 zeros)
    input.extend_from_slice(b"Hello, world!"); // message
    input.extend_from_slice(&[0u8; 32]); // point (32 zeros)
    vectors.push(TestVector {
        name: "tos_signature_hash".to_string(),
        description: Some("TOS hash_and_point_to_scalar style: pubkey(32) + message + point(32)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: input.len(),
        expected_hex: sha3_512(&input),
    });

    let test_file = Sha3TestFile {
        algorithm: "SHA3-512".to_string(),
        output_size: 64,
        block_size: 72,
        test_vectors: vectors,
    };

    // Output YAML
    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    // Also write to file
    let mut file = File::create("sha3_512.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to sha3_512.yaml");
}
