/// Generate and verify SHA3-512 test vectors
/// Run: cargo run --bin gen_crypto_vectors

use sha3::{Digest, Sha3_512};
use hex;

fn sha3_512_hex(input: &[u8]) -> String {
    let hash = Sha3_512::digest(input);
    hex::encode(hash)
}

fn main() {
    println!("# SHA3-512 Test Vectors");
    println!("# Generated from Rust sha3 crate v0.10\n");

    // Test 1: Empty string
    let hash = sha3_512_hex(b"");
    println!("empty_string: {}", hash);

    // Test 2: "abc"
    let hash = sha3_512_hex(b"abc");
    println!("abc: {}", hash);

    // Test 3: "Hello, world!"
    let hash = sha3_512_hex(b"Hello, world!");
    println!("hello_world: {}", hash);

    // Test 4: 71 bytes of 'a'
    let input = vec![0x61u8; 71];
    let hash = sha3_512_hex(&input);
    println!("71_bytes_a: {}", hash);

    // Test 5: 72 bytes of 'a' (exactly one block)
    let input = vec![0x61u8; 72];
    let hash = sha3_512_hex(&input);
    println!("72_bytes_a: {}", hash);

    // Test 6: 73 bytes of 'a'
    let input = vec![0x61u8; 73];
    let hash = sha3_512_hex(&input);
    println!("73_bytes_a: {}", hash);

    // Test 7: 144 bytes of 'a' (exactly two blocks)
    let input = vec![0x61u8; 144];
    let hash = sha3_512_hex(&input);
    println!("144_bytes_a: {}", hash);

    // Test 8: TOS signature style (pubkey + message + point)
    let mut input = Vec::new();
    input.extend_from_slice(&[0u8; 32]); // pubkey
    input.extend_from_slice(b"Hello, world!"); // message
    input.extend_from_slice(&[0u8; 32]); // point
    let hash = sha3_512_hex(&input);
    println!("tos_sig_style: {}", hash);
    println!("  input_hex: {}", hex::encode(&input));
}
