// Generate BigInt (uint256) test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_bigint_vectors

use num_bigint::BigUint;
use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct ArithVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    a_hex: String,
    b_hex: String,
    add_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sub_hex: Option<String>,
    mul_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    div_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mod_hex: Option<String>,
}

#[derive(Serialize)]
struct CompareVector {
    name: String,
    a_hex: String,
    b_hex: String,
    a_lt_b: bool,
    a_eq_b: bool,
    a_gt_b: bool,
}

#[derive(Serialize)]
struct ShiftVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_hex: String,
    shift: u32,
    left_hex: String,
    right_hex: String,
}

#[derive(Serialize)]
struct BigIntTestFile {
    algorithm: String,
    word_size: usize,
    arith_vectors: Vec<ArithVector>,
    compare_vectors: Vec<CompareVector>,
    shift_vectors: Vec<ShiftVector>,
}

fn to_hex_32(n: &BigUint) -> String {
    let bytes = n.to_bytes_le();
    let mut result = vec![0u8; 32];
    let len = bytes.len().min(32);
    result[..len].copy_from_slice(&bytes[..len]);
    hex::encode(&result)
}

fn main() {
    let mut arith_vectors = Vec::new();
    let mut compare_vectors = Vec::new();
    let mut shift_vectors = Vec::new();

    // Arithmetic test 1: Simple addition
    let a = BigUint::from(1u64);
    let b = BigUint::from(2u64);
    arith_vectors.push(ArithVector {
        name: "simple_1_2".to_string(),
        description: Some("1 + 2 = 3".to_string()),
        a_hex: to_hex_32(&a),
        b_hex: to_hex_32(&b),
        add_hex: to_hex_32(&(&a + &b)),
        sub_hex: None, // 1 - 2 would underflow
        mul_hex: to_hex_32(&(&a * &b)),
        div_hex: Some(to_hex_32(&(&a / &b))),
        mod_hex: Some(to_hex_32(&(&a % &b))),
    });

    // Arithmetic test 2: Larger values
    let a = BigUint::from(0xDEADBEEFu64);
    let b = BigUint::from(0xCAFEBABEu64);
    arith_vectors.push(ArithVector {
        name: "deadbeef_cafebabe".to_string(),
        description: None,
        a_hex: to_hex_32(&a),
        b_hex: to_hex_32(&b),
        add_hex: to_hex_32(&(&a + &b)),
        sub_hex: Some(to_hex_32(&(&a - &b))),
        mul_hex: to_hex_32(&(&a * &b)),
        div_hex: Some(to_hex_32(&(&a / &b))),
        mod_hex: Some(to_hex_32(&(&a % &b))),
    });

    // Arithmetic test 3: 128-bit values
    let a = BigUint::parse_bytes(b"ffffffffffffffffffffffffffffffff", 16).unwrap();
    let b = BigUint::from(1u64);
    arith_vectors.push(ArithVector {
        name: "max_128bit_plus_one".to_string(),
        description: Some("2^128 - 1 + 1".to_string()),
        a_hex: to_hex_32(&a),
        b_hex: to_hex_32(&b),
        add_hex: to_hex_32(&(&a + &b)),
        sub_hex: Some(to_hex_32(&(&a - &b))),
        mul_hex: to_hex_32(&(&a * &b)),
        div_hex: Some(to_hex_32(&(&a / &b))),
        mod_hex: Some(to_hex_32(&(&a % &b))),
    });

    // Arithmetic test 4: 256-bit multiplication (may overflow)
    let a = BigUint::parse_bytes(b"0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20", 16).unwrap();
    let b = BigUint::from(2u64);
    arith_vectors.push(ArithVector {
        name: "256bit_mul_2".to_string(),
        description: Some("Large 256-bit value * 2".to_string()),
        a_hex: to_hex_32(&a),
        b_hex: to_hex_32(&b),
        add_hex: to_hex_32(&(&a + &b)),
        sub_hex: Some(to_hex_32(&(&a - &b))),
        mul_hex: to_hex_32(&(&a * &b)),
        div_hex: Some(to_hex_32(&(&a / &b))),
        mod_hex: Some(to_hex_32(&(&a % &b))),
    });

    // Arithmetic test 5: Division and modulo
    let a = BigUint::from(100u64);
    let b = BigUint::from(7u64);
    arith_vectors.push(ArithVector {
        name: "div_mod_100_7".to_string(),
        description: Some("100 / 7 = 14 rem 2".to_string()),
        a_hex: to_hex_32(&a),
        b_hex: to_hex_32(&b),
        add_hex: to_hex_32(&(&a + &b)),
        sub_hex: Some(to_hex_32(&(&a - &b))),
        mul_hex: to_hex_32(&(&a * &b)),
        div_hex: Some(to_hex_32(&(&a / &b))),
        mod_hex: Some(to_hex_32(&(&a % &b))),
    });

    // Compare test 1
    let a = BigUint::from(100u64);
    let b = BigUint::from(200u64);
    compare_vectors.push(CompareVector {
        name: "100_vs_200".to_string(),
        a_hex: to_hex_32(&a),
        b_hex: to_hex_32(&b),
        a_lt_b: a < b,
        a_eq_b: a == b,
        a_gt_b: a > b,
    });

    // Compare test 2: Equal
    let a = BigUint::from(42u64);
    let b = BigUint::from(42u64);
    compare_vectors.push(CompareVector {
        name: "equal_42".to_string(),
        a_hex: to_hex_32(&a),
        b_hex: to_hex_32(&b),
        a_lt_b: a < b,
        a_eq_b: a == b,
        a_gt_b: a > b,
    });

    // Compare test 3: Large values
    let a = BigUint::parse_bytes(b"fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe", 16).unwrap();
    let b = BigUint::parse_bytes(b"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", 16).unwrap();
    compare_vectors.push(CompareVector {
        name: "max_minus_one_vs_max".to_string(),
        a_hex: to_hex_32(&a),
        b_hex: to_hex_32(&b),
        a_lt_b: a < b,
        a_eq_b: a == b,
        a_gt_b: a > b,
    });

    // Shift test 1: Simple shift
    let input = BigUint::from(1u64);
    shift_vectors.push(ShiftVector {
        name: "shift_1_by_1".to_string(),
        description: Some("1 << 1 = 2, 1 >> 1 = 0".to_string()),
        input_hex: to_hex_32(&input),
        shift: 1,
        left_hex: to_hex_32(&(&input << 1u32)),
        right_hex: to_hex_32(&(&input >> 1u32)),
    });

    // Shift test 2: Shift by 64
    let input = BigUint::from(1u64);
    shift_vectors.push(ShiftVector {
        name: "shift_1_by_64".to_string(),
        description: Some("1 << 64".to_string()),
        input_hex: to_hex_32(&input),
        shift: 64,
        left_hex: to_hex_32(&(&input << 64u32)),
        right_hex: to_hex_32(&(&input >> 64u32)),
    });

    // Shift test 3: Shift larger value
    let input = BigUint::from(0xDEADBEEFCAFEBABEu64);
    shift_vectors.push(ShiftVector {
        name: "shift_large_by_8".to_string(),
        description: None,
        input_hex: to_hex_32(&input),
        shift: 8,
        left_hex: to_hex_32(&(&input << 8u32)),
        right_hex: to_hex_32(&(&input >> 8u32)),
    });

    // Shift test 4: 256-bit value shift
    let input = BigUint::parse_bytes(b"8000000000000000000000000000000000000000000000000000000000000000", 16).unwrap();
    shift_vectors.push(ShiftVector {
        name: "shift_highbit_by_1".to_string(),
        description: Some("Highest bit set, shift by 1".to_string()),
        input_hex: to_hex_32(&input),
        shift: 1,
        left_hex: to_hex_32(&(&input << 1u32)),
        right_hex: to_hex_32(&(&input >> 1u32)),
    });

    let test_file = BigIntTestFile {
        algorithm: "uint256".to_string(),
        word_size: 32,
        arith_vectors,
        compare_vectors,
        shift_vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("bigint.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to bigint.yaml");
}
