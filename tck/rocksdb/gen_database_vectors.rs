// Generate RocksDB database test vectors for TOS/Avatar compatibility
// Run: cd ~/tos/tck/rocksdb && cargo run --release --bin gen_database_vectors
//
// This generator creates a test RocksDB database with known data that can be
// read by Avatar to verify database interoperability.

use serde::Serialize;
use std::fs::File;
use std::io::Write;

// ============================================================================
// Database Operation Test Vectors
// These describe operations that should produce identical databases in TOS/Avatar
// ============================================================================

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn write_u64_be(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn write_u8(value: u8) -> Vec<u8> {
    vec![value]
}

fn write_option_u64_be(value: Option<u64>) -> Vec<u8> {
    match value {
        None => vec![0x00],
        Some(v) => {
            let mut buf = vec![0x01];
            buf.extend(v.to_be_bytes());
            buf
        }
    }
}

// ============================================================================
// Account Database Operations
// ============================================================================

#[derive(Serialize)]
struct AccountDbOperation {
    name: String,
    description: String,
    operation: String,
    column_family: String,
    key_hex: String,
    value_hex: String,
    // Account fields for verification
    account_id: u64,
    registered_at: Option<u64>,
    nonce_pointer: Option<u64>,
    multisig_pointer: Option<u64>,
    energy_pointer: Option<u64>,
}

fn serialize_account(
    id: u64,
    registered_at: Option<u64>,
    nonce_pointer: Option<u64>,
    multisig_pointer: Option<u64>,
    energy_pointer: Option<u64>,
) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(write_u64_be(id));
    buf.extend(write_option_u64_be(registered_at));
    buf.extend(write_option_u64_be(nonce_pointer));
    buf.extend(write_option_u64_be(multisig_pointer));
    buf.extend(write_option_u64_be(energy_pointer));
    buf
}

fn generate_account_db_operations() -> Vec<AccountDbOperation> {
    vec![
        {
            let id = 0u64;
            let key = write_u64_be(id);
            let value = serialize_account(id, None, None, None, None);
            AccountDbOperation {
                name: "put_account_genesis".to_string(),
                description: "Insert genesis account (id=0)".to_string(),
                operation: "put".to_string(),
                column_family: "accounts".to_string(),
                key_hex: to_hex(&key),
                value_hex: to_hex(&value),
                account_id: id,
                registered_at: None,
                nonce_pointer: None,
                multisig_pointer: None,
                energy_pointer: None,
            }
        },
        {
            let id = 1u64;
            let key = write_u64_be(id);
            let value = serialize_account(id, Some(0), Some(0), None, None);
            AccountDbOperation {
                name: "put_account_1".to_string(),
                description: "Insert account 1 with registration".to_string(),
                operation: "put".to_string(),
                column_family: "accounts".to_string(),
                key_hex: to_hex(&key),
                value_hex: to_hex(&value),
                account_id: id,
                registered_at: Some(0),
                nonce_pointer: Some(0),
                multisig_pointer: None,
                energy_pointer: None,
            }
        },
        {
            let id = 42u64;
            let key = write_u64_be(id);
            let value = serialize_account(id, Some(100), Some(200), Some(300), Some(400));
            AccountDbOperation {
                name: "put_account_42".to_string(),
                description: "Insert account 42 with all fields".to_string(),
                operation: "put".to_string(),
                column_family: "accounts".to_string(),
                key_hex: to_hex(&key),
                value_hex: to_hex(&value),
                account_id: id,
                registered_at: Some(100),
                nonce_pointer: Some(200),
                multisig_pointer: Some(300),
                energy_pointer: Some(400),
            }
        },
    ]
}

// ============================================================================
// Nonce Database Operations
// ============================================================================

#[derive(Serialize)]
struct NonceDbOperation {
    name: String,
    description: String,
    operation: String,
    column_family: String,
    key_hex: String,
    value_hex: String,
    // Nonce fields
    account_id: u64,
    topoheight: u64,
    previous_topoheight: Option<u64>,
    nonce: u64,
}

fn serialize_versioned_nonce(previous_topoheight: Option<u64>, nonce: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(write_option_u64_be(previous_topoheight));
    buf.extend(write_u64_be(nonce));
    buf
}

