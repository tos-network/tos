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

    // SECURITY FIX (Issue #38): Enforce combined approval count limit
    // TransferKyc has two approval lists, but the total should not exceed
    // a reasonable limit to prevent DoS via oversized transactions.
    // We use 2 * MAX_APPROVALS as the combined limit for dual-committee transfers.
    let combined_approval_count =
        payload.get_source_approvals().len() + payload.get_dest_approvals().len();
    let max_combined_approvals = MAX_APPROVALS * 2;
    if combined_approval_count > max_combined_approvals {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "TransferKyc combined approval count {} exceeds maximum {} ({}+{} per committee)",
            combined_approval_count,
            max_combined_approvals,
            MAX_APPROVALS,
            MAX_APPROVALS
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

    // SECURITY FIX (Issue #43): Appeal submitted_at must be within a reasonable window
    // to prevent backdating attacks that could bypass time-based appeal policies.
    // We enforce both upper and lower bounds on the submission timestamp.
    let max_future = current_time + 3600; // 1 hour future tolerance
    let max_past = current_time.saturating_sub(3600); // 1 hour past tolerance

    if payload.get_submitted_at() > max_future {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Appeal submission timestamp too far in the future"
        )));
    }

    if payload.get_submitted_at() < max_past {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Appeal submission timestamp too far in the past (possible backdating)"
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
///
/// SECURITY (Issue #33): Requires sender to be BOOTSTRAP_ADDRESS to prevent
/// unauthorized accounts from seizing control of the global committee.
pub fn verify_bootstrap_committee<E>(
    payload: &BootstrapCommitteePayload,
    sender: &crate::crypto::elgamal::CompressedPublicKey,
) -> Result<(), VerificationError<E>> {
    // SECURITY FIX (Issue #33): Verify sender is BOOTSTRAP_ADDRESS
    // Only the designated bootstrap address can create the global committee
    let bootstrap_pubkey = {
        use crate::crypto::Address;
        // Parse bootstrap address - this is a compile-time constant
        // Note: PublicKey is an alias for CompressedPublicKey, no compression needed
        let addr = Address::from_string(crate::config::BOOTSTRAP_ADDRESS).map_err(|e| {
            VerificationError::AnyError(anyhow::anyhow!(
                "Invalid bootstrap address configuration: {}",
                e
            ))
        })?;
        addr.to_public_key()
    };

    if sender != &bootstrap_pubkey {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "BootstrapCommittee can only be submitted by BOOTSTRAP_ADDRESS"
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

    // SECURITY FIX (Issue #35): Count only members who can approve (non-observers)
    // Observers cannot approve, so thresholds must be achievable with actual approvers
    let approver_count = payload
        .get_members()
        .iter()
        .filter(|m| m.role.can_approve())
        .count();

    // Validate threshold >= 2/3 of approvers (not all members)
    let min_threshold = calculate_min_threshold(approver_count);
    if (payload.get_threshold() as usize) < min_threshold {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Governance threshold {} is below minimum {} (2/3 of {} approvers)",
            payload.get_threshold(),
            min_threshold,
            approver_count
        )));
    }

    // SECURITY FIX (Issue #35): Threshold must be <= approver count
    if (payload.get_threshold() as usize) > approver_count {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Governance threshold {} exceeds approver count {} (observers cannot approve)",
            payload.get_threshold(),
            approver_count
        )));
    }

    // SECURITY: Ensure threshold doesn't exceed MAX_APPROVALS
    // Otherwise governance operations become impossible (can't submit enough approvals)
    if (payload.get_threshold() as usize) > MAX_APPROVALS {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Governance threshold {} exceeds maximum approvals allowed per transaction ({})",
            payload.get_threshold(),
            MAX_APPROVALS
        )));
    }

    // Validate KYC threshold
    if payload.get_kyc_threshold() == 0 {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "KYC threshold cannot be 0"
        )));
    }

    // SECURITY FIX (Issue #35): KYC threshold must be <= approver count
    if (payload.get_kyc_threshold() as usize) > approver_count {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "KYC threshold {} exceeds approver count {} (observers cannot approve)",
            payload.get_kyc_threshold(),
            approver_count
        )));
    }

    // SECURITY FIX (Issue #19): Ensure kyc_threshold doesn't exceed MAX_APPROVALS
    // Otherwise KYC operations become impossible (can't submit enough approvals)
    if (payload.get_kyc_threshold() as usize) > MAX_APPROVALS {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "KYC threshold {} exceeds maximum approvals allowed per transaction ({})",
            payload.get_kyc_threshold(),
            MAX_APPROVALS
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

    // SECURITY FIX (Issue #35): Count only members who can approve (non-observers)
    // Observers cannot approve, so thresholds must be achievable with actual approvers
    let approver_count = payload
        .get_members()
        .iter()
        .filter(|m| m.role.can_approve())
        .count();

    // Validate threshold >= 2/3 of approvers (not all members)
    let min_threshold = calculate_min_threshold(approver_count);
    if (payload.get_threshold() as usize) < min_threshold {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Governance threshold {} is below minimum {} (2/3 of {} approvers)",
            payload.get_threshold(),
            min_threshold,
            approver_count
        )));
    }

    // SECURITY FIX (Issue #35): Threshold must be <= approver count
    if (payload.get_threshold() as usize) > approver_count {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Governance threshold {} exceeds approver count {} (observers cannot approve)",
            payload.get_threshold(),
            approver_count
        )));
    }

    // SECURITY: Ensure threshold doesn't exceed MAX_APPROVALS
    // Otherwise governance operations become impossible (can't submit enough approvals)
    if (payload.get_threshold() as usize) > MAX_APPROVALS {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "Governance threshold {} exceeds maximum approvals allowed per transaction ({})",
            payload.get_threshold(),
            MAX_APPROVALS
        )));
    }

    // Validate KYC threshold
    if payload.get_kyc_threshold() == 0 {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "KYC threshold cannot be 0"
        )));
    }

    // SECURITY FIX (Issue #35): KYC threshold must be <= approver count
    if (payload.get_kyc_threshold() as usize) > approver_count {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "KYC threshold {} exceeds approver count {} (observers cannot approve)",
            payload.get_kyc_threshold(),
            approver_count
        )));
    }

    // SECURITY FIX (Issue #19): Ensure kyc_threshold doesn't exceed MAX_APPROVALS
    // Otherwise KYC operations become impossible (can't submit enough approvals)
    if (payload.get_kyc_threshold() as usize) > MAX_APPROVALS {
        return Err(VerificationError::AnyError(anyhow::anyhow!(
            "KYC threshold {} exceeds maximum approvals allowed per transaction ({})",
            payload.get_kyc_threshold(),
            MAX_APPROVALS
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
    /// Current number of active members who can approve (excludes observers)
    /// SECURITY FIX (Issue #36): Used for threshold validation to prevent
    /// setting thresholds higher than the number of members who can actually approve.
    pub approver_count: usize,
    /// Total member count including inactive/suspended/removed members
    /// SECURITY: Used for MAX_COMMITTEE_MEMBERS enforcement to prevent
    /// committees from bypassing limits by suspending members.
    pub total_member_count: usize,
    /// Current governance threshold
    pub threshold: u8,
    /// Current KYC threshold
    /// SECURITY FIX (Issue #37): Used to validate role changes don't brick KYC operations
    pub kyc_threshold: u8,
    /// Target member is active (for member updates/removals)
    pub target_is_active: Option<bool>,
    /// Target member can approve (role-based, for member updates/removals)
    pub target_can_approve: Option<bool>,
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
            // SECURITY FIX (Issue #36): Use approver_count instead of member_count
            // Observers cannot approve, so threshold must be achievable with actual approvers
            let approver_count = committee_info.approver_count;

            // Threshold must be <= approver count (not member count, which includes observers)
            if new_threshold > approver_count {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Governance threshold {} exceeds approver count {} (observers cannot approve)",
                    new_threshold,
                    approver_count
                )));
            }

            // Threshold must meet 2/3 rule: threshold >= ceil(2/3 * approvers)
            let min_threshold = calculate_min_threshold(approver_count);
            if new_threshold < min_threshold {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Governance threshold {} is below minimum {} (2/3 of {} approvers)",
                    new_threshold,
                    min_threshold,
                    approver_count
                )));
            }

            // SECURITY: Ensure threshold doesn't exceed MAX_APPROVALS
            // Otherwise governance operations become impossible (can't submit enough approvals)
            if new_threshold > MAX_APPROVALS {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Governance threshold {} exceeds maximum approvals allowed per transaction ({})",
                    new_threshold,
                    MAX_APPROVALS
                )));
            }
        }
        crate::transaction::payload::CommitteeUpdateData::RemoveMember { .. } => {
            let current_members = committee_info.member_count;
            let threshold = committee_info.threshold as usize;
            let kyc_threshold = committee_info.kyc_threshold as usize;
            let target_is_active = committee_info.target_is_active.unwrap_or(true);
            let target_can_approve = committee_info.target_can_approve.unwrap_or(true);

            // After removal, remaining members count
            let remaining_members =
                current_members.saturating_sub(if target_is_active { 1 } else { 0 });
            let remaining_approvers = committee_info.approver_count.saturating_sub(
                if target_is_active && target_can_approve {
                    1
                } else {
                    0
                },
            );

            // Remaining members must be >= threshold (otherwise committee becomes inoperable)
            if remaining_approvers < threshold {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Cannot remove member: remaining {} approvers would be less than threshold {}",
                    remaining_approvers,
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

            // Remaining approvers must be >= KYC threshold
            if remaining_approvers < kyc_threshold {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Cannot remove member: remaining {} approvers would be less than KYC threshold {}",
                    remaining_approvers,
                    kyc_threshold
                )));
            }
        }
        crate::transaction::payload::CommitteeUpdateData::UpdateKycThreshold {
            new_kyc_threshold,
        } => {
            let new_kyc_threshold = *new_kyc_threshold as usize;
            // SECURITY FIX (Issue #36): Use approver_count instead of member_count
            // Observers cannot approve, so KYC threshold must be achievable with actual approvers
            let approver_count = committee_info.approver_count;

            // KYC threshold must be <= approver count (not member count, which includes observers)
            if new_kyc_threshold > approver_count {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "KYC threshold {} exceeds approver count {} (observers cannot approve)",
                    new_kyc_threshold,
                    approver_count
                )));
            }

            // SECURITY FIX (Issue #19): Ensure kyc_threshold doesn't exceed MAX_APPROVALS
            // Otherwise KYC operations become impossible (can't submit enough approvals)
            if new_kyc_threshold > MAX_APPROVALS {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "KYC threshold {} exceeds maximum approvals allowed per transaction ({})",
                    new_kyc_threshold,
                    MAX_APPROVALS
                )));
            }
        }
        crate::transaction::payload::CommitteeUpdateData::AddMember { role, .. } => {
            // SECURITY FIX (Issue #23): Use total member count (including inactive/suspended)
            // for MAX_COMMITTEE_MEMBERS check. This prevents committees from bypassing the
            // hard cap by suspending members and then adding new ones.
            let total_members = committee_info.total_member_count;
            let new_total_count = total_members.saturating_add(1);
            if new_total_count > MAX_COMMITTEE_MEMBERS {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Cannot add member: {} total members would exceed maximum {} allowed",
                    new_total_count,
                    MAX_COMMITTEE_MEMBERS
                )));
            }

            // SECURITY FIX (Issue #18): After adding a member, the current threshold may
            // no longer meet the 2/3 governance invariant. Re-validate that:
            // threshold >= ceil(2/3 * new_approver_count)
            // Use approver count since observers cannot approve governance actions
            let current_approvers = committee_info.approver_count;
            let new_approver_count =
                current_approvers.saturating_add(if role.can_approve() { 1 } else { 0 });
            let threshold = committee_info.threshold as usize;
            let min_threshold = calculate_min_threshold(new_approver_count);
            if threshold < min_threshold {
                return Err(VerificationError::AnyError(anyhow::anyhow!(
                    "Cannot add member: current threshold {} would be below required minimum {} (2/3 of {} approvers). Increase threshold first.",
                    threshold,
                    min_threshold,
                    new_approver_count
                )));
            }
        }
        crate::transaction::payload::CommitteeUpdateData::UpdateMemberStatus {
            new_status, ..
        } => {
            // SECURITY FIX (Issue #17): UpdateMemberStatus can deactivate members without
            // checking governance invariants. When a member is set to Suspended or Removed,
            // we must ensure that remaining active members >= threshold and >= MIN_COMMITTEE_MEMBERS.
            use crate::kyc::MemberStatus;

            let current_active = committee_info.member_count;
            let current_approvers = committee_info.approver_count;
            let threshold = committee_info.threshold as usize;
            let kyc_threshold = committee_info.kyc_threshold as usize;
            let target_is_active = committee_info.target_is_active.unwrap_or(true);
            let target_can_approve = committee_info.target_can_approve.unwrap_or(true);

            // Validate if the new status would make the member inactive
            if *new_status == MemberStatus::Suspended || *new_status == MemberStatus::Removed {
                // After status change, one fewer active member
                let remaining_active =
                    current_active.saturating_sub(if target_is_active { 1 } else { 0 });
                let remaining_approvers =
                    current_approvers.saturating_sub(if target_is_active && target_can_approve {
                        1
                    } else {
                        0
                    });

                // Remaining active members must be >= threshold
                if remaining_approvers < threshold {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "Cannot deactivate member: remaining {} approvers would be less than threshold {}",
                        remaining_approvers,
                        threshold
                    )));
                }

                // Remaining active members must be >= MIN_COMMITTEE_MEMBERS
                if remaining_active < MIN_COMMITTEE_MEMBERS {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "Cannot deactivate member: remaining {} active members would be below minimum {} required",
                        remaining_active,
                        MIN_COMMITTEE_MEMBERS
                    )));
                }

                // Remaining approvers must be >= KYC threshold
                if remaining_approvers < kyc_threshold {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "Cannot deactivate member: remaining {} approvers would be less than KYC threshold {}",
                        remaining_approvers,
                        kyc_threshold
                    )));
                }
            } else if *new_status == MemberStatus::Active && !target_is_active && target_can_approve
            {
                // SECURITY FIX (Issue #42): When reactivating a suspended/removed member who is an approver,
                // approver_count increases. Must revalidate 2/3 governance invariant.
                let new_approver_count = current_approvers + 1;
                let required_threshold = (new_approver_count * 2).div_ceil(3);

                if threshold < required_threshold {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "Cannot reactivate approver member: governance threshold {} is below required 2/3 ({}) for {} approvers. Increase threshold first.",
                        threshold,
                        required_threshold,
                        new_approver_count
                    )));
                }
            }
        }
        crate::transaction::payload::CommitteeUpdateData::UpdateMemberRole { new_role, .. } => {
            // SECURITY FIX (Issue #37): UpdateMemberRole can downgrade approvers to Observer
            // without checking if remaining approvers >= threshold/kyc_threshold.
            // When a member is changed to Observer, they can no longer approve operations.
            use crate::kyc::MemberRole;

            let current_approvers = committee_info.approver_count;
            let threshold = committee_info.threshold as usize;
            let kyc_threshold = committee_info.kyc_threshold as usize;
            let target_is_active = committee_info.target_is_active.unwrap_or(true);
            let target_can_approve = committee_info.target_can_approve.unwrap_or(true);

            // Check if role change reduces approval capability (downgrade to Observer)
            if *new_role == MemberRole::Observer {
                // After role change to Observer, reduce approver count only if target is an active approver
                let remaining_approvers =
                    current_approvers.saturating_sub(if target_is_active && target_can_approve {
                        1
                    } else {
                        0
                    });

                // Remaining approvers must be >= governance threshold
                if remaining_approvers < threshold {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "Cannot change member to Observer: remaining {} approvers would be less than governance threshold {}",
                        remaining_approvers,
                        threshold
                    )));
                }

                // Remaining approvers must be >= KYC threshold
                if remaining_approvers < kyc_threshold {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "Cannot change member to Observer: remaining {} approvers would be less than KYC threshold {}",
                        remaining_approvers,
                        kyc_threshold
                    )));
                }
            } else if new_role.can_approve() && !target_can_approve && target_is_active {
                // SECURITY FIX (Issue #42): When promoting from Observer to Member/Chair,
                // approver_count increases. Must revalidate 2/3 governance invariant.
                let new_approver_count = current_approvers + 1;
                let required_threshold = (new_approver_count * 2).div_ceil(3);

                if threshold < required_threshold {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "Cannot promote member to approver role: governance threshold {} is below required 2/3 ({}) for {} approvers. Increase threshold first.",
                        threshold,
                        required_threshold,
                        new_approver_count
                    )));
                }
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
            CompressedPublicKey::from_bytes(&[1u8; 32]).expect("test assertion")
        })
    }

    fn create_bootstrap_sender() -> CompressedPublicKey {
        use crate::crypto::Address;
        let addr = Address::from_string(crate::config::BOOTSTRAP_ADDRESS)
            .expect("Bootstrap address should be valid");
        // PublicKey is an alias for CompressedPublicKey
        addr.to_public_key()
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
            Signature::from_bytes(&[0u8; 64]).expect("test assertion")
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

        let bootstrap_sender = create_bootstrap_sender();
        let result: Result<(), VerificationError<()>> =
            verify_bootstrap_committee(&payload, &bootstrap_sender);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_bootstrap_committee_wrong_sender() {
        let members: Vec<CommitteeMemberInit> = (1..=5)
            .map(|i| CommitteeMemberInit::new(create_test_pubkey(i), None, MemberRole::Member))
            .collect();

        let payload =
            BootstrapCommitteePayload::new("Global Committee".to_string(), members, 4, 1, 32767);

        // Use a random sender instead of bootstrap address
        let wrong_sender = create_test_pubkey(99);
        let result: Result<(), VerificationError<()>> =
            verify_bootstrap_committee(&payload, &wrong_sender);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_bootstrap_committee_too_few_members() {
        let members: Vec<CommitteeMemberInit> = (1..=2)
            .map(|i| CommitteeMemberInit::new(create_test_pubkey(i), None, MemberRole::Member))
            .collect();

        let payload =
            BootstrapCommitteePayload::new("Test Committee".to_string(), members, 2, 1, 32767);

        let bootstrap_sender = create_bootstrap_sender();
        let result: Result<(), VerificationError<()>> =
            verify_bootstrap_committee(&payload, &bootstrap_sender);
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

        let bootstrap_sender = create_bootstrap_sender();
        let result: Result<(), VerificationError<()>> =
            verify_bootstrap_committee(&payload, &bootstrap_sender);
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

        // 5 approvers, min threshold = 4 (ceil(5*2/3))
        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            approver_count: 5,
            total_member_count: 5,
            threshold: 4,
            kyc_threshold: 1,
            target_is_active: None,
            target_can_approve: None,
        };

        // Valid: threshold 4 <= 5 approvers AND 4 >= 4 (2/3 rule)
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
    fn test_update_threshold_exceeds_approver_count() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            approver_count: 5,
            total_member_count: 5,
            threshold: 4,
            kyc_threshold: 1,
            target_is_active: None,
            target_can_approve: None,
        };

        // Invalid: threshold 6 > 5 approvers
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateThreshold { new_threshold: 6 },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("exceeds approver count"));
    }

    #[test]
    fn test_update_threshold_below_two_thirds() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let committee_info = CommitteeGovernanceInfo {
            member_count: 6,
            approver_count: 6,
            total_member_count: 6,
            threshold: 4,
            kyc_threshold: 1,
            target_is_active: None,
            target_can_approve: None,
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
            approver_count: 5,
            total_member_count: 5,
            threshold: 4,
            kyc_threshold: 1,
            target_is_active: Some(true),
            target_can_approve: Some(true),
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
            approver_count: 4,
            total_member_count: 4,
            threshold: 4,
            kyc_threshold: 1,
            target_is_active: Some(true),
            target_can_approve: Some(true),
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
            approver_count: 3,
            total_member_count: 3,
            threshold: 2,
            kyc_threshold: 1,
            target_is_active: Some(true),
            target_can_approve: Some(true),
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
            approver_count: 5,
            total_member_count: 5,
            threshold: 4,
            kyc_threshold: 1,
            target_is_active: None,
            target_can_approve: None,
        };

        // Valid: KYC threshold 3 <= 5 approvers
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
    fn test_update_kyc_threshold_exceeds_approver_count() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            approver_count: 5,
            total_member_count: 5,
            threshold: 4,
            kyc_threshold: 1,
            target_is_active: None,
            target_can_approve: None,
        };

        // Invalid: KYC threshold 6 > 5 approvers
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
        assert!(err_msg.contains("exceeds approver count"));
    }

    #[test]
    fn test_add_member_always_valid() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            approver_count: 5,
            total_member_count: 5,
            threshold: 4,
            kyc_threshold: 1,
            target_is_active: None,
            target_can_approve: None,
        };

        // AddMember should pass state validation (adding is safe within limits)
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

    #[test]
    fn test_remove_member_breaks_kyc_threshold_when_approver_removed() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 2 approvers, kyc_threshold=2, removing an approver would leave 1 < 2
        let committee_info = CommitteeGovernanceInfo {
            member_count: 4,
            approver_count: 2,
            total_member_count: 4,
            threshold: 1,
            kyc_threshold: 2,
            target_is_active: Some(true),
            target_can_approve: Some(true),
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
        assert!(err_msg.contains("KYC threshold"));
    }

    #[test]
    fn test_update_member_status_observer_does_not_reduce_approvers() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Suspending an observer should not reduce approver count
        let committee_info = CommitteeGovernanceInfo {
            member_count: 4,
            approver_count: 2,
            total_member_count: 4,
            threshold: 2,
            kyc_threshold: 1,
            target_is_active: Some(true),
            target_can_approve: Some(false),
        };

        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateMemberStatus {
                public_key: create_test_pubkey(99),
                new_status: crate::kyc::MemberStatus::Suspended,
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_add_member_observer_does_not_force_threshold_increase() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Adding an Observer should not change approver count or require threshold changes
        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            approver_count: 3,
            total_member_count: 5,
            threshold: 2,
            kyc_threshold: 1,
            target_is_active: None,
            target_can_approve: None,
        };

        let payload = create_update_committee_payload(
            CommitteeUpdateData::AddMember {
                public_key: create_test_pubkey(99),
                name: Some("Observer".to_string()),
                role: MemberRole::Observer,
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

    // ============================================================================
    // ROUND 15: Role Heterogeneity & Observer/Approver Distinction Tests
    // ============================================================================
    // These tests address gaps identified in security review Round 14-15:
    // - Threshold validation must count approvers only, not observers
    // - UpdateCommittee thresholds validated against approvers, not all members
    // - UpdateMemberRole must check approver-count safety

    #[test]
    fn test_bootstrap_committee_with_observers_threshold_uses_approver_count() {
        // Bootstrap with observers should validate threshold against approver_count
        // 5 total members: 3 approvers (Chair, Member, Member) + 2 observers
        // threshold=4 should FAIL because only 3 can approve
        let members: Vec<CommitteeMemberInit> = vec![
            CommitteeMemberInit::new(create_test_pubkey(1), None, MemberRole::Chair),
            CommitteeMemberInit::new(create_test_pubkey(2), None, MemberRole::Member),
            CommitteeMemberInit::new(create_test_pubkey(3), None, MemberRole::Member),
            CommitteeMemberInit::new(create_test_pubkey(4), None, MemberRole::Observer),
            CommitteeMemberInit::new(create_test_pubkey(5), None, MemberRole::Observer),
        ];

        // threshold=4 exceeds approver_count=3
        let payload = BootstrapCommitteePayload::new(
            "Test Committee".to_string(),
            members,
            4, // threshold > approver_count (3)
            1,
            32767,
        );

        let bootstrap_sender = create_bootstrap_sender();
        let result: Result<(), VerificationError<()>> =
            verify_bootstrap_committee(&payload, &bootstrap_sender);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("exceeds") || err_msg.contains("approver"),
            "Expected error about threshold exceeding approver count, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_bootstrap_committee_with_observers_valid_threshold() {
        // 5 total members: 3 approvers + 2 observers
        // threshold=2 (2/3 of 3 approvers) should PASS
        let members: Vec<CommitteeMemberInit> = vec![
            CommitteeMemberInit::new(create_test_pubkey(1), None, MemberRole::Chair),
            CommitteeMemberInit::new(create_test_pubkey(2), None, MemberRole::Member),
            CommitteeMemberInit::new(create_test_pubkey(3), None, MemberRole::Member),
            CommitteeMemberInit::new(create_test_pubkey(4), None, MemberRole::Observer),
            CommitteeMemberInit::new(create_test_pubkey(5), None, MemberRole::Observer),
        ];

        // threshold=2 meets 2/3 of approver_count=3 (ceil(3*2/3)=2)
        let payload = BootstrapCommitteePayload::new(
            "Test Committee".to_string(),
            members,
            2, // threshold <= approver_count AND >= 2/3 of approvers
            1,
            32767,
        );

        let bootstrap_sender = create_bootstrap_sender();
        let result: Result<(), VerificationError<()>> =
            verify_bootstrap_committee(&payload, &bootstrap_sender);
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_threshold_with_observers_uses_approver_count() {
        // UpdateThreshold should use approver_count, not member_count
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 5 total members, but only 3 approvers (2 are observers)
        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,   // active members (includes observers)
            approver_count: 3, // only members who can approve
            total_member_count: 5,
            threshold: 2,
            kyc_threshold: 1,
            target_is_active: None,
            target_can_approve: None,
        };

        // Invalid: threshold 4 > 3 approvers
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateThreshold { new_threshold: 4 },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("exceeds approver count"),
            "Expected error about threshold exceeding approver count, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_update_threshold_with_observers_valid() {
        // Valid threshold update when accounting for observers
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 5 total members, 3 approvers
        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            approver_count: 3,
            total_member_count: 5,
            threshold: 2,
            kyc_threshold: 1,
            target_is_active: None,
            target_can_approve: None,
        };

        // Valid: threshold 3 <= 3 approvers AND 3 >= ceil(3*2/3) = 2
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateThreshold { new_threshold: 3 },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_kyc_threshold_with_observers_exceeds_approvers() {
        // UpdateKycThreshold should also use approver_count
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 6 total members, only 4 approvers
        let committee_info = CommitteeGovernanceInfo {
            member_count: 6,
            approver_count: 4,
            total_member_count: 6,
            threshold: 3,
            kyc_threshold: 2,
            target_is_active: None,
            target_can_approve: None,
        };

        // Invalid: KYC threshold 5 > 4 approvers
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateKycThreshold {
                new_kyc_threshold: 5,
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("exceeds approver count"),
            "Expected error about KYC threshold exceeding approver count, got: {}",
            err_msg
        );
    }

    // ============================================================================
    // UpdateMemberRole Safety Tests
    // ============================================================================

    #[test]
    fn test_update_member_role_to_observer_breaks_governance_threshold() {
        // Changing member to Observer should check if it breaks threshold
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 4 approvers, threshold=4 (exactly at limit)
        let committee_info = CommitteeGovernanceInfo {
            member_count: 4,
            approver_count: 4,
            total_member_count: 4,
            threshold: 4, // governance threshold
            kyc_threshold: 2,
            target_is_active: None,
            target_can_approve: None,
        };

        // Changing one to Observer would leave 3 approvers < threshold 4
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateMemberRole {
                public_key: create_test_pubkey(1),
                new_role: MemberRole::Observer,
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("governance threshold") || err_msg.contains("Observer"),
            "Expected error about breaking governance threshold, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_update_member_role_to_observer_breaks_kyc_threshold() {
        // Changing member to Observer should also check KYC threshold
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 5 approvers, governance_threshold=3, kyc_threshold=5
        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            approver_count: 5,
            total_member_count: 5,
            threshold: 3, // governance threshold (OK after change: 4 >= 3)
            kyc_threshold: 5,
            target_is_active: None,
            target_can_approve: None, // KYC threshold (will break: 4 < 5)
        };

        // Changing one to Observer would leave 4 approvers < kyc_threshold 5
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateMemberRole {
                public_key: create_test_pubkey(1),
                new_role: MemberRole::Observer,
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("KYC threshold") || err_msg.contains("Observer"),
            "Expected error about breaking KYC threshold, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_update_member_role_to_observer_valid() {
        // Valid role change when enough approvers remain
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 5 approvers, threshold=3, kyc_threshold=3
        let committee_info = CommitteeGovernanceInfo {
            member_count: 5,
            approver_count: 5,
            total_member_count: 5,
            threshold: 3,
            kyc_threshold: 3,
            target_is_active: None,
            target_can_approve: None,
        };

        // Changing one to Observer leaves 4 approvers >= both thresholds
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateMemberRole {
                public_key: create_test_pubkey(1),
                new_role: MemberRole::Observer,
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_member_role_to_member_always_valid() {
        // Changing Observer to Member should always be valid (increases approvers)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 3 approvers with threshold=3 (at limit)
        let committee_info = CommitteeGovernanceInfo {
            member_count: 4, // includes 1 observer
            approver_count: 3,
            total_member_count: 4,
            threshold: 3,
            kyc_threshold: 3,
            target_is_active: None,
            target_can_approve: None,
        };

        // Changing Observer to Member increases approvers (safe)
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateMemberRole {
                public_key: create_test_pubkey(4),
                new_role: MemberRole::Member,
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_member_role_to_chair_always_valid() {
        // Changing to Chair should be valid (Chair can approve)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let committee_info = CommitteeGovernanceInfo {
            member_count: 4,
            approver_count: 3,
            total_member_count: 4,
            threshold: 3,
            kyc_threshold: 2,
            target_is_active: None,
            target_can_approve: None,
        };

        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateMemberRole {
                public_key: create_test_pubkey(1),
                new_role: MemberRole::Chair,
            },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_ok());
    }

    // ============================================================================
    // State Divergence Tests: member_count vs approver_count
    // ============================================================================

    #[test]
    fn test_threshold_two_thirds_rule_uses_approver_count() {
        // The 2/3 rule should be calculated on approver_count, not member_count
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 6 members but only 3 approvers (3 are observers)
        // min_threshold for 3 approvers = ceil(3*2/3) = 2
        let committee_info = CommitteeGovernanceInfo {
            member_count: 6,
            approver_count: 3,
            total_member_count: 6,
            threshold: 2,
            kyc_threshold: 1,
            target_is_active: None,
            target_can_approve: None,
        };

        // threshold=1 is below 2/3 of approver_count (should fail)
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateThreshold { new_threshold: 1 },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("below minimum"),
            "Expected error about threshold below 2/3 minimum, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_large_committee_with_many_observers() {
        // Stress test: 15 members, only 5 approvers (10 observers)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let committee_info = CommitteeGovernanceInfo {
            member_count: 15,
            approver_count: 5,
            total_member_count: 15,
            threshold: 4, // 4/5 = 80% >= 2/3 of approvers
            kyc_threshold: 3,
            target_is_active: None,
            target_can_approve: None,
        };

        // threshold=6 exceeds approver_count=5
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateThreshold { new_threshold: 6 },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_err());
    }

    #[test]
    fn test_boundary_approver_equals_threshold() {
        // Boundary test: exactly at the limit
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 5 approvers, threshold=5 (exactly at limit)
        let committee_info = CommitteeGovernanceInfo {
            member_count: 7,
            approver_count: 5,
            total_member_count: 7,
            threshold: 4,
            kyc_threshold: 3,
            target_is_active: None,
            target_can_approve: None,
        };

        // threshold=5 == approver_count (should pass)
        let payload = create_update_committee_payload(
            CommitteeUpdateData::UpdateThreshold { new_threshold: 5 },
            vec![create_test_approval(1)],
        );

        let result: Result<(), VerificationError<()>> =
            verify_update_committee_with_state(&payload, &committee_info, now);
        assert!(result.is_ok());
    }
}
