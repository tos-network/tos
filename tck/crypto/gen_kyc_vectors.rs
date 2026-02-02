// KYC Transaction Test Vector Generator (Types 9-17)
// Generates test vectors for cross-language verification between TOS Rust and Avatar C
//
// Transaction Types:
//   Type 9:  SetKyc - Set user KYC level
//   Type 10: RevokeKyc - Revoke user KYC
//   Type 11: RenewKyc - Renew expiring KYC
//   Type 12: BootstrapCommittee - Create Global Committee (one-time)
//   Type 13: RegisterCommittee - Create regional committee
//   Type 14: UpdateCommittee - Modify committee configuration
//   Type 15: EmergencySuspend - Fast-track KYC suspension
//   Type 16: TransferKyc - Transfer KYC across regions
//   Type 17: AppealKyc - Appeal rejected/revoked KYC

use hex;
use serde::Serialize;
use std::fs::File;
use std::io::Write;
use tos_common::crypto::elgamal::CompressedPublicKey;
use tos_common::crypto::{Hash, Signature};
use tos_common::kyc::{CommitteeApproval, KycRegion, MemberRole};
use tos_common::serializer::Serializer;
use tos_common::transaction::{
    AppealKycPayload, BootstrapCommitteePayload, CommitteeMemberInit, CommitteeUpdateData,
    EmergencySuspendPayload, NewCommitteeMember, RegisterCommitteePayload, RenewKycPayload,
    RevokeKycPayload, SetKycPayload, TransferKycPayload, UpdateCommitteePayload,
};

// ============================================================================
// YAML Structures
// ============================================================================

#[derive(Serialize)]
struct KycTestVectors {
    algorithm: String,
    version: u32,
    set_kyc_vectors: Vec<SetKycVector>,
    revoke_kyc_vectors: Vec<RevokeKycVector>,
    renew_kyc_vectors: Vec<RenewKycVector>,
    bootstrap_committee_vectors: Vec<BootstrapCommitteeVector>,
    register_committee_vectors: Vec<RegisterCommitteeVector>,
    update_committee_vectors: Vec<UpdateCommitteeVector>,
    emergency_suspend_vectors: Vec<EmergencySuspendVector>,
    transfer_kyc_vectors: Vec<TransferKycVector>,
    appeal_kyc_vectors: Vec<AppealKycVector>,
}

