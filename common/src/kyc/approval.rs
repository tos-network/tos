//! # Approval Verification Module
//!
//! This module provides stateful verification of committee approvals
//! for KYC transactions. It verifies:
//! 1. Committee existence and active status
//! 2. Approver membership and active status
//! 3. Signature validity using domain-separated messages
//! 4. Threshold enforcement
//!
//! # Threshold Requirements Summary
//!
//! All KYC operations require cryptographically signed approvals from active
//! committee members. The required number of approvals varies by operation type.
//!
//! ## Threshold Calculation by Operation Type
//!
//! | Operation | Threshold Source | Formula | Example (7 members) |
//! |-----------|-----------------|---------|---------------------|
//! | SetKyc (Tier 1-4) | `kyc_threshold` | Fixed | 1 |
//! | SetKyc (Tier 5+) | `kyc_threshold + 1` | High-tier bonus | 2 |
//! | RevokeKyc | `kyc_threshold` | Fixed | 1 |
//! | RenewKyc | `kyc_threshold` | Fixed | 1 |
//! | TransferKyc | `kyc_threshold` | Both committees | 1 each |
//! | EmergencySuspend | Fixed | Always 2 | 2 |
//! | RegisterCommittee | `threshold` | Parent's governance | 5 (≥2/3) |
//! | UpdateCommittee | `threshold` | Governance | 5 (≥2/3) |
//! | AddMember | `threshold` | Governance | 5 (≥2/3) |
//! | RemoveMember | `threshold` | Governance | 5 (≥2/3) |
//! | AppealKyc | `threshold` | Parent committee | 5 (≥2/3) |
//!
//! ## Key Concepts
//!
//! ### Two Threshold Types
//!
//! Each committee has two separate thresholds:
//!
//! - **`kyc_threshold`**: For routine KYC operations (SetKyc, RevokeKyc, RenewKyc, TransferKyc)
//!   - Typically set to 1-3 for efficiency
//!   - Allows daily KYC approvals without requiring full committee consensus
//!
//! - **`threshold`** (governance threshold): For structural/governance changes
//!   - Must be ≥ 2/3 of member count (enforced by `verify_update_committee_with_state`)
//!   - Used for: RegisterCommittee, UpdateCommittee, AddMember, RemoveMember, AppealKyc
//!   - Ensures strong consensus for important decisions
//!
//! ### RegisterCommittee Threshold
//!
//! Creating a new committee requires approval from the **parent committee**:
//!
//! ```text
//! Required = parent_committee.threshold  (governance threshold)
//! ```
//!
//! For example, if Global Committee has 11 members with threshold=8:
//! - Creating a new regional committee requires 8 approvals from Global members
//!
//! ### High-Tier KYC Bonus
//!
//! For SetKyc operations at Tier 5 or higher, an additional approval is required:
//!
//! ```text
//! Required = kyc_threshold + 1  (if tier >= 5)
//! ```
//!
//! This adds extra security for high-value KYC levels.
//!
//! ### Emergency Operations
//!
//! Emergency operations have fixed thresholds regardless of committee configuration:
//!
//! - `EmergencySuspend`: 2 approvals (allows quick response)
//! - `EmergencyRemoveMember`: 3 approvals
//!
//! ## Security
//!
//! - **Domain separation**: Each operation type has a unique message prefix
//! - **Timestamp binding**: Approvals include timestamps to prevent replay
//! - **Member validation**: Only active committee members can approve
//! - **Cryptographic verification**: All signatures are verified against the message
//!
//! SECURITY: This is critical for consensus - all approval validations
//! must be deterministic and use chain state.

use crate::crypto::{Hash, PublicKey};
use crate::kyc::{
    level_to_tier, CommitteeApproval, CommitteeStatus, MemberStatus, OperationType,
    SecurityCommittee,
};

/// Result of approval verification
#[derive(Debug, Clone)]
pub struct ApprovalVerificationResult {
    /// Number of valid approvals (signature valid + member active)
    pub valid_count: usize,
    /// Required threshold for the operation
    pub required_threshold: u8,
    /// Whether threshold was met
    pub threshold_met: bool,
    /// Detailed results for each approval
    pub approval_results: Vec<ApprovalCheckResult>,
}

