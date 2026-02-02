//! Block Hash Test Vector Generator
//!
//! Generates YAML test vectors for cross-language verification of block hash computation.
//! These vectors verify that Avatar C computes block hashes identically to TOS Rust.

use indexmap::IndexSet;
use serde::Serialize;
use std::fs::File;
use std::io::Write;
use tos_common::block::{BlockHeader, BlockVersion, EXTRA_NONCE_SIZE};
use tos_common::crypto::{Hash, Hashable};
use tos_common::serializer::Serializer;

#[derive(Serialize)]
struct BlockHashVector {
    name: String,
    description: String,
    // Input fields
    version: u8,
    height: u64,
    timestamp: u64,
    nonce: u64,
    extra_nonce_hex: String,
    miner_hex: String,
    tips_hex: Vec<String>,
    txs_hashes_hex: Vec<String>,
    // Intermediate values for debugging
    tips_hash_hex: String,
    txs_hash_hex: String,
    work_hash_hex: String,
    // Output
    block_hash_hex: String,
}

fn hash_to_hex(hash: &Hash) -> String {
    hex::encode(hash.as_bytes())
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

fn generate_vector(
    name: &str,
    description: &str,
    version: BlockVersion,
    height: u64,
    timestamp: u64,
    nonce: u64,
    extra_nonce: [u8; EXTRA_NONCE_SIZE],
    miner: &tos_common::crypto::elgamal::CompressedPublicKey,
    tips: IndexSet<Hash>,
    txs_hashes: IndexSet<Hash>,
) -> BlockHashVector {
    let mut header = BlockHeader::new(
        version,
        height,
        timestamp,
        tips.clone(),
        extra_nonce,
        miner.clone(),
        txs_hashes.clone(),
    );

    // Set the nonce (not set by new())
    header.nonce = nonce;

    // Get intermediate values
    let tips_hash = header.get_tips_hash();
    let txs_hash = header.get_txs_hash();
    let work_hash = header.get_work_hash();
    let block_hash = header.hash();

    BlockHashVector {
        name: name.to_string(),
        description: description.to_string(),
        version: version.to_bytes()[0],
        height,
        timestamp,
        nonce,
        extra_nonce_hex: bytes_to_hex(&extra_nonce),
        miner_hex: bytes_to_hex(miner.as_bytes()),
        tips_hex: tips.iter().map(|h| hash_to_hex(h)).collect(),
        txs_hashes_hex: txs_hashes.iter().map(|h| hash_to_hex(h)).collect(),
        tips_hash_hex: hash_to_hex(&tips_hash),
        txs_hash_hex: hash_to_hex(&txs_hash),
        work_hash_hex: hash_to_hex(&work_hash),
        block_hash_hex: hash_to_hex(&block_hash),
    }
}

fn main() {
    let mut vectors = Vec::new();

    // Generate a deterministic miner key using raw bytes
    let miner_bytes: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
        0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
    ];
    let miner = tos_common::crypto::elgamal::CompressedPublicKey::from_bytes(&miner_bytes)
        .expect("Invalid miner key");

    // Test 1: Single tip, no transactions
    {
        let mut tips = IndexSet::new();
        tips.insert(Hash::new([0x11; 32]));
        let txs: IndexSet<Hash> = IndexSet::new();
        let extra_nonce = [0xaa; 32];

        vectors.push(generate_vector(
            "single_tip_no_txs",
            "Block with single tip and no transactions",
            BlockVersion::Nobunaga,
            100,
            1700000000000,
            12345,
            extra_nonce,
            &miner,
            tips,
            txs,
        ));
    }

    // Test 2: Single tip, single transaction
    {
        let mut tips = IndexSet::new();
        tips.insert(Hash::new([0x22; 32]));
        let mut txs = IndexSet::new();
        txs.insert(Hash::new([0x33; 32]));
        let extra_nonce = [0xbb; 32];

        vectors.push(generate_vector(
            "single_tip_single_tx",
            "Block with single tip and one transaction",
            BlockVersion::Nobunaga,
            200,
            1700000001000,
            67890,
            extra_nonce,
            &miner,
            tips,
            txs,
        ));
    }

    // Test 3: Two tips, multiple transactions
    {
        let mut tips = IndexSet::new();
        tips.insert(Hash::new([0x44; 32]));
        tips.insert(Hash::new([0x55; 32]));
        let mut txs = IndexSet::new();
        txs.insert(Hash::new([0x66; 32]));
        txs.insert(Hash::new([0x77; 32]));
        txs.insert(Hash::new([0x88; 32]));
        let extra_nonce = [0xcc; 32];

        vectors.push(generate_vector(
            "two_tips_three_txs",
            "Block with two tips and three transactions",
            BlockVersion::Nobunaga,
            300,
            1700000002000,
            11111,
            extra_nonce,
            &miner,
            tips,
            txs,
        ));
    }

    // Test 4: Three tips (maximum), many transactions
    {
        let mut tips = IndexSet::new();
        tips.insert(Hash::new([0x99; 32]));
        tips.insert(Hash::new([0xaa; 32]));
        tips.insert(Hash::new([0xbb; 32]));
        let mut txs = IndexSet::new();
        for i in 0..10u8 {
            let mut h = [0u8; 32];
            h[0] = i;
            h[31] = i;
            txs.insert(Hash::new(h));
        }
        let extra_nonce = [0xdd; 32];

        vectors.push(generate_vector(
            "three_tips_ten_txs",
            "Block with three tips (max) and ten transactions",
            BlockVersion::Nobunaga,
            400,
            1700000003000,
            22222,
            extra_nonce,
            &miner,
            tips,
            txs,
        ));
    }

    // Test 5: Genesis-like block (height 0)
    {
        let mut tips = IndexSet::new();
        tips.insert(Hash::zero());
        let txs: IndexSet<Hash> = IndexSet::new();
        let extra_nonce = [0x00; 32];

        vectors.push(generate_vector(
            "genesis_like",
            "Genesis-like block at height 0 with zero tip",
            BlockVersion::Nobunaga,
            0,
            1600000000000,
            0,
            extra_nonce,
            &miner,
            tips,
            txs,
        ));
    }

    // Test 6: Large nonce values
    {
        let mut tips = IndexSet::new();
        tips.insert(Hash::new([0xee; 32]));
        let txs: IndexSet<Hash> = IndexSet::new();
        let extra_nonce = [0xff; 32];

        vectors.push(generate_vector(
            "large_nonce",
            "Block with maximum nonce values",
            BlockVersion::Nobunaga,
            u64::MAX - 1,
            u64::MAX,
            u64::MAX,
            extra_nonce,
            &miner,
            tips,
            txs,
        ));
    }

    // Output YAML with proper structure for Avatar C YAML parser
    let output_path = "block_hash.yaml";
    let mut file = File::create(output_path).expect("Failed to create file");
    file.write_all(b"# Block Hash Test Vectors\n").unwrap();
    file.write_all(b"# Generated by gen_block_hash_vectors.rs\n").unwrap();
    file.write_all(b"# Verifies TOS Rust == Avatar C block hash computation\n").unwrap();
    file.write_all(b"algorithm: BLOCK_HASH_TOS\n").unwrap();
    file.write_all(b"description: TOS Rust block hash algorithm (BLAKE3-based, multi-step)\n").unwrap();
    file.write_all(b"test_vectors:\n").unwrap();

    // Write each vector with proper indentation
    for v in &vectors {
        write!(file, "- name: {}\n", v.name).unwrap();
        write!(file, "  description: {}\n", v.description).unwrap();
        write!(file, "  version: {}\n", v.version).unwrap();
        write!(file, "  height: {}\n", v.height).unwrap();
        write!(file, "  timestamp: {}\n", v.timestamp).unwrap();
        write!(file, "  nonce: {}\n", v.nonce).unwrap();
        write!(file, "  extra_nonce_hex: {}\n", v.extra_nonce_hex).unwrap();
        write!(file, "  miner_hex: {}\n", v.miner_hex).unwrap();
        file.write_all(b"  tips_hex:\n").unwrap();
        for tip in &v.tips_hex {
            write!(file, "  - '{}'\n", tip).unwrap();
        }
        file.write_all(b"  txs_hashes_hex:\n").unwrap();
        if v.txs_hashes_hex.is_empty() {
            // Empty array handled differently
        } else {
            for tx in &v.txs_hashes_hex {
                write!(file, "  - '{}'\n", tx).unwrap();
            }
        }
        write!(file, "  tips_hash_hex: {}\n", v.tips_hash_hex).unwrap();
        write!(file, "  txs_hash_hex: {}\n", v.txs_hash_hex).unwrap();
        write!(file, "  work_hash_hex: {}\n", v.work_hash_hex).unwrap();
        write!(file, "  block_hash_hex: {}\n", v.block_hash_hex).unwrap();
    }

    println!("Generated {} test vectors to {}", vectors.len(), output_path);
    
    // Also print to stdout for verification
    for v in &vectors {
        println!("\n=== {} ===", v.name);
        println!("  tips_hash:  {}", v.tips_hash_hex);
        println!("  txs_hash:   {}", v.txs_hash_hex);
        println!("  work_hash:  {}", v.work_hash_hex);
        println!("  block_hash: {}", v.block_hash_hex);
    }
}
