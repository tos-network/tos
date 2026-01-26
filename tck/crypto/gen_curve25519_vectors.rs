// Generate Curve25519 scalar and point operation test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_curve25519_vectors

use curve25519_dalek_ng::scalar::Scalar;
use curve25519_dalek_ng::constants::RISTRETTO_BASEPOINT_POINT;
use curve25519_dalek_ng::ristretto::RistrettoPoint;
use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct ScalarReduceVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_hex: String,
    input_length: usize,
    reduced_hex: String,
}

#[derive(Serialize)]
struct ScalarArithVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    a_hex: String,
    b_hex: String,
    add_hex: String,
    sub_hex: String,
    mul_hex: String,
}

#[derive(Serialize)]
struct ScalarInvertVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_hex: String,
    inverted_hex: String,
}

#[derive(Serialize)]
struct ScalarMulBaseVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    scalar_hex: String,
    point_hex: String,
}

#[derive(Serialize)]
struct Curve25519TestFile {
    algorithm: String,
    scalar_size: usize,
    point_size: usize,
    scalar_reduce_vectors: Vec<ScalarReduceVector>,
    scalar_arith_vectors: Vec<ScalarArithVector>,
    scalar_invert_vectors: Vec<ScalarInvertVector>,
    scalar_mul_base_vectors: Vec<ScalarMulBaseVector>,
}