/// Result of checking a single approval
#[derive(Debug, Clone)]
pub struct ApprovalCheckResult {
    /// The approver's public key
    pub approver: PublicKey,
    /// Whether the approver is an active committee member
    pub is_active_member: bool,
    /// Whether the signature is valid
    pub signature_valid: bool,
    /// Whether the approval is expired
    pub is_expired: bool,
    /// Combined validity (all checks pass)
    pub is_valid: bool,
}

/// Error types for approval verification
#[derive(Debug, Clone, thiserror::Error)]
pub enum ApprovalError {
    #[error("Committee not found: {0}")]
    CommitteeNotFound(Hash),

    #[error("Committee is not active: {0}")]
    CommitteeNotActive(Hash),

    #[error("No approvals provided")]
    NoApprovals,

    #[error("Threshold not met: required {required}, got {actual}")]
    ThresholdNotMet { required: u8, actual: usize },

    #[error("Invalid approval signature from {0:?}")]
    InvalidSignature(PublicKey),

    #[error("Approver is not an active committee member: {0:?}")]
    NotActiveMember(PublicKey),

    #[error("Approval expired for {0:?}")]
    ApprovalExpired(PublicKey),

    #[error("Committee cannot grant level {level} (max: {max_level})")]
    LevelExceedsMax { level: u16, max_level: u16 },
}

/// Verify approvals for SetKyc operation
///
/// Validates:
/// - Committee exists and is active
/// - Each approver is an active member
/// - Each signature is valid for the SetKyc message
/// - Threshold is met for the KYC level
pub fn verify_set_kyc_approvals(
    committee: &SecurityCommittee,
    approvals: &[CommitteeApproval],
    account: &PublicKey,
    level: u16,
    data_hash: &Hash,
    current_time: u64,
) -> Result<ApprovalVerificationResult, ApprovalError> {
    // Check committee is active
    if committee.status != CommitteeStatus::Active {
        return Err(ApprovalError::CommitteeNotActive(committee.id.clone()));
    }

    // Check committee can grant this level
    if level > committee.max_kyc_level {
        return Err(ApprovalError::LevelExceedsMax {
            level,
            max_level: committee.max_kyc_level,
        });
    }

    // Build the signing message
    let build_message = |approval: &CommitteeApproval| {
        CommitteeApproval::build_set_kyc_message(
            &committee.id,
            account,
            level,
            data_hash,
            approval.timestamp,
        )
    };

    // Determine required threshold
    let tier = level_to_tier(level);
    let required = committee.required_threshold(&OperationType::SetKyc, Some(tier));

    verify_approvals_internal(committee, approvals, build_message, required, current_time)
}

/// Verify approvals for RevokeKyc operation
pub fn verify_revoke_kyc_approvals(
    committee: &SecurityCommittee,
    approvals: &[CommitteeApproval],
    account: &PublicKey,
    reason_hash: &Hash,
    current_time: u64,
) -> Result<ApprovalVerificationResult, ApprovalError> {
    // Check committee is active
    if committee.status != CommitteeStatus::Active {
        return Err(ApprovalError::CommitteeNotActive(committee.id.clone()));
    }

    let build_message = |approval: &CommitteeApproval| {
        CommitteeApproval::build_revoke_kyc_message(
            &committee.id,
            account,
            reason_hash,
            approval.timestamp,
        )
    };

    let required = committee.required_threshold(&OperationType::RevokeKyc, None);

    verify_approvals_internal(committee, approvals, build_message, required, current_time)
}

/// Verify approvals for RenewKyc operation
pub fn verify_renew_kyc_approvals(
    committee: &SecurityCommittee,
    approvals: &[CommitteeApproval],
    account: &PublicKey,
    data_hash: &Hash,
    current_time: u64,
) -> Result<ApprovalVerificationResult, ApprovalError> {
    // Check committee is active
    if committee.status != CommitteeStatus::Active {
        return Err(ApprovalError::CommitteeNotActive(committee.id.clone()));
    }

    let build_message = |approval: &CommitteeApproval| {
        CommitteeApproval::build_renew_kyc_message(
            &committee.id,
            account,
            data_hash,
            approval.timestamp,
        )
    };

    let required = committee.required_threshold(&OperationType::RenewKyc, None);

    verify_approvals_internal(committee, approvals, build_message, required, current_time)
}

