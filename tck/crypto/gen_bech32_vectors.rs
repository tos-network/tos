// Generate Bech32 test vectors for TOS addresses
// Run: cd /Users/tomisetsu/tos/tck/crypto && cargo run --release --bin gen_bech32_vectors

use serde::Serialize;
use std::fs::File;
use std::io::Write;

/// Bech32 constants
const CHARSET: &str = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
const GENERATOR: [u32; 5] = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];

fn polymod(values: &[u8]) -> u32 {
    let mut chk: u32 = 1;
    for value in values {
        let top = chk >> 25;
        chk = (chk & 0x1ffffff) << 5 ^ *value as u32;
        for (i, item) in GENERATOR.iter().enumerate() {
            if (top >> i) & 1 == 1 {
                chk ^= item;
            }
        }
    }
    chk
}

fn hrp_expand(hrp: &str) -> Vec<u8> {
    let mut result: Vec<u8> = Vec::new();
    for c in hrp.bytes() {
        result.push(c >> 5);
    }
    result.push(0);
    for c in hrp.bytes() {
        result.push(c & 31);
    }
    result
}

fn create_checksum(hrp: &str, data: &[u8]) -> [u8; 6] {
    let mut values: Vec<u8> = Vec::new();
    values.extend(hrp_expand(hrp));
    values.extend(data);
    let mut result: [u8; 6] = [0; 6];
    values.extend(&result);
    let polymod = polymod(&values) ^ 1;
    for (i, byte) in result.iter_mut().enumerate() {
        *byte = (polymod >> (5 * (5 - i)) & 31) as u8;
    }
    result
}

fn convert_bits(data: &[u8], from: u16, to: u16, pad: bool) -> Vec<u8> {
    let mut acc: u16 = 0;
    let mut bits: u16 = 0;
    let mut result: Vec<u8> = vec![];
    let max_value = (1 << to) - 1;
    for v in data {
        let value = *v as u16;
        acc = (acc << from) | value;
        bits += from;
        while bits >= to {
            bits -= to;
            result.push(((acc >> bits) & max_value) as u8);
        }
    }
    if pad && bits > 0 {
        result.push(((acc << (to - bits)) & max_value) as u8);
    }
    result
}

fn encode(hrp: &str, data: &[u8]) -> String {
    let hrp = hrp.to_lowercase();
    let mut combined: Vec<u8> = Vec::new();
    combined.extend(data);
    combined.extend(&create_checksum(&hrp, data));

    let mut result = hrp.clone();
    result.push('1');
    for value in combined.iter() {
        result.push(CHARSET.chars().nth(*value as usize).unwrap());
    }
    result
}

#[derive(Serialize)]
struct AddressTestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    mainnet: bool,
    public_key_hex: String,
    address: String,
}

#[derive(Serialize)]
struct Bech32TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    hrp: String,
    data_hex: String,
    data_5bit_hex: String,
    encoded: String,
}

#[derive(Serialize)]
struct Bech32TestFile {
    algorithm: String,
    mainnet_prefix: String,
    testnet_prefix: String,
    bech32_vectors: Vec<Bech32TestVector>,
    address_vectors: Vec<AddressTestVector>,
}