fn main() {
    let mut scalar_reduce_vectors = Vec::new();
    let mut scalar_arith_vectors = Vec::new();
    let mut scalar_invert_vectors = Vec::new();
    let mut scalar_mul_base_vectors = Vec::new();

    // Scalar reduction test 1: 64-byte input (all zeros)
    let input = [0u8; 64];
    let reduced = Scalar::from_bytes_mod_order_wide(&input);
    scalar_reduce_vectors.push(ScalarReduceVector {
        name: "zero_64".to_string(),
        description: Some("64 zero bytes reduced".to_string()),
        input_hex: hex::encode(&input),
        input_length: 64,
        reduced_hex: hex::encode(reduced.as_bytes()),
    });

    // Scalar reduction test 2: 64-byte input (all 0xFF)
    let input = [0xffu8; 64];
    let reduced = Scalar::from_bytes_mod_order_wide(&input);
    scalar_reduce_vectors.push(ScalarReduceVector {
        name: "ff_64".to_string(),
        description: Some("64 0xFF bytes reduced".to_string()),
        input_hex: hex::encode(&input),
        input_length: 64,
        reduced_hex: hex::encode(reduced.as_bytes()),
    });

    // Scalar reduction test 3: Sequential bytes
    let input: [u8; 64] = core::array::from_fn(|i| i as u8);
    let reduced = Scalar::from_bytes_mod_order_wide(&input);
    scalar_reduce_vectors.push(ScalarReduceVector {
        name: "sequential_64".to_string(),
        description: Some("Bytes 0x00-0x3F".to_string()),
        input_hex: hex::encode(&input),
        input_length: 64,
        reduced_hex: hex::encode(reduced.as_bytes()),
    });

    // Scalar arithmetic tests
    let a_bytes: [u8; 32] = [
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let b_bytes: [u8; 32] = [
        0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let a = Scalar::from_bytes_mod_order(a_bytes);
    let b = Scalar::from_bytes_mod_order(b_bytes);
    scalar_arith_vectors.push(ScalarArithVector {
        name: "simple_1_2".to_string(),
        description: Some("a=1, b=2".to_string()),
        a_hex: hex::encode(a.as_bytes()),
        b_hex: hex::encode(b.as_bytes()),
        add_hex: hex::encode((a + b).as_bytes()),
        sub_hex: hex::encode((a - b).as_bytes()),
        mul_hex: hex::encode((a * b).as_bytes()),
    });

    // Larger values
    let a_bytes: [u8; 32] = [
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let b_bytes: [u8; 32] = [
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let a = Scalar::from_bytes_mod_order(a_bytes);
    let b = Scalar::from_bytes_mod_order(b_bytes);
    scalar_arith_vectors.push(ScalarArithVector {
        name: "larger_values".to_string(),
        description: None,
        a_hex: hex::encode(a.as_bytes()),
        b_hex: hex::encode(b.as_bytes()),
        add_hex: hex::encode((a + b).as_bytes()),
        sub_hex: hex::encode((a - b).as_bytes()),
        mul_hex: hex::encode((a * b).as_bytes()),
    });

    // Scalar inversion tests
    let input_bytes: [u8; 32] = [
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let s = Scalar::from_bytes_mod_order(input_bytes);
    let inv = s.invert();
    scalar_invert_vectors.push(ScalarInvertVector {
        name: "invert_one".to_string(),
        description: Some("Inverse of 1 is 1".to_string()),
        input_hex: hex::encode(s.as_bytes()),
        inverted_hex: hex::encode(inv.as_bytes()),
    });

    let input_bytes: [u8; 32] = [
        0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let s = Scalar::from_bytes_mod_order(input_bytes);
    let inv = s.invert();
    scalar_invert_vectors.push(ScalarInvertVector {
        name: "invert_two".to_string(),
        description: Some("Inverse of 2".to_string()),
        input_hex: hex::encode(s.as_bytes()),
        inverted_hex: hex::encode(inv.as_bytes()),
    });

    let input_bytes: [u8; 32] = [
        0x42, 0x42, 0x42, 0x42, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let s = Scalar::from_bytes_mod_order(input_bytes);
    let inv = s.invert();
    scalar_invert_vectors.push(ScalarInvertVector {
        name: "invert_random".to_string(),
        description: None,
        input_hex: hex::encode(s.as_bytes()),
        inverted_hex: hex::encode(inv.as_bytes()),
    });

    // Scalar multiply base point tests
    let scalar_bytes: [u8; 32] = [
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let scalar = Scalar::from_bytes_mod_order(scalar_bytes);
    let point: RistrettoPoint = &scalar * &RISTRETTO_BASEPOINT_POINT;
    scalar_mul_base_vectors.push(ScalarMulBaseVector {
        name: "mul_base_one".to_string(),
        description: Some("1 * G = G (basepoint)".to_string()),
        scalar_hex: hex::encode(scalar.as_bytes()),
        point_hex: hex::encode(point.compress().as_bytes()),
    });

    let scalar_bytes: [u8; 32] = [
        0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let scalar = Scalar::from_bytes_mod_order(scalar_bytes);
    let point: RistrettoPoint = &scalar * &RISTRETTO_BASEPOINT_POINT;
    scalar_mul_base_vectors.push(ScalarMulBaseVector {
        name: "mul_base_two".to_string(),
        description: Some("2 * G".to_string()),
        scalar_hex: hex::encode(scalar.as_bytes()),
        point_hex: hex::encode(point.compress().as_bytes()),
    });

    // Random-ish scalar
    let scalar_bytes: [u8; 32] = [
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x09,
    ];
    let scalar = Scalar::from_bytes_mod_order(scalar_bytes);
    let point: RistrettoPoint = &scalar * &RISTRETTO_BASEPOINT_POINT;
    scalar_mul_base_vectors.push(ScalarMulBaseVector {
        name: "mul_base_random".to_string(),
        description: None,
        scalar_hex: hex::encode(scalar.as_bytes()),
        point_hex: hex::encode(point.compress().as_bytes()),
    });

    let test_file = Curve25519TestFile {
        algorithm: "Curve25519-Scalar".to_string(),
        scalar_size: 32,
        point_size: 32,
        scalar_reduce_vectors,
        scalar_arith_vectors,
        scalar_invert_vectors,
        scalar_mul_base_vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("curve25519.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to curve25519.yaml");
}
