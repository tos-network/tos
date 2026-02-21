// Generate RocksDB serialization test vectors for TOS/Avatar compatibility
// Run: cd ~/tos/tck/rocksdb && cargo run --release --bin gen_serialization_vectors
//
// These test vectors verify that Avatar (C) serialization matches TOS (Rust) exactly,
// enabling database interoperability between implementations.
//
// Coverage Strategy:
// - All combinations of optional fields (2^n permutations)
// - Boundary values: 0, 1, typical, max-1, max
// - All enum variants
// - Edge cases specific to blockchain operations

use serde::Serialize;
use std::fs::File;
use std::io::Write;

// ============================================================================
// TOS Serialization Primitives (Big-Endian)
// ============================================================================

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

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ============================================================================
// Big-Endian Encoding Vectors - Boundary Values
// ============================================================================

#[derive(Serialize)]
struct BigEndianTestVector {
    name: String,
    description: String,
    value_u64: u64,
    encoded_hex: String,
}

fn generate_big_endian_vectors() -> Vec<BigEndianTestVector> {
    vec![
        // Boundary values
        BigEndianTestVector {
            name: "be_zero".to_string(),
            description: "Zero value (minimum)".to_string(),
            value_u64: 0,
            encoded_hex: to_hex(&0u64.to_be_bytes()),
        },
        BigEndianTestVector {
            name: "be_one".to_string(),
            description: "Value 1 (smallest positive)".to_string(),
            value_u64: 1,
            encoded_hex: to_hex(&1u64.to_be_bytes()),
        },
        BigEndianTestVector {
            name: "be_max_minus_one".to_string(),
            description: "Maximum minus one".to_string(),
            value_u64: u64::MAX - 1,
            encoded_hex: to_hex(&(u64::MAX - 1).to_be_bytes()),
        },
        BigEndianTestVector {
            name: "be_max".to_string(),
            description: "Maximum u64 value".to_string(),
            value_u64: u64::MAX,
            encoded_hex: to_hex(&u64::MAX.to_be_bytes()),
        },
        // Byte boundary values
        BigEndianTestVector {
            name: "be_byte1_max".to_string(),
            description: "Max value in 1 byte (255)".to_string(),
            value_u64: 0xFF,
            encoded_hex: to_hex(&0xFFu64.to_be_bytes()),
        },
        BigEndianTestVector {
            name: "be_byte2_max".to_string(),
            description: "Max value in 2 bytes (65535)".to_string(),
            value_u64: 0xFFFF,
            encoded_hex: to_hex(&0xFFFFu64.to_be_bytes()),
        },
        BigEndianTestVector {
            name: "be_byte4_max".to_string(),
            description: "Max value in 4 bytes (u32::MAX)".to_string(),
            value_u64: 0xFFFFFFFF,
            encoded_hex: to_hex(&0xFFFFFFFFu64.to_be_bytes()),
        },
        // Typical blockchain values
        BigEndianTestVector {
            name: "be_topoheight_typical".to_string(),
            description: "Typical topoheight (1 million)".to_string(),
            value_u64: 1_000_000,
            encoded_hex: to_hex(&1_000_000u64.to_be_bytes()),
        },
        BigEndianTestVector {
            name: "be_balance_1_tos".to_string(),
            description: "1 TOS in atomic units (8 decimals)".to_string(),
            value_u64: 100_000_000,
            encoded_hex: to_hex(&100_000_000u64.to_be_bytes()),
        },
        BigEndianTestVector {
            name: "be_balance_max_supply".to_string(),
            description: "TOS max supply (100M TOS)".to_string(),
            value_u64: 100_000_000 * 100_000_000, // 100M TOS
            encoded_hex: to_hex(&(100_000_000u64 * 100_000_000).to_be_bytes()),
        },
        // Pattern verification
        BigEndianTestVector {
            name: "be_sequential".to_string(),
            description: "Sequential bytes for endian verification".to_string(),
            value_u64: 0x0102030405060708,
            encoded_hex: to_hex(&0x0102030405060708u64.to_be_bytes()),
        },
        BigEndianTestVector {
            name: "be_alternating".to_string(),
            description: "Alternating bits pattern".to_string(),
            value_u64: 0xAAAAAAAAAAAAAAAA,
            encoded_hex: to_hex(&0xAAAAAAAAAAAAAAAAu64.to_be_bytes()),
        },
    ]
}

// ============================================================================
// Option<u64> Encoding Vectors
// ============================================================================

#[derive(Serialize)]
struct OptionU64TestVector {
    name: String,
    description: String,
    is_some: bool,
    value: Option<u64>,
    encoded_hex: String,
    encoded_len: usize,
}