fn main() {
    let mut bech32_vectors = Vec::new();
    let mut address_vectors = Vec::new();

    // Test 1: Simple encoding with "tos" prefix
    {
        let hrp = "tos";
        let data: [u8; 4] = [0x00, 0x14, 0x75, 0x1e];
        let data_5bit = convert_bits(&data, 8, 5, true);
        let encoded = encode(hrp, &data_5bit);

        bech32_vectors.push(Bech32TestVector {
            name: "simple_tos".to_string(),
            description: Some("Simple 4-byte encoding with tos prefix".to_string()),
            hrp: hrp.to_string(),
            data_hex: hex::encode(&data),
            data_5bit_hex: hex::encode(&data_5bit),
            encoded,
        });
    }

    // Test 2: Simple encoding with "tst" prefix
    {
        let hrp = "tst";
        let data: [u8; 4] = [0xab, 0xcd, 0xef, 0x12];
        let data_5bit = convert_bits(&data, 8, 5, true);
        let encoded = encode(hrp, &data_5bit);

        bech32_vectors.push(Bech32TestVector {
            name: "simple_tst".to_string(),
            description: Some("Simple 4-byte encoding with tst prefix".to_string()),
            hrp: hrp.to_string(),
            data_hex: hex::encode(&data),
            data_5bit_hex: hex::encode(&data_5bit),
            encoded,
        });
    }

    // Test 3: All zeros public key (mainnet)
    {
        let public_key = [0u8; 32];
        let mut raw = vec![0u8]; // AddressType::Normal
        raw.extend_from_slice(&public_key);
        let data_5bit = convert_bits(&raw, 8, 5, true);
        let address = encode("tos", &data_5bit);

        address_vectors.push(AddressTestVector {
            name: "zeros_mainnet".to_string(),
            description: Some("All-zeros public key on mainnet".to_string()),
            mainnet: true,
            public_key_hex: hex::encode(&public_key),
            address,
        });
    }

    // Test 4: All zeros public key (testnet)
    {
        let public_key = [0u8; 32];
        let mut raw = vec![0u8]; // AddressType::Normal
        raw.extend_from_slice(&public_key);
        let data_5bit = convert_bits(&raw, 8, 5, true);
        let address = encode("tst", &data_5bit);

        address_vectors.push(AddressTestVector {
            name: "zeros_testnet".to_string(),
            description: Some("All-zeros public key on testnet".to_string()),
            mainnet: false,
            public_key_hex: hex::encode(&public_key),
            address,
        });
    }

    // Test 5: Sequential bytes public key (mainnet)
    {
        let mut public_key = [0u8; 32];
        for i in 0..32 {
            public_key[i] = i as u8;
        }
        let mut raw = vec![0u8]; // AddressType::Normal
        raw.extend_from_slice(&public_key);
        let data_5bit = convert_bits(&raw, 8, 5, true);
        let address = encode("tos", &data_5bit);

        address_vectors.push(AddressTestVector {
            name: "sequential_mainnet".to_string(),
            description: Some("Sequential bytes 0x00..0x1f as public key".to_string()),
            mainnet: true,
            public_key_hex: hex::encode(&public_key),
            address,
        });
    }

    // Test 6: All 0xFF public key (testnet)
    {
        let public_key = [0xffu8; 32];
        let mut raw = vec![0u8]; // AddressType::Normal
        raw.extend_from_slice(&public_key);
        let data_5bit = convert_bits(&raw, 8, 5, true);
        let address = encode("tst", &data_5bit);

        address_vectors.push(AddressTestVector {
            name: "all_ff_testnet".to_string(),
            description: Some("All 0xFF bytes as public key on testnet".to_string()),
            mainnet: false,
            public_key_hex: hex::encode(&public_key),
            address,
        });
    }

    // Test 7: Random-looking public key (Schnorr test vector public key)
    {
        let public_key = hex::decode("3cc4fec02e2342dca15352d5c5c27135f9e42c5805c07ca9dc500e000f89a665").unwrap();
        let mut raw = vec![0u8]; // AddressType::Normal
        raw.extend_from_slice(&public_key);
        let data_5bit = convert_bits(&raw, 8, 5, true);
        let address = encode("tos", &data_5bit);

        address_vectors.push(AddressTestVector {
            name: "schnorr_pubkey_mainnet".to_string(),
            description: Some("Public key from Schnorr test vector on mainnet".to_string()),
            mainnet: true,
            public_key_hex: hex::encode(&public_key),
            address,
        });
    }

    let test_file = Bech32TestFile {
        algorithm: "Bech32".to_string(),
        mainnet_prefix: "tos".to_string(),
        testnet_prefix: "tst".to_string(),
        bech32_vectors,
        address_vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("bech32.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to bech32.yaml");
}
