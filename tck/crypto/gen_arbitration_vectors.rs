// Generate Arbitration (Types 33-38, 44-47) wire format test vectors
// Run: cd ~/tos/tck/crypto && cargo run --release --bin gen_arbitration_vectors
//
// These vectors are authoritative for Avatar C cross-validation.
// TOS Rust is the reference implementation.
//
// Transaction Types:
//   33: RegisterArbiter
//   34: UpdateArbiter
//   35: CommitArbitrationOpen
//   36: CommitVoteRequest
//   37: CommitSelectionCommitment
//   38: CommitJurorVote
//   44: SlashArbiter
//   45: RequestArbiterExit
//   46: WithdrawArbiterStake
//   47: CancelArbiterExit

use hex;
use serde::Serialize;
use std::fs::File;
use std::io::Write;

// Import TOS common types
use tos_common::arbitration::{ArbiterStatus, ExpertiseDomain};
use tos_common::crypto::{Hash, PublicKey, Signature};
use tos_common::kyc::CommitteeApproval;
use tos_common::serializer::Serializer;
use tos_common::transaction::{
    CancelArbiterExitPayload, CommitArbitrationOpenPayload, CommitJurorVotePayload,
    CommitSelectionCommitmentPayload, CommitVoteRequestPayload, RegisterArbiterPayload,
    RequestArbiterExitPayload, SlashArbiterPayload, UpdateArbiterPayload,
    WithdrawArbiterStakePayload,
};