fn generate_nonce_db_operations() -> Vec<NonceDbOperation> {
    vec![
        {
            let account_id = 1u64;
            let topoheight = 0u64;
            let mut key = Vec::new();
            key.extend(write_u64_be(account_id));
            key.extend(write_u64_be(topoheight));
            let value = serialize_versioned_nonce(None, 0);
            NonceDbOperation {
                name: "put_nonce_initial".to_string(),
                description: "Initial nonce for account 1 at genesis".to_string(),
                operation: "put".to_string(),
                column_family: "nonces".to_string(),
                key_hex: to_hex(&key),
                value_hex: to_hex(&value),
                account_id,
                topoheight,
                previous_topoheight: None,
                nonce: 0,
            }
        },
        {
            let account_id = 1u64;
            let topoheight = 100u64;
            let mut key = Vec::new();
            key.extend(write_u64_be(account_id));
            key.extend(write_u64_be(topoheight));
            let value = serialize_versioned_nonce(Some(0), 5);
            NonceDbOperation {
                name: "put_nonce_after_txs".to_string(),
                description: "Nonce after 5 transactions".to_string(),
                operation: "put".to_string(),
                column_family: "nonces".to_string(),
                key_hex: to_hex(&key),
                value_hex: to_hex(&value),
                account_id,
                topoheight,
                previous_topoheight: Some(0),
                nonce: 5,
            }
        },
    ]
}

// ============================================================================
// Balance Database Operations
// ============================================================================

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum BalanceType {
    Input = 0,
    Output = 1,
    Both = 2,
}

#[derive(Serialize)]
struct BalanceDbOperation {
    name: String,
    description: String,
    operation: String,
    column_family: String,
    key_hex: String,
    value_hex: String,
    // Balance fields
    account_id: u64,
    asset_hash_hex: String,
    topoheight: u64,
    previous_topoheight: Option<u64>,
    balance_type: BalanceType,
    final_balance: u64,
    output_balance: Option<u64>,
}

fn serialize_versioned_balance(
    previous_topoheight: Option<u64>,
    balance_type: BalanceType,
    final_balance: u64,
    output_balance: Option<u64>,
) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(write_option_u64_be(previous_topoheight));
    buf.extend(write_u8(balance_type as u8));
    buf.extend(write_u64_be(final_balance));
    buf.extend(write_option_u64_be(output_balance));
    buf
}

fn generate_balance_db_operations() -> Vec<BalanceDbOperation> {
    let native_asset = [0u8; 32];

    vec![
        {
            let account_id = 1u64;
            let topoheight = 0u64;
            let mut key = Vec::new();
            key.extend(write_u64_be(account_id));
            key.extend(&native_asset);
            key.extend(write_u64_be(topoheight));
            let value = serialize_versioned_balance(
                None,
                BalanceType::Input,
                100_000_000_000, // 1000 TOS
                None,
            );
            BalanceDbOperation {
                name: "put_balance_genesis".to_string(),
                description: "Genesis balance for account 1 (1000 TOS)".to_string(),
                operation: "put".to_string(),
                column_family: "balances".to_string(),
                key_hex: to_hex(&key),
                value_hex: to_hex(&value),
                account_id,
                asset_hash_hex: to_hex(&native_asset),
                topoheight,
                previous_topoheight: None,
                balance_type: BalanceType::Input,
                final_balance: 100_000_000_000,
                output_balance: None,
            }
        },
        {
            let account_id = 1u64;
            let topoheight = 100u64;
            let mut key = Vec::new();
            key.extend(write_u64_be(account_id));
            key.extend(&native_asset);
            key.extend(write_u64_be(topoheight));
            let value = serialize_versioned_balance(
                Some(0),
                BalanceType::Both,
                90_000_000_000,  // 900 TOS final
                Some(10_000_000_000), // 100 TOS pending output
            );
            BalanceDbOperation {
                name: "put_balance_after_send".to_string(),
                description: "Balance after sending 100 TOS".to_string(),
                operation: "put".to_string(),
                column_family: "balances".to_string(),
                key_hex: to_hex(&key),
                value_hex: to_hex(&value),
                account_id,
                asset_hash_hex: to_hex(&native_asset),
                topoheight,
                previous_topoheight: Some(0),
                balance_type: BalanceType::Both,
                final_balance: 90_000_000_000,
                output_balance: Some(10_000_000_000),
            }
        },
    ]
}

// ============================================================================
// Topoheight Database Operations
// ============================================================================

#[derive(Serialize)]
struct TopoheightDbOperation {
    name: String,
    description: String,
    operation: String,
    column_family: String,
    key_hex: String,
    value_hex: String,
    topoheight: u64,
    block_hash_hex: String,
}

fn generate_topoheight_db_operations() -> Vec<TopoheightDbOperation> {
    // Genesis block hash (placeholder)
    let genesis_hash: [u8; 32] = {
        let mut h = [0u8; 32];
        h[31] = 0x01; // Simple marker
        h
    };

    vec![
        {
            let key = write_u64_be(0);
            TopoheightDbOperation {
                name: "put_topo_genesis".to_string(),
                description: "Map topoheight 0 to genesis block".to_string(),
                operation: "put".to_string(),
                column_family: "topoheight_to_hash".to_string(),
                key_hex: to_hex(&key),
                value_hex: to_hex(&genesis_hash),
                topoheight: 0,
                block_hash_hex: to_hex(&genesis_hash),
            }
        },
    ]
}

