// KYC Transaction Verification Module
//
// This module provides static verification logic for KYC transactions.
// It validates payload structure and basic constraints that can be checked
// without accessing blockchain state.
//
// State-dependent validations (committee existence, member authorization, etc.)
// are handled at execution time by the KycProvider.

use crate::kyc::{is_valid_kyc_level, level_to_tier, CommitteeApproval, APPROVAL_EXPIRY_SECONDS};
use crate::transaction::payload::{
    AppealKycPayload, BootstrapCommitteePayload, EmergencySuspendPayload, RegisterCommitteePayload,
    RenewKycPayload, RevokeKycPayload, SetKycPayload, TransferKycPayload, UpdateCommitteePayload,
};

use super::VerificationError;

/// Maximum number of committee members
pub const MAX_COMMITTEE_MEMBERS: usize = 21;

/// Minimum number of committee members
pub const MIN_COMMITTEE_MEMBERS: usize = 3;

/// Maximum number of approvals in a single transaction
pub const MAX_APPROVALS: usize = 15;

/// Minimum approvals for emergency suspend
pub const EMERGENCY_SUSPEND_MIN_APPROVALS: usize = 2;

/// Maximum committee name length
pub const MAX_COMMITTEE_NAME_LEN: usize = 128;

/// Maximum member name length
pub const MAX_MEMBER_NAME_LEN: usize = 64;

/// 24 hours in seconds (emergency suspend timeout)
pub const EMERGENCY_SUSPEND_TIMEOUT: u64 = 24 * 60 * 60;

/// Verify SetKyc transaction payload
///
/// # Arguments
/// * `payload` - The SetKyc payload to verify
/// * `current_time` - Current timestamp (block timestamp for deterministic validation)
pub fn verify_set_kyc<E>(
    payload: &SetKycPayload,
    current_time: u64,
) -> Result<(), VerificationError<E>> {
    // Validate KYC level
    if !is_valid_kyc_level(payload.get_level()) {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Invalid KYC level: {}. Valid levels are 0, 7, 31, 63, 255, 2047, 8191, 16383, 32767",
            payload.get_level()
        )));
    }

    // Validate approvals
    verify_approvals(payload.get_approvals(), current_time)?;

    // All KYC operations require at least 1 approval
    if payload.get_approvals().is_empty() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "SetKyc requires at least 1 approval"
        )));
    }

    // Validate verified_at is reasonable (not in far future)
    // Allow up to 1 hour in the future for clock skew
    let max_future = current_time + 3600;

    if payload.get_verified_at() > max_future {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Verification timestamp too far in the future"
        )));
    }

    // Tier 5+ requires additional approvals
    let tier = level_to_tier(payload.get_level());
    if tier >= 5 && payload.get_approvals().len() < 2 {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Tier {} KYC requires at least 2 approvals, got {}",
            tier,
            payload.get_approvals().len()
        )));
    }

    Ok(())
}

/// Verify RevokeKyc transaction payload
///
/// # Arguments
/// * `payload` - The RevokeKyc payload to verify
/// * `current_time` - Current timestamp (block timestamp for deterministic validation)
pub fn verify_revoke_kyc<E>(
    payload: &RevokeKycPayload,
    current_time: u64,
) -> Result<(), VerificationError<E>> {
    // Validate approvals
    verify_approvals(payload.get_approvals(), current_time)?;

    // Must have at least 1 approval
    if payload.get_approvals().is_empty() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "RevokeKyc requires at least 1 approval"
        )));
    }

    Ok(())
}

/// Verify RenewKyc transaction payload
///
/// # Arguments
/// * `payload` - The RenewKyc payload to verify
/// * `current_time` - Current timestamp (block timestamp for deterministic validation)
pub fn verify_renew_kyc<E>(
    payload: &RenewKycPayload,
    current_time: u64,
) -> Result<(), VerificationError<E>> {
    // Validate approvals
    verify_approvals(payload.get_approvals(), current_time)?;

    // Must have at least 1 approval
    if payload.get_approvals().is_empty() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "RenewKyc requires at least 1 approval"
        )));
    }

    // Validate verified_at is reasonable
    let max_future = current_time + 3600;

    if payload.get_verified_at() > max_future {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Verification timestamp too far in the future"
        )));
    }

    Ok(())
}

