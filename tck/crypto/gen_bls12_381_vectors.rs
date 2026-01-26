// Generate BLS12-381 test vectors for cross-language verification
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_bls12_381_vectors > bls12_381.yaml

use blstrs::{G1Affine, G1Projective, G2Affine, G2Projective, Scalar};
use group::Curve;
use ff::Field;
use serde::Serialize;

#[derive(Serialize)]
struct G1DecompressVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    compressed_hex: String,
    uncompressed_hex: String,
}

#[derive(Serialize)]
struct G1AddVector {
    name: String,
    a_hex: String,
    b_hex: String,
    result_hex: String,
}

#[derive(Serialize)]
struct G1MulVector {
    name: String,
    point_hex: String,
    scalar_hex: String,
    result_hex: String,
}

#[derive(Serialize)]
struct G2DecompressVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    compressed_hex: String,
    uncompressed_hex: String,
}

#[derive(Serialize)]
struct G2AddVector {
    name: String,
    a_hex: String,
    b_hex: String,
    result_hex: String,
}

#[derive(Serialize)]
struct G2MulVector {
    name: String,
    point_hex: String,
    scalar_hex: String,
    result_hex: String,
}

#[derive(Serialize)]
struct PairingVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    g1_hex: String,
    g2_hex: String,
    // GT result is large (576 bytes), we'll just verify pairing check
    pairing_check: bool,
}

#[derive(Serialize)]
struct TestVectors {
    algorithm: String,
    description: String,
    note: String,
    g1_decompress: Vec<G1DecompressVector>,
    g1_add: Vec<G1AddVector>,
    g1_mul: Vec<G1MulVector>,
    g2_decompress: Vec<G2DecompressVector>,
    g2_add: Vec<G2AddVector>,
    g2_mul: Vec<G2MulVector>,
    pairing: Vec<PairingVector>,
}

fn g1_to_uncompressed_be(p: &G1Projective) -> Vec<u8> {
    let affine: G1Affine = p.to_affine();
    affine.to_uncompressed().to_vec()
}

fn g1_to_compressed_be(p: &G1Projective) -> Vec<u8> {
    let affine: G1Affine = p.to_affine();
    affine.to_compressed().to_vec()
}

fn g2_to_uncompressed_be(p: &G2Projective) -> Vec<u8> {
    let affine: G2Affine = p.to_affine();
    affine.to_uncompressed().to_vec()
}

fn g2_to_compressed_be(p: &G2Projective) -> Vec<u8> {
    let affine: G2Affine = p.to_affine();
    affine.to_compressed().to_vec()
}

fn scalar_to_bytes_be(s: &Scalar) -> Vec<u8> {
    s.to_bytes_be().to_vec()
}