/// Verify approvals for TransferKyc - source committee side
pub fn verify_transfer_kyc_source_approvals(
    source_committee: &SecurityCommittee,
    approvals: &[CommitteeApproval],
    dest_committee_id: &Hash,
    account: &PublicKey,
    current_time: u64,
) -> Result<ApprovalVerificationResult, ApprovalError> {
    if source_committee.status != CommitteeStatus::Active {
        return Err(ApprovalError::CommitteeNotActive(
            source_committee.id.clone(),
        ));
    }

    let source_id = source_committee.id.clone();
    let dest_id = dest_committee_id.clone();
    let account = account.clone();

    let build_message = move |approval: &CommitteeApproval| {
        CommitteeApproval::build_transfer_kyc_source_message(
            &source_id,
            &dest_id,
            &account,
            approval.timestamp,
        )
    };

    let required = source_committee.required_threshold(&OperationType::TransferKyc, None);

    verify_approvals_internal(
        source_committee,
        approvals,
        build_message,
        required,
        current_time,
    )
}

/// Verify approvals for TransferKyc - destination committee side
pub fn verify_transfer_kyc_dest_approvals(
    dest_committee: &SecurityCommittee,
    approvals: &[CommitteeApproval],
    source_committee_id: &Hash,
    account: &PublicKey,
    new_data_hash: &Hash,
    current_time: u64,
) -> Result<ApprovalVerificationResult, ApprovalError> {
    if dest_committee.status != CommitteeStatus::Active {
        return Err(ApprovalError::CommitteeNotActive(dest_committee.id.clone()));
    }

    let source_id = source_committee_id.clone();
    let dest_id = dest_committee.id.clone();
    let account = account.clone();
    let new_data_hash = new_data_hash.clone();

    let build_message = move |approval: &CommitteeApproval| {
        CommitteeApproval::build_transfer_kyc_dest_message(
            &source_id,
            &dest_id,
            &account,
            &new_data_hash,
            approval.timestamp,
        )
    };

    let required = dest_committee.required_threshold(&OperationType::TransferKyc, None);

    verify_approvals_internal(
        dest_committee,
        approvals,
        build_message,
        required,
        current_time,
    )
}

/// Verify approvals for EmergencySuspend operation
///
/// # Policy Decision
/// Emergency suspend operations are allowed even if the issuing committee is suspended.
/// Rationale: Emergency actions (e.g., responding to fraud, security incidents) must
/// remain available regardless of committee operational status. A suspended committee
/// may still need to protect users by issuing emergency suspensions.
///
/// However, **Dissolved** committees cannot issue emergency suspensions, as they
/// no longer have operational authority.
/// Approved: 2025-12-29
pub fn verify_emergency_suspend_approvals(
    committee: &SecurityCommittee,
    approvals: &[CommitteeApproval],
    account: &PublicKey,
    reason_hash: &Hash,
    expires_at: u64,
    current_time: u64,
) -> Result<ApprovalVerificationResult, ApprovalError> {
    // POLICY DECISION: Emergency operations are allowed if committee is Active or Suspended,
    // but NOT if Dissolved. Dissolved committees have no operational authority.
    if committee.status == CommitteeStatus::Dissolved {
        return Err(ApprovalError::CommitteeNotActive(committee.id.clone()));
    }

    let build_message = |approval: &CommitteeApproval| {
        CommitteeApproval::build_emergency_suspend_message(
            &committee.id,
            account,
            reason_hash,
            expires_at,
            approval.timestamp,
        )
    };

    let required = committee.required_threshold(&OperationType::EmergencySuspend, None);

    verify_approvals_internal(committee, approvals, build_message, required, current_time)
}

/// Verify approvals for RegisterCommittee operation
///
/// The `config_hash` binds the approval signatures to the full committee configuration
/// (members, thresholds, max_kyc_level), preventing approval replay with different configs.
pub fn verify_register_committee_approvals(
    parent_committee: &SecurityCommittee,
    approvals: &[CommitteeApproval],
    name: &str,
    region: crate::kyc::KycRegion,
    config_hash: &Hash,
    current_time: u64,
) -> Result<ApprovalVerificationResult, ApprovalError> {
    if parent_committee.status != CommitteeStatus::Active {
        return Err(ApprovalError::CommitteeNotActive(
            parent_committee.id.clone(),
        ));
    }

    let parent_id = parent_committee.id.clone();
    let name = name.to_string();
    let config_hash = config_hash.clone();

    let build_message = move |approval: &CommitteeApproval| {
        CommitteeApproval::build_register_committee_message(
            &parent_id,
            &name,
            region,
            &config_hash,
            approval.timestamp,
        )
    };

    let required = parent_committee.required_threshold(&OperationType::RegisterCommittee, None);

    verify_approvals_internal(
        parent_committee,
        approvals,
        build_message,
        required,
        current_time,
    )
}