/// Verify TransferKyc transaction payload
///
/// # Arguments
/// * `payload` - The TransferKyc payload to verify
/// * `current_time` - Current timestamp (block timestamp for deterministic validation)
///
/// # Validation Rules
/// 1. Source and destination committees must be different
/// 2. Both source and destination must have at least 1 approval
/// 3. All approvals must be valid (not expired, no duplicates)
/// 4. transferred_at must be reasonable (not far in the future)
pub fn verify_transfer_kyc<E>(
    payload: &TransferKycPayload,
    current_time: u64,
) -> Result<(), VerificationError<E>> {
    // Source and destination committees must be different
    if payload.get_source_committee_id() == payload.get_dest_committee_id() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Source and destination committees must be different"
        )));
    }

    // Validate source approvals
    verify_approvals(payload.get_source_approvals(), current_time)?;

    // Source must have at least 1 approval
    if payload.get_source_approvals().is_empty() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "TransferKyc requires at least 1 approval from source committee"
        )));
    }

    // Validate destination approvals
    verify_approvals(payload.get_dest_approvals(), current_time)?;

    // Destination must have at least 1 approval
    if payload.get_dest_approvals().is_empty() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "TransferKyc requires at least 1 approval from destination committee"
        )));
    }

    // Check for duplicate approvers across both committees
    // (same person cannot approve for both source and destination)
    let mut all_approvers = std::collections::HashSet::new();
    for approval in payload.get_source_approvals() {
        all_approvers.insert(approval.member_pubkey.as_bytes());
    }
    for approval in payload.get_dest_approvals() {
        if !all_approvers.insert(approval.member_pubkey.as_bytes()) {
            return Err(VerificationError::AnyError(anyhow::anyhow!(
                "Same member cannot approve for both source and destination committees"
            )));
        }
    }

    // Validate transferred_at is reasonable (not in far future)
    let max_future = current_time + 3600;
    if payload.get_transferred_at() > max_future {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Transfer timestamp too far in the future"
        )));
    }

    Ok(())
}

/// Verify AppealKyc transaction payload
///
/// This validates the basic structure of an appeal transaction.
/// State-dependent validations (original committee exists, parent committee exists,
/// user has revoked KYC, parent is actually parent of original, etc.)
/// are handled at execution time.
pub fn verify_appeal_kyc<E>(
    payload: &AppealKycPayload,
    current_time: u64,
) -> Result<(), VerificationError<E>> {
    // Original and parent committee must be different
    if payload.get_original_committee_id() == payload.get_parent_committee_id() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Original committee and parent committee must be different"
        )));
    }

    // Appeal submitted_at must not be in far future
    let max_future = current_time + 3600; // 1 hour tolerance
    if payload.get_submitted_at() > max_future {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Appeal submission timestamp too far in the future"
        )));
    }

    // Reason hash and documents hash cannot be zero (empty)
    let zero_hash = crate::crypto::Hash::zero();
    if payload.get_reason_hash() == &zero_hash {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Appeal reason hash cannot be empty"
        )));
    }

    if payload.get_documents_hash() == &zero_hash {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Appeal documents hash cannot be empty"
        )));
    }

    Ok(())
}