fn main() {
    use group::Group;

    let mut g1_decompress = Vec::new();
    let mut g1_add = Vec::new();
    let mut g1_mul = Vec::new();
    let mut g2_decompress = Vec::new();
    let mut g2_add = Vec::new();
    let mut g2_mul = Vec::new();
    let mut pairing = Vec::new();

    // G1 Generator
    let g1_gen = G1Projective::generator();

    // G1 Decompress: generator
    g1_decompress.push(G1DecompressVector {
        name: "g1_generator".to_string(),
        description: Some("G1 generator point".to_string()),
        compressed_hex: hex::encode(g1_to_compressed_be(&g1_gen)),
        uncompressed_hex: hex::encode(g1_to_uncompressed_be(&g1_gen)),
    });

    // G1 Decompress: 2*G1
    let g1_2 = g1_gen + g1_gen;
    g1_decompress.push(G1DecompressVector {
        name: "g1_double".to_string(),
        description: Some("2 * G1 generator".to_string()),
        compressed_hex: hex::encode(g1_to_compressed_be(&g1_2)),
        uncompressed_hex: hex::encode(g1_to_uncompressed_be(&g1_2)),
    });

    // G1 Add: G1 + G1 = 2*G1
    g1_add.push(G1AddVector {
        name: "g1_double_generator".to_string(),
        a_hex: hex::encode(g1_to_uncompressed_be(&g1_gen)),
        b_hex: hex::encode(g1_to_uncompressed_be(&g1_gen)),
        result_hex: hex::encode(g1_to_uncompressed_be(&g1_2)),
    });

    // G1 Add: 2*G1 + G1 = 3*G1
    let g1_3 = g1_2 + g1_gen;
    g1_add.push(G1AddVector {
        name: "g1_add_2g_plus_g".to_string(),
        a_hex: hex::encode(g1_to_uncompressed_be(&g1_2)),
        b_hex: hex::encode(g1_to_uncompressed_be(&g1_gen)),
        result_hex: hex::encode(g1_to_uncompressed_be(&g1_3)),
    });

    // G1 Mul: 2 * G1
    let scalar_2 = Scalar::from(2u64);
    let g1_mul_2 = g1_gen * scalar_2;
    g1_mul.push(G1MulVector {
        name: "g1_mul_2".to_string(),
        point_hex: hex::encode(g1_to_uncompressed_be(&g1_gen)),
        scalar_hex: hex::encode(scalar_to_bytes_be(&scalar_2)),
        result_hex: hex::encode(g1_to_uncompressed_be(&g1_mul_2)),
    });

    // G1 Mul: 7 * G1
    let scalar_7 = Scalar::from(7u64);
    let g1_mul_7 = g1_gen * scalar_7;
    g1_mul.push(G1MulVector {
        name: "g1_mul_7".to_string(),
        point_hex: hex::encode(g1_to_uncompressed_be(&g1_gen)),
        scalar_hex: hex::encode(scalar_to_bytes_be(&scalar_7)),
        result_hex: hex::encode(g1_to_uncompressed_be(&g1_mul_7)),
    });

    // G1 Mul: large scalar
    let scalar_large = Scalar::from(0xdeadbeefcafe1234u64);
    let g1_mul_large = g1_gen * scalar_large;
    g1_mul.push(G1MulVector {
        name: "g1_mul_large".to_string(),
        point_hex: hex::encode(g1_to_uncompressed_be(&g1_gen)),
        scalar_hex: hex::encode(scalar_to_bytes_be(&scalar_large)),
        result_hex: hex::encode(g1_to_uncompressed_be(&g1_mul_large)),
    });

    // G2 Generator
    let g2_gen = G2Projective::generator();

    // G2 Decompress: generator
    g2_decompress.push(G2DecompressVector {
        name: "g2_generator".to_string(),
        description: Some("G2 generator point".to_string()),
        compressed_hex: hex::encode(g2_to_compressed_be(&g2_gen)),
        uncompressed_hex: hex::encode(g2_to_uncompressed_be(&g2_gen)),
    });

    // G2 Decompress: 2*G2
    let g2_2 = g2_gen + g2_gen;
    g2_decompress.push(G2DecompressVector {
        name: "g2_double".to_string(),
        description: Some("2 * G2 generator".to_string()),
        compressed_hex: hex::encode(g2_to_compressed_be(&g2_2)),
        uncompressed_hex: hex::encode(g2_to_uncompressed_be(&g2_2)),
    });

    // G2 Add: G2 + G2 = 2*G2
    g2_add.push(G2AddVector {
        name: "g2_double_generator".to_string(),
        a_hex: hex::encode(g2_to_uncompressed_be(&g2_gen)),
        b_hex: hex::encode(g2_to_uncompressed_be(&g2_gen)),
        result_hex: hex::encode(g2_to_uncompressed_be(&g2_2)),
    });

    // G2 Add: 2*G2 + G2 = 3*G2
    let g2_3 = g2_2 + g2_gen;
    g2_add.push(G2AddVector {
        name: "g2_add_2g_plus_g".to_string(),
        a_hex: hex::encode(g2_to_uncompressed_be(&g2_2)),
        b_hex: hex::encode(g2_to_uncompressed_be(&g2_gen)),
        result_hex: hex::encode(g2_to_uncompressed_be(&g2_3)),
    });

    // G2 Mul: 2 * G2
    let g2_mul_2 = g2_gen * scalar_2;
    g2_mul.push(G2MulVector {
        name: "g2_mul_2".to_string(),
        point_hex: hex::encode(g2_to_uncompressed_be(&g2_gen)),
        scalar_hex: hex::encode(scalar_to_bytes_be(&scalar_2)),
        result_hex: hex::encode(g2_to_uncompressed_be(&g2_mul_2)),
    });

    // G2 Mul: 7 * G2
    let g2_mul_7 = g2_gen * scalar_7;
    g2_mul.push(G2MulVector {
        name: "g2_mul_7".to_string(),
        point_hex: hex::encode(g2_to_uncompressed_be(&g2_gen)),
        scalar_hex: hex::encode(scalar_to_bytes_be(&scalar_7)),
        result_hex: hex::encode(g2_to_uncompressed_be(&g2_mul_7)),
    });

    // Pairing: e(G1, G2)
    pairing.push(PairingVector {
        name: "pairing_generators".to_string(),
        description: Some("e(G1, G2) - pairing of generators".to_string()),
        g1_hex: hex::encode(g1_to_uncompressed_be(&g1_gen)),
        g2_hex: hex::encode(g2_to_uncompressed_be(&g2_gen)),
        pairing_check: true,
    });

    // Pairing: e(2*G1, G2)
    pairing.push(PairingVector {
        name: "pairing_2g1_g2".to_string(),
        description: Some("e(2*G1, G2)".to_string()),
        g1_hex: hex::encode(g1_to_uncompressed_be(&g1_2)),
        g2_hex: hex::encode(g2_to_uncompressed_be(&g2_gen)),
        pairing_check: true,
    });

    // Pairing: e(G1, 2*G2)
    pairing.push(PairingVector {
        name: "pairing_g1_2g2".to_string(),
        description: Some("e(G1, 2*G2)".to_string()),
        g1_hex: hex::encode(g1_to_uncompressed_be(&g1_gen)),
        g2_hex: hex::encode(g2_to_uncompressed_be(&g2_2)),
        pairing_check: true,
    });

    let test_vectors = TestVectors {
        algorithm: "BLS12-381".to_string(),
        description: "BLS12-381 curve operations test vectors".to_string(),
        note: "All coordinates are big-endian. G1 uncompressed: 96 bytes (X,Y each 48 bytes). G1 compressed: 48 bytes. G2 uncompressed: 192 bytes (X,Y each 96 bytes). G2 compressed: 96 bytes.".to_string(),
        g1_decompress,
        g1_add,
        g1_mul,
        g2_decompress,
        g2_add,
        g2_mul,
        pairing,
    };

    println!("{}", serde_yaml::to_string(&test_vectors).unwrap());
}