#[derive(Serialize)]
struct RegisterArbiterVector {
    name: String,
    description: String,
    name_str: String,
    name_len: u8,
    expertise_domains: Vec<u8>,  // ExpertiseDomain enum values
    stake_amount: u64,
    min_escrow_value: u64,
    max_escrow_value: u64,
    fee_basis_points: u16,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct UpdateArbiterVector {
    name: String,
    description: String,
    has_name: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    name_str: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name_len: Option<u8>,
    has_expertise: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    expertise_domains: Option<Vec<u8>>,  // ExpertiseDomain enum values
    has_fee: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    fee_basis_points: Option<u16>,
    has_min_escrow: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_escrow_value: Option<u64>,
    has_max_escrow: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_escrow_value: Option<u64>,
    has_add_stake: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    add_stake: Option<u64>,
    has_status: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u8>,
    deactivate: bool,
    wire_hex: String,
    expected_size: usize,
}

// Type 35: SlashArbiter
#[derive(Serialize)]
struct SlashArbiterVector {
    name: String,
    description: String,
    committee_id_hex: String,
    arbiter_pubkey_hex: String,
    amount: u64,
    reason_hash_hex: String,
    approvals_count: usize,
    wire_hex: String,
    expected_size: usize,
}

// Type 36: RequestArbiterExit (empty payload)
#[derive(Serialize)]
struct RequestArbiterExitVector {
    name: String,
    description: String,
    wire_hex: String,
    expected_size: usize,
}

// Type 37: WithdrawArbiterStake
#[derive(Serialize)]
struct WithdrawArbiterStakeVector {
    name: String,
    description: String,
    amount: u64,
    wire_hex: String,
    expected_size: usize,
}

// Type 38: CancelArbiterExit (empty payload)
#[derive(Serialize)]
struct CancelArbiterExitVector {
    name: String,
    description: String,
    wire_hex: String,
    expected_size: usize,
}

// Type 44: CommitArbitrationOpen
#[derive(Serialize)]
struct CommitArbitrationOpenVector {
    name: String,
    description: String,
    escrow_id_hex: String,
    dispute_id_hex: String,
    round: u32,
    request_id_hex: String,
    arbitration_open_hash_hex: String,
    opener_signature_hex: String,
    payload_len: usize,
    wire_hex: String,
    expected_size: usize,
}

// Type 45: CommitVoteRequest
#[derive(Serialize)]
struct CommitVoteRequestVector {
    name: String,
    description: String,
    request_id_hex: String,
    vote_request_hash_hex: String,
    coordinator_signature_hex: String,
    payload_len: usize,
    wire_hex: String,
    expected_size: usize,
}

// Type 46: CommitSelectionCommitment
#[derive(Serialize)]
struct CommitSelectionCommitmentVector {
    name: String,
    description: String,
    request_id_hex: String,
    selection_commitment_id_hex: String,
    payload_len: usize,
    wire_hex: String,
    expected_size: usize,
}

// Type 47: CommitJurorVote
#[derive(Serialize)]
struct CommitJurorVoteVector {
    name: String,
    description: String,
    request_id_hex: String,
    juror_pubkey_hex: String,
    vote_hash_hex: String,
    juror_signature_hex: String,
    payload_len: usize,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct ArbitrationTestFile {
    algorithm: String,
    version: u32,
    register_arbiter_vectors: Vec<RegisterArbiterVector>,
    update_arbiter_vectors: Vec<UpdateArbiterVector>,
    slash_arbiter_vectors: Vec<SlashArbiterVector>,
    request_arbiter_exit_vectors: Vec<RequestArbiterExitVector>,
    withdraw_arbiter_stake_vectors: Vec<WithdrawArbiterStakeVector>,
    cancel_arbiter_exit_vectors: Vec<CancelArbiterExitVector>,
    commit_arbitration_open_vectors: Vec<CommitArbitrationOpenVector>,
    commit_vote_request_vectors: Vec<CommitVoteRequestVector>,
    commit_selection_commitment_vectors: Vec<CommitSelectionCommitmentVector>,
    commit_juror_vote_vectors: Vec<CommitJurorVoteVector>,
}

fn expertise_to_u8(domain: &ExpertiseDomain) -> u8 {
    match domain {
        ExpertiseDomain::General => 0,
        ExpertiseDomain::AIAgent => 1,
        ExpertiseDomain::SmartContract => 2,
        ExpertiseDomain::Payment => 3,
        ExpertiseDomain::DeFi => 4,
        ExpertiseDomain::Governance => 5,
        ExpertiseDomain::Identity => 6,
        ExpertiseDomain::Data => 7,
        ExpertiseDomain::Security => 8,
        ExpertiseDomain::Gaming => 9,
        ExpertiseDomain::DataService => 10,
        ExpertiseDomain::DigitalAsset => 11,
        ExpertiseDomain::CrossChain => 12,
        ExpertiseDomain::Nft => 13,
    }
}

fn status_to_u8(status: &ArbiterStatus) -> u8 {
    match status {
        ArbiterStatus::Active => 0,
        ArbiterStatus::Suspended => 1,
        ArbiterStatus::Exiting => 2,
        ArbiterStatus::Removed => 3,
    }
}

fn test_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

fn test_pubkey(seed: u8) -> PublicKey {
    PublicKey::from_bytes(&[seed; 32]).expect("Valid pubkey bytes")
}

fn test_signature() -> Signature {
    Signature::from_bytes(&[0x04u8; 64]).expect("Valid signature bytes")
}

fn test_approval(seed: u8, timestamp: u64) -> CommitteeApproval {
    CommitteeApproval::new(
        test_pubkey(seed),
        test_signature(),
        timestamp,
    )
}

fn main() {
    let mut register_vectors = Vec::new();
    let mut update_vectors = Vec::new();

    // ========================================================================
    // RegisterArbiter (Type 33) Test Vectors
    // ========================================================================

    // Test 1: Basic RegisterArbiter with no expertise domains
    {
        let expertise: Vec<ExpertiseDomain> = vec![];
        let payload = RegisterArbiterPayload::new(
            "Alice".to_string(),
            expertise.clone(),
            10_000_000_000,  // 10 TOS stake
            100_000_000,    // 0.1 TOS min escrow
            1_000_000_000_000, // 1000 TOS max escrow
            500,            // 5% fee
        );
        register_vectors.push(RegisterArbiterVector {
            name: "register_basic_no_expertise".to_string(),
            description: "Basic RegisterArbiter with no expertise domains".to_string(),
            name_str: "Alice".to_string(),
            name_len: 5,
            expertise_domains: expertise.iter().map(|d| expertise_to_u8(d)).collect(),
            stake_amount: 10_000_000_000,
            min_escrow_value: 100_000_000,
            max_escrow_value: 1_000_000_000_000,
            fee_basis_points: 500,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: RegisterArbiter with DeFi, Security, NFT expertise
    {
        let expertise = vec![
            ExpertiseDomain::DeFi,
            ExpertiseDomain::Security,
            ExpertiseDomain::Nft,
        ];
        let payload = RegisterArbiterPayload::new(
            "BobArbiter".to_string(),
            expertise.clone(),
            50_000_000_000, // 50 TOS stake
            1_000_000_000,  // 1 TOS min escrow
            100_000_000_000, // 100 TOS max escrow
            250,            // 2.5% fee
        );
        register_vectors.push(RegisterArbiterVector {
            name: "register_with_expertise".to_string(),
            description: "RegisterArbiter with DeFi, Security, NFT expertise".to_string(),
            name_str: "BobArbiter".to_string(),
            name_len: 10,
            expertise_domains: expertise.iter().map(|d| expertise_to_u8(d)).collect(),
            stake_amount: 50_000_000_000,
            min_escrow_value: 1_000_000_000,
            max_escrow_value: 100_000_000_000,
            fee_basis_points: 250,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 3: RegisterArbiter with all 14 expertise domains
    {
        let expertise = vec![
            ExpertiseDomain::General,
            ExpertiseDomain::AIAgent,
            ExpertiseDomain::SmartContract,
            ExpertiseDomain::Payment,
            ExpertiseDomain::DeFi,
            ExpertiseDomain::Governance,
            ExpertiseDomain::Identity,
            ExpertiseDomain::Data,
            ExpertiseDomain::Security,
            ExpertiseDomain::Gaming,
            ExpertiseDomain::DataService,
            ExpertiseDomain::DigitalAsset,
            ExpertiseDomain::CrossChain,
            ExpertiseDomain::Nft,
        ];
        let payload = RegisterArbiterPayload::new(
            "Expert".to_string(),
            expertise.clone(),
            100_000_000_000,     // 100 TOS stake
            0,                   // 0 min escrow
            u64::MAX,            // max escrow
            10000,               // 100% fee (max)
        );
        register_vectors.push(RegisterArbiterVector {
            name: "register_all_expertise".to_string(),
            description: "RegisterArbiter with all 14 expertise domains".to_string(),
            name_str: "Expert".to_string(),
            name_len: 6,
            expertise_domains: expertise.iter().map(|d| expertise_to_u8(d)).collect(),
            stake_amount: 100_000_000_000,
            min_escrow_value: 0,
            max_escrow_value: u64::MAX,
            fee_basis_points: 10000,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 4: RegisterArbiter with single General expertise
    {
        let expertise = vec![ExpertiseDomain::General];
        let payload = RegisterArbiterPayload::new(
            "Gen".to_string(),
            expertise.clone(),
            1_000_000_000, // 1 TOS stake
            0,
            0,
            0, // 0% fee
        );
        register_vectors.push(RegisterArbiterVector {
            name: "register_single_expertise".to_string(),
            description: "RegisterArbiter with single General expertise".to_string(),
            name_str: "Gen".to_string(),
            name_len: 3,
            expertise_domains: expertise.iter().map(|d| expertise_to_u8(d)).collect(),
            stake_amount: 1_000_000_000,
            min_escrow_value: 0,
            max_escrow_value: 0,
            fee_basis_points: 0,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 5: RegisterArbiter with 0% fee
    {
        let expertise: Vec<ExpertiseDomain> = vec![];
        let payload = RegisterArbiterPayload::new(
            "FreeArb".to_string(),
            expertise.clone(),
            5_000_000_000,  // 5 TOS stake
            1_000_000,      // 0.001 TOS min escrow
            1_000_000_000,  // 1 TOS max escrow
            0,              // 0% fee
        );
        register_vectors.push(RegisterArbiterVector {
            name: "register_zero_fee".to_string(),
            description: "RegisterArbiter with 0% fee".to_string(),
            name_str: "FreeArb".to_string(),
            name_len: 7,
            expertise_domains: expertise.iter().map(|d| expertise_to_u8(d)).collect(),
            stake_amount: 5_000_000_000,
            min_escrow_value: 1_000_000,
            max_escrow_value: 1_000_000_000,
            fee_basis_points: 0,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // UpdateArbiter (Type 34) Test Vectors
    // ========================================================================

    // Test 1: UpdateArbiter with all Option fields as None (minimum size)
    {
        let payload = UpdateArbiterPayload::new(
            None, None, None, None, None, None, None, false,
        );
        update_vectors.push(UpdateArbiterVector {
            name: "update_all_none".to_string(),
            description: "UpdateArbiter with all Option fields as None (minimum size)".to_string(),
            has_name: false,
            name_str: None,
            name_len: None,
            has_expertise: false,
            expertise_domains: None,
            has_fee: false,
            fee_basis_points: None,
            has_min_escrow: false,
            min_escrow_value: None,
            has_max_escrow: false,
            max_escrow_value: None,
            has_add_stake: false,
            add_stake: None,
            has_status: false,
            status: None,
            deactivate: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: UpdateArbiter with only deactivate=true
    {
        let payload = UpdateArbiterPayload::new(
            None, None, None, None, None, None, None, true,
        );
        update_vectors.push(UpdateArbiterVector {
            name: "update_deactivate_only".to_string(),
            description: "UpdateArbiter with only deactivate=true".to_string(),
            has_name: false,
            name_str: None,
            name_len: None,
            has_expertise: false,
            expertise_domains: None,
            has_fee: false,
            fee_basis_points: None,
            has_min_escrow: false,
            min_escrow_value: None,
            has_max_escrow: false,
            max_escrow_value: None,
            has_add_stake: false,
            add_stake: None,
            has_status: false,
            status: None,
            deactivate: true,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 3: UpdateArbiter with only name update
    {
        let payload = UpdateArbiterPayload::new(
            Some("NewName".to_string()),
            None, None, None, None, None, None, false,
        );
        update_vectors.push(UpdateArbiterVector {
            name: "update_name_only".to_string(),
            description: "UpdateArbiter with only name update".to_string(),
            has_name: true,
            name_str: Some("NewName".to_string()),
            name_len: Some(7),
            has_expertise: false,
            expertise_domains: None,
            has_fee: false,
            fee_basis_points: None,
            has_min_escrow: false,
            min_escrow_value: None,
            has_max_escrow: false,
            max_escrow_value: None,
            has_add_stake: false,
            add_stake: None,
            has_status: false,
            status: None,
            deactivate: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 4: UpdateArbiter with only fee update
    {
        let payload = UpdateArbiterPayload::new(
            None, None, Some(750), None, None, None, None, false,
        );
        update_vectors.push(UpdateArbiterVector {
            name: "update_fee_only".to_string(),
            description: "UpdateArbiter with only fee update".to_string(),
            has_name: false,
            name_str: None,
            name_len: None,
            has_expertise: false,
            expertise_domains: None,
            has_fee: true,
            fee_basis_points: Some(750),
            has_min_escrow: false,
            min_escrow_value: None,
            has_max_escrow: false,
            max_escrow_value: None,
            has_add_stake: false,
            add_stake: None,
            has_status: false,
            status: None,
            deactivate: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 5: UpdateArbiter with add_stake
    {
        let payload = UpdateArbiterPayload::new(
            None, None, None, None, None, Some(5_000_000_000), None, false,
        );
        update_vectors.push(UpdateArbiterVector {
            name: "update_add_stake".to_string(),
            description: "UpdateArbiter with add_stake".to_string(),
            has_name: false,
            name_str: None,
            name_len: None,
            has_expertise: false,
            expertise_domains: None,
            has_fee: false,
            fee_basis_points: None,
            has_min_escrow: false,
            min_escrow_value: None,
            has_max_escrow: false,
            max_escrow_value: None,
            has_add_stake: true,
            add_stake: Some(5_000_000_000),
            has_status: false,
            status: None,
            deactivate: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 6: UpdateArbiter changing status to Suspended
    {
        let payload = UpdateArbiterPayload::new(
            None, None, None, None, None, None, Some(ArbiterStatus::Suspended), false,
        );
        update_vectors.push(UpdateArbiterVector {
            name: "update_status_suspended".to_string(),
            description: "UpdateArbiter changing status to Suspended".to_string(),
            has_name: false,
            name_str: None,
            name_len: None,
            has_expertise: false,
            expertise_domains: None,
            has_fee: false,
            fee_basis_points: None,
            has_min_escrow: false,
            min_escrow_value: None,
            has_max_escrow: false,
            max_escrow_value: None,
            has_add_stake: false,
            add_stake: None,
            has_status: true,
            status: Some(status_to_u8(&ArbiterStatus::Suspended)),
            deactivate: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 7: UpdateArbiter with only expertise update
    {
        let expertise = vec![ExpertiseDomain::DeFi, ExpertiseDomain::Security];
        let payload = UpdateArbiterPayload::new(
            None, Some(expertise.clone()), None, None, None, None, None, false,
        );
        update_vectors.push(UpdateArbiterVector {
            name: "update_expertise_only".to_string(),
            description: "UpdateArbiter with only expertise update".to_string(),
            has_name: false,
            name_str: None,
            name_len: None,
            has_expertise: true,
            expertise_domains: Some(expertise.iter().map(|d| expertise_to_u8(d)).collect()),
            has_fee: false,
            fee_basis_points: None,
            has_min_escrow: false,
            min_escrow_value: None,
            has_max_escrow: false,
            max_escrow_value: None,
            has_add_stake: false,
            add_stake: None,
            has_status: false,
            status: None,
            deactivate: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 8: UpdateArbiter with min and max escrow updates
    {
        let payload = UpdateArbiterPayload::new(
            None, None, None,
            Some(500_000_000),      // min escrow
            Some(50_000_000_000),   // max escrow
            None, None, false,
        );
        update_vectors.push(UpdateArbiterVector {
            name: "update_escrow_bounds".to_string(),
            description: "UpdateArbiter with min and max escrow updates".to_string(),
            has_name: false,
            name_str: None,
            name_len: None,
            has_expertise: false,
            expertise_domains: None,
            has_fee: false,
            fee_basis_points: None,
            has_min_escrow: true,
            min_escrow_value: Some(500_000_000),
            has_max_escrow: true,
            max_escrow_value: Some(50_000_000_000),
            has_add_stake: false,
            add_stake: None,
            has_status: false,
            status: None,
            deactivate: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 9: UpdateArbiter with all fields set
    {
        let expertise = vec![
            ExpertiseDomain::General,
            ExpertiseDomain::DeFi,
            ExpertiseDomain::Security,
        ];
        let payload = UpdateArbiterPayload::new(
            Some("FullUpd".to_string()),
            Some(expertise.clone()),
            Some(300),             // fee
            Some(100_000_000),     // min escrow
            Some(10_000_000_000),  // max escrow
            Some(1_000_000_000),   // add stake
            Some(ArbiterStatus::Active),
            false,
        );
        update_vectors.push(UpdateArbiterVector {
            name: "update_all_fields".to_string(),
            description: "UpdateArbiter with all fields set".to_string(),
            has_name: true,
            name_str: Some("FullUpd".to_string()),
            name_len: Some(7),
            has_expertise: true,
            expertise_domains: Some(expertise.iter().map(|d| expertise_to_u8(d)).collect()),
            has_fee: true,
            fee_basis_points: Some(300),
            has_min_escrow: true,
            min_escrow_value: Some(100_000_000),
            has_max_escrow: true,
            max_escrow_value: Some(10_000_000_000),
            has_add_stake: true,
            add_stake: Some(1_000_000_000),
            has_status: true,
            status: Some(status_to_u8(&ArbiterStatus::Active)),
            deactivate: false,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // SlashArbiter (Type 35) Test Vectors
    // ========================================================================

    let mut slash_vectors = Vec::new();

    // Test 1: SlashArbiter with no approvals
    {
        let committee_id = test_hash(0x11);
        let arbiter_pubkey = test_pubkey(0x22);
        let amount = 1_000_000_000u64;
        let reason_hash = test_hash(0x33);
        let approvals: Vec<CommitteeApproval> = vec![];

        let payload = SlashArbiterPayload::new(
            committee_id.clone(),
            arbiter_pubkey.clone(),
            amount,
            reason_hash.clone(),
            approvals,
        );
        slash_vectors.push(SlashArbiterVector {
            name: "slash_arbiter_no_approvals".to_string(),
            description: "SlashArbiter with no approvals".to_string(),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            arbiter_pubkey_hex: hex::encode(arbiter_pubkey.as_bytes()),
            amount,
            reason_hash_hex: hex::encode(reason_hash.as_bytes()),
            approvals_count: 0,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: SlashArbiter with 2 approvals
    {
        let committee_id = test_hash(0x44);
        let arbiter_pubkey = test_pubkey(0x55);
        let amount = 5_000_000_000u64;
        let reason_hash = test_hash(0x66);
        let approvals = vec![
            test_approval(0x77, 1700000000),
            test_approval(0x88, 1700000001),
        ];

        let payload = SlashArbiterPayload::new(
            committee_id.clone(),
            arbiter_pubkey.clone(),
            amount,
            reason_hash.clone(),
            approvals.clone(),
        );
        slash_vectors.push(SlashArbiterVector {
            name: "slash_arbiter_2_approvals".to_string(),
            description: "SlashArbiter with 2 committee approvals".to_string(),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            arbiter_pubkey_hex: hex::encode(arbiter_pubkey.as_bytes()),
            amount,
            reason_hash_hex: hex::encode(reason_hash.as_bytes()),
            approvals_count: 2,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // RequestArbiterExit (Type 36) Test Vectors
    // ========================================================================

    let mut request_exit_vectors = Vec::new();

    // Test 1: RequestArbiterExit (empty payload)
    {
        let payload = RequestArbiterExitPayload::new();
        request_exit_vectors.push(RequestArbiterExitVector {
            name: "request_arbiter_exit".to_string(),
            description: "RequestArbiterExit (empty payload)".to_string(),
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // WithdrawArbiterStake (Type 37) Test Vectors
    // ========================================================================

    let mut withdraw_vectors = Vec::new();

    // Test 1: WithdrawArbiterStake with specific amount
    {
        let amount = 5_000_000_000u64;
        let payload = WithdrawArbiterStakePayload::new(amount);
        withdraw_vectors.push(WithdrawArbiterStakeVector {
            name: "withdraw_arbiter_stake_specific".to_string(),
            description: "Withdraw specific amount of arbiter stake".to_string(),
            amount,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Test 2: WithdrawArbiterStake with 0 (withdraw all)
    {
        let amount = 0u64;
        let payload = WithdrawArbiterStakePayload::new(amount);
        withdraw_vectors.push(WithdrawArbiterStakeVector {
            name: "withdraw_arbiter_stake_all".to_string(),
            description: "Withdraw all available arbiter stake (amount=0)".to_string(),
            amount,
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // CancelArbiterExit (Type 38) Test Vectors
    // ========================================================================

    let mut cancel_exit_vectors = Vec::new();

    // Test 1: CancelArbiterExit (empty payload)
    {
        let payload = CancelArbiterExitPayload::new();
        cancel_exit_vectors.push(CancelArbiterExitVector {
            name: "cancel_arbiter_exit".to_string(),
            description: "CancelArbiterExit (empty payload)".to_string(),
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // CommitArbitrationOpen (Type 44) Test Vectors
    // ========================================================================

    let mut commit_arb_open_vectors = Vec::new();

    // Test 1: Basic CommitArbitrationOpen
    {
        let escrow_id = test_hash(0xAA);
        let dispute_id = test_hash(0xBB);
        let round = 1u32;
        let request_id = test_hash(0xCC);
        let arb_open_hash = test_hash(0xDD);
        let opener_sig = test_signature();
        let arb_payload = vec![0xEEu8; 32];

        let payload = CommitArbitrationOpenPayload {
            escrow_id: escrow_id.clone(),
            dispute_id: dispute_id.clone(),
            round,
            request_id: request_id.clone(),
            arbitration_open_hash: arb_open_hash.clone(),
            opener_signature: opener_sig.clone(),
            arbitration_open_payload: arb_payload.clone(),
        };
        commit_arb_open_vectors.push(CommitArbitrationOpenVector {
            name: "commit_arbitration_open_basic".to_string(),
            description: "Basic CommitArbitrationOpen with 32-byte payload".to_string(),
            escrow_id_hex: hex::encode(escrow_id.as_bytes()),
            dispute_id_hex: hex::encode(dispute_id.as_bytes()),
            round,
            request_id_hex: hex::encode(request_id.as_bytes()),
            arbitration_open_hash_hex: hex::encode(arb_open_hash.as_bytes()),
            opener_signature_hex: hex::encode(opener_sig.to_bytes()),
            payload_len: arb_payload.len(),
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // CommitVoteRequest (Type 45) Test Vectors
    // ========================================================================

    let mut commit_vote_req_vectors = Vec::new();

    // Test 1: Basic CommitVoteRequest
    {
        let request_id = test_hash(0x11);
        let vote_req_hash = test_hash(0x22);
        let coord_sig = test_signature();
        let vote_payload = vec![0x33u8; 64];

        let payload = CommitVoteRequestPayload {
            request_id: request_id.clone(),
            vote_request_hash: vote_req_hash.clone(),
            coordinator_signature: coord_sig.clone(),
            vote_request_payload: vote_payload.clone(),
        };
        commit_vote_req_vectors.push(CommitVoteRequestVector {
            name: "commit_vote_request_basic".to_string(),
            description: "Basic CommitVoteRequest with 64-byte payload".to_string(),
            request_id_hex: hex::encode(request_id.as_bytes()),
            vote_request_hash_hex: hex::encode(vote_req_hash.as_bytes()),
            coordinator_signature_hex: hex::encode(coord_sig.to_bytes()),
            payload_len: vote_payload.len(),
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // CommitSelectionCommitment (Type 46) Test Vectors
    // ========================================================================

    let mut commit_selection_vectors = Vec::new();

    // Test 1: Basic CommitSelectionCommitment
    {
        let request_id = test_hash(0x44);
        let selection_id = test_hash(0x55);
        let selection_payload = vec![0x66u8; 16];

        let payload = CommitSelectionCommitmentPayload {
            request_id: request_id.clone(),
            selection_commitment_id: selection_id.clone(),
            selection_commitment_payload: selection_payload.clone(),
        };
        commit_selection_vectors.push(CommitSelectionCommitmentVector {
            name: "commit_selection_commitment_basic".to_string(),
            description: "Basic CommitSelectionCommitment with 16-byte payload".to_string(),
            request_id_hex: hex::encode(request_id.as_bytes()),
            selection_commitment_id_hex: hex::encode(selection_id.as_bytes()),
            payload_len: selection_payload.len(),
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // ========================================================================
    // CommitJurorVote (Type 47) Test Vectors
    // ========================================================================

    let mut commit_juror_vote_vectors = Vec::new();

    // Test 1: Basic CommitJurorVote
    {
        let request_id = test_hash(0x77);
        let juror_pubkey = test_pubkey(0x88);
        let vote_hash = test_hash(0x99);
        let juror_sig = test_signature();
        let vote_payload = vec![0xAAu8; 48];

        let payload = CommitJurorVotePayload {
            request_id: request_id.clone(),
            juror_pubkey: juror_pubkey.clone(),
            vote_hash: vote_hash.clone(),
            juror_signature: juror_sig.clone(),
            vote_payload: vote_payload.clone(),
        };
        commit_juror_vote_vectors.push(CommitJurorVoteVector {
            name: "commit_juror_vote_basic".to_string(),
            description: "Basic CommitJurorVote with 48-byte payload".to_string(),
            request_id_hex: hex::encode(request_id.as_bytes()),
            juror_pubkey_hex: hex::encode(juror_pubkey.as_bytes()),
            vote_hash_hex: hex::encode(vote_hash.as_bytes()),
            juror_signature_hex: hex::encode(juror_sig.to_bytes()),
            payload_len: vote_payload.len(),
            wire_hex: payload.to_hex(),
            expected_size: payload.size(),
        });
    }

    // Write output
    let test_file = ArbitrationTestFile {
        algorithm: "Arbitration-Transactions".to_string(),
        version: 1,
        register_arbiter_vectors: register_vectors,
        update_arbiter_vectors: update_vectors,
        slash_arbiter_vectors: slash_vectors,
        request_arbiter_exit_vectors: request_exit_vectors,
        withdraw_arbiter_stake_vectors: withdraw_vectors,
        cancel_arbiter_exit_vectors: cancel_exit_vectors,
        commit_arbitration_open_vectors: commit_arb_open_vectors,
        commit_vote_request_vectors: commit_vote_req_vectors,
        commit_selection_commitment_vectors: commit_selection_vectors,
        commit_juror_vote_vectors: commit_juror_vote_vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).expect("YAML serialization failed");

    // Add header comment
    let header = r#"# Arbitration Transactions Test Vectors (Types 33-38, 44-47)
# Generated by TOS Rust - gen_arbitration_vectors
# Cross-language verification between TOS Rust and Avatar C
#
# Transaction Types:
#   33: RegisterArbiter    - Register new arbiter
#   34: UpdateArbiter      - Update arbiter settings
#   35: SlashArbiter       - Slash arbiter stake via committee
#   36: RequestArbiterExit - Initiate arbiter exit (empty payload)
#   37: WithdrawArbiterStake - Withdraw stake after cooldown
#   38: CancelArbiterExit  - Cancel exit request (empty payload)
#   44: CommitArbitrationOpen - Commit arbitration open message
#   45: CommitVoteRequest  - Commit vote request message
#   46: CommitSelectionCommitment - Commit selection commitment
#   47: CommitJurorVote    - Commit juror vote message
#
# ExpertiseDomain enum:
#   General=0, AIAgent=1, SmartContract=2, Payment=3, DeFi=4,
#   Governance=5, Identity=6, Data=7, Security=8, Gaming=9,
#   DataService=10, DigitalAsset=11, CrossChain=12, NFT=13
#
# ArbiterStatus enum:
#   Active=0, Suspended=1, Exiting=2, Removed=3

"#;

    let full_yaml = format!("{}{}", header, yaml);
    println!("{}", full_yaml);

    let mut file = File::create("arbitration.yaml").expect("Failed to create file");
    file.write_all(full_yaml.as_bytes())
        .expect("Failed to write file");
    eprintln!("Written to arbitration.yaml");
}
