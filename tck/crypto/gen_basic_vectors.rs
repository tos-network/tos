// Generate Basic Transaction (Types 0, 1, 5) wire format test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_basic_vectors
//
// These vectors are authoritative for Avatar C cross-validation.
// TOS Rust is the reference implementation.

use serde::Serialize;
use std::fs::File;
use std::io::Write;

// Import TOS common types
use tos_common::account::FreezeDuration;
use tos_common::crypto::{elgamal::CompressedPublicKey, Hash, PublicKey};
use tos_common::serializer::Serializer;
use tos_common::transaction::{BurnPayload, EnergyPayload, TransferPayload, DelegationEntry};

// ============================================================================
// Test Vector Structs
// ============================================================================

#[derive(Serialize)]
struct BurnVector {
    name: String,
    description: String,
    asset_hex: String,
    amount: u64,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct TransferVector {
    name: String,
    description: String,
    asset_hex: String,
    destination_hex: String,
    amount: u64,
    has_extra_data: bool,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct EnergyVector {
    name: String,
    description: String,
    variant: u8,
    amount: u64,
    duration_days: Option<u32>,
    delegatees_cnt: Option<u16>,
    from_delegation: Option<bool>,
    has_record_index: Option<bool>,
    has_delegatee_address: Option<bool>,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct BasicTestFile {
    algorithm: String,
    version: u32,
    burn_vectors: Vec<BurnVector>,
    transfer_vectors: Vec<TransferVector>,
    energy_vectors: Vec<EnergyVector>,
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    // Pre-generate test values with deterministic bytes
    let test_asset = Hash::new([0xAAu8; 32]);
    let test_destination = CompressedPublicKey::from_bytes(&[0x01u8; 32]).unwrap();
    let test_delegatee1 = PublicKey::from_bytes(&[0x11u8; 32]).unwrap();
    let test_delegatee2 = PublicKey::from_bytes(&[0x22u8; 32]).unwrap();

    let mut burn_vectors = Vec::new();
    let mut transfer_vectors = Vec::new();
    let mut energy_vectors = Vec::new();

    // ========================================================================
    // Burn (Type 0) Test Vectors
    // ========================================================================

    // Test 1: Basic burn
    {
        let payload = BurnPayload {
            asset: test_asset.clone(),
            amount: 1_000_000_000,
        };
        burn_vectors.push(BurnVector {
            name: "burn_basic".to_string(),
            description: "Basic burn of 10 TOS".to_string(),
            asset_hex: hex::encode(test_asset.as_bytes()),
            amount: 1_000_000_000,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: Burn max amount
    {
        let payload = BurnPayload {
            asset: test_asset.clone(),
            amount: u64::MAX,
        };
        burn_vectors.push(BurnVector {
            name: "burn_max".to_string(),
            description: "Burn maximum u64 amount".to_string(),
            asset_hex: hex::encode(test_asset.as_bytes()),
            amount: u64::MAX,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 3: Burn 1 unit
    {
        let payload = BurnPayload {
            asset: test_asset.clone(),
            amount: 1,
        };
        burn_vectors.push(BurnVector {
            name: "burn_min".to_string(),
            description: "Burn minimum (1 unit)".to_string(),
            asset_hex: hex::encode(test_asset.as_bytes()),
            amount: 1,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // Transfer (Type 1) Test Vectors
    // ========================================================================

    // Test 1: Basic transfer without extra_data
    {
        let payload = TransferPayload::new(
            test_asset.clone(),
            test_destination.clone(),
            500_000_000,
            None,
        );
        transfer_vectors.push(TransferVector {
            name: "transfer_basic".to_string(),
            description: "Basic transfer without extra data".to_string(),
            asset_hex: hex::encode(test_asset.as_bytes()),
            destination_hex: hex::encode(test_destination.as_bytes()),
            amount: 500_000_000,
            has_extra_data: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: Large transfer
    {
        let payload = TransferPayload::new(
            test_asset.clone(),
            test_destination.clone(),
            100_000_000_000, // 1000 TOS
            None,
        );
        transfer_vectors.push(TransferVector {
            name: "transfer_large".to_string(),
            description: "Large transfer (1000 TOS)".to_string(),
            asset_hex: hex::encode(test_asset.as_bytes()),
            destination_hex: hex::encode(test_destination.as_bytes()),
            amount: 100_000_000_000,
            has_extra_data: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // Energy (Type 5) Test Vectors
    // ========================================================================

    // Test 1: FreezeTos (variant 0)
    {
        let duration = FreezeDuration::new(7).unwrap();
        let payload = EnergyPayload::FreezeTos {
            amount: 100_000_000, // 1 TOS
            duration,
        };
        energy_vectors.push(EnergyVector {
            name: "energy_freeze_basic".to_string(),
            description: "Freeze 1 TOS for 7 days".to_string(),
            variant: 0,
            amount: 100_000_000,
            duration_days: Some(7),
            delegatees_cnt: None,
            from_delegation: None,
            has_record_index: None,
            has_delegatee_address: None,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: FreezeTos with 30 days
    {
        let duration = FreezeDuration::new(30).unwrap();
        let payload = EnergyPayload::FreezeTos {
            amount: 500_000_000, // 5 TOS
            duration,
        };
        energy_vectors.push(EnergyVector {
            name: "energy_freeze_30days".to_string(),
            description: "Freeze 5 TOS for 30 days".to_string(),
            variant: 0,
            amount: 500_000_000,
            duration_days: Some(30),
            delegatees_cnt: None,
            from_delegation: None,
            has_record_index: None,
            has_delegatee_address: None,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 3: FreezeTosDelegate (variant 1)
    {
        let duration = FreezeDuration::new(14).unwrap();
        let delegatees = vec![
            DelegationEntry {
                delegatee: test_delegatee1.clone(),
                amount: 100_000_000, // 1 TOS
            },
            DelegationEntry {
                delegatee: test_delegatee2.clone(),
                amount: 200_000_000, // 2 TOS
            },
        ];
        let payload = EnergyPayload::FreezeTosDelegate {
            delegatees,
            duration,
        };
        energy_vectors.push(EnergyVector {
            name: "energy_delegate_multi".to_string(),
            description: "Delegate to 2 accounts for 14 days".to_string(),
            variant: 1,
            amount: 300_000_000, // Total
            duration_days: Some(14),
            delegatees_cnt: Some(2),
            from_delegation: None,
            has_record_index: None,
            has_delegatee_address: None,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 4: UnfreezeTos basic (variant 2)
    {
        let payload = EnergyPayload::UnfreezeTos {
            amount: 100_000_000,
            from_delegation: false,
            record_index: None,
            delegatee_address: None,
        };
        energy_vectors.push(EnergyVector {
            name: "energy_unfreeze_basic".to_string(),
            description: "Unfreeze 1 TOS from self-freeze".to_string(),
            variant: 2,
            amount: 100_000_000,
            duration_days: None,
            delegatees_cnt: None,
            from_delegation: Some(false),
            has_record_index: Some(false),
            has_delegatee_address: Some(false),
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 5: UnfreezeTos from delegation with record_index
    {
        let payload = EnergyPayload::UnfreezeTos {
            amount: 200_000_000,
            from_delegation: true,
            record_index: Some(5),
            delegatee_address: None,
        };
        energy_vectors.push(EnergyVector {
            name: "energy_unfreeze_delegation".to_string(),
            description: "Unfreeze from delegation with record index".to_string(),
            variant: 2,
            amount: 200_000_000,
            duration_days: None,
            delegatees_cnt: None,
            from_delegation: Some(true),
            has_record_index: Some(true),
            has_delegatee_address: Some(false),
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 6: UnfreezeTos with delegatee_address
    {
        let payload = EnergyPayload::UnfreezeTos {
            amount: 100_000_000,
            from_delegation: true,
            record_index: Some(0),
            delegatee_address: Some(test_delegatee1.clone()),
        };
        energy_vectors.push(EnergyVector {
            name: "energy_unfreeze_with_delegatee".to_string(),
            description: "Unfreeze with delegatee address".to_string(),
            variant: 2,
            amount: 100_000_000,
            duration_days: None,
            delegatees_cnt: None,
            from_delegation: Some(true),
            has_record_index: Some(true),
            has_delegatee_address: Some(true),
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 7: WithdrawUnfrozen (variant 3)
    {
        let payload = EnergyPayload::WithdrawUnfrozen;
        energy_vectors.push(EnergyVector {
            name: "energy_withdraw".to_string(),
            description: "Withdraw unfrozen TOS".to_string(),
            variant: 3,
            amount: 0,
            duration_days: None,
            delegatees_cnt: None,
            from_delegation: None,
            has_record_index: None,
            has_delegatee_address: None,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // Write Output
    // ========================================================================

    let test_file = BasicTestFile {
        algorithm: "Basic-Transactions".to_string(),
        version: 1,
        burn_vectors,
        transfer_vectors,
        energy_vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).expect("YAML serialization failed");

    // Add header comment
    let header = r#"# Basic Transactions Test Vectors (Types 0, 1, 5)
# Generated by TOS Rust - gen_basic_vectors
# Cross-language verification between TOS Rust and Avatar C
#
# Transaction Types:
#   Type 0: Burn - Destroy tokens permanently
#   Type 1: Transfer - Send tokens to destination
#   Type 5: Energy - Freeze/delegate/unfreeze operations
#
# Energy Variants:
#   0: FreezeTos - Lock TOS for self
#   1: FreezeTosDelegate - Lock TOS and delegate energy to others
#   2: UnfreezeTos - Unlock frozen TOS
#   3: WithdrawUnfrozen - Retrieve TOS after cooldown

"#;

    let full_yaml = format!("{}{}", header, yaml);
    println!("{}", full_yaml);

    let mut file = File::create("basic.yaml").expect("Failed to create file");
    file.write_all(full_yaml.as_bytes())
        .expect("Failed to write file");
    eprintln!("Written to basic.yaml");
}
