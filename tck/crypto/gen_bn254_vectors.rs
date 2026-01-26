// Generate BN254 test vectors (G1/G2 operations)
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_bn254_vectors

use ark_bn254::{Fr, G1Affine, G1Projective, G2Affine, G2Projective, Fq, Fq2};
use ark_ec::{AffineRepr, CurveGroup, Group};
use ark_ff::{Field, PrimeField, BigInteger};
use ark_serialize::{CanonicalSerialize, CanonicalDeserialize};
use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct G1AddVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    p1_x_hex: String,
    p1_y_hex: String,
    p2_x_hex: String,
    p2_y_hex: String,
    result_x_hex: String,
    result_y_hex: String,
}

#[derive(Serialize)]
struct G1MulVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    point_x_hex: String,
    point_y_hex: String,
    scalar_hex: String,
    result_x_hex: String,
    result_y_hex: String,
}

#[derive(Serialize)]
struct G1CompressVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    x_hex: String,
    y_hex: String,
    compressed_hex: String,
}

#[derive(Serialize)]
struct Bn254TestFile {
    algorithm: String,
    curve: String,
    g1_add_vectors: Vec<G1AddVector>,
    g1_mul_vectors: Vec<G1MulVector>,
    g1_compress_vectors: Vec<G1CompressVector>,
}

fn fq_to_be_hex(fq: &Fq) -> String {
    let bytes = fq.into_bigint().to_bytes_be();
    hex::encode(&bytes)
}

fn fr_to_be_hex(fr: &Fr) -> String {
    let bytes = fr.into_bigint().to_bytes_be();
    hex::encode(&bytes)
}

fn g1_to_uncompressed_be(p: &G1Affine) -> (String, String) {
    if p.is_zero() {
        // Point at infinity
        let zero = "0000000000000000000000000000000000000000000000000000000000000000";
        return (zero.to_string(), zero.to_string());
    }
    (fq_to_be_hex(&p.x), fq_to_be_hex(&p.y))
}

fn g1_to_compressed_be(p: &G1Affine) -> String {
    let mut bytes = Vec::new();
    p.serialize_compressed(&mut bytes).unwrap();
    // ark-serialize uses little-endian, we need big-endian
    bytes.reverse();
    hex::encode(&bytes)
}

fn main() {
    let mut g1_add_vectors = Vec::new();
    let mut g1_mul_vectors = Vec::new();
    let mut g1_compress_vectors = Vec::new();

    // G1 generator
    let g1_gen = G1Affine::generator();
    let (g1_x, g1_y) = g1_to_uncompressed_be(&g1_gen);

    // G1 Add: G + G = 2G
    {
        let g2 = (G1Projective::from(g1_gen) + G1Projective::from(g1_gen)).into_affine();
        let (r_x, r_y) = g1_to_uncompressed_be(&g2);

        g1_add_vectors.push(G1AddVector {
            name: "g1_double".to_string(),
            description: Some("G1 generator doubled: G + G = 2G".to_string()),
            p1_x_hex: g1_x.clone(),
            p1_y_hex: g1_y.clone(),
            p2_x_hex: g1_x.clone(),
            p2_y_hex: g1_y.clone(),
            result_x_hex: r_x,
            result_y_hex: r_y,
        });
    }

    // G1 Add: 2G + G = 3G
    {
        let g2 = (G1Projective::from(g1_gen) * Fr::from(2u64)).into_affine();
        let g3 = (G1Projective::from(g1_gen) * Fr::from(3u64)).into_affine();
        let (p2_x, p2_y) = g1_to_uncompressed_be(&g2);
        let (r_x, r_y) = g1_to_uncompressed_be(&g3);

        g1_add_vectors.push(G1AddVector {
            name: "g1_add_2g_g".to_string(),
            description: Some("2G + G = 3G".to_string()),
            p1_x_hex: p2_x,
            p1_y_hex: p2_y,
            p2_x_hex: g1_x.clone(),
            p2_y_hex: g1_y.clone(),
            result_x_hex: r_x,
            result_y_hex: r_y,
        });
    }

    // G1 Scalar Mul: 1 * G = G
    {
        let scalar = Fr::from(1u64);
        g1_mul_vectors.push(G1MulVector {
            name: "g1_mul_one".to_string(),
            description: Some("1 * G = G".to_string()),
            point_x_hex: g1_x.clone(),
            point_y_hex: g1_y.clone(),
            scalar_hex: fr_to_be_hex(&scalar),
            result_x_hex: g1_x.clone(),
            result_y_hex: g1_y.clone(),
        });
    }

    // G1 Scalar Mul: 2 * G
    {
        let scalar = Fr::from(2u64);
        let result = (G1Projective::from(g1_gen) * scalar).into_affine();
        let (r_x, r_y) = g1_to_uncompressed_be(&result);

        g1_mul_vectors.push(G1MulVector {
            name: "g1_mul_two".to_string(),
            description: Some("2 * G".to_string()),
            point_x_hex: g1_x.clone(),
            point_y_hex: g1_y.clone(),
            scalar_hex: fr_to_be_hex(&scalar),
            result_x_hex: r_x,
            result_y_hex: r_y,
        });
    }

    // G1 Scalar Mul: 42 * G
    {
        let scalar = Fr::from(42u64);
        let result = (G1Projective::from(g1_gen) * scalar).into_affine();
        let (r_x, r_y) = g1_to_uncompressed_be(&result);

        g1_mul_vectors.push(G1MulVector {
            name: "g1_mul_42".to_string(),
            description: Some("42 * G".to_string()),
            point_x_hex: g1_x.clone(),
            point_y_hex: g1_y.clone(),
            scalar_hex: fr_to_be_hex(&scalar),
            result_x_hex: r_x,
            result_y_hex: r_y,
        });
    }

    // G1 Scalar Mul: large scalar
    {
        let scalar = Fr::from(0xDEADBEEFCAFEBABEu64);
        let result = (G1Projective::from(g1_gen) * scalar).into_affine();
        let (r_x, r_y) = g1_to_uncompressed_be(&result);

        g1_mul_vectors.push(G1MulVector {
            name: "g1_mul_large".to_string(),
            description: Some("0xDEADBEEFCAFEBABE * G".to_string()),
            point_x_hex: g1_x.clone(),
            point_y_hex: g1_y.clone(),
            scalar_hex: fr_to_be_hex(&scalar),
            result_x_hex: r_x,
            result_y_hex: r_y,
        });
    }

    // G1 Compress: generator
    {
        let compressed = g1_to_compressed_be(&g1_gen);
        g1_compress_vectors.push(G1CompressVector {
            name: "g1_compress_gen".to_string(),
            description: Some("Compress G1 generator".to_string()),
            x_hex: g1_x.clone(),
            y_hex: g1_y.clone(),
            compressed_hex: compressed,
        });
    }

    // G1 Compress: 2G
    {
        let p = (G1Projective::from(g1_gen) * Fr::from(2u64)).into_affine();
        let (x, y) = g1_to_uncompressed_be(&p);
        let compressed = g1_to_compressed_be(&p);
        g1_compress_vectors.push(G1CompressVector {
            name: "g1_compress_2g".to_string(),
            description: Some("Compress 2*G".to_string()),
            x_hex: x,
            y_hex: y,
            compressed_hex: compressed,
        });
    }

    let test_file = Bn254TestFile {
        algorithm: "BN254".to_string(),
        curve: "alt_bn128".to_string(),
        g1_add_vectors,
        g1_mul_vectors,
        g1_compress_vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("bn254.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to bn254.yaml");
}
