// Generate Range Proofs test vectors (Bulletproofs)
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_rangeproofs_vectors

use bulletproofs::{BulletproofGens, PedersenGens, RangeProof};
use curve25519_dalek_ng::scalar::Scalar;
use rand::rngs::OsRng;
use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    // Value being proven (for reference, not part of proof)
    value: u64,
    bit_length: usize,
    // Pedersen commitment: V = v*G + blinding*H
    commitment_hex: String,
    // Range proof bytes (serialized)
    proof_hex: String,
    // Expected verification result
    should_verify: bool,
}

#[derive(Serialize)]
struct RangeProofsTestFile {
    algorithm: String,
    description: String,
    test_vectors: Vec<TestVector>,
}

fn main() {
    let mut vectors = Vec::new();

    // Generators
    let pc_gens = PedersenGens::default();
    let bp_gens = BulletproofGens::new(64, 1);

    // Test 1: Simple value (42)
    {
        let value = 42u64;
        let blinding = Scalar::random(&mut OsRng);

        let mut transcript = merlin::Transcript::new(b"RangeProofTest");
        let (proof, commitment) = RangeProof::prove_single(
            &bp_gens,
            &pc_gens,
            &mut transcript,
            value,
            &blinding,
            64,
        )
        .expect("proof creation failed");

        vectors.push(TestVector {
            name: "simple_42".to_string(),
            description: Some("Range proof for value 42".to_string()),
            value,
            bit_length: 64,
            commitment_hex: hex::encode(commitment.as_bytes()),
            proof_hex: hex::encode(proof.to_bytes()),
            should_verify: true,
        });
    }

    // Test 2: Zero value
    {
        let value = 0u64;
        let blinding = Scalar::random(&mut OsRng);

        let mut transcript = merlin::Transcript::new(b"RangeProofTest");
        let (proof, commitment) = RangeProof::prove_single(
            &bp_gens,
            &pc_gens,
            &mut transcript,
            value,
            &blinding,
            64,
        )
        .expect("proof creation failed");

        vectors.push(TestVector {
            name: "zero_value".to_string(),
            description: Some("Range proof for value 0".to_string()),
            value,
            bit_length: 64,
            commitment_hex: hex::encode(commitment.as_bytes()),
            proof_hex: hex::encode(proof.to_bytes()),
            should_verify: true,
        });
    }

    // Test 3: Maximum u64 value
    {
        let value = u64::MAX;
        let blinding = Scalar::random(&mut OsRng);

        let mut transcript = merlin::Transcript::new(b"RangeProofTest");
        let (proof, commitment) = RangeProof::prove_single(
            &bp_gens,
            &pc_gens,
            &mut transcript,
            value,
            &blinding,
            64,
        )
        .expect("proof creation failed");

        vectors.push(TestVector {
            name: "max_u64".to_string(),
            description: Some("Range proof for u64::MAX".to_string()),
            value,
            bit_length: 64,
            commitment_hex: hex::encode(commitment.as_bytes()),
            proof_hex: hex::encode(proof.to_bytes()),
            should_verify: true,
        });
    }

    // Test 4: Power of 2 value
    {
        let value = 1u64 << 32; // 2^32
        let blinding = Scalar::random(&mut OsRng);

        let mut transcript = merlin::Transcript::new(b"RangeProofTest");
        let (proof, commitment) = RangeProof::prove_single(
            &bp_gens,
            &pc_gens,
            &mut transcript,
            value,
            &blinding,
            64,
        )
        .expect("proof creation failed");

        vectors.push(TestVector {
            name: "power_of_2".to_string(),
            description: Some("Range proof for 2^32".to_string()),
            value,
            bit_length: 64,
            commitment_hex: hex::encode(commitment.as_bytes()),
            proof_hex: hex::encode(proof.to_bytes()),
            should_verify: true,
        });
    }

    // Test 5: Random large value
    {
        let value = 0xDEADBEEFCAFEBABEu64;
        let blinding = Scalar::random(&mut OsRng);

        let mut transcript = merlin::Transcript::new(b"RangeProofTest");
        let (proof, commitment) = RangeProof::prove_single(
            &bp_gens,
            &pc_gens,
            &mut transcript,
            value,
            &blinding,
            64,
        )
        .expect("proof creation failed");

        vectors.push(TestVector {
            name: "deadbeef".to_string(),
            description: Some("Range proof for 0xDEADBEEFCAFEBABE".to_string()),
            value,
            bit_length: 64,
            commitment_hex: hex::encode(commitment.as_bytes()),
            proof_hex: hex::encode(proof.to_bytes()),
            should_verify: true,
        });
    }

    // Test 6: 32-bit range (value fits in 32 bits)
    {
        let value = 1000000u64;
        let blinding = Scalar::random(&mut OsRng);
        let bp_gens_32 = BulletproofGens::new(32, 1);

        let mut transcript = merlin::Transcript::new(b"RangeProofTest");
        let (proof, commitment) = RangeProof::prove_single(
            &bp_gens_32,
            &pc_gens,
            &mut transcript,
            value,
            &blinding,
            32,
        )
        .expect("proof creation failed");

        vectors.push(TestVector {
            name: "bit32_million".to_string(),
            description: Some("32-bit range proof for 1000000".to_string()),
            value,
            bit_length: 32,
            commitment_hex: hex::encode(commitment.as_bytes()),
            proof_hex: hex::encode(proof.to_bytes()),
            should_verify: true,
        });
    }

    let test_file = RangeProofsTestFile {
        algorithm: "Bulletproofs".to_string(),
        description: "Range proofs using Bulletproofs protocol on Ristretto255".to_string(),
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("rangeproofs.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to rangeproofs.yaml");
}