/// Verify BootstrapCommittee transaction payload
pub fn verify_bootstrap_committee<E>(
    payload: &BootstrapCommitteePayload,
) -> Result<(), VerificationError<E>> {
    // Validate committee name
    if payload.get_name().is_empty() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Committee name cannot be empty"
        )));
    }

    if payload.get_name().len() > MAX_COMMITTEE_NAME_LEN {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Committee name too long: {} > {}",
            payload.get_name().len(),
            MAX_COMMITTEE_NAME_LEN
        )));
    }

    // Validate member count
    let member_count = payload.get_members().len();
    if member_count < MIN_COMMITTEE_MEMBERS {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Committee requires at least {} members, got {}",
            MIN_COMMITTEE_MEMBERS,
            member_count
        )));
    }

    if member_count > MAX_COMMITTEE_MEMBERS {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Committee can have at most {} members, got {}",
            MAX_COMMITTEE_MEMBERS,
            member_count
        )));
    }

    // Validate member names
    for member in payload.get_members() {
        if let Some(ref name) = member.name {
            if name.len() > MAX_MEMBER_NAME_LEN {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Member name too long: {} > {}",
                    name.len(),
                    MAX_MEMBER_NAME_LEN
                )));
            }
        }
    }

    // Validate threshold >= 2/3 of members
    let min_threshold = calculate_min_threshold(member_count);
    if (payload.get_threshold() as usize) < min_threshold {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Governance threshold {} is below minimum {} (2/3 of {} members)",
            payload.get_threshold(),
            min_threshold,
            member_count
        )));
    }

    if (payload.get_threshold() as usize) > member_count {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Governance threshold {} exceeds member count {}",
            payload.get_threshold(),
            member_count
        )));
    }

    // Validate KYC threshold
    if payload.get_kyc_threshold() == 0 {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "KYC threshold cannot be 0"
        )));
    }

    if (payload.get_kyc_threshold() as usize) > member_count {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "KYC threshold {} exceeds member count {}",
            payload.get_kyc_threshold(),
            member_count
        )));
    }

    // Validate max_kyc_level
    if !is_valid_kyc_level(payload.get_max_kyc_level()) {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Invalid max KYC level: {}",
            payload.get_max_kyc_level()
        )));
    }

    // Check for duplicate member public keys
    let mut seen_keys = std::collections::HashSet::new();
    for member in payload.get_members() {
        if !seen_keys.insert(member.public_key.as_bytes()) {
            return Err(VerificationError::AnyError(anyhow::anyhow!(
                "Duplicate member public key in committee"
            )));
        }
    }

    Ok(())
}

/// Verify RegisterCommittee transaction payload
///
/// # Arguments
/// * `payload` - The RegisterCommittee payload to verify
/// * `current_time` - Current timestamp (block timestamp for deterministic validation)
pub fn verify_register_committee<E>(
    payload: &RegisterCommitteePayload,
    current_time: u64,
) -> Result<(), VerificationError<E>> {
    // Validate approvals from parent committee
    verify_approvals(payload.get_approvals(), current_time)?;

    // RegisterCommittee requires at least 1 approval from parent committee
    if payload.get_approvals().is_empty() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "RegisterCommittee requires at least 1 approval from parent committee"
        )));
    }

    // Validate committee name
    if payload.get_name().is_empty() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Committee name cannot be empty"
        )));
    }

    if payload.get_name().len() > MAX_COMMITTEE_NAME_LEN {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Committee name too long: {} > {}",
            payload.get_name().len(),
            MAX_COMMITTEE_NAME_LEN
        )));
    }

    // Validate member count
    let member_count = payload.get_members().len();
    if member_count < MIN_COMMITTEE_MEMBERS {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Committee requires at least {} members, got {}",
            MIN_COMMITTEE_MEMBERS,
            member_count
        )));
    }

    if member_count > MAX_COMMITTEE_MEMBERS {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Committee can have at most {} members, got {}",
            MAX_COMMITTEE_MEMBERS,
            member_count
        )));
    }

    // Validate member names
    for member in payload.get_members() {
        if let Some(ref name) = member.name {
            if name.len() > MAX_MEMBER_NAME_LEN {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Member name too long: {} > {}",
                    name.len(),
                    MAX_MEMBER_NAME_LEN
                )));
            }
        }
    }

    // Validate threshold >= 2/3 of members
    let min_threshold = calculate_min_threshold(member_count);
    if (payload.get_threshold() as usize) < min_threshold {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Governance threshold {} is below minimum {} (2/3 of {} members)",
            payload.get_threshold(),
            min_threshold,
            member_count
        )));
    }

    if (payload.get_threshold() as usize) > member_count {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Governance threshold {} exceeds member count {}",
            payload.get_threshold(),
            member_count
        )));
    }

    // Validate KYC threshold
    if payload.get_kyc_threshold() == 0 {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "KYC threshold cannot be 0"
        )));
    }

    if (payload.get_kyc_threshold() as usize) > member_count {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "KYC threshold {} exceeds member count {}",
            payload.get_kyc_threshold(),
            member_count
        )));
    }

    // Validate max_kyc_level
    if !is_valid_kyc_level(payload.get_max_kyc_level()) {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Invalid max KYC level: {}",
            payload.get_max_kyc_level()
        )));
    }

    // Check for duplicate member public keys
    let mut seen_keys = std::collections::HashSet::new();
    for member in payload.get_members() {
        if !seen_keys.insert(member.public_key.as_bytes()) {
            return Err(VerificationError::AnyError(anyhow::anyhow!(
                "Duplicate member public key in committee"
            )));
        }
    }

    Ok(())
}

