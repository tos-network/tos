// Generate Escrow (Types 24-32) wire format test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_escrow_vectors
//
// These vectors are authoritative for Avatar C cross-validation.
// TOS Rust is the reference implementation.

use serde::Serialize;
use std::fs::File;
use std::io::Write;

// Import TOS common types
use tos_common::crypto::{Hash, PublicKey, Signature};
use tos_common::escrow::{ArbitrationConfig, ArbitrationMode};
use tos_common::serializer::Serializer;
use tos_common::transaction::{
    AppealEscrowPayload, AppealMode, ArbiterSignature, ChallengeEscrowPayload,
    CreateEscrowPayload, DepositEscrowPayload, DisputeEscrowPayload,
    RefundEscrowPayload, ReleaseEscrowPayload, SubmitVerdictPayload,
};

// ============================================================================
// Test Vector Structs
// ============================================================================

#[derive(Serialize)]
struct CreateEscrowVector {
    name: String,
    description: String,
    task_id: String,
    provider_hex: String,
    amount: u64,
    asset_hex: String,
    timeout_blocks: u64,
    challenge_window: u64,
    challenge_deposit_bps: u16,
    optimistic_release: bool,
    has_arbitration: bool,
    has_metadata: bool,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct DepositEscrowVector {
    name: String,
    description: String,
    escrow_id_hex: String,
    amount: u64,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct ReleaseEscrowVector {
    name: String,
    description: String,
    escrow_id_hex: String,
    amount: u64,
    has_completion_proof: bool,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct RefundEscrowVector {
    name: String,
    description: String,
    escrow_id_hex: String,
    amount: u64,
    has_reason: bool,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct ChallengeEscrowVector {
    name: String,
    description: String,
    escrow_id_hex: String,
    reason: String,
    has_evidence_hash: bool,
    deposit: u64,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct DisputeEscrowVector {
    name: String,
    description: String,
    escrow_id_hex: String,
    reason: String,
    has_evidence_hash: bool,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct AppealEscrowVector {
    name: String,
    description: String,
    escrow_id_hex: String,
    reason: String,
    has_new_evidence_hash: bool,
    appeal_deposit: u64,
    appeal_mode: u8,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct SubmitVerdictVector {
    name: String,
    description: String,
    escrow_id_hex: String,
    dispute_id_hex: String,
    round: u32,
    payer_amount: u64,
    payee_amount: u64,
    signatures_cnt: u16,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct EscrowTestFile {
    algorithm: String,
    version: u32,
    create_escrow_vectors: Vec<CreateEscrowVector>,
    deposit_escrow_vectors: Vec<DepositEscrowVector>,
    release_escrow_vectors: Vec<ReleaseEscrowVector>,
    refund_escrow_vectors: Vec<RefundEscrowVector>,
    challenge_escrow_vectors: Vec<ChallengeEscrowVector>,
    dispute_escrow_vectors: Vec<DisputeEscrowVector>,
    appeal_escrow_vectors: Vec<AppealEscrowVector>,
    submit_verdict_vectors: Vec<SubmitVerdictVector>,
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    // Pre-generate test values with deterministic bytes
    let test_escrow_id = Hash::new([0x11u8; 32]);
    let test_dispute_id = Hash::new([0x22u8; 32]);
    let test_provider = PublicKey::from_bytes(&[0x01u8; 32]).unwrap();
    let test_asset = Hash::new([0xAAu8; 32]);
    let test_evidence_hash = Hash::new([0xEEu8; 32]);
    let test_completion_proof = Hash::new([0xCCu8; 32]);
    let test_arbiter1 = PublicKey::from_bytes(&[0x33u8; 32]).unwrap();
    let test_arbiter2 = PublicKey::from_bytes(&[0x44u8; 32]).unwrap();

    let mut create_vectors = Vec::new();
    let mut deposit_vectors = Vec::new();
    let mut release_vectors = Vec::new();
    let mut refund_vectors = Vec::new();
    let mut challenge_vectors = Vec::new();
    let mut dispute_vectors = Vec::new();
    let mut appeal_vectors = Vec::new();
    let mut verdict_vectors = Vec::new();

    // ========================================================================
    // CreateEscrow (Type 24) Test Vectors
    // ========================================================================

    // Test 1: Basic escrow with no arbitration
    {
        let payload = CreateEscrowPayload {
            task_id: "task-001".to_string(),
            provider: test_provider.clone(),
            amount: 1_000_000_000,
            asset: test_asset.clone(),
            timeout_blocks: 1000,
            challenge_window: 100,
            challenge_deposit_bps: 500,
            optimistic_release: false,
            arbitration_config: None,
            metadata: None,
        };
        create_vectors.push(CreateEscrowVector {
            name: "create_basic".to_string(),
            description: "Basic escrow without arbitration or metadata".to_string(),
            task_id: "task-001".to_string(),
            provider_hex: hex::encode(test_provider.as_bytes()),
            amount: 1_000_000_000,
            asset_hex: hex::encode(test_asset.as_bytes()),
            timeout_blocks: 1000,
            challenge_window: 100,
            challenge_deposit_bps: 500,
            optimistic_release: false,
            has_arbitration: false,
            has_metadata: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: Escrow with optimistic release
    {
        let payload = CreateEscrowPayload {
            task_id: "task-optimistic".to_string(),
            provider: test_provider.clone(),
            amount: 5_000_000_000,
            asset: test_asset.clone(),
            timeout_blocks: 5000,
            challenge_window: 200,
            challenge_deposit_bps: 1000,
            optimistic_release: true,
            arbitration_config: None,
            metadata: None,
        };
        create_vectors.push(CreateEscrowVector {
            name: "create_optimistic".to_string(),
            description: "Escrow with optimistic release enabled".to_string(),
            task_id: "task-optimistic".to_string(),
            provider_hex: hex::encode(test_provider.as_bytes()),
            amount: 5_000_000_000,
            asset_hex: hex::encode(test_asset.as_bytes()),
            timeout_blocks: 5000,
            challenge_window: 200,
            challenge_deposit_bps: 1000,
            optimistic_release: true,
            has_arbitration: false,
            has_metadata: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 3: Escrow with single arbiter
    {
        let arb_config = ArbitrationConfig {
            mode: ArbitrationMode::Single,
            arbiters: vec![test_arbiter1.clone()],
            threshold: None,
            fee_amount: 100_000,
            allow_appeal: true,
        };
        let payload = CreateEscrowPayload {
            task_id: "task-arb".to_string(),
            provider: test_provider.clone(),
            amount: 10_000_000_000,
            asset: test_asset.clone(),
            timeout_blocks: 10000,
            challenge_window: 500,
            challenge_deposit_bps: 250,
            optimistic_release: false,
            arbitration_config: Some(arb_config),
            metadata: None,
        };
        create_vectors.push(CreateEscrowVector {
            name: "create_single_arbiter".to_string(),
            description: "Escrow with single arbiter arbitration".to_string(),
            task_id: "task-arb".to_string(),
            provider_hex: hex::encode(test_provider.as_bytes()),
            amount: 10_000_000_000,
            asset_hex: hex::encode(test_asset.as_bytes()),
            timeout_blocks: 10000,
            challenge_window: 500,
            challenge_deposit_bps: 250,
            optimistic_release: false,
            has_arbitration: true,
            has_metadata: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 4: Escrow with metadata
    {
        let payload = CreateEscrowPayload {
            task_id: "task-meta".to_string(),
            provider: test_provider.clone(),
            amount: 2_000_000_000,
            asset: test_asset.clone(),
            timeout_blocks: 2000,
            challenge_window: 50,
            challenge_deposit_bps: 100,
            optimistic_release: true,
            arbitration_config: None,
            metadata: Some(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        };
        create_vectors.push(CreateEscrowVector {
            name: "create_with_metadata".to_string(),
            description: "Escrow with metadata bytes".to_string(),
            task_id: "task-meta".to_string(),
            provider_hex: hex::encode(test_provider.as_bytes()),
            amount: 2_000_000_000,
            asset_hex: hex::encode(test_asset.as_bytes()),
            timeout_blocks: 2000,
            challenge_window: 50,
            challenge_deposit_bps: 100,
            optimistic_release: true,
            has_arbitration: false,
            has_metadata: true,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // DepositEscrow (Type 25) Test Vectors
    // ========================================================================

    // Test 1: Basic deposit
    {
        let payload = DepositEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            amount: 500_000_000,
        };
        deposit_vectors.push(DepositEscrowVector {
            name: "deposit_basic".to_string(),
            description: "Basic deposit to escrow".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            amount: 500_000_000,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: Large deposit
    {
        let payload = DepositEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            amount: 100_000_000_000,
        };
        deposit_vectors.push(DepositEscrowVector {
            name: "deposit_large".to_string(),
            description: "Large deposit (100 TOS)".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            amount: 100_000_000_000,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // ReleaseEscrow (Type 26) Test Vectors
    // ========================================================================

    // Test 1: Release without completion proof
    {
        let payload = ReleaseEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            amount: 1_000_000_000,
            completion_proof: None,
        };
        release_vectors.push(ReleaseEscrowVector {
            name: "release_basic".to_string(),
            description: "Basic release without completion proof".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            amount: 1_000_000_000,
            has_completion_proof: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: Release with completion proof
    {
        let payload = ReleaseEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            amount: 2_500_000_000,
            completion_proof: Some(test_completion_proof.clone()),
        };
        release_vectors.push(ReleaseEscrowVector {
            name: "release_with_proof".to_string(),
            description: "Release with completion proof hash".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            amount: 2_500_000_000,
            has_completion_proof: true,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // RefundEscrow (Type 27) Test Vectors
    // ========================================================================

    // Test 1: Refund without reason
    {
        let payload = RefundEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            amount: 500_000_000,
            reason: None,
        };
        refund_vectors.push(RefundEscrowVector {
            name: "refund_basic".to_string(),
            description: "Basic refund without reason".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            amount: 500_000_000,
            has_reason: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: Refund with reason
    {
        let payload = RefundEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            amount: 1_000_000_000,
            reason: Some("Task not completed".to_string()),
        };
        refund_vectors.push(RefundEscrowVector {
            name: "refund_with_reason".to_string(),
            description: "Refund with reason string".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            amount: 1_000_000_000,
            has_reason: true,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // ChallengeEscrow (Type 28) Test Vectors
    // ========================================================================

    // Test 1: Challenge without evidence
    {
        let payload = ChallengeEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            reason: "Work quality issue".to_string(),
            evidence_hash: None,
            deposit: 50_000_000,
        };
        challenge_vectors.push(ChallengeEscrowVector {
            name: "challenge_basic".to_string(),
            description: "Challenge without evidence hash".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            reason: "Work quality issue".to_string(),
            has_evidence_hash: false,
            deposit: 50_000_000,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: Challenge with evidence
    {
        let payload = ChallengeEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            reason: "Deliverables not as specified".to_string(),
            evidence_hash: Some(test_evidence_hash.clone()),
            deposit: 100_000_000,
        };
        challenge_vectors.push(ChallengeEscrowVector {
            name: "challenge_with_evidence".to_string(),
            description: "Challenge with evidence hash".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            reason: "Deliverables not as specified".to_string(),
            has_evidence_hash: true,
            deposit: 100_000_000,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // DisputeEscrow (Type 30) Test Vectors
    // ========================================================================

    // Test 1: Dispute without evidence
    {
        let payload = DisputeEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            reason: "Provider failed to deliver".to_string(),
            evidence_hash: None,
        };
        dispute_vectors.push(DisputeEscrowVector {
            name: "dispute_basic".to_string(),
            description: "Dispute without evidence hash".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            reason: "Provider failed to deliver".to_string(),
            has_evidence_hash: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: Dispute with evidence
    {
        let payload = DisputeEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            reason: "Incomplete work submitted".to_string(),
            evidence_hash: Some(test_evidence_hash.clone()),
        };
        dispute_vectors.push(DisputeEscrowVector {
            name: "dispute_with_evidence".to_string(),
            description: "Dispute with evidence hash".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            reason: "Incomplete work submitted".to_string(),
            has_evidence_hash: true,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // AppealEscrow (Type 31) Test Vectors
    // ========================================================================

    // Test 1: Appeal to committee
    {
        let payload = AppealEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            reason: "Arbiter biased decision".to_string(),
            new_evidence_hash: None,
            appeal_deposit: 500_000_000,
            appeal_mode: AppealMode::Committee,
        };
        appeal_vectors.push(AppealEscrowVector {
            name: "appeal_committee".to_string(),
            description: "Appeal to committee without new evidence".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            reason: "Arbiter biased decision".to_string(),
            has_new_evidence_hash: false,
            appeal_deposit: 500_000_000,
            appeal_mode: 0,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: Appeal to DAO with evidence
    {
        let payload = AppealEscrowPayload {
            escrow_id: test_escrow_id.clone(),
            reason: "Committee ruling unjust".to_string(),
            new_evidence_hash: Some(test_evidence_hash.clone()),
            appeal_deposit: 2_000_000_000,
            appeal_mode: AppealMode::DaoGovernance,
        };
        appeal_vectors.push(AppealEscrowVector {
            name: "appeal_dao_with_evidence".to_string(),
            description: "Appeal to DAO governance with new evidence".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            reason: "Committee ruling unjust".to_string(),
            has_new_evidence_hash: true,
            appeal_deposit: 2_000_000_000,
            appeal_mode: 1,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // SubmitVerdict (Type 29) Test Vectors
    // ========================================================================

    // Test 1: Single signature verdict (split decision)
    // Use deterministic signature bytes - patterns like [4u8; 64] are known valid
    {
        let sig1 = ArbiterSignature {
            arbiter_pubkey: test_arbiter1.clone(),
            signature: Signature::from_bytes(&[4u8; 64]).unwrap(),
            timestamp: 1700000000,
        };
        let payload = SubmitVerdictPayload {
            escrow_id: test_escrow_id.clone(),
            dispute_id: test_dispute_id.clone(),
            round: 1,
            payer_amount: 600_000_000,
            payee_amount: 400_000_000,
            signatures: vec![sig1],
        };
        verdict_vectors.push(SubmitVerdictVector {
            name: "verdict_single_split".to_string(),
            description: "Single arbiter verdict with split decision".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            dispute_id_hex: hex::encode(test_dispute_id.as_bytes()),
            round: 1,
            payer_amount: 600_000_000,
            payee_amount: 400_000_000,
            signatures_cnt: 1,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: Committee verdict (full to payee)
    {
        let sig1 = ArbiterSignature {
            arbiter_pubkey: test_arbiter1.clone(),
            signature: Signature::from_bytes(&[1u8; 64]).unwrap(),
            timestamp: 1700000000,
        };
        let sig2 = ArbiterSignature {
            arbiter_pubkey: test_arbiter2.clone(),
            signature: Signature::from_bytes(&[2u8; 64]).unwrap(),
            timestamp: 1700000001,
        };
        let payload = SubmitVerdictPayload {
            escrow_id: test_escrow_id.clone(),
            dispute_id: test_dispute_id.clone(),
            round: 2,
            payer_amount: 0,
            payee_amount: 1_000_000_000,
            signatures: vec![sig1, sig2],
        };
        verdict_vectors.push(SubmitVerdictVector {
            name: "verdict_committee_full_payee".to_string(),
            description: "Committee verdict awarding full amount to payee".to_string(),
            escrow_id_hex: hex::encode(test_escrow_id.as_bytes()),
            dispute_id_hex: hex::encode(test_dispute_id.as_bytes()),
            round: 2,
            payer_amount: 0,
            payee_amount: 1_000_000_000,
            signatures_cnt: 2,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // Write Output
    // ========================================================================

    let test_file = EscrowTestFile {
        algorithm: "Escrow-Transactions".to_string(),
        version: 1,
        create_escrow_vectors: create_vectors,
        deposit_escrow_vectors: deposit_vectors,
        release_escrow_vectors: release_vectors,
        refund_escrow_vectors: refund_vectors,
        challenge_escrow_vectors: challenge_vectors,
        dispute_escrow_vectors: dispute_vectors,
        appeal_escrow_vectors: appeal_vectors,
        submit_verdict_vectors: verdict_vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).expect("YAML serialization failed");

    // Add header comment
    let header = r#"# Escrow Transactions Test Vectors (Types 24-32)
# Generated by TOS Rust - gen_escrow_vectors
# Cross-language verification between TOS Rust and Avatar C
#
# Transaction Types:
#   Type 24: CreateEscrow
#   Type 25: DepositEscrow
#   Type 26: ReleaseEscrow
#   Type 27: RefundEscrow
#   Type 28: ChallengeEscrow
#   Type 29: SubmitVerdict
#   Type 30: DisputeEscrow
#   Type 31: AppealEscrow
#   Type 32: SubmitVerdictByJuror (same format as 29)
#
# ArbitrationMode enum:
#   None=0, Single=1, Committee=2, DaoGovernance=3
#
# AppealMode enum:
#   Committee=0, DaoGovernance=1

"#;

    let full_yaml = format!("{}{}", header, yaml);
    println!("{}", full_yaml);

    let mut file = File::create("escrow.yaml").expect("Failed to create file");
    file.write_all(full_yaml.as_bytes())
        .expect("Failed to write file");
    eprintln!("Written to escrow.yaml");
}