/// Verify approvals for UpdateCommittee operation
pub fn verify_update_committee_approvals(
    committee: &SecurityCommittee,
    approvals: &[CommitteeApproval],
    update_type: u8,
    update_data_hash: &Hash,
    current_time: u64,
) -> Result<ApprovalVerificationResult, ApprovalError> {
    // Allow updates even if committee is suspended (to unsuspend it)

    let committee_id = committee.id.clone();
    let update_data_hash = update_data_hash.clone();

    let build_message = move |approval: &CommitteeApproval| {
        CommitteeApproval::build_update_committee_message(
            &committee_id,
            update_type,
            &update_data_hash,
            approval.timestamp,
        )
    };

    // Use governance threshold for all update operations
    let required = committee.threshold;

    verify_approvals_internal(committee, approvals, build_message, required, current_time)
}

/// Internal function to verify approvals against a committee
///
/// This function deduplicates approvals by member_pubkey to prevent a single
/// member's approval from being counted multiple times toward the threshold.
fn verify_approvals_internal<F>(
    committee: &SecurityCommittee,
    approvals: &[CommitteeApproval],
    build_message: F,
    required_threshold: u8,
    current_time: u64,
) -> Result<ApprovalVerificationResult, ApprovalError>
where
    F: Fn(&CommitteeApproval) -> Vec<u8>,
{
    use std::collections::HashSet;

    if approvals.is_empty() {
        return Err(ApprovalError::NoApprovals);
    }

    let mut approval_results = Vec::with_capacity(approvals.len());
    let mut valid_count = 0usize;
    // Track seen approvers to prevent duplicate counting
    let mut seen_approvers: HashSet<PublicKey> = HashSet::new();

    for approval in approvals {
        // Check for duplicate approvers - each member can only approve once
        let is_duplicate = seen_approvers.contains(&approval.member_pubkey);

        // Check if approval is expired
        let is_expired = approval.is_expired(current_time);

        // Check if approver is an active committee member with approval rights
        // Observers cannot approve (role.can_approve() returns false for Observer)
        let is_active_member = committee.members.iter().any(|m| {
            m.public_key == approval.member_pubkey
                && m.status == MemberStatus::Active
                && m.role.can_approve()
        });

        // Verify signature
        let message = build_message(approval);
        let signature_valid = if is_active_member && !is_expired && !is_duplicate {
            approval.verify_signature(&message)
        } else {
            false
        };

        // Approval is valid only if not duplicate, not expired, is active member, and signature valid
        let is_valid = !is_duplicate && is_active_member && signature_valid && !is_expired;

        if is_valid {
            valid_count += 1;
            // Mark this approver as seen to prevent double-counting
            seen_approvers.insert(approval.member_pubkey.clone());
        }

        approval_results.push(ApprovalCheckResult {
            approver: approval.member_pubkey.clone(),
            is_active_member,
            signature_valid,
            is_expired,
            is_valid,
        });
    }

    let threshold_met = valid_count >= required_threshold as usize;

    if !threshold_met {
        return Err(ApprovalError::ThresholdNotMet {
            required: required_threshold,
            actual: valid_count,
        });
    }

    Ok(ApprovalVerificationResult {
        valid_count,
        required_threshold,
        threshold_met,
        approval_results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;
    use crate::kyc::{CommitteeMember, KycRegion, MemberRole};

    fn create_test_committee(member_count: usize) -> (SecurityCommittee, Vec<KeyPair>) {
        let mut keypairs = Vec::with_capacity(member_count);
        let mut members = Vec::with_capacity(member_count);

        for i in 0..member_count {
            let keypair = KeyPair::new();
            let role = if i == 0 {
                MemberRole::Chair
            } else {
                MemberRole::Member
            };
            members.push(CommitteeMember::new(
                keypair.get_public_key().compress(),
                Some(format!("Member {}", i)),
                role,
                1000,
            ));
            keypairs.push(keypair);
        }

        let committee = SecurityCommittee::new(
            Hash::zero(),
            KycRegion::Global,
            "Test Committee".to_string(),
            members,
            4,     // threshold
            32767, // max level
            None,
            1000,
        );

        (committee, keypairs)
    }

    fn create_signed_approval(
        keypair: &KeyPair,
        message: &[u8],
        timestamp: u64,
    ) -> CommitteeApproval {
        let signature = keypair.sign(message);
        CommitteeApproval::new(keypair.get_public_key().compress(), signature, timestamp)
    }

    #[test]
    fn test_verify_set_kyc_approvals_valid() {
        let (committee, keypairs) = create_test_committee(5);
        let account = keypairs[0].get_public_key().compress();
        let level = 31u16;
        let data_hash = Hash::zero();
        let current_time = 2000u64;

        // Create valid approvals from first 2 members
        let mut approvals = Vec::new();
        for keypair in keypairs.iter().take(2) {
            let timestamp = current_time - 100;
            let message = CommitteeApproval::build_set_kyc_message(
                &committee.id,
                &account,
                level,
                &data_hash,
                timestamp,
            );
            approvals.push(create_signed_approval(keypair, &message, timestamp));
        }

        let result = verify_set_kyc_approvals(
            &committee,
            &approvals,
            &account,
            level,
            &data_hash,
            current_time,
        );

        // Should pass because kyc_threshold defaults to 1
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.valid_count, 2);
        assert!(result.threshold_met);
    }

    #[test]
    fn test_verify_set_kyc_approvals_invalid_signature() {
        let (committee, keypairs) = create_test_committee(5);
        let account = keypairs[0].get_public_key().compress();
        let level = 31u16;
        let data_hash = Hash::zero();
        let current_time = 2000u64;

        // Create approval with wrong message (forged signature)
        let timestamp = current_time - 100;
        let wrong_message = b"wrong message";
        let signature = keypairs[0].sign(wrong_message);
        let forged_approval = CommitteeApproval::new(
            keypairs[0].get_public_key().compress(),
            signature,
            timestamp,
        );

        let result = verify_set_kyc_approvals(
            &committee,
            &[forged_approval],
            &account,
            level,
            &data_hash,
            current_time,
        );

        // Should fail because signature is invalid
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_set_kyc_approvals_non_member() {
        let (committee, keypairs) = create_test_committee(5);
        let account = keypairs[0].get_public_key().compress();
        let level = 31u16;
        let data_hash = Hash::zero();
        let current_time = 2000u64;

        // Create approval from non-member
        let outsider = KeyPair::new();
        let timestamp = current_time - 100;
        let message = CommitteeApproval::build_set_kyc_message(
            &committee.id,
            &account,
            level,
            &data_hash,
            timestamp,
        );
        let approval = create_signed_approval(&outsider, &message, timestamp);

        let result = verify_set_kyc_approvals(
            &committee,
            &[approval],
            &account,
            level,
            &data_hash,
            current_time,
        );

        // Should fail because approver is not a member
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_set_kyc_approvals_expired() {
        let (committee, keypairs) = create_test_committee(5);
        let account = keypairs[0].get_public_key().compress();
        let level = 31u16;
        let data_hash = Hash::zero();
        let current_time = 100_000u64;

        // Create approval with old timestamp (expired)
        let timestamp = 1000u64; // Very old
        let message = CommitteeApproval::build_set_kyc_message(
            &committee.id,
            &account,
            level,
            &data_hash,
            timestamp,
        );
        let approval = create_signed_approval(&keypairs[0], &message, timestamp);

        let result = verify_set_kyc_approvals(
            &committee,
            &[approval],
            &account,
            level,
            &data_hash,
            current_time,
        );

        // Should fail because approval is expired
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_set_kyc_level_exceeds_max() {
        let mut committee_data = create_test_committee(5);
        committee_data.0.max_kyc_level = 255; // Tier 4 max
        let (committee, keypairs) = committee_data;

        let account = keypairs[0].get_public_key().compress();
        let level = 2047u16; // Tier 5, exceeds max
        let data_hash = Hash::zero();
        let current_time = 2000u64;

        let timestamp = current_time - 100;
        let message = CommitteeApproval::build_set_kyc_message(
            &committee.id,
            &account,
            level,
            &data_hash,
            timestamp,
        );
        let approval = create_signed_approval(&keypairs[0], &message, timestamp);

        let result = verify_set_kyc_approvals(
            &committee,
            &[approval],
            &account,
            level,
            &data_hash,
            current_time,
        );

        assert!(matches!(
            result,
            Err(ApprovalError::LevelExceedsMax {
                level: 2047,
                max_level: 255
            })
        ));
    }
}