/// Verify UpdateCommittee transaction payload
///
/// # Arguments
/// * `payload` - The UpdateCommittee payload to verify
/// * `current_time` - Current timestamp (block timestamp for deterministic validation)
pub fn verify_update_committee<E>(
    payload: &UpdateCommitteePayload,
    current_time: u64,
) -> Result<(), VerificationError<E>> {
    // Validate approvals
    verify_approvals(payload.get_approvals(), current_time)?;

    // Must have at least 1 approval
    if payload.get_approvals().is_empty() {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "UpdateCommittee requires at least 1 approval"
        )));
    }

    // Validate update-specific constraints (structural validation only)
    match payload.get_update() {
        crate::transaction::payload::CommitteeUpdateData::AddMember {
            name: Some(ref n), ..
        } => {
            if n.len() > MAX_MEMBER_NAME_LEN {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Member name too long: {} > {}",
                    n.len(),
                    MAX_MEMBER_NAME_LEN
                )));
            }
        }
        crate::transaction::payload::CommitteeUpdateData::AddMember { name: None, .. } => {
            // No name validation needed
        }
        crate::transaction::payload::CommitteeUpdateData::UpdateThreshold { new_threshold } => {
            if *new_threshold == 0 {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Governance threshold cannot be 0"
                )));
            }
        }
        crate::transaction::payload::CommitteeUpdateData::UpdateKycThreshold {
            new_kyc_threshold,
        } => {
            if *new_kyc_threshold == 0 {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "KYC threshold cannot be 0"
                )));
            }
        }
        crate::transaction::payload::CommitteeUpdateData::UpdateName { new_name } => {
            if new_name.is_empty() {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Committee name cannot be empty"
                )));
            }
            if new_name.len() > MAX_COMMITTEE_NAME_LEN {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Committee name too long: {} > {}",
                    new_name.len(),
                    MAX_COMMITTEE_NAME_LEN
                )));
            }
        }
        _ => {}
    }

    Ok(())
}

/// Committee governance info for state-dependent validation
pub struct CommitteeGovernanceInfo {
    /// Current number of active members in the committee
    pub member_count: usize,
    /// Current governance threshold
    pub threshold: u8,
}

