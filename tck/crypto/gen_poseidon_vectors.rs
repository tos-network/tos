// Generate Poseidon test vectors (BN254 scalar field)
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_poseidon_vectors

use light_poseidon::{Poseidon, PoseidonHasher};
use ark_bn254::Fr;
use ark_ff::{PrimeField, BigInteger};
use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    // Concatenated inputs (each 32 bytes in little-endian)
    inputs_hex: String,
    num_inputs: usize,
    expected_hex: String,
    big_endian: bool,
}

#[derive(Serialize)]
struct PoseidonTestFile {
    algorithm: String,
    field: String,
    input_size: usize,
    max_inputs: usize,
    test_vectors: Vec<TestVector>,
}

fn fr_to_le_hex(fr: &Fr) -> String {
    let bytes = fr.into_bigint().to_bytes_le();
    hex::encode(&bytes)
}

fn fr_to_be_hex(fr: &Fr) -> String {
    let bytes = fr.into_bigint().to_bytes_be();
    hex::encode(&bytes)
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: Single input (1)
    {
        let mut poseidon = Poseidon::<Fr>::new_circom(1).unwrap();
        let input = Fr::from(1u64);
        let hash = poseidon.hash(&[input]).unwrap();

        vectors.push(TestVector {
            name: "single_one".to_string(),
            description: Some("Hash of single element 1".to_string()),
            inputs_hex: fr_to_le_hex(&input),
            num_inputs: 1,
            expected_hex: fr_to_le_hex(&hash),
            big_endian: false,
        });
    }

    // Test 2: Two inputs
    {
        let mut poseidon = Poseidon::<Fr>::new_circom(2).unwrap();
        let input1 = Fr::from(1u64);
        let input2 = Fr::from(2u64);
        let hash = poseidon.hash(&[input1, input2]).unwrap();

        vectors.push(TestVector {
            name: "two_inputs_1_2".to_string(),
            description: Some("Hash of [1, 2]".to_string()),
            inputs_hex: format!("{}{}", fr_to_le_hex(&input1), fr_to_le_hex(&input2)),
            num_inputs: 2,
            expected_hex: fr_to_le_hex(&hash),
            big_endian: false,
        });
    }

    // Test 3: Three inputs
    {
        let mut poseidon = Poseidon::<Fr>::new_circom(3).unwrap();
        let input1 = Fr::from(0x42u64);
        let input2 = Fr::from(0x43u64);
        let input3 = Fr::from(0x44u64);
        let hash = poseidon.hash(&[input1, input2, input3]).unwrap();

        vectors.push(TestVector {
            name: "three_inputs".to_string(),
            description: Some("Hash of [0x42, 0x43, 0x44]".to_string()),
            inputs_hex: format!("{}{}{}", fr_to_le_hex(&input1), fr_to_le_hex(&input2), fr_to_le_hex(&input3)),
            num_inputs: 3,
            expected_hex: fr_to_le_hex(&hash),
            big_endian: false,
        });
    }

    // Test 4: Zero input
    {
        let mut poseidon = Poseidon::<Fr>::new_circom(1).unwrap();
        let input = Fr::from(0u64);
        let hash = poseidon.hash(&[input]).unwrap();

        vectors.push(TestVector {
            name: "single_zero".to_string(),
            description: Some("Hash of single element 0".to_string()),
            inputs_hex: fr_to_le_hex(&input),
            num_inputs: 1,
            expected_hex: fr_to_le_hex(&hash),
            big_endian: false,
        });
    }

    // Test 5: Large value
    {
        let mut poseidon = Poseidon::<Fr>::new_circom(1).unwrap();
        let input = Fr::from(0xDEADBEEFCAFEBABEu64);
        let hash = poseidon.hash(&[input]).unwrap();

        vectors.push(TestVector {
            name: "large_value".to_string(),
            description: Some("Hash of 0xDEADBEEFCAFEBABE".to_string()),
            inputs_hex: fr_to_le_hex(&input),
            num_inputs: 1,
            expected_hex: fr_to_le_hex(&hash),
            big_endian: false,
        });
    }

    // Test 6: Two zeros
    {
        let mut poseidon = Poseidon::<Fr>::new_circom(2).unwrap();
        let input1 = Fr::from(0u64);
        let input2 = Fr::from(0u64);
        let hash = poseidon.hash(&[input1, input2]).unwrap();

        vectors.push(TestVector {
            name: "two_zeros".to_string(),
            description: Some("Hash of [0, 0]".to_string()),
            inputs_hex: format!("{}{}", fr_to_le_hex(&input1), fr_to_le_hex(&input2)),
            num_inputs: 2,
            expected_hex: fr_to_le_hex(&hash),
            big_endian: false,
        });
    }

    // Test 7: Big endian format
    {
        let mut poseidon = Poseidon::<Fr>::new_circom(2).unwrap();
        let input1 = Fr::from(1u64);
        let input2 = Fr::from(2u64);
        let hash = poseidon.hash(&[input1, input2]).unwrap();

        vectors.push(TestVector {
            name: "two_inputs_be".to_string(),
            description: Some("Hash of [1, 2] in big endian".to_string()),
            inputs_hex: format!("{}{}", fr_to_be_hex(&input1), fr_to_be_hex(&input2)),
            num_inputs: 2,
            expected_hex: fr_to_be_hex(&hash),
            big_endian: true,
        });
    }

    // Test 8: Four inputs
    {
        let mut poseidon = Poseidon::<Fr>::new_circom(4).unwrap();
        let inputs: Vec<Fr> = (1..=4).map(|i| Fr::from(i as u64)).collect();
        let hash = poseidon.hash(&inputs).unwrap();

        let inputs_hex: String = inputs.iter().map(fr_to_le_hex).collect();
        vectors.push(TestVector {
            name: "four_inputs".to_string(),
            description: Some("Hash of [1, 2, 3, 4]".to_string()),
            inputs_hex,
            num_inputs: 4,
            expected_hex: fr_to_le_hex(&hash),
            big_endian: false,
        });
    }

    let test_file = PoseidonTestFile {
        algorithm: "Poseidon".to_string(),
        field: "BN254 scalar field".to_string(),
        input_size: 32,
        max_inputs: 12,
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("poseidon.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to poseidon.yaml");
}
