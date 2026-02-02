// Referral/Agent Transaction Test Vector Generator (Types 7, 8, 23)
// Generates test vectors for cross-language verification between TOS Rust and Avatar C
//
// Transaction Types:
//   Type 7:  BindReferrer - Bind a referrer to sender account (one-time)
//   Type 8:  BatchReferralReward - Distribute rewards to uplines
//   Type 23: AgentAccount - AI agent account operations

use hex;
use serde::Serialize;
use std::fs::File;
use std::io::Write;
use tos_common::account::SessionKey;
use tos_common::crypto::elgamal::CompressedPublicKey;
use tos_common::crypto::{Hash, PublicKey};
use tos_common::serializer::Serializer;
use tos_common::transaction::{AgentAccountPayload, BatchReferralRewardPayload, BindReferrerPayload};

// ============================================================================
// YAML Structures
// ============================================================================

#[derive(Serialize)]
struct ReferralTestVectors {
    algorithm: String,
    version: u32,
    bind_referrer_vectors: Vec<BindReferrerVector>,
    batch_referral_vectors: Vec<BatchReferralVector>,
    agent_account_vectors: Vec<AgentAccountVector>,
}

#[derive(Serialize)]
struct BindReferrerVector {
    name: String,
    description: String,
    referrer_hex: String,
    has_extra_data: bool,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct BatchReferralVector {
    name: String,
    description: String,
    asset_hex: String,
    from_user_hex: String,
    total_amount: u64,
    levels: u8,
    ratios: Vec<u16>,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct AgentAccountVector {
    name: String,
    description: String,
    variant: u8,
    wire_hex: String,
    expected_size: usize,
}

// ============================================================================
// Helper Functions
// ============================================================================

fn test_pubkey(seed: u8) -> CompressedPublicKey {
    let bytes = [seed; 32];
    CompressedPublicKey::from_bytes(&bytes).expect("Valid pubkey bytes")
}

fn test_full_pubkey(seed: u8) -> PublicKey {
    let bytes = [seed; 32];
    PublicKey::from_bytes(&bytes).expect("Valid pubkey bytes")
}

fn test_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

// ============================================================================
// Vector Generation
// ============================================================================

fn gen_bind_referrer_vectors() -> Vec<BindReferrerVector> {
    let mut vectors = Vec::new();

    // Basic bind referrer without extra data
    {
        let referrer = test_pubkey(0x11);
        let payload = BindReferrerPayload::new(referrer.clone(), None);
        let wire = payload.to_bytes();

        vectors.push(BindReferrerVector {
            name: "bind_referrer_basic".to_string(),
            description: "Bind referrer without extra data".to_string(),
            referrer_hex: hex::encode(referrer.as_bytes()),
            has_extra_data: false,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Another referrer with different seed
    {
        let referrer = test_pubkey(0xAA);
        let payload = BindReferrerPayload::new(referrer.clone(), None);
        let wire = payload.to_bytes();

        vectors.push(BindReferrerVector {
            name: "bind_referrer_alt".to_string(),
            description: "Bind referrer with alternate pubkey".to_string(),
            referrer_hex: hex::encode(referrer.as_bytes()),
            has_extra_data: false,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

fn gen_batch_referral_vectors() -> Vec<BatchReferralVector> {
    let mut vectors = Vec::new();

    // Basic batch reward with 3 levels
    {
        let asset = test_hash(0xAA);
        let from_user = test_pubkey(0x11);
        let total_amount = 1_000_000_000u64; // 10 TOS
        let levels = 3u8;
        let ratios = vec![1000u16, 500, 300]; // 10%, 5%, 3%

        let payload = BatchReferralRewardPayload::new(
            asset.clone(),
            from_user.clone(),
            total_amount,
            levels,
            ratios.clone(),
        );
        let wire = payload.to_bytes();

        vectors.push(BatchReferralVector {
            name: "batch_referral_3levels".to_string(),
            description: "Batch reward to 3 upline levels".to_string(),
            asset_hex: hex::encode(asset.as_bytes()),
            from_user_hex: hex::encode(from_user.as_bytes()),
            total_amount,
            levels,
            ratios,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Single level reward
    {
        let asset = test_hash(0xBB);
        let from_user = test_pubkey(0x22);
        let total_amount = 500_000_000u64; // 5 TOS
        let levels = 1u8;
        let ratios = vec![500u16]; // 5%

        let payload = BatchReferralRewardPayload::new(
            asset.clone(),
            from_user.clone(),
            total_amount,
            levels,
            ratios.clone(),
        );
        let wire = payload.to_bytes();

        vectors.push(BatchReferralVector {
            name: "batch_referral_single".to_string(),
            description: "Batch reward to single upline level".to_string(),
            asset_hex: hex::encode(asset.as_bytes()),
            from_user_hex: hex::encode(from_user.as_bytes()),
            total_amount,
            levels,
            ratios,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Five levels with varied ratios
    {
        let asset = test_hash(0xCC);
        let from_user = test_pubkey(0x33);
        let total_amount = 10_000_000_000u64; // 100 TOS
        let levels = 5u8;
        let ratios = vec![1000u16, 800, 600, 400, 200]; // 10%, 8%, 6%, 4%, 2%

        let payload = BatchReferralRewardPayload::new(
            asset.clone(),
            from_user.clone(),
            total_amount,
            levels,
            ratios.clone(),
        );
        let wire = payload.to_bytes();

        vectors.push(BatchReferralVector {
            name: "batch_referral_5levels".to_string(),
            description: "Batch reward to 5 upline levels".to_string(),
            asset_hex: hex::encode(asset.as_bytes()),
            from_user_hex: hex::encode(from_user.as_bytes()),
            total_amount,
            levels,
            ratios,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

fn gen_agent_account_vectors() -> Vec<AgentAccountVector> {
    let mut vectors = Vec::new();

    // Register agent (variant 0)
    {
        let controller = test_full_pubkey(0x11);
        let policy_hash = test_hash(0x22);
        let payload = AgentAccountPayload::Register {
            controller,
            policy_hash,
            energy_pool: None,
            session_key_root: None,
        };
        let wire = payload.to_bytes();

        vectors.push(AgentAccountVector {
            name: "agent_register_basic".to_string(),
            description: "Register AI agent without optional fields".to_string(),
            variant: 0,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Register with energy pool
    {
        let controller = test_full_pubkey(0x33);
        let policy_hash = test_hash(0x44);
        let energy_pool = Some(test_full_pubkey(0x55));
        let payload = AgentAccountPayload::Register {
            controller,
            policy_hash,
            energy_pool,
            session_key_root: None,
        };
        let wire = payload.to_bytes();

        vectors.push(AgentAccountVector {
            name: "agent_register_with_energy".to_string(),
            description: "Register AI agent with energy pool".to_string(),
            variant: 0,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Register with all optional fields
    {
        let controller = test_full_pubkey(0x66);
        let policy_hash = test_hash(0x77);
        let energy_pool = Some(test_full_pubkey(0x88));
        let session_key_root = Some(test_hash(0x99));
        let payload = AgentAccountPayload::Register {
            controller,
            policy_hash,
            energy_pool,
            session_key_root,
        };
        let wire = payload.to_bytes();

        vectors.push(AgentAccountVector {
            name: "agent_register_full".to_string(),
            description: "Register AI agent with all options".to_string(),
            variant: 0,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // UpdatePolicy (variant 1)
    {
        let policy_hash = test_hash(0xAA);
        let payload = AgentAccountPayload::UpdatePolicy { policy_hash };
        let wire = payload.to_bytes();

        vectors.push(AgentAccountVector {
            name: "agent_update_policy".to_string(),
            description: "Update agent policy hash".to_string(),
            variant: 1,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // RotateController (variant 2)
    {
        let new_controller = test_full_pubkey(0xBB);
        let payload = AgentAccountPayload::RotateController { new_controller };
        let wire = payload.to_bytes();

        vectors.push(AgentAccountVector {
            name: "agent_rotate_controller".to_string(),
            description: "Rotate agent controller".to_string(),
            variant: 2,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // SetStatus (variant 3)
    {
        let payload = AgentAccountPayload::SetStatus { status: 1 };
        let wire = payload.to_bytes();

        vectors.push(AgentAccountVector {
            name: "agent_set_status".to_string(),
            description: "Set agent status to active (1)".to_string(),
            variant: 3,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // SetEnergyPool (variant 4)
    {
        let energy_pool = Some(test_full_pubkey(0xCC));
        let payload = AgentAccountPayload::SetEnergyPool { energy_pool };
        let wire = payload.to_bytes();

        vectors.push(AgentAccountVector {
            name: "agent_set_energy_pool".to_string(),
            description: "Set agent energy pool".to_string(),
            variant: 4,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // SetSessionKeyRoot (variant 5)
    {
        let session_key_root = Some(test_hash(0xDD));
        let payload = AgentAccountPayload::SetSessionKeyRoot { session_key_root };
        let wire = payload.to_bytes();

        vectors.push(AgentAccountVector {
            name: "agent_set_session_key_root".to_string(),
            description: "Set agent session key root".to_string(),
            variant: 5,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // RevokeSessionKey (variant 7)
    {
        let payload = AgentAccountPayload::RevokeSessionKey { key_id: 12345 };
        let wire = payload.to_bytes();

        vectors.push(AgentAccountVector {
            name: "agent_revoke_session_key".to_string(),
            description: "Revoke agent session key".to_string(),
            variant: 7,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let vectors = ReferralTestVectors {
        algorithm: "Referral-Agent-Transactions".to_string(),
        version: 1,
        bind_referrer_vectors: gen_bind_referrer_vectors(),
        batch_referral_vectors: gen_batch_referral_vectors(),
        agent_account_vectors: gen_agent_account_vectors(),
    };

    let yaml = serde_yaml::to_string(&vectors).expect("Failed to serialize to YAML");

    // Add header comment
    let header = r#"# Referral/Agent Transactions Test Vectors (Types 7, 8, 23)
# Generated by TOS Rust - gen_referral_vectors
# Cross-language verification between TOS Rust and Avatar C
#
# Transaction Types:
#   Type 7:  BindReferrer - Bind a referrer to sender account (one-time)
#   Type 8:  BatchReferralReward - Distribute rewards to uplines
#   Type 23: AgentAccount - AI agent account operations
#
# AgentAccount Variants:
#   0: Register - Create new AI agent
#   1: UpdatePolicy - Update policy hash
#   2: RotateController - Change controller
#   3: SetStatus - Enable/disable agent
#   4: SetEnergyPool - Set energy pool
#   5: SetSessionKeyRoot - Set session key root
#   6: AddSessionKey - Add session key
#   7: RevokeSessionKey - Revoke session key

"#;

    let output = format!("{}{}", header, yaml);

    // Write to file
    let output_path = "referral.yaml";
    let mut file = File::create(output_path).expect("Failed to create output file");
    file.write_all(output.as_bytes())
        .expect("Failed to write output");

    println!("Generated Referral/Agent vectors to {}", output_path);
    println!("  BindReferrer: {}", vectors.bind_referrer_vectors.len());
    println!("  BatchReferral: {}", vectors.batch_referral_vectors.len());
    println!("  AgentAccount: {}", vectors.agent_account_vectors.len());
}