/// Verify UpdateCommittee transaction payload with state-dependent validation
///
/// This function validates governance constraints that require committee state:
/// - UpdateThreshold: new_threshold <= member_count AND new_threshold >= ceil(2/3 * members)
/// - RemoveMember: remaining members >= threshold AND remaining members >= MIN_COMMITTEE_MEMBERS
/// - UpdateKycThreshold: new_kyc_threshold <= member_count
///
/// # Arguments
/// * `payload` - The UpdateCommittee payload to verify
/// * `committee_info` - Current committee governance info (member count and threshold)
/// * `current_time` - Current timestamp (block timestamp for deterministic validation)
pub fn verify_update_committee_with_state<E>(
    payload: &UpdateCommitteePayload,
    committee_info: &CommitteeGovernanceInfo,
    current_time: u64,
) -> Result<(), VerificationError<E>> {
    // First perform structural validation
    verify_update_committee(payload, current_time)?;

    // Now perform state-dependent governance validation
    match payload.get_update() {
        crate::transaction::payload::CommitteeUpdateData::UpdateThreshold { new_threshold } => {
            let new_threshold = *new_threshold as usize;
            let member_count = committee_info.member_count;

            // Threshold must be <= member count
            if new_threshold > member_count {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Governance threshold {} exceeds member count {}",
                    new_threshold,
                    member_count
                )));
            }

            // Threshold must meet 2/3 rule: threshold >= ceil(2/3 * members)
            let min_threshold = calculate_min_threshold(member_count);
            if new_threshold < min_threshold {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Governance threshold {} is below minimum {} (2/3 of {} members)",
                    new_threshold,
                    min_threshold,
                    member_count
                )));
            }
        }
        crate::transaction::payload::CommitteeUpdateData::RemoveMember { .. } => {
            let current_members = committee_info.member_count;
            let threshold = committee_info.threshold as usize;

            // After removal, remaining members count
            let remaining_members = current_members.saturating_sub(1);

            // Remaining members must be >= threshold (otherwise committee becomes inoperable)
            if remaining_members < threshold {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Cannot remove member: remaining {} members would be less than threshold {}",
                    remaining_members,
                    threshold
                )));
            }

            // Remaining members must be >= MIN_COMMITTEE_MEMBERS (minimum viable committee)
            if remaining_members < MIN_COMMITTEE_MEMBERS {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Cannot remove member: remaining {} members would be below minimum {} required",
                    remaining_members,
                    MIN_COMMITTEE_MEMBERS
                )));
            }
        }
        crate::transaction::payload::CommitteeUpdateData::UpdateKycThreshold {
            new_kyc_threshold,
        } => {
            let new_kyc_threshold = *new_kyc_threshold as usize;
            let member_count = committee_info.member_count;

            // KYC threshold must be <= member count
            if new_kyc_threshold > member_count {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "KYC threshold {} exceeds member count {}",
                    new_kyc_threshold,
                    member_count
                )));
            }
        }
        crate::transaction::payload::CommitteeUpdateData::AddMember { .. } => {
            let current_members = committee_info.member_count;

            // After adding, new member count must not exceed MAX_COMMITTEE_MEMBERS
            let new_member_count = current_members.saturating_add(1);
            if new_member_count > MAX_COMMITTEE_MEMBERS {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Cannot add member: {} members would exceed maximum {} allowed",
                    new_member_count,
                    MAX_COMMITTEE_MEMBERS
                )));
            }
        }
        // Other operations: No governance constraints to check
        _ => {}
    }

    Ok(())
}

/// Verify EmergencySuspend transaction payload
///
/// # Arguments
/// * `payload` - The EmergencySuspend payload to verify
/// * `current_time` - Current timestamp (block timestamp for deterministic validation)
pub fn verify_emergency_suspend<E>(
    payload: &EmergencySuspendPayload,
    current_time: u64,
) -> Result<(), VerificationError<E>> {
    // Validate approvals
    verify_approvals(payload.get_approvals(), current_time)?;

    // Emergency suspend requires at least 2 approvals
    if payload.get_approvals().len() < EMERGENCY_SUSPEND_MIN_APPROVALS {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "EmergencySuspend requires at least {} approvals, got {}",
            EMERGENCY_SUSPEND_MIN_APPROVALS,
            payload.get_approvals().len()
        )));
    }

    // Validate expires_at is reasonable (24 hours from now, with some tolerance)
    // expires_at should be approximately 24 hours from now (allow 1 hour tolerance)
    let min_expires = current_time + EMERGENCY_SUSPEND_TIMEOUT - 3600;
    let max_expires = current_time + EMERGENCY_SUSPEND_TIMEOUT + 3600;

    if payload.get_expires_at() < min_expires || payload.get_expires_at() > max_expires {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "EmergencySuspend expires_at must be approximately 24 hours from now"
        )));
    }

    Ok(())
}

/// Validate a list of committee approvals
///
/// # Arguments
/// * `approvals` - List of committee approvals to validate
/// * `current_time` - Current timestamp (block timestamp for deterministic validation)
fn verify_approvals<E>(
    approvals: &[CommitteeApproval],
    current_time: u64,
) -> Result<(), VerificationError<E>> {
    // Check max approvals
    if approvals.len() > MAX_APPROVALS {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Too many approvals: {} > {}",
            approvals.len(),
            MAX_APPROVALS
        )));
    }

    // Check for duplicate approvers
    let mut seen_keys = std::collections::HashSet::new();
    for approval in approvals {
        if !seen_keys.insert(approval.member_pubkey.as_bytes()) {
            return Err(VerificationError::AnyError(anyhow::anyhow!(
                "Duplicate approver public key"
            )));
        }
    }

    // Validate approval timestamps are reasonable
    // 1. Not too far in the future (1 hour tolerance)
    // 2. Not too old (expires after APPROVAL_EXPIRY_SECONDS)
    let max_future = current_time + 3600;
    let min_valid = current_time.saturating_sub(APPROVAL_EXPIRY_SECONDS);

    for approval in approvals {
        // Reject approvals from far future
        if approval.timestamp > max_future {
            return Err(VerificationError::AnyError(anyhow::anyhow!(
                "Approval timestamp too far in the future"
            )));
        }

        // Reject expired approvals (older than APPROVAL_EXPIRY_SECONDS)
        if approval.timestamp < min_valid {
            return Err(VerificationError::AnyError(anyhow::anyhow!(
                "Approval has expired (older than {} seconds)",
                APPROVAL_EXPIRY_SECONDS
            )));
        }
    }

    Ok(())
}