#[derive(Serialize)]
struct SetKycVector {
    name: String,
    description: String,
    account_hex: String,
    level: u16,
    verified_at: u64,
    data_hash_hex: String,
    committee_id_hex: String,
    approvals_cnt: usize,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct RevokeKycVector {
    name: String,
    description: String,
    account_hex: String,
    reason_hash_hex: String,
    committee_id_hex: String,
    approvals_cnt: usize,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct RenewKycVector {
    name: String,
    description: String,
    account_hex: String,
    verified_at: u64,
    data_hash_hex: String,
    committee_id_hex: String,
    approvals_cnt: usize,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct BootstrapCommitteeVector {
    name: String,
    description: String,
    committee_name: String,
    members_cnt: usize,
    threshold: u8,
    kyc_threshold: u8,
    max_kyc_level: u16,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct RegisterCommitteeVector {
    name: String,
    description: String,
    committee_name: String,
    region: u8,
    members_cnt: usize,
    threshold: u8,
    kyc_threshold: u8,
    max_kyc_level: u16,
    parent_id_hex: String,
    approvals_cnt: usize,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct UpdateCommitteeVector {
    name: String,
    description: String,
    committee_id_hex: String,
    update_type: u8,
    approvals_cnt: usize,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct EmergencySuspendVector {
    name: String,
    description: String,
    account_hex: String,
    reason_hash_hex: String,
    committee_id_hex: String,
    approvals_cnt: usize,
    expires_at: u64,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct TransferKycVector {
    name: String,
    description: String,
    account_hex: String,
    source_committee_id_hex: String,
    source_approvals_cnt: usize,
    dest_committee_id_hex: String,
    dest_approvals_cnt: usize,
    new_data_hash_hex: String,
    transferred_at: u64,
    wire_hex: String,
    expected_size: usize,
}

#[derive(Serialize)]
struct AppealKycVector {
    name: String,
    description: String,
    account_hex: String,
    original_committee_id_hex: String,
    parent_committee_id_hex: String,
    reason_hash_hex: String,
    documents_hash_hex: String,
    submitted_at: u64,
    wire_hex: String,
    expected_size: usize,
}

// ============================================================================
// Helper Functions
// ============================================================================

fn test_pubkey(seed: u8) -> CompressedPublicKey {
    // Use deterministic byte pattern (from_bytes handles validity)
    let bytes = [seed; 32];
    CompressedPublicKey::from_bytes(&bytes).expect("Valid pubkey bytes")
}

fn test_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

fn test_signature() -> Signature {
    // Use known-valid signature pattern (all 4s works in TOS tests)
    Signature::from_bytes(&[4u8; 64]).expect("Valid signature pattern")
}

fn test_approval(seed: u8, timestamp: u64) -> CommitteeApproval {
    CommitteeApproval::new(test_pubkey(seed), test_signature(), timestamp)
}

// ============================================================================
// Vector Generation
// ============================================================================

fn gen_set_kyc_vectors() -> Vec<SetKycVector> {
    let mut vectors = Vec::new();

    // Basic SetKyc with single approval
    {
        let account = test_pubkey(0x11);
        let level = 7u16; // Tier 1
        let verified_at = 1700000000u64;
        let data_hash = test_hash(0x22);
        let committee_id = test_hash(0x33);
        let approvals = vec![test_approval(0x44, 1700000000)];

        let payload = SetKycPayload::new(
            account.clone(),
            level,
            verified_at,
            data_hash.clone(),
            committee_id.clone(),
            approvals.clone(),
        );
        let wire = payload.to_bytes();

        vectors.push(SetKycVector {
            name: "set_kyc_tier1_single_approval".to_string(),
            description: "Set KYC Tier 1 with single approval".to_string(),
            account_hex: hex::encode(account.as_bytes()),
            level,
            verified_at,
            data_hash_hex: hex::encode(data_hash.as_bytes()),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            approvals_cnt: 1,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // SetKyc with multiple approvals (Tier 5+ requires kyc_threshold + 1)
    {
        let account = test_pubkey(0x55);
        let level = 2047u16; // Tier 5
        let verified_at = 1700100000u64;
        let data_hash = test_hash(0x66);
        let committee_id = test_hash(0x77);
        let approvals = vec![
            test_approval(0x88, 1700100000),
            test_approval(0x99, 1700100001),
        ];

        let payload = SetKycPayload::new(
            account.clone(),
            level,
            verified_at,
            data_hash.clone(),
            committee_id.clone(),
            approvals.clone(),
        );
        let wire = payload.to_bytes();

        vectors.push(SetKycVector {
            name: "set_kyc_tier5_multi_approval".to_string(),
            description: "Set KYC Tier 5 with 2 approvals".to_string(),
            account_hex: hex::encode(account.as_bytes()),
            level,
            verified_at,
            data_hash_hex: hex::encode(data_hash.as_bytes()),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            approvals_cnt: 2,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // SetKyc with no approvals (edge case)
    {
        let account = test_pubkey(0xAA);
        let level = 0u16; // No KYC
        let verified_at = 1700200000u64;
        let data_hash = test_hash(0xBB);
        let committee_id = test_hash(0xCC);
        let approvals: Vec<CommitteeApproval> = vec![];

        let payload = SetKycPayload::new(
            account.clone(),
            level,
            verified_at,
            data_hash.clone(),
            committee_id.clone(),
            approvals,
        );
        let wire = payload.to_bytes();

        vectors.push(SetKycVector {
            name: "set_kyc_no_approvals".to_string(),
            description: "Set KYC with no approvals (invalid but serializable)".to_string(),
            account_hex: hex::encode(account.as_bytes()),
            level,
            verified_at,
            data_hash_hex: hex::encode(data_hash.as_bytes()),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            approvals_cnt: 0,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

fn gen_revoke_kyc_vectors() -> Vec<RevokeKycVector> {
    let mut vectors = Vec::new();

    // Basic revocation
    {
        let account = test_pubkey(0x11);
        let reason_hash = test_hash(0x22);
        let committee_id = test_hash(0x33);
        let approvals = vec![test_approval(0x44, 1700000000)];

        let payload = RevokeKycPayload::new(
            account.clone(),
            reason_hash.clone(),
            committee_id.clone(),
            approvals,
        );
        let wire = payload.to_bytes();

        vectors.push(RevokeKycVector {
            name: "revoke_kyc_basic".to_string(),
            description: "Basic KYC revocation with single approval".to_string(),
            account_hex: hex::encode(account.as_bytes()),
            reason_hash_hex: hex::encode(reason_hash.as_bytes()),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            approvals_cnt: 1,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Revocation with multiple approvals
    {
        let account = test_pubkey(0x55);
        let reason_hash = test_hash(0x66);
        let committee_id = test_hash(0x77);
        let approvals = vec![
            test_approval(0x88, 1700100000),
            test_approval(0x99, 1700100001),
            test_approval(0xAA, 1700100002),
        ];

        let payload = RevokeKycPayload::new(
            account.clone(),
            reason_hash.clone(),
            committee_id.clone(),
            approvals,
        );
        let wire = payload.to_bytes();

        vectors.push(RevokeKycVector {
            name: "revoke_kyc_multi_approval".to_string(),
            description: "KYC revocation with 3 approvals".to_string(),
            account_hex: hex::encode(account.as_bytes()),
            reason_hash_hex: hex::encode(reason_hash.as_bytes()),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            approvals_cnt: 3,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

fn gen_renew_kyc_vectors() -> Vec<RenewKycVector> {
    let mut vectors = Vec::new();

    // Basic renewal
    {
        let account = test_pubkey(0x11);
        let verified_at = 1700000000u64;
        let data_hash = test_hash(0x22);
        let committee_id = test_hash(0x33);
        let approvals = vec![test_approval(0x44, 1700000000)];

        let payload = RenewKycPayload::new(
            account.clone(),
            verified_at,
            data_hash.clone(),
            committee_id.clone(),
            approvals,
        );
        let wire = payload.to_bytes();

        vectors.push(RenewKycVector {
            name: "renew_kyc_basic".to_string(),
            description: "Basic KYC renewal with single approval".to_string(),
            account_hex: hex::encode(account.as_bytes()),
            verified_at,
            data_hash_hex: hex::encode(data_hash.as_bytes()),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            approvals_cnt: 1,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

fn gen_bootstrap_committee_vectors() -> Vec<BootstrapCommitteeVector> {
    let mut vectors = Vec::new();

    // Global Committee bootstrap with 3 members (minimum)
    {
        let name = "Global".to_string();
        let members = vec![
            CommitteeMemberInit::new(test_pubkey(0x11), Some("Alice".to_string()), MemberRole::Chair),
            CommitteeMemberInit::new(test_pubkey(0x22), Some("Bob".to_string()), MemberRole::ViceChair),
            CommitteeMemberInit::new(test_pubkey(0x33), None, MemberRole::Member),
        ];
        let threshold = 2u8;
        let kyc_threshold = 1u8;
        let max_kyc_level = 32767u16; // Tier 8

        let payload = BootstrapCommitteePayload::new(
            name.clone(),
            members.clone(),
            threshold,
            kyc_threshold,
            max_kyc_level,
        );
        let wire = payload.to_bytes();

        vectors.push(BootstrapCommitteeVector {
            name: "bootstrap_global_3members".to_string(),
            description: "Bootstrap Global Committee with 3 members".to_string(),
            committee_name: name,
            members_cnt: 3,
            threshold,
            kyc_threshold,
            max_kyc_level,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Bootstrap with single member (edge case)
    {
        let name = "Test".to_string();
        let members = vec![CommitteeMemberInit::new(
            test_pubkey(0x44),
            None,
            MemberRole::Chair,
        )];
        let threshold = 1u8;
        let kyc_threshold = 1u8;
        let max_kyc_level = 255u16;

        let payload = BootstrapCommitteePayload::new(
            name.clone(),
            members,
            threshold,
            kyc_threshold,
            max_kyc_level,
        );
        let wire = payload.to_bytes();

        vectors.push(BootstrapCommitteeVector {
            name: "bootstrap_single_member".to_string(),
            description: "Bootstrap committee with single member".to_string(),
            committee_name: name,
            members_cnt: 1,
            threshold,
            kyc_threshold,
            max_kyc_level,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

fn gen_register_committee_vectors() -> Vec<RegisterCommitteeVector> {
    let mut vectors = Vec::new();

    // Regional committee registration
    {
        let name = "Asia Pacific".to_string();
        let region = KycRegion::AsiaPacific;
        let members = vec![
            NewCommitteeMember::new(test_pubkey(0x11), Some("Member1".to_string()), MemberRole::Chair),
            NewCommitteeMember::new(test_pubkey(0x22), None, MemberRole::Member),
            NewCommitteeMember::new(test_pubkey(0x33), None, MemberRole::Member),
        ];
        let threshold = 2u8;
        let kyc_threshold = 1u8;
        let max_kyc_level = 8191u16; // Tier 6
        let parent_id = test_hash(0x44);
        let approvals = vec![test_approval(0x55, 1700000000)];

        let payload = RegisterCommitteePayload::new(
            name.clone(),
            region,
            members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            parent_id.clone(),
            approvals,
        );
        let wire = payload.to_bytes();

        vectors.push(RegisterCommitteeVector {
            name: "register_apac_committee".to_string(),
            description: "Register Asia Pacific regional committee".to_string(),
            committee_name: name,
            region: region as u8,
            members_cnt: 3,
            threshold,
            kyc_threshold,
            max_kyc_level,
            parent_id_hex: hex::encode(parent_id.as_bytes()),
            approvals_cnt: 1,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

fn gen_update_committee_vectors() -> Vec<UpdateCommitteeVector> {
    let mut vectors = Vec::new();

    // Update threshold
    {
        let committee_id = test_hash(0x11);
        let update = CommitteeUpdateData::UpdateThreshold { new_threshold: 3 };
        let approvals = vec![test_approval(0x22, 1700000000)];

        let payload = UpdateCommitteePayload::new(committee_id.clone(), update, approvals);
        let wire = payload.to_bytes();

        vectors.push(UpdateCommitteeVector {
            name: "update_threshold".to_string(),
            description: "Update committee governance threshold".to_string(),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            update_type: 4, // UpdateThreshold
            approvals_cnt: 1,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Add member
    {
        let committee_id = test_hash(0x33);
        let update = CommitteeUpdateData::AddMember {
            public_key: test_pubkey(0x44),
            name: Some("NewMember".to_string()),
            role: MemberRole::Member,
        };
        let approvals = vec![
            test_approval(0x55, 1700000000),
            test_approval(0x66, 1700000001),
        ];

        let payload = UpdateCommitteePayload::new(committee_id.clone(), update, approvals);
        let wire = payload.to_bytes();

        vectors.push(UpdateCommitteeVector {
            name: "update_add_member".to_string(),
            description: "Add new member to committee".to_string(),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            update_type: 0, // AddMember
            approvals_cnt: 2,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Remove member
    {
        let committee_id = test_hash(0x77);
        let update = CommitteeUpdateData::RemoveMember {
            public_key: test_pubkey(0x88),
        };
        let approvals = vec![test_approval(0x99, 1700000000)];

        let payload = UpdateCommitteePayload::new(committee_id.clone(), update, approvals);
        let wire = payload.to_bytes();

        vectors.push(UpdateCommitteeVector {
            name: "update_remove_member".to_string(),
            description: "Remove member from committee".to_string(),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            update_type: 1, // RemoveMember
            approvals_cnt: 1,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Suspend committee
    {
        let committee_id = test_hash(0xAA);
        let update = CommitteeUpdateData::SuspendCommittee;
        let approvals = vec![test_approval(0xBB, 1700000000)];

        let payload = UpdateCommitteePayload::new(committee_id.clone(), update, approvals);
        let wire = payload.to_bytes();

        vectors.push(UpdateCommitteeVector {
            name: "update_suspend_committee".to_string(),
            description: "Suspend committee".to_string(),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            update_type: 7, // SuspendCommittee
            approvals_cnt: 1,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

fn gen_emergency_suspend_vectors() -> Vec<EmergencySuspendVector> {
    let mut vectors = Vec::new();

    // Basic emergency suspension
    {
        let account = test_pubkey(0x11);
        let reason_hash = test_hash(0x22);
        let committee_id = test_hash(0x33);
        let approvals = vec![
            test_approval(0x44, 1700000000),
            test_approval(0x55, 1700000001),
        ];
        let expires_at = 1700086400u64; // 24 hours later

        let payload = EmergencySuspendPayload::new(
            account.clone(),
            reason_hash.clone(),
            committee_id.clone(),
            approvals,
            expires_at,
        );
        let wire = payload.to_bytes();

        vectors.push(EmergencySuspendVector {
            name: "emergency_suspend_basic".to_string(),
            description: "Emergency KYC suspension with 2 approvals".to_string(),
            account_hex: hex::encode(account.as_bytes()),
            reason_hash_hex: hex::encode(reason_hash.as_bytes()),
            committee_id_hex: hex::encode(committee_id.as_bytes()),
            approvals_cnt: 2,
            expires_at,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

fn gen_transfer_kyc_vectors() -> Vec<TransferKycVector> {
    let mut vectors = Vec::new();

    // Transfer KYC between regions
    {
        let account = test_pubkey(0x11);
        let source_committee_id = test_hash(0x22);
        let source_approvals = vec![test_approval(0x33, 1700000000)];
        let dest_committee_id = test_hash(0x44);
        let dest_approvals = vec![test_approval(0x55, 1700000001)];
        let new_data_hash = test_hash(0x66);
        let transferred_at = 1700000000u64;

        let payload = TransferKycPayload::new(
            account.clone(),
            source_committee_id.clone(),
            source_approvals,
            dest_committee_id.clone(),
            dest_approvals,
            new_data_hash.clone(),
            transferred_at,
        );
        let wire = payload.to_bytes();

        vectors.push(TransferKycVector {
            name: "transfer_kyc_basic".to_string(),
            description: "Transfer KYC between regions".to_string(),
            account_hex: hex::encode(account.as_bytes()),
            source_committee_id_hex: hex::encode(source_committee_id.as_bytes()),
            source_approvals_cnt: 1,
            dest_committee_id_hex: hex::encode(dest_committee_id.as_bytes()),
            dest_approvals_cnt: 1,
            new_data_hash_hex: hex::encode(new_data_hash.as_bytes()),
            transferred_at,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    // Transfer with no destination approvals (edge case)
    {
        let account = test_pubkey(0x77);
        let source_committee_id = test_hash(0x88);
        let source_approvals = vec![test_approval(0x99, 1700100000)];
        let dest_committee_id = test_hash(0xAA);
        let dest_approvals: Vec<CommitteeApproval> = vec![];
        let new_data_hash = test_hash(0xBB);
        let transferred_at = 1700100000u64;

        let payload = TransferKycPayload::new(
            account.clone(),
            source_committee_id.clone(),
            source_approvals,
            dest_committee_id.clone(),
            dest_approvals,
            new_data_hash.clone(),
            transferred_at,
        );
        let wire = payload.to_bytes();

        vectors.push(TransferKycVector {
            name: "transfer_kyc_no_dest_approvals".to_string(),
            description: "Transfer KYC with no destination approvals (invalid but serializable)"
                .to_string(),
            account_hex: hex::encode(account.as_bytes()),
            source_committee_id_hex: hex::encode(source_committee_id.as_bytes()),
            source_approvals_cnt: 1,
            dest_committee_id_hex: hex::encode(dest_committee_id.as_bytes()),
            dest_approvals_cnt: 0,
            new_data_hash_hex: hex::encode(new_data_hash.as_bytes()),
            transferred_at,
            wire_hex: hex::encode(&wire),
            expected_size: wire.len(),
        });
    }

    vectors
}

fn gen_appeal_kyc_vectors() -> Vec<AppealKycVector> {
    let mut vectors = Vec::new();

    // Basic appeal
    {
        let account = test_pubkey(0x11);
        let original_committee_id = test_hash(0x22);
        let parent_committee_id = test_hash(0x33);
        let reason_hash = test_hash(0x44);
        let documents_hash = test_hash(0x55);
        let submitted_at = 1700000000u64;

        let payload = AppealKycPayload::new(
            account.clone(),
            original_committee_id.clone(),
            parent_committee_id.clone(),
            reason_hash.clone(),
            documents_hash.clone(),
            submitted_at,
        );
        let wire = payload.to_bytes();

        vectors.push(AppealKycVector {
            name: "appeal_kyc_basic".to_string(),
            description: "Basic KYC appeal to parent committee".to_string(),
            account_hex: hex::encode(account.as_bytes()),
            original_committee_id_hex: hex::encode(original_committee_id.as_bytes()),
            parent_committee_id_hex: hex::encode(parent_committee_id.as_bytes()),
            reason_hash_hex: hex::encode(reason_hash.as_bytes()),
            documents_hash_hex: hex::encode(documents_hash.as_bytes()),
            submitted_at,
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
    let vectors = KycTestVectors {
        algorithm: "KYC-Transactions".to_string(),
        version: 1,
        set_kyc_vectors: gen_set_kyc_vectors(),
        revoke_kyc_vectors: gen_revoke_kyc_vectors(),
        renew_kyc_vectors: gen_renew_kyc_vectors(),
        bootstrap_committee_vectors: gen_bootstrap_committee_vectors(),
        register_committee_vectors: gen_register_committee_vectors(),
        update_committee_vectors: gen_update_committee_vectors(),
        emergency_suspend_vectors: gen_emergency_suspend_vectors(),
        transfer_kyc_vectors: gen_transfer_kyc_vectors(),
        appeal_kyc_vectors: gen_appeal_kyc_vectors(),
    };

    let yaml = serde_yaml::to_string(&vectors).expect("Failed to serialize to YAML");

    // Add header comment
    let header = r#"# KYC Transactions Test Vectors (Types 9-17)
# Generated by TOS Rust - gen_kyc_vectors
# Cross-language verification between TOS Rust and Avatar C
#
# Transaction Types:
#   Type 9:  SetKyc - Set user KYC level (with committee approvals)
#   Type 10: RevokeKyc - Revoke user KYC
#   Type 11: RenewKyc - Renew expiring KYC
#   Type 12: BootstrapCommittee - Create Global Committee (one-time)
#   Type 13: RegisterCommittee - Create regional committee
#   Type 14: UpdateCommittee - Modify committee configuration
#   Type 15: EmergencySuspend - Fast-track KYC suspension (2 members, 24h timeout)
#   Type 16: TransferKyc - Transfer KYC across regions (dual committee approval)
#   Type 17: AppealKyc - Appeal rejected/revoked KYC to parent committee
#
# CommitteeApproval wire format:
#   member_pubkey (32 bytes) + signature (64 bytes) + timestamp (8 bytes BE)

"#;

    let output = format!("{}{}", header, yaml);

    // Write to file
    let output_path = "kyc.yaml";
    let mut file = File::create(output_path).expect("Failed to create output file");
    file.write_all(output.as_bytes())
        .expect("Failed to write output");

    println!("Generated {} vectors to {}", "KYC", output_path);
    println!("  SetKyc: {}", vectors.set_kyc_vectors.len());
    println!("  RevokeKyc: {}", vectors.revoke_kyc_vectors.len());
    println!("  RenewKyc: {}", vectors.renew_kyc_vectors.len());
    println!(
        "  BootstrapCommittee: {}",
        vectors.bootstrap_committee_vectors.len()
    );
    println!(
        "  RegisterCommittee: {}",
        vectors.register_committee_vectors.len()
    );
    println!(
        "  UpdateCommittee: {}",
        vectors.update_committee_vectors.len()
    );
    println!(
        "  EmergencySuspend: {}",
        vectors.emergency_suspend_vectors.len()
    );
    println!("  TransferKyc: {}", vectors.transfer_kyc_vectors.len());
    println!("  AppealKyc: {}", vectors.appeal_kyc_vectors.len());
}
