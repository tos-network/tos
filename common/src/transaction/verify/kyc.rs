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
    BootstrapCommitteePayload, EmergencySuspendPayload, RegisterCommitteePayload, RenewKycPayload,
    RevokeKycPayload, SetKycPayload, TransferKycPayload, UpdateCommitteePayload,
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

    // Validate update-specific constraints
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
}