/// Calculate minimum threshold (2/3 of members, rounded up)
fn calculate_min_threshold(member_count: usize) -> usize {
    (member_count * 2).div_ceil(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{elgamal::CompressedPublicKey, Hash, Signature};
    use crate::kyc::MemberRole;
    use crate::serializer::Serializer;
    use crate::transaction::payload::CommitteeMemberInit;

    fn create_test_pubkey(seed: u8) -> CompressedPublicKey {
        let bytes = [seed; 32];
        CompressedPublicKey::from_bytes(&bytes).unwrap_or_else(|_| {
            // If invalid, create a valid one from curve25519
            CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap()
        })
    }

    fn create_test_approval(seed: u8) -> CommitteeApproval {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Create a valid signature from bytes
        let sig_bytes = [seed; 64];
        let signature = Signature::from_bytes(&sig_bytes).unwrap_or_else(|_| {
            // Fallback to default bytes if invalid
            Signature::from_bytes(&[0u8; 64]).unwrap()
        });

        CommitteeApproval::new(create_test_pubkey(seed), signature, now)
    }

    #[test]
    fn test_verify_set_kyc_valid() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let payload = SetKycPayload::new(
            create_test_pubkey(1),
            31, // Valid level (Tier 2)
            now,
            Hash::zero(),
            Hash::zero(),
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> = verify_set_kyc(&payload, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_set_kyc_invalid_level() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let payload = SetKycPayload::new(
            create_test_pubkey(1),
            100, // Invalid level
            now,
            Hash::zero(),
            Hash::zero(),
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> = verify_set_kyc(&payload, now);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_set_kyc_tier5_needs_2_approvals() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Tier 5 with only 1 approval should fail
        let payload = SetKycPayload::new(
            create_test_pubkey(1),
            2047, // Tier 5
            now,
            Hash::zero(),
            Hash::zero(),
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> = verify_set_kyc(&payload, now);
        assert!(result.is_err());

        // Tier 5 with 2 approvals should succeed
        let payload = SetKycPayload::new(
            create_test_pubkey(1),
            2047, // Tier 5
            now,
            Hash::zero(),
            Hash::zero(),
            vec![create_test_approval(1), create_test_approval(2)],
        );

        let result: Result<(), VerificationError<()>> = verify_set_kyc(&payload, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_bootstrap_committee_valid() {
        let members: Vec<CommitteeMemberInit> = (1..=5)
            .map(|i| CommitteeMemberInit::new(create_test_pubkey(i), None, MemberRole::Member))
            .collect();

        let payload = BootstrapCommitteePayload::new(
            "Global Committee".to_string(),
            members,
            4, // 4/5 = 80% >= 67%
            1,
            32767, // Max level
        );

        let result: Result<(), VerificationError<()>> = verify_bootstrap_committee(&payload);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_bootstrap_committee_too_few_members() {
        let members: Vec<CommitteeMemberInit> = (1..=2)
            .map(|i| CommitteeMemberInit::new(create_test_pubkey(i), None, MemberRole::Member))
            .collect();

        let payload =
            BootstrapCommitteePayload::new("Test Committee".to_string(), members, 2, 1, 32767);

        let result: Result<(), VerificationError<()>> = verify_bootstrap_committee(&payload);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_bootstrap_committee_threshold_too_low() {
        let members: Vec<CommitteeMemberInit> = (1..=6)
            .map(|i| CommitteeMemberInit::new(create_test_pubkey(i), None, MemberRole::Member))
            .collect();

        let payload = BootstrapCommitteePayload::new(
            "Test Committee".to_string(),
            members,
            2, // 2/6 = 33% < 67%
            1,
            32767,
        );

        let result: Result<(), VerificationError<()>> = verify_bootstrap_committee(&payload);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_emergency_suspend_valid() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let payload = EmergencySuspendPayload::new(
            create_test_pubkey(1),
            Hash::zero(),
            Hash::zero(),
            vec![create_test_approval(1), create_test_approval(2)],
            now + EMERGENCY_SUSPEND_TIMEOUT,
        );

        let result: Result<(), VerificationError<()>> = verify_emergency_suspend(&payload, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_emergency_suspend_needs_2_approvals() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let payload = EmergencySuspendPayload::new(
            create_test_pubkey(1),
            Hash::zero(),
            Hash::zero(),
            vec![create_test_approval(1)], // Only 1 approval
            now + EMERGENCY_SUSPEND_TIMEOUT,
        );

        let result: Result<(), VerificationError<()>> = verify_emergency_suspend(&payload, now);
        assert!(result.is_err());
    }

    #[test]
    fn test_calculate_min_threshold() {
        // 3 members -> 2 (ceil(3*2/3) = ceil(2) = 2)
        assert_eq!(calculate_min_threshold(3), 2);
        // 5 members -> 4 (ceil(5*2/3) = ceil(3.33) = 4)
        assert_eq!(calculate_min_threshold(5), 4);
        // 7 members -> 5 (ceil(7*2/3) = ceil(4.67) = 5)
        assert_eq!(calculate_min_threshold(7), 5);
        // 11 members -> 8 (ceil(11*2/3) = ceil(7.33) = 8)
        assert_eq!(calculate_min_threshold(11), 8);
        // 15 members -> 10 (ceil(15*2/3) = ceil(10) = 10)
        assert_eq!(calculate_min_threshold(15), 10);
    }

    #[test]
    fn test_verify_approvals_no_duplicates() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let approvals = vec![
            create_test_approval(1),
            create_test_approval(1), // Duplicate
        ];

        let result: Result<(), VerificationError<()>> = verify_approvals(&approvals, now);
        assert!(result.is_err());
    }

    // Tests for verify_update_committee_with_state

    use crate::transaction::payload::CommitteeUpdateData;

    fn create_update_committee_payload(
        update: CommitteeUpdateData,
        approvals: Vec<CommitteeApproval>,
    ) -> UpdateCommitteePayload {
        UpdateCommitteePayload::new(Hash::zero(), update, approvals)
    }

    #[test]
    fn test_update_threshold_valid() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 5 members, min threshold = 4 (ceil(5*2/3))
        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            threshold: 4,
        };

        // Valid: threshold 4 <= 5 members AND 4 >= 4 (2/3 rule)
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateThreshold { new_threshold: 4 },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_ok());

        // Valid: threshold 5 is also acceptable
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateThreshold { new_threshold: 5 },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_threshold_exceeds_member_count() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            threshold: 4,
        };

        // Invalid: threshold 6 > 5 members
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateThreshold { new_threshold: 6 },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("exceeds member count"));
    }

    #[test]
    fn test_update_threshold_below_two_thirds() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let committee_info = CommitteeGovernanceInfo {
            member_count: 6,
            threshold: 4,
        };

        // Invalid: threshold 3 < 4 (ceil(6*2/3) = 4)
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateThreshold { new_threshold: 3 },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("below minimum"));
    }

    #[test]
    fn test_remove_member_valid() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 5 members with threshold 4, removing one leaves 4 members >= threshold 4
        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            threshold: 4,
        };

        let payload = create_update_committee_payload(
            CommitteeUpdateData::RemoveMember {
                public_key: create_test_pubkey(99),
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_member_would_break_threshold() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 4 members with threshold 4, removing one leaves 3 members < threshold 4
        let committee_info = CommitteeGovernanceInfo {
            member_count: 4,
            threshold: 4,
        };

        let payload = create_update_committee_payload(
            CommitteeUpdateData::RemoveMember {
                public_key: create_test_pubkey(99),
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("less than threshold"));
    }

    #[test]
    fn test_remove_member_would_break_minimum() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 3 members (minimum), threshold 2, removing one leaves 2 members < MIN_COMMITTEE_MEMBERS
        let committee_info = CommitteeGovernanceInfo {
            member_count: 3,
            threshold: 2,
        };

        let payload = create_update_committee_payload(
            CommitteeUpdateData::RemoveMember {
                public_key: create_test_pubkey(99),
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("below minimum"));
    }

    #[test]
    fn test_update_kyc_threshold_valid() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            threshold: 4,
        };

        // Valid: KYC threshold 3 <= 5 members
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateKycThreshold {
                new_kyc_threshold: 3,
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_kyc_threshold_exceeds_member_count() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            threshold: 4,
        };

        // Invalid: KYC threshold 6 > 5 members
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateKycThreshold {
                new_kyc_threshold: 6,
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("exceeds member count"));
    }

    #[test]
    fn test_add_member_always_valid() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            threshold: 4,
        };

        // AddMember should always pass state validation (adding is safe)
        let payload = create_update_committee_payload(
            CommitteeUpdateData::AddMember {
                public_key: create_test_pubkey(99),
                name: Some("New Member".to_string()),
                role: MemberRole::Member,
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_ok());
    }

    // Tests for verify_appeal_kyc

    use super::AppealKycPayload;

    fn create_appeal_payload(
        reason_hash: crate::crypto::Hash,
        documents_hash: crate::crypto::Hash,
        submitted_at: u64,
    ) -> AppealKycPayload {
        AppealKycPayload::new(
            create_test_pubkey(1), // account
            Hash::new([1u8; 32]),  // original_committee_id
            Hash::new([2u8; 32]),  // parent_committee_id
            reason_hash,
            documents_hash,
            submitted_at,
        )
    }

    #[test]
    fn test_verify_appeal_kyc_valid() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let payload = create_appeal_payload(
            Hash::new([3u8; 32]), // Valid reason_hash (non-zero)
            Hash::new([4u8; 32]), // Valid documents_hash (non-zero)
            now,
        );

        let result: Result<(), VerificationError<()>> = verify_appeal_kyc(&payload, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_appeal_kyc_same_committee_fails() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Create payload where original and parent committee are the same
        let committee_id = Hash::new([1u8; 32]);
        let payload = AppealKycPayload::new(
            create_test_pubkey(1),
            committee_id.clone(),
            committee_id, // Same as original
            Hash::new([3u8; 32]),
            Hash::new([4u8; 32]),
            now,
        );

        let result: Result<(), VerificationError<()>> = verify_appeal_kyc(&payload, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("Original committee and parent committee must be different"));
    }

    #[test]
    fn test_verify_appeal_kyc_future_timestamp_fails() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // submitted_at more than 1 hour in the future
        let payload = create_appeal_payload(
            Hash::new([3u8; 32]),
            Hash::new([4u8; 32]),
            now + 7200, // 2 hours in future
        );

        let result: Result<(), VerificationError<()>> = verify_appeal_kyc(&payload, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("too far in the future"));
    }

    #[test]
    fn test_verify_appeal_kyc_empty_reason_hash_fails() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let payload = create_appeal_payload(
            Hash::zero(), // Empty reason_hash
            Hash::new([4u8; 32]),
            now,
        );

        let result: Result<(), VerificationError<()>> = verify_appeal_kyc(&payload, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("Appeal reason hash cannot be empty"));
    }

    #[test]
    fn test_verify_appeal_kyc_empty_documents_hash_fails() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let payload = create_appeal_payload(
            Hash::new([3u8; 32]),
            Hash::zero(), // Empty documents_hash
            now,
        );

        let result: Result<(), VerificationError<()>> = verify_appeal_kyc(&payload, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("Appeal documents hash cannot be empty"));
    }

    #[test]
    fn test_verify_appeal_kyc_both_hashes_empty_fails_on_reason_first() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let payload = create_appeal_payload(
            Hash::zero(), // Empty reason_hash
            Hash::zero(), // Empty documents_hash
            now,
        );

        // Should fail on reason_hash first (since it's checked first)
        let result: Result<(), VerificationError<()>> = verify_appeal_kyc(&payload, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("Appeal reason hash cannot be empty"));
    }
}