fn generate_option_u64_vectors() -> Vec<OptionU64TestVector> {
    vec![
        // None case
        {
            let e = write_option_u64_be(None);
            OptionU64TestVector {
                name: "option_none".to_string(),
                description: "None value (single byte 0x00)".to_string(),
                is_some: false,
                value: None,
                encoded_hex: to_hex(&e),
                encoded_len: e.len(),
            }
        },
        // Some with boundary values
        {
            let e = write_option_u64_be(Some(0));
            OptionU64TestVector {
                name: "option_some_zero".to_string(),
                description: "Some(0) - distinguishes from None".to_string(),
                is_some: true,
                value: Some(0),
                encoded_hex: to_hex(&e),
                encoded_len: e.len(),
            }
        },
        {
            let e = write_option_u64_be(Some(1));
            OptionU64TestVector {
                name: "option_some_one".to_string(),
                description: "Some(1)".to_string(),
                is_some: true,
                value: Some(1),
                encoded_hex: to_hex(&e),
                encoded_len: e.len(),
            }
        },
        {
            let e = write_option_u64_be(Some(u64::MAX - 1));
            OptionU64TestVector {
                name: "option_some_max_minus_one".to_string(),
                description: "Some(u64::MAX - 1)".to_string(),
                is_some: true,
                value: Some(u64::MAX - 1),
                encoded_hex: to_hex(&e),
                encoded_len: e.len(),
            }
        },
        {
            let e = write_option_u64_be(Some(u64::MAX));
            OptionU64TestVector {
                name: "option_some_max".to_string(),
                description: "Some(u64::MAX)".to_string(),
                is_some: true,
                value: Some(u64::MAX),
                encoded_hex: to_hex(&e),
                encoded_len: e.len(),
            }
        },
        // Typical values
        {
            let e = write_option_u64_be(Some(0x123));
            OptionU64TestVector {
                name: "option_some_small".to_string(),
                description: "Some(0x123) - small value".to_string(),
                is_some: true,
                value: Some(0x123),
                encoded_hex: to_hex(&e),
                encoded_len: e.len(),
            }
        },
        {
            let e = write_option_u64_be(Some(1_000_000));
            OptionU64TestVector {
                name: "option_some_topoheight".to_string(),
                description: "Some(1000000) - typical topoheight".to_string(),
                is_some: true,
                value: Some(1_000_000),
                encoded_hex: to_hex(&e),
                encoded_len: e.len(),
            }
        },
    ]
}

// ============================================================================
// Account Serialization - All Optional Field Combinations
// ============================================================================

#[derive(Serialize)]
struct AccountTestVector {
    name: String,
    description: String,
    // Input fields
    id: u64,
    registered_at: Option<u64>,
    nonce_pointer: Option<u64>,
    multisig_pointer: Option<u64>,
    // Expected output
    serialized_hex: String,
    serialized_len: usize,
}

fn serialize_account(
    id: u64,
    registered_at: Option<u64>,
    nonce_pointer: Option<u64>,
    multisig_pointer: Option<u64>,
) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(write_u64_be(id));
    buf.extend(write_option_u64_be(registered_at));
    buf.extend(write_option_u64_be(nonce_pointer));
    buf.extend(write_option_u64_be(multisig_pointer));
    buf
}

