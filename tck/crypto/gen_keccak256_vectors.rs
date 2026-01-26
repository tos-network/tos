// Generate Keccak256 test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_keccak256_vectors

use tiny_keccak::{Hasher, Keccak};
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

fn keccak256(input: &[u8]) -> String {
    let mut hasher = Keccak::v256();
    hasher.update(input);
    let mut output = [0u8; 32];
    hasher.finalize(&mut output);
    hex::encode(output)
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
        expected_hex: keccak256(b""),
    });

    // Test 2: "abc"
    vectors.push(TestVector {
        name: "abc".to_string(),
        description: None,
        input_hex: hex::encode(b"abc"),
        input_ascii: Some("abc".to_string()),
        input_length: 3,
        expected_hex: keccak256(b"abc"),
    });

    // Test 3: "Hello, world!"
    vectors.push(TestVector {
        name: "hello_world".to_string(),
        description: None,
        input_hex: hex::encode(b"Hello, world!"),
        input_ascii: Some("Hello, world!".to_string()),
        input_length: 13,
        expected_hex: keccak256(b"Hello, world!"),
    });

    // Test 4: Ethereum address derivation style
    // keccak256 of public key (64 bytes) -> last 20 bytes = address
    let input = vec![0x04u8; 64];
    vectors.push(TestVector {
        name: "ethereum_pubkey".to_string(),
        description: Some("64-byte public key for Ethereum address derivation".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 64,
        expected_hex: keccak256(&input),
    });

    // Test 5: 135 bytes (one less than block)
    let input = vec![0x61u8; 135];
    vectors.push(TestVector {
        name: "135_bytes_a".to_string(),
        description: Some("One byte less than Keccak256 block size".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 135,
        expected_hex: keccak256(&input),
    });

    // Test 6: 136 bytes (exactly one block)
    let input = vec![0x61u8; 136];
    vectors.push(TestVector {
        name: "136_bytes_a".to_string(),
        description: Some("Exactly one Keccak256 block (136 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 136,
        expected_hex: keccak256(&input),
    });

    // Test 7: 137 bytes (one more than block)
    let input = vec![0x61u8; 137];
    vectors.push(TestVector {
        name: "137_bytes_a".to_string(),
        description: Some("One byte more than Keccak256 block size".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 137,
        expected_hex: keccak256(&input),
    });

    // Test 8: 272 bytes (exactly two blocks)
    let input = vec![0x61u8; 272];
    vectors.push(TestVector {
        name: "272_bytes_a".to_string(),
        description: Some("Exactly two Keccak256 blocks (272 bytes)".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 272,
        expected_hex: keccak256(&input),
    });

    // Test 9: Binary data with all byte values
    let input: Vec<u8> = (0u8..=255).collect();
    vectors.push(TestVector {
        name: "all_bytes".to_string(),
        description: Some("All byte values 0x00-0xFF".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 256,
        expected_hex: keccak256(&input),
    });

    // Test 10: Solana/TOS program derived address style (32 bytes)
    let input = [0xffu8; 32];
    vectors.push(TestVector {
        name: "pda_seed".to_string(),
        description: Some("32-byte seed for PDA derivation".to_string()),
        input_hex: hex::encode(&input),
        input_ascii: None,
        input_length: 32,
        expected_hex: keccak256(&input),
    });

    let test_file = HashTestFile {
        algorithm: "Keccak256".to_string(),
        output_size: 32,
        block_size: 136,
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("keccak256.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to keccak256.yaml");
}