// ============================================================================
// Query Test Vectors (expected results)
// ============================================================================

#[derive(Serialize)]
struct QueryTestVector {
    name: String,
    description: String,
    query_type: String,
    column_family: String,
    key_hex: String,
    expected_found: bool,
    expected_value_hex: Option<String>,
}

fn generate_query_vectors() -> Vec<QueryTestVector> {
    vec![
        {
            let key = write_u64_be(42);
            let value = serialize_account(42, Some(100), Some(200), Some(300), Some(400));
            QueryTestVector {
                name: "query_account_exists".to_string(),
                description: "Query existing account 42".to_string(),
                query_type: "get".to_string(),
                column_family: "accounts".to_string(),
                key_hex: to_hex(&key),
                expected_found: true,
                expected_value_hex: Some(to_hex(&value)),
            }
        },
        {
            let key = write_u64_be(9999);
            QueryTestVector {
                name: "query_account_not_exists".to_string(),
                description: "Query non-existent account".to_string(),
                query_type: "get".to_string(),
                column_family: "accounts".to_string(),
                key_hex: to_hex(&key),
                expected_found: false,
                expected_value_hex: None,
            }
        },
    ]
}

// ============================================================================
// Column Family Configuration
// ============================================================================

#[derive(Serialize)]
struct ColumnFamilyConfig {
    name: String,
    description: String,
    key_format: String,
    value_format: String,
}

fn generate_cf_configs() -> Vec<ColumnFamilyConfig> {
    vec![
        ColumnFamilyConfig {
            name: "accounts".to_string(),
            description: "Account state storage".to_string(),
            key_format: "account_id (u64 BE)".to_string(),
            value_format: "Account struct (id + optional fields)".to_string(),
        },
        ColumnFamilyConfig {
            name: "nonces".to_string(),
            description: "Account nonce versions".to_string(),
            key_format: "account_id (u64 BE) + topoheight (u64 BE)".to_string(),
            value_format: "VersionedNonce (previous_topo + nonce)".to_string(),
        },
        ColumnFamilyConfig {
            name: "balances".to_string(),
            description: "Account balance versions".to_string(),
            key_format: "account_id (u64 BE) + asset_hash (32 bytes) + topoheight (u64 BE)".to_string(),
            value_format: "VersionedBalance (previous_topo + type + final + output)".to_string(),
        },
        ColumnFamilyConfig {
            name: "topoheight_to_hash".to_string(),
            description: "Map topoheight to block hash".to_string(),
            key_format: "topoheight (u64 BE)".to_string(),
            value_format: "block_hash (32 bytes)".to_string(),
        },
        ColumnFamilyConfig {
            name: "hash_to_topoheight".to_string(),
            description: "Map block hash to topoheight".to_string(),
            key_format: "block_hash (32 bytes)".to_string(),
            value_format: "topoheight (u64 BE)".to_string(),
        },
    ]
}

// ============================================================================
// Main Output Structure
// ============================================================================

#[derive(Serialize)]
struct DatabaseTestVectors {
    description: String,
    version: String,
    note: String,
    column_family_configs: Vec<ColumnFamilyConfig>,
    account_operations: Vec<AccountDbOperation>,
    nonce_operations: Vec<NonceDbOperation>,
    balance_operations: Vec<BalanceDbOperation>,
    topoheight_operations: Vec<TopoheightDbOperation>,
    query_vectors: Vec<QueryTestVector>,
}

fn main() {
    let vectors = DatabaseTestVectors {
        description: "RocksDB database operation test vectors for TOS/Avatar compatibility".to_string(),
        version: "1.0".to_string(),
        note: "Execute operations in order, then verify queries return expected results.".to_string(),
        column_family_configs: generate_cf_configs(),
        account_operations: generate_account_db_operations(),
        nonce_operations: generate_nonce_db_operations(),
        balance_operations: generate_balance_db_operations(),
        topoheight_operations: generate_topoheight_db_operations(),
        query_vectors: generate_query_vectors(),
    };

    let yaml = serde_yaml::to_string(&vectors).expect("Failed to serialize to YAML");

    let mut file = File::create("database.yaml").expect("Failed to create file");
    file.write_all(yaml.as_bytes()).expect("Failed to write file");

    println!("Generated database.yaml");
    println!("  - {} column family configs", vectors.column_family_configs.len());
    println!("  - {} account operations", vectors.account_operations.len());
    println!("  - {} nonce operations", vectors.nonce_operations.len());
    println!("  - {} balance operations", vectors.balance_operations.len());
    println!("  - {} topoheight operations", vectors.topoheight_operations.len());
    println!("  - {} query vectors", vectors.query_vectors.len());
}
