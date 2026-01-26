// Generate Base58 test vectors (Bitcoin/Solana style)
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_base58_vectors

use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_hex: String,
    input_length: usize,
    expected_base58: String,
}

#[derive(Serialize)]
struct Base58TestFile {
    algorithm: String,
    alphabet: String,
    test_vectors: Vec<TestVector>,
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: Empty input
    vectors.push(TestVector {
        name: "empty".to_string(),
        description: None,
        input_hex: "".to_string(),
        input_length: 0,
        expected_base58: bs58::encode(&[] as &[u8]).into_string(),
    });

    // Test 2: Single zero byte
    let input = [0x00u8];
    vectors.push(TestVector {
        name: "single_zero".to_string(),
        description: Some("Single zero byte encodes to '1'".to_string()),
        input_hex: hex::encode(&input),
        input_length: 1,
        expected_base58: bs58::encode(&input).into_string(),
    });

    // Test 3: Multiple leading zeros
    let input = [0x00, 0x00, 0x00, 0x01];
    vectors.push(TestVector {
        name: "leading_zeros".to_string(),
        description: Some("Leading zeros become '1' characters".to_string()),
        input_hex: hex::encode(&input),
        input_length: input.len(),
        expected_base58: bs58::encode(&input).into_string(),
    });

    // Test 4: "Hello World"
    let input = b"Hello World";
    vectors.push(TestVector {
        name: "hello_world".to_string(),
        description: None,
        input_hex: hex::encode(input),
        input_length: input.len(),
        expected_base58: bs58::encode(input).into_string(),
    });

    // Test 5: 32-byte public key (typical Solana/TOS address)
    let input = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
        0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
    ];
    vectors.push(TestVector {
        name: "pubkey_32bytes".to_string(),
        description: Some("32-byte public key encoding".to_string()),
        input_hex: hex::encode(&input),
        input_length: 32,
        expected_base58: bs58::encode(&input).into_string(),
    });

    // Test 6: All 0xFF bytes (max values)
    let input = [0xffu8; 32];
    vectors.push(TestVector {
        name: "all_ff".to_string(),
        description: Some("32 bytes of 0xFF".to_string()),
        input_hex: hex::encode(&input),
        input_length: 32,
        expected_base58: bs58::encode(&input).into_string(),
    });

    // Test 7: Known Solana system program address
    let input = [0u8; 32]; // System program is all zeros
    vectors.push(TestVector {
        name: "solana_system_program".to_string(),
        description: Some("32 zero bytes (Solana system program)".to_string()),
        input_hex: hex::encode(&input),
        input_length: 32,
        expected_base58: bs58::encode(&input).into_string(),
    });

    // Test 8: 64-byte signature
    let input = [0x42u8; 64];
    vectors.push(TestVector {
        name: "signature_64bytes".to_string(),
        description: Some("64-byte signature encoding".to_string()),
        input_hex: hex::encode(&input),
        input_length: 64,
        expected_base58: bs58::encode(&input).into_string(),
    });

    // Test 9: Binary with all byte values (first 58 bytes)
    let input: Vec<u8> = (0u8..58).collect();
    vectors.push(TestVector {
        name: "sequential_bytes".to_string(),
        description: Some("Bytes 0x00-0x39".to_string()),
        input_hex: hex::encode(&input),
        input_length: input.len(),
        expected_base58: bs58::encode(&input).into_string(),
    });

    // Test 10: Single byte tests for each character
    let input = [0x00u8];
    vectors.push(TestVector {
        name: "byte_00".to_string(),
        description: None,
        input_hex: hex::encode(&input),
        input_length: 1,
        expected_base58: bs58::encode(&input).into_string(),
    });

    let input = [0x39u8]; // 57 decimal
    vectors.push(TestVector {
        name: "byte_39".to_string(),
        description: None,
        input_hex: hex::encode(&input),
        input_length: 1,
        expected_base58: bs58::encode(&input).into_string(),
    });

    let test_file = Base58TestFile {
        algorithm: "Base58".to_string(),
        alphabet: "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz".to_string(),
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("base58.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to base58.yaml");
}
