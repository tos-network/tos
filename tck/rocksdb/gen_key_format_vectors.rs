// Generate RocksDB key format test vectors for TOS/Avatar compatibility
// Run: cd ~/tos/tck/rocksdb && cargo run --release --bin gen_key_format_vectors
//
// These test vectors verify that Avatar key encoding matches TOS exactly,
// ensuring correct lookups in the shared RocksDB database.

use serde::Serialize;
use std::fs::File;
use std::io::Write;

// ============================================================================
// Key Encoding Helpers
// ============================================================================

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn write_u64_be(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

// ============================================================================
// Account Key Format
// Column Family: accounts
// Key Format: account_id (8 bytes, big-endian)
// ============================================================================

#[derive(Serialize)]
struct AccountKeyVector {
    name: String,
    description: String,
    account_id: u64,
    key_hex: String,
    key_len: usize,
}

fn generate_account_key_vectors() -> Vec<AccountKeyVector> {
    vec![
        {
            let key = write_u64_be(0);
            AccountKeyVector {
                name: "account_key_zero".to_string(),
                description: "Account key for id=0".to_string(),
                account_id: 0,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
        {
            let key = write_u64_be(1);
            AccountKeyVector {
                name: "account_key_one".to_string(),
                description: "Account key for id=1".to_string(),
                account_id: 1,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
        {
            let key = write_u64_be(42);
            AccountKeyVector {
                name: "account_key_42".to_string(),
                description: "Account key for typical id".to_string(),
                account_id: 42,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
        {
            let key = write_u64_be(u64::MAX);
            AccountKeyVector {
                name: "account_key_max".to_string(),
                description: "Account key for maximum id".to_string(),
                account_id: u64::MAX,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
    ]
}

// ============================================================================
// Balance Key Format
// Column Family: balances
// Key Format: account_id (8 bytes) + asset_hash (32 bytes) + topoheight (8 bytes)
// All big-endian
// ============================================================================

#[derive(Serialize)]
struct BalanceKeyVector {
    name: String,
    description: String,
    account_id: u64,
    asset_hash_hex: String,
    topoheight: u64,
    key_hex: String,
    key_len: usize,
}

fn generate_balance_key_vectors() -> Vec<BalanceKeyVector> {
    // Native TOS asset hash (SHA3-256 of empty or known value)
    let native_asset: [u8; 32] = [0u8; 32]; // Placeholder for native asset

    // Example custom asset hash
    let custom_asset: [u8; 32] = {
        let mut h = [0u8; 32];
        for i in 0..32 {
            h[i] = (i + 1) as u8;
        }
        h
    };

    vec![
        {
            let mut key = Vec::new();
            key.extend(write_u64_be(1)); // account_id
            key.extend(&native_asset);    // asset_hash
            key.extend(write_u64_be(0));  // topoheight
            BalanceKeyVector {
                name: "balance_key_native_initial".to_string(),
                description: "Balance key for account 1, native asset at genesis".to_string(),
                account_id: 1,
                asset_hash_hex: to_hex(&native_asset),
                topoheight: 0,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
        {
            let mut key = Vec::new();
            key.extend(write_u64_be(42)); // account_id
            key.extend(&custom_asset);     // asset_hash
            key.extend(write_u64_be(1000)); // topoheight
            BalanceKeyVector {
                name: "balance_key_custom_asset".to_string(),
                description: "Balance key for account 42, custom asset at topo 1000".to_string(),
                account_id: 42,
                asset_hash_hex: to_hex(&custom_asset),
                topoheight: 1000,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
        {
            let mut key = Vec::new();
            key.extend(write_u64_be(u64::MAX));
            key.extend(&[0xFF; 32]);
            key.extend(write_u64_be(u64::MAX));
            BalanceKeyVector {
                name: "balance_key_max_values".to_string(),
                description: "Balance key with maximum values".to_string(),
                account_id: u64::MAX,
                asset_hash_hex: to_hex(&[0xFF; 32]),
                topoheight: u64::MAX,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
    ]
}

// ============================================================================
// Nonce Key Format
// Column Family: nonces
// Key Format: account_id (8 bytes) + topoheight (8 bytes)
// All big-endian
// ============================================================================

#[derive(Serialize)]
struct NonceKeyVector {
    name: String,
    description: String,
    account_id: u64,
    topoheight: u64,
    key_hex: String,
    key_len: usize,
}

fn generate_nonce_key_vectors() -> Vec<NonceKeyVector> {
    vec![
        {
            let mut key = Vec::new();
            key.extend(write_u64_be(0));
            key.extend(write_u64_be(0));
            NonceKeyVector {
                name: "nonce_key_genesis".to_string(),
                description: "Nonce key for account 0 at genesis".to_string(),
                account_id: 0,
                topoheight: 0,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
        {
            let mut key = Vec::new();
            key.extend(write_u64_be(1));
            key.extend(write_u64_be(100));
            NonceKeyVector {
                name: "nonce_key_account1_topo100".to_string(),
                description: "Nonce key for account 1 at topoheight 100".to_string(),
                account_id: 1,
                topoheight: 100,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
        {
            let mut key = Vec::new();
            key.extend(write_u64_be(1000));
            key.extend(write_u64_be(999999));
            NonceKeyVector {
                name: "nonce_key_large".to_string(),
                description: "Nonce key with larger values".to_string(),
                account_id: 1000,
                topoheight: 999999,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
    ]
}

// ============================================================================
// Transaction Hash Key Format
// Column Family: transactions
// Key Format: tx_hash (32 bytes)
// ============================================================================

#[derive(Serialize)]
struct TxHashKeyVector {
    name: String,
    description: String,
    tx_hash_hex: String,
    key_hex: String,
    key_len: usize,
}

fn generate_tx_hash_key_vectors() -> Vec<TxHashKeyVector> {
    let zero_hash = [0u8; 32];
    let sequential_hash: [u8; 32] = {
        let mut h = [0u8; 32];
        for i in 0..32 {
            h[i] = i as u8;
        }
        h
    };
    let max_hash = [0xFF; 32];

    vec![
        TxHashKeyVector {
            name: "tx_key_zero".to_string(),
            description: "Transaction key with zero hash".to_string(),
            tx_hash_hex: to_hex(&zero_hash),
            key_hex: to_hex(&zero_hash),
            key_len: 32,
        },
        TxHashKeyVector {
            name: "tx_key_sequential".to_string(),
            description: "Transaction key with sequential bytes".to_string(),
            tx_hash_hex: to_hex(&sequential_hash),
            key_hex: to_hex(&sequential_hash),
            key_len: 32,
        },
        TxHashKeyVector {
            name: "tx_key_max".to_string(),
            description: "Transaction key with all 0xFF bytes".to_string(),
            tx_hash_hex: to_hex(&max_hash),
            key_hex: to_hex(&max_hash),
            key_len: 32,
        },
    ]
}

// ============================================================================
// Block Hash Key Format
// Column Family: blocks
// Key Format: block_hash (32 bytes)
// ============================================================================

#[derive(Serialize)]
struct BlockHashKeyVector {
    name: String,
    description: String,
    block_hash_hex: String,
    key_hex: String,
    key_len: usize,
}

fn generate_block_hash_key_vectors() -> Vec<BlockHashKeyVector> {
    // Genesis block hash example (placeholder)
    let genesis_hash: [u8; 32] = {
        let mut h = [0u8; 32];
        h[0] = 0x9E; // Placeholder marker
        h
    };

    vec![
        BlockHashKeyVector {
            name: "block_key_genesis".to_string(),
            description: "Block key for genesis block".to_string(),
            block_hash_hex: to_hex(&genesis_hash),
            key_hex: to_hex(&genesis_hash),
            key_len: 32,
        },
    ]
}

// ============================================================================
// Topoheight Key Format
// Column Family: topoheight_to_hash, hash_to_topoheight
// ============================================================================

#[derive(Serialize)]
struct TopoheightKeyVector {
    name: String,
    description: String,
    topoheight: u64,
    key_hex: String,
    key_len: usize,
}

fn generate_topoheight_key_vectors() -> Vec<TopoheightKeyVector> {
    vec![
        {
            let key = write_u64_be(0);
            TopoheightKeyVector {
                name: "topo_key_genesis".to_string(),
                description: "Topoheight key for genesis (0)".to_string(),
                topoheight: 0,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
        {
            let key = write_u64_be(1);
            TopoheightKeyVector {
                name: "topo_key_one".to_string(),
                description: "Topoheight key for block 1".to_string(),
                topoheight: 1,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
        {
            let key = write_u64_be(1000000);
            TopoheightKeyVector {
                name: "topo_key_million".to_string(),
                description: "Topoheight key for block 1000000".to_string(),
                topoheight: 1000000,
                key_hex: to_hex(&key),
                key_len: key.len(),
            }
        },
    ]
}

// ============================================================================
// Prefix Key Format (for range queries)
// ============================================================================

#[derive(Serialize)]
struct PrefixKeyVector {
    name: String,
    description: String,
    column_family: String,
    account_id: u64,
    prefix_hex: String,
    prefix_len: usize,
}

fn generate_prefix_key_vectors() -> Vec<PrefixKeyVector> {
    vec![
        {
            let prefix = write_u64_be(1);
            PrefixKeyVector {
                name: "balance_prefix_account1".to_string(),
                description: "Prefix for all balances of account 1".to_string(),
                column_family: "balances".to_string(),
                account_id: 1,
                prefix_hex: to_hex(&prefix),
                prefix_len: prefix.len(),
            }
        },
        {
            let prefix = write_u64_be(42);
            PrefixKeyVector {
                name: "nonce_prefix_account42".to_string(),
                description: "Prefix for all nonces of account 42".to_string(),
                column_family: "nonces".to_string(),
                account_id: 42,
                prefix_hex: to_hex(&prefix),
                prefix_len: prefix.len(),
            }
        },
    ]
}

// ============================================================================
// Main Output Structure
// ============================================================================

#[derive(Serialize)]
struct KeyFormatTestVectors {
    description: String,
    version: String,
    note: String,
    account_key_vectors: Vec<AccountKeyVector>,
    balance_key_vectors: Vec<BalanceKeyVector>,
    nonce_key_vectors: Vec<NonceKeyVector>,
    tx_hash_key_vectors: Vec<TxHashKeyVector>,
    block_hash_key_vectors: Vec<BlockHashKeyVector>,
    topoheight_key_vectors: Vec<TopoheightKeyVector>,
    prefix_key_vectors: Vec<PrefixKeyVector>,
}

fn main() {
    let vectors = KeyFormatTestVectors {
        description: "RocksDB key format test vectors for TOS/Avatar compatibility".to_string(),
        version: "1.0".to_string(),
        note: "All multi-byte integers use big-endian encoding. Keys are raw bytes.".to_string(),
        account_key_vectors: generate_account_key_vectors(),
        balance_key_vectors: generate_balance_key_vectors(),
        nonce_key_vectors: generate_nonce_key_vectors(),
        tx_hash_key_vectors: generate_tx_hash_key_vectors(),
        block_hash_key_vectors: generate_block_hash_key_vectors(),
        topoheight_key_vectors: generate_topoheight_key_vectors(),
        prefix_key_vectors: generate_prefix_key_vectors(),
    };

    let yaml = serde_yaml::to_string(&vectors).expect("Failed to serialize to YAML");

    let mut file = File::create("key_format.yaml").expect("Failed to create file");
    file.write_all(yaml.as_bytes()).expect("Failed to write file");

    println!("Generated key_format.yaml");
    println!("  - {} account key vectors", vectors.account_key_vectors.len());
    println!("  - {} balance key vectors", vectors.balance_key_vectors.len());
    println!("  - {} nonce key vectors", vectors.nonce_key_vectors.len());
    println!("  - {} tx hash key vectors", vectors.tx_hash_key_vectors.len());
    println!("  - {} block hash key vectors", vectors.block_hash_key_vectors.len());
    println!("  - {} topoheight key vectors", vectors.topoheight_key_vectors.len());
    println!("  - {} prefix key vectors", vectors.prefix_key_vectors.len());
}