fn generate_account_vectors() -> Vec<AccountTestVector> {
    let mut vectors = Vec::new();

    // === All 8 combinations of optional fields (2^3) ===
    // Format: RNM where R=registered_at, N=nonce, M=multisig
    // 0 = None, 1 = Some

    for mask in 0u8..8 {
        let has_r = (mask & 0b100) != 0;
        let has_n = (mask & 0b010) != 0;
        let has_m = (mask & 0b001) != 0;

        let r = if has_r { Some(100) } else { None };
        let n = if has_n { Some(200) } else { None };
        let m = if has_m { Some(300) } else { None };

        let s = serialize_account(mask as u64, r, n, m);
        let pattern = format!("{}{}{}",
            if has_r { "R" } else { "_" },
            if has_n { "N" } else { "_" },
            if has_m { "M" } else { "_" }
        );

        vectors.push(AccountTestVector {
            name: format!("account_combo_{:02}_{}", mask, pattern),
            description: format!("Account with optional fields: {} (mask=0b{:03b})", pattern, mask),
            id: mask as u64,
            registered_at: r,
            nonce_pointer: n,
            multisig_pointer: m,
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // === Boundary value tests ===

    // All fields at maximum
    {
        let s = serialize_account(u64::MAX, Some(u64::MAX), Some(u64::MAX), Some(u64::MAX));
        vectors.push(AccountTestVector {
            name: "account_all_max".to_string(),
            description: "All fields at maximum u64 value".to_string(),
            id: u64::MAX,
            registered_at: Some(u64::MAX),
            nonce_pointer: Some(u64::MAX),
            multisig_pointer: Some(u64::MAX),
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // All fields at zero (but Some)
    {
        let s = serialize_account(0, Some(0), Some(0), Some(0));
        vectors.push(AccountTestVector {
            name: "account_all_zero_some".to_string(),
            description: "All optional fields are Some(0)".to_string(),
            id: 0,
            registered_at: Some(0),
            nonce_pointer: Some(0),
            multisig_pointer: Some(0),
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // Typical genesis account
    {
        let s = serialize_account(0, Some(0), Some(0), None);
        vectors.push(AccountTestVector {
            name: "account_genesis_typical".to_string(),
            description: "Typical genesis account (registered at topo 0, nonce 0)".to_string(),
            id: 0,
            registered_at: Some(0),
            nonce_pointer: Some(0),
            multisig_pointer: None,
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // Active account
    {
        let s = serialize_account(12345, Some(1000), Some(5000), None);
        vectors.push(AccountTestVector {
            name: "account_active".to_string(),
            description: "Active account with recent nonce".to_string(),
            id: 12345,
            registered_at: Some(1000),
            nonce_pointer: Some(5000),
            multisig_pointer: None,
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // Multisig account
    {
        let s = serialize_account(99999, Some(500), Some(600), Some(700));
        vectors.push(AccountTestVector {
            name: "account_multisig_full".to_string(),
            description: "Multisig account with all features".to_string(),
            id: 99999,
            registered_at: Some(500),
            nonce_pointer: Some(600),
            multisig_pointer: Some(700),
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // Large account ID
    {
        let s = serialize_account(u64::MAX - 1, Some(1), None, None);
        vectors.push(AccountTestVector {
            name: "account_large_id".to_string(),
            description: "Account with very large ID".to_string(),
            id: u64::MAX - 1,
            registered_at: Some(1),
            nonce_pointer: None,
            multisig_pointer: None,
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    vectors
}

// ============================================================================
// VersionedNonce Serialization
// Order: previous_topoheight FIRST, then nonce
// ============================================================================

#[derive(Serialize)]
struct VersionedNonceTestVector {
    name: String,
    description: String,
    previous_topoheight: Option<u64>,
    nonce: u64,
    serialized_hex: String,
    serialized_len: usize,
}

fn serialize_versioned_nonce(previous_topoheight: Option<u64>, nonce: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(write_option_u64_be(previous_topoheight));
    buf.extend(write_u64_be(nonce));
    buf
}

fn generate_versioned_nonce_vectors() -> Vec<VersionedNonceTestVector> {
    vec![
        // Initial state (no previous)
        {
            let s = serialize_versioned_nonce(None, 0);
            VersionedNonceTestVector {
                name: "nonce_initial".to_string(),
                description: "Initial nonce (no previous, nonce=0)".to_string(),
                previous_topoheight: None,
                nonce: 0,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        // First transaction (nonce becomes 1)
        {
            let s = serialize_versioned_nonce(Some(0), 1);
            VersionedNonceTestVector {
                name: "nonce_first_tx".to_string(),
                description: "After first transaction".to_string(),
                previous_topoheight: Some(0),
                nonce: 1,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        // Chain of versions
        {
            let s = serialize_versioned_nonce(Some(100), 5);
            VersionedNonceTestVector {
                name: "nonce_chain_middle".to_string(),
                description: "Middle of version chain".to_string(),
                previous_topoheight: Some(100),
                nonce: 5,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        // High nonce value
        {
            let s = serialize_versioned_nonce(Some(999999), 1000);
            VersionedNonceTestVector {
                name: "nonce_high_activity".to_string(),
                description: "High activity account".to_string(),
                previous_topoheight: Some(999999),
                nonce: 1000,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        // Boundary: max values
        {
            let s = serialize_versioned_nonce(Some(u64::MAX), u64::MAX);
            VersionedNonceTestVector {
                name: "nonce_max_values".to_string(),
                description: "Maximum values for both fields".to_string(),
                previous_topoheight: Some(u64::MAX),
                nonce: u64::MAX,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        // Boundary: previous=Some(0), nonce=max
        {
            let s = serialize_versioned_nonce(Some(0), u64::MAX);
            VersionedNonceTestVector {
                name: "nonce_prev_zero_nonce_max".to_string(),
                description: "Previous at genesis, max nonce".to_string(),
                previous_topoheight: Some(0),
                nonce: u64::MAX,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        // No previous, max nonce
        {
            let s = serialize_versioned_nonce(None, u64::MAX);
            VersionedNonceTestVector {
                name: "nonce_no_prev_max_nonce".to_string(),
                description: "No previous, max nonce (edge case)".to_string(),
                previous_topoheight: None,
                nonce: u64::MAX,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
    ]
}

// ============================================================================
// VersionedBalance Serialization - All Combinations
// Order: previous_topoheight, balance_type, final_balance, output_balance
// ============================================================================

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
enum BalanceType {
    Input = 0,
    Output = 1,
    Both = 2,
}

#[derive(Serialize)]
struct VersionedBalanceTestVector {
    name: String,
    description: String,
    previous_topoheight: Option<u64>,
    balance_type: BalanceType,
    final_balance: u64,
    output_balance: Option<u64>,
    serialized_hex: String,
    serialized_len: usize,
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

fn generate_versioned_balance_vectors() -> Vec<VersionedBalanceTestVector> {
    let mut vectors = Vec::new();

    // === All 12 combinations: 3 types × 2 (prev) × 2 (output) ===

    let types = [
        (BalanceType::Input, "input"),
        (BalanceType::Output, "output"),
        (BalanceType::Both, "both"),
    ];

    for (bt, bt_name) in &types {
        for has_prev in [false, true] {
            for has_out in [false, true] {
                let prev = if has_prev { Some(100) } else { None };
                let out = if has_out { Some(50) } else { None };
                let s = serialize_versioned_balance(prev, *bt, 1000, out);

                let prev_str = if has_prev { "prev" } else { "noprev" };
                let out_str = if has_out { "out" } else { "noout" };

                vectors.push(VersionedBalanceTestVector {
                    name: format!("balance_{}_{}_{}",  bt_name, prev_str, out_str),
                    description: format!("Type={:?}, prev={}, output={}", bt, has_prev, has_out),
                    previous_topoheight: prev,
                    balance_type: *bt,
                    final_balance: 1000,
                    output_balance: out,
                    serialized_hex: to_hex(&s),
                    serialized_len: s.len(),
                });
            }
        }
    }

    // === Boundary value tests ===

    // Zero balance
    {
        let s = serialize_versioned_balance(None, BalanceType::Input, 0, None);
        vectors.push(VersionedBalanceTestVector {
            name: "balance_zero".to_string(),
            description: "Zero balance (empty account)".to_string(),
            previous_topoheight: None,
            balance_type: BalanceType::Input,
            final_balance: 0,
            output_balance: None,
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // Max balance
    {
        let s = serialize_versioned_balance(Some(u64::MAX), BalanceType::Both, u64::MAX, Some(u64::MAX));
        vectors.push(VersionedBalanceTestVector {
            name: "balance_all_max".to_string(),
            description: "All fields at maximum".to_string(),
            previous_topoheight: Some(u64::MAX),
            balance_type: BalanceType::Both,
            final_balance: u64::MAX,
            output_balance: Some(u64::MAX),
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // Typical: receiving funds
    {
        let s = serialize_versioned_balance(Some(1000), BalanceType::Input, 10_000_000_000, None);
        vectors.push(VersionedBalanceTestVector {
            name: "balance_receive_100_tos".to_string(),
            description: "Received 100 TOS (input only)".to_string(),
            previous_topoheight: Some(1000),
            balance_type: BalanceType::Input,
            final_balance: 10_000_000_000, // 100 TOS
            output_balance: None,
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // Typical: sending funds
    {
        let s = serialize_versioned_balance(Some(2000), BalanceType::Output, 5_000_000_000, Some(5_000_000_000));
        vectors.push(VersionedBalanceTestVector {
            name: "balance_send_50_tos".to_string(),
            description: "Sent 50 TOS (output type)".to_string(),
            previous_topoheight: Some(2000),
            balance_type: BalanceType::Output,
            final_balance: 5_000_000_000, // 50 TOS remaining
            output_balance: Some(5_000_000_000), // 50 TOS sent
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // Complex: both input and output in same block
    {
        let s = serialize_versioned_balance(Some(3000), BalanceType::Both, 15_000_000_000, Some(2_000_000_000));
        vectors.push(VersionedBalanceTestVector {
            name: "balance_both_in_out".to_string(),
            description: "Both received and sent in same block".to_string(),
            previous_topoheight: Some(3000),
            balance_type: BalanceType::Both,
            final_balance: 15_000_000_000, // 150 TOS final
            output_balance: Some(2_000_000_000), // 20 TOS pending output
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // Genesis balance
    {
        let s = serialize_versioned_balance(None, BalanceType::Input, 100_000_000_000_000_000, None);
        vectors.push(VersionedBalanceTestVector {
            name: "balance_genesis_premine".to_string(),
            description: "Genesis premine balance (1B TOS)".to_string(),
            previous_topoheight: None,
            balance_type: BalanceType::Input,
            final_balance: 100_000_000_000_000_000, // 1B TOS
            output_balance: None,
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // output_balance = Some(0)
    {
        let s = serialize_versioned_balance(Some(500), BalanceType::Both, 1000, Some(0));
        vectors.push(VersionedBalanceTestVector {
            name: "balance_output_zero".to_string(),
            description: "Output balance is Some(0)".to_string(),
            previous_topoheight: Some(500),
            balance_type: BalanceType::Both,
            final_balance: 1000,
            output_balance: Some(0),
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // final_balance = 0, output_balance = Some(value)
    {
        let s = serialize_versioned_balance(Some(600), BalanceType::Output, 0, Some(1000));
        vectors.push(VersionedBalanceTestVector {
            name: "balance_final_zero_output_nonzero".to_string(),
            description: "Final balance 0 with pending output".to_string(),
            previous_topoheight: Some(600),
            balance_type: BalanceType::Output,
            final_balance: 0,
            output_balance: Some(1000),
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    vectors
}

// ============================================================================
// Asset Serialization
// Structure: id (u64) + data_pointer (Option<u64>) + supply_pointer (Option<u64>)
// ============================================================================

#[derive(Serialize)]
struct AssetTestVector {
    name: String,
    description: String,
    id: u64,
    data_pointer: Option<u64>,
    supply_pointer: Option<u64>,
    serialized_hex: String,
    serialized_len: usize,
}

fn serialize_asset(id: u64, data_pointer: Option<u64>, supply_pointer: Option<u64>) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(write_u64_be(id));
    buf.extend(write_option_u64_be(data_pointer));
    buf.extend(write_option_u64_be(supply_pointer));
    buf
}

fn generate_asset_vectors() -> Vec<AssetTestVector> {
    let mut vectors = Vec::new();

    // All 4 combinations of optional fields
    for mask in 0u8..4 {
        let has_data = (mask & 0b10) != 0;
        let has_supply = (mask & 0b01) != 0;
        let data = if has_data { Some(100) } else { None };
        let supply = if has_supply { Some(200) } else { None };
        let s = serialize_asset(mask as u64, data, supply);

        vectors.push(AssetTestVector {
            name: format!("asset_combo_{:02}", mask),
            description: format!("Asset with data={}, supply={}", has_data, has_supply),
            id: mask as u64,
            data_pointer: data,
            supply_pointer: supply,
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // Boundary values
    {
        let s = serialize_asset(u64::MAX, Some(u64::MAX), Some(u64::MAX));
        vectors.push(AssetTestVector {
            name: "asset_all_max".to_string(),
            description: "All fields at maximum".to_string(),
            id: u64::MAX,
            data_pointer: Some(u64::MAX),
            supply_pointer: Some(u64::MAX),
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    // Native asset (id=0)
    {
        let s = serialize_asset(0, Some(0), Some(1));
        vectors.push(AssetTestVector {
            name: "asset_native".to_string(),
            description: "Native TOS asset (id=0)".to_string(),
            id: 0,
            data_pointer: Some(0),
            supply_pointer: Some(1),
            serialized_hex: to_hex(&s),
            serialized_len: s.len(),
        });
    }

    vectors
}

// ============================================================================
// Contract Serialization
// Structure: id (u64) + module_pointer (Option<u64>)
// ============================================================================

#[derive(Serialize)]
struct ContractTestVector {
    name: String,
    description: String,
    id: u64,
    module_pointer: Option<u64>,
    serialized_hex: String,
    serialized_len: usize,
}

fn serialize_contract(id: u64, module_pointer: Option<u64>) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(write_u64_be(id));
    buf.extend(write_option_u64_be(module_pointer));
    buf
}

fn generate_contract_vectors() -> Vec<ContractTestVector> {
    vec![
        {
            let s = serialize_contract(0, None);
            ContractTestVector {
                name: "contract_no_module".to_string(),
                description: "Contract without module pointer".to_string(),
                id: 0,
                module_pointer: None,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let s = serialize_contract(1, Some(100));
            ContractTestVector {
                name: "contract_with_module".to_string(),
                description: "Contract with module pointer".to_string(),
                id: 1,
                module_pointer: Some(100),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let s = serialize_contract(u64::MAX, Some(u64::MAX));
            ContractTestVector {
                name: "contract_max_values".to_string(),
                description: "Contract with max values".to_string(),
                id: u64::MAX,
                module_pointer: Some(u64::MAX),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let s = serialize_contract(12345, Some(0));
            ContractTestVector {
                name: "contract_module_zero".to_string(),
                description: "Contract with module_pointer=Some(0)".to_string(),
                id: 12345,
                module_pointer: Some(0),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
    ]
}

// ============================================================================
// AgentAccountMetaPointer Serialization
// Structure: has_meta (bool as u8: 0x00 or 0x01)
// ============================================================================

#[derive(Serialize)]
struct AgentAccountMetaPointerTestVector {
    name: String,
    description: String,
    has_meta: bool,
    serialized_hex: String,
    serialized_len: usize,
}

fn serialize_agent_meta(has_meta: bool) -> Vec<u8> {
    vec![if has_meta { 0x01 } else { 0x00 }]
}

fn generate_agent_meta_vectors() -> Vec<AgentAccountMetaPointerTestVector> {
    vec![
        {
            let s = serialize_agent_meta(false);
            AgentAccountMetaPointerTestVector {
                name: "agent_meta_false".to_string(),
                description: "No agent metadata".to_string(),
                has_meta: false,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let s = serialize_agent_meta(true);
            AgentAccountMetaPointerTestVector {
                name: "agent_meta_true".to_string(),
                description: "Has agent metadata".to_string(),
                has_meta: true,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
    ]
}

// ============================================================================
// TopoHeightMetadata Serialization
// Structure: rewards (u64) + emitted_supply (u64) + burned_supply (u64)
// ============================================================================

#[derive(Serialize)]
struct TopoHeightMetadataTestVector {
    name: String,
    description: String,
    rewards: u64,
    emitted_supply: u64,
    burned_supply: u64,
    serialized_hex: String,
    serialized_len: usize,
}

fn serialize_topo_metadata(rewards: u64, emitted_supply: u64, burned_supply: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(write_u64_be(rewards));
    buf.extend(write_u64_be(emitted_supply));
    buf.extend(write_u64_be(burned_supply));
    buf
}

fn generate_topo_metadata_vectors() -> Vec<TopoHeightMetadataTestVector> {
    vec![
        {
            let s = serialize_topo_metadata(0, 0, 0);
            TopoHeightMetadataTestVector {
                name: "topo_meta_genesis".to_string(),
                description: "Genesis block metadata (all zeros)".to_string(),
                rewards: 0,
                emitted_supply: 0,
                burned_supply: 0,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let s = serialize_topo_metadata(100_000_000, 1_000_000_000_000, 0);
            TopoHeightMetadataTestVector {
                name: "topo_meta_early".to_string(),
                description: "Early block with rewards, no burns".to_string(),
                rewards: 100_000_000, // 1 TOS reward
                emitted_supply: 1_000_000_000_000, // 10k TOS emitted
                burned_supply: 0,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let s = serialize_topo_metadata(50_000_000, 5_000_000_000_000_000, 100_000_000_000);
            TopoHeightMetadataTestVector {
                name: "topo_meta_mature".to_string(),
                description: "Mature network with burns".to_string(),
                rewards: 50_000_000, // 0.5 TOS reward (halved)
                emitted_supply: 5_000_000_000_000_000, // 50M TOS
                burned_supply: 100_000_000_000, // 1000 TOS burned
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let s = serialize_topo_metadata(u64::MAX, u64::MAX, u64::MAX);
            TopoHeightMetadataTestVector {
                name: "topo_meta_max".to_string(),
                description: "All maximum values".to_string(),
                rewards: u64::MAX,
                emitted_supply: u64::MAX,
                burned_supply: u64::MAX,
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
    ]
}

// ============================================================================
// VarUint Serialization (for BlockDifficulty)
// Format: 1 byte length (1-32) + big-endian bytes
// ============================================================================

#[derive(Serialize)]
struct VarUintTestVector {
    name: String,
    description: String,
    value_hex: String,  // U256 as hex
    serialized_hex: String,
    serialized_len: usize,
}

fn serialize_varuint(bytes: &[u8]) -> Vec<u8> {
    // Trim leading zeros
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len() - 1);
    let trimmed = &bytes[start..];
    let len = trimmed.len().max(1);

    let mut buf = vec![len as u8];
    if trimmed.is_empty() {
        buf.push(0);
    } else {
        buf.extend(trimmed);
    }
    buf
}

fn u256_from_u64(val: u64) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[24..32].copy_from_slice(&val.to_be_bytes());
    bytes
}

fn generate_varuint_vectors() -> Vec<VarUintTestVector> {
    vec![
        {
            let bytes = u256_from_u64(0);
            let s = serialize_varuint(&bytes);
            VarUintTestVector {
                name: "varuint_zero".to_string(),
                description: "VarUint(0) - minimum encoding".to_string(),
                value_hex: "0".to_string(),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let bytes = u256_from_u64(1);
            let s = serialize_varuint(&bytes);
            VarUintTestVector {
                name: "varuint_one".to_string(),
                description: "VarUint(1)".to_string(),
                value_hex: "1".to_string(),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let bytes = u256_from_u64(255);
            let s = serialize_varuint(&bytes);
            VarUintTestVector {
                name: "varuint_255".to_string(),
                description: "VarUint(255) - 1 byte value".to_string(),
                value_hex: "ff".to_string(),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let bytes = u256_from_u64(256);
            let s = serialize_varuint(&bytes);
            VarUintTestVector {
                name: "varuint_256".to_string(),
                description: "VarUint(256) - 2 byte value".to_string(),
                value_hex: "100".to_string(),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let bytes = u256_from_u64(0xFFFF);
            let s = serialize_varuint(&bytes);
            VarUintTestVector {
                name: "varuint_u16_max".to_string(),
                description: "VarUint(65535)".to_string(),
                value_hex: "ffff".to_string(),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let bytes = u256_from_u64(1_000_000_000);
            let s = serialize_varuint(&bytes);
            VarUintTestVector {
                name: "varuint_1b".to_string(),
                description: "VarUint(1 billion) - typical difficulty".to_string(),
                value_hex: "3b9aca00".to_string(),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let bytes = u256_from_u64(u64::MAX);
            let s = serialize_varuint(&bytes);
            VarUintTestVector {
                name: "varuint_u64_max".to_string(),
                description: "VarUint(u64::MAX)".to_string(),
                value_hex: "ffffffffffffffff".to_string(),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        // Full U256 max (32 bytes)
        {
            let bytes = [0xFF; 32];
            let s = serialize_varuint(&bytes);
            VarUintTestVector {
                name: "varuint_u256_max".to_string(),
                description: "VarUint(U256::MAX) - maximum encoding".to_string(),
                value_hex: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
    ]
}

// ============================================================================
// BlockDifficulty Serialization
// Structure: difficulty (VarUint) + cumulative_difficulty (VarUint) + covariance (VarUint)
// ============================================================================

#[derive(Serialize)]
struct BlockDifficultyTestVector {
    name: String,
    description: String,
    difficulty_hex: String,
    cumulative_difficulty_hex: String,
    covariance_hex: String,
    serialized_hex: String,
    serialized_len: usize,
}

fn generate_block_difficulty_vectors() -> Vec<BlockDifficultyTestVector> {
    vec![
        {
            let d = serialize_varuint(&u256_from_u64(1));
            let cd = serialize_varuint(&u256_from_u64(1));
            let cov = serialize_varuint(&u256_from_u64(0));
            let mut s = Vec::new();
            s.extend(&d);
            s.extend(&cd);
            s.extend(&cov);
            BlockDifficultyTestVector {
                name: "block_diff_genesis".to_string(),
                description: "Genesis block difficulty".to_string(),
                difficulty_hex: "1".to_string(),
                cumulative_difficulty_hex: "1".to_string(),
                covariance_hex: "0".to_string(),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let d = serialize_varuint(&u256_from_u64(1_000_000_000));
            let cd = serialize_varuint(&u256_from_u64(100_000_000_000));
            let cov = serialize_varuint(&u256_from_u64(500_000));
            let mut s = Vec::new();
            s.extend(&d);
            s.extend(&cd);
            s.extend(&cov);
            BlockDifficultyTestVector {
                name: "block_diff_typical".to_string(),
                description: "Typical mainnet difficulty".to_string(),
                difficulty_hex: "3b9aca00".to_string(),
                cumulative_difficulty_hex: "174876e800".to_string(),
                covariance_hex: "7a120".to_string(),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
        {
            let d = serialize_varuint(&u256_from_u64(u64::MAX));
            let cd = serialize_varuint(&u256_from_u64(u64::MAX));
            let cov = serialize_varuint(&u256_from_u64(u64::MAX));
            let mut s = Vec::new();
            s.extend(&d);
            s.extend(&cd);
            s.extend(&cov);
            BlockDifficultyTestVector {
                name: "block_diff_max_u64".to_string(),
                description: "Maximum u64 values".to_string(),
                difficulty_hex: "ffffffffffffffff".to_string(),
                cumulative_difficulty_hex: "ffffffffffffffff".to_string(),
                covariance_hex: "ffffffffffffffff".to_string(),
                serialized_hex: to_hex(&s),
                serialized_len: s.len(),
            }
        },
    ]
}

// ============================================================================
// Main Output Structure
// ============================================================================

#[derive(Serialize)]
struct SerializationTestVectors {
    description: String,
    version: String,
    note: String,
    total_vectors: usize,
    big_endian_vectors: Vec<BigEndianTestVector>,
    option_u64_vectors: Vec<OptionU64TestVector>,
    account_vectors: Vec<AccountTestVector>,
    versioned_nonce_vectors: Vec<VersionedNonceTestVector>,
    versioned_balance_vectors: Vec<VersionedBalanceTestVector>,
    asset_vectors: Vec<AssetTestVector>,
    contract_vectors: Vec<ContractTestVector>,
    agent_meta_vectors: Vec<AgentAccountMetaPointerTestVector>,
    topo_metadata_vectors: Vec<TopoHeightMetadataTestVector>,
    varuint_vectors: Vec<VarUintTestVector>,
    block_difficulty_vectors: Vec<BlockDifficultyTestVector>,
}

fn main() {
    let be_vectors = generate_big_endian_vectors();
    let opt_vectors = generate_option_u64_vectors();
    let acc_vectors = generate_account_vectors();
    let nonce_vectors = generate_versioned_nonce_vectors();
    let balance_vectors = generate_versioned_balance_vectors();
    let asset_vectors = generate_asset_vectors();
    let contract_vectors = generate_contract_vectors();
    let agent_meta_vectors = generate_agent_meta_vectors();
    let topo_metadata_vectors = generate_topo_metadata_vectors();
    let varuint_vectors = generate_varuint_vectors();
    let block_difficulty_vectors = generate_block_difficulty_vectors();

    let total = be_vectors.len() + opt_vectors.len() + acc_vectors.len()
        + nonce_vectors.len() + balance_vectors.len()
        + asset_vectors.len() + contract_vectors.len() + agent_meta_vectors.len()
        + topo_metadata_vectors.len() + varuint_vectors.len()
        + block_difficulty_vectors.len();

    let vectors = SerializationTestVectors {
        description: "RocksDB serialization test vectors for TOS/Avatar compatibility".to_string(),
        version: "3.0".to_string(),
        note: "Complete coverage of all RocksDB storage types. All integers use big-endian encoding. Option<T> uses 1-byte flag (0x00=None, 0x01=Some). VarUint uses 1-byte length prefix.".to_string(),
        total_vectors: total,
        big_endian_vectors: be_vectors,
        option_u64_vectors: opt_vectors,
        account_vectors: acc_vectors,
        versioned_nonce_vectors: nonce_vectors,
        versioned_balance_vectors: balance_vectors,
        asset_vectors,
        contract_vectors,
        agent_meta_vectors,
        topo_metadata_vectors,
        varuint_vectors,
        block_difficulty_vectors,
    };

    let yaml = serde_yaml::to_string(&vectors).expect("Failed to serialize to YAML");

    let mut file = File::create("serialization.yaml").expect("Failed to create file");
    file.write_all(yaml.as_bytes()).expect("Failed to write file");

    println!("Generated serialization.yaml (v3.0 - 100% type coverage)");
    println!("  Total vectors: {}", total);
    println!("  Primitives:");
    println!("    - {} big-endian vectors", vectors.big_endian_vectors.len());
    println!("    - {} option<u64> vectors", vectors.option_u64_vectors.len());
    println!("    - {} varuint vectors", vectors.varuint_vectors.len());
    println!("  Account Types:");
    println!("    - {} account vectors", vectors.account_vectors.len());
    println!("    - {} versioned_nonce vectors", vectors.versioned_nonce_vectors.len());
    println!("    - {} versioned_balance vectors", vectors.versioned_balance_vectors.len());
    println!("  Asset/Contract Types:");
    println!("    - {} asset vectors", vectors.asset_vectors.len());
    println!("    - {} contract vectors", vectors.contract_vectors.len());
    println!("    - {} agent_meta vectors", vectors.agent_meta_vectors.len());
    println!("  Block Types:");
    println!("    - {} topo_metadata vectors", vectors.topo_metadata_vectors.len());
    println!("    - {} block_difficulty vectors", vectors.block_difficulty_vectors.len());
}
