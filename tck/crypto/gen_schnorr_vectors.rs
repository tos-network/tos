// Generate TOS Schnorr signature test vectors
// Run: cd /Users/tomisetsu/tos/tck/crypto && cargo run --release --bin gen_schnorr_vectors
//
// This generates test vectors using the same libraries as TOS:
// - bulletproofs (which uses curve25519-dalek-ng internally)
// - sha3 for SHA3-512

use bulletproofs::PedersenGens;
use curve25519_dalek_ng::{ristretto::RistrettoPoint, scalar::Scalar};
use serde::Serialize;
use sha3::{Digest, Sha3_512};
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    private_key_hex: String,
    public_key_hex: String,
    message_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_ascii: Option<String>,
    // For deterministic testing, we include the k value used
    k_hex: String,
    signature_s_hex: String,
    signature_e_hex: String,
}

#[derive(Serialize)]
struct GeneratorInfo {
    name: String,
    description: String,
    compressed_hex: String,
}

#[derive(Serialize)]
struct SchnorrTestFile {
    algorithm: String,
    curve: String,
    hash: String,
    signature_size: usize,
    generators: Vec<GeneratorInfo>,
    test_vectors: Vec<TestVector>,
}

fn hash_and_point_to_scalar(
    pubkey_compressed: &[u8; 32],
    message: &[u8],
    r_compressed: &[u8; 32],
) -> Scalar {
    let mut hasher = Sha3_512::new();
    hasher.update(pubkey_compressed);
    hasher.update(message);
    hasher.update(r_compressed);
    let hash = hasher.finalize();
    let hash_bytes: [u8; 64] = hash.into();
    Scalar::from_bytes_mod_order_wide(&hash_bytes)
}

fn sign_deterministic(
    private_key: &Scalar,
    public_key: &RistrettoPoint,
    message: &[u8],
    k: &Scalar,
    h: &RistrettoPoint,
) -> (Scalar, Scalar) {
    let r = k * h;
    let pubkey_compressed = public_key.compress().to_bytes();
    let r_compressed = r.compress().to_bytes();
    let e = hash_and_point_to_scalar(&pubkey_compressed, message, &r_compressed);
    let s = private_key.invert() * e + k;
    (s, e)
}

fn main() {
    let pc_gens = PedersenGens::default();
    let g = pc_gens.B;
    let h = pc_gens.B_blinding;

    let mut generators = Vec::new();
    generators.push(GeneratorInfo {
        name: "G".to_string(),
        description: "Pedersen commitment base point (B)".to_string(),
        compressed_hex: hex::encode(g.compress().to_bytes()),
    });
    generators.push(GeneratorInfo {
        name: "H".to_string(),
        description: "Pedersen blinding generator (B_blinding) - used for TOS signatures".to_string(),
        compressed_hex: hex::encode(h.compress().to_bytes()),
    });

    let mut vectors = Vec::new();

    // Test 1: Simple "Hello, world!" message with known private key
    {
        // Use a deterministic private key for reproducible tests
        let priv_bytes: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
            0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x00,
        ];
        let private_key = Scalar::from_bytes_mod_order(priv_bytes);
        let public_key = private_key.invert() * h;

        // Deterministic k for reproducible signature
        let k_bytes: [u8; 32] = [
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11,
            0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
            0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x00,
        ];
        let k = Scalar::from_bytes_mod_order(k_bytes);

        let message = b"Hello, world!";
        let (s, e) = sign_deterministic(&private_key, &public_key, message, &k, &h);

        vectors.push(TestVector {
            name: "hello_world".to_string(),
            description: Some("Standard test message with deterministic keys".to_string()),
            private_key_hex: hex::encode(private_key.as_bytes()),
            public_key_hex: hex::encode(public_key.compress().to_bytes()),
            message_hex: hex::encode(message),
            message_ascii: Some("Hello, world!".to_string()),
            k_hex: hex::encode(k.as_bytes()),
            signature_s_hex: hex::encode(s.as_bytes()),
            signature_e_hex: hex::encode(e.as_bytes()),
        });
    }

    // Test 2: Empty message
    {
        let priv_bytes: [u8; 32] = [
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x01,
        ];
        let private_key = Scalar::from_bytes_mod_order(priv_bytes);
        let public_key = private_key.invert() * h;

        let k_bytes: [u8; 32] = [
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
            0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x02,
        ];
        let k = Scalar::from_bytes_mod_order(k_bytes);

        let message = b"";
        let (s, e) = sign_deterministic(&private_key, &public_key, message, &k, &h);

        vectors.push(TestVector {
            name: "empty_message".to_string(),
            description: Some("Empty message test".to_string()),
            private_key_hex: hex::encode(private_key.as_bytes()),
            public_key_hex: hex::encode(public_key.compress().to_bytes()),
            message_hex: "".to_string(),
            message_ascii: Some("".to_string()),
            k_hex: hex::encode(k.as_bytes()),
            signature_s_hex: hex::encode(s.as_bytes()),
            signature_e_hex: hex::encode(e.as_bytes()),
        });
    }

    // Test 3: 64-byte message (hash output size)
    {
        let priv_bytes: [u8; 32] = [
            0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
            0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
            0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
            0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x03,
        ];
        let private_key = Scalar::from_bytes_mod_order(priv_bytes);
        let public_key = private_key.invert() * h;

        let k_bytes: [u8; 32] = [
            0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44,
            0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44,
            0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44,
            0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x04,
        ];
        let k = Scalar::from_bytes_mod_order(k_bytes);

        let message = [0x55u8; 64];
        let (s, e) = sign_deterministic(&private_key, &public_key, &message, &k, &h);

        vectors.push(TestVector {
            name: "64_bytes_0x55".to_string(),
            description: Some("64-byte message (hash output size)".to_string()),
            private_key_hex: hex::encode(private_key.as_bytes()),
            public_key_hex: hex::encode(public_key.compress().to_bytes()),
            message_hex: hex::encode(&message),
            message_ascii: None,
            k_hex: hex::encode(k.as_bytes()),
            signature_s_hex: hex::encode(s.as_bytes()),
            signature_e_hex: hex::encode(e.as_bytes()),
        });
    }

    // Test 4: Binary message with zeros
    {
        let priv_bytes: [u8; 32] = [
            0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
            0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
            0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77,
            0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x77, 0x07,
        ];
        let private_key = Scalar::from_bytes_mod_order(priv_bytes);
        let public_key = private_key.invert() * h;

        let k_bytes: [u8; 32] = [
            0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88,
            0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88,
            0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88,
            0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x08,
        ];
        let k = Scalar::from_bytes_mod_order(k_bytes);

        let message = [0x00u8; 32];
        let (s, e) = sign_deterministic(&private_key, &public_key, &message, &k, &h);

        vectors.push(TestVector {
            name: "32_zeros".to_string(),
            description: Some("32 zero bytes message".to_string()),
            private_key_hex: hex::encode(private_key.as_bytes()),
            public_key_hex: hex::encode(public_key.compress().to_bytes()),
            message_hex: hex::encode(&message),
            message_ascii: None,
            k_hex: hex::encode(k.as_bytes()),
            signature_s_hex: hex::encode(s.as_bytes()),
            signature_e_hex: hex::encode(e.as_bytes()),
        });
    }

    let test_file = SchnorrTestFile {
        algorithm: "TOS-Schnorr".to_string(),
        curve: "Ristretto255".to_string(),
        hash: "SHA3-512".to_string(),
        signature_size: 64,
        generators,
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("schnorr.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to schnorr.yaml");
}
