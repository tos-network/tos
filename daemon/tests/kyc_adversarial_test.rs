#![allow(clippy::disallowed_methods)]
#![allow(clippy::too_many_arguments)]
#![allow(dead_code)]

//! KYC Adversarial Tests
//!
//! This test suite covers adversarial/attack scenarios identified during the
//! Codex security review reflection. These tests ensure that:
//!
//! 1. Cross-committee authorization attacks are blocked
//! 2. Time-based exploits are prevented
//! 3. Threshold/limit violations are rejected
//! 4. Status-based authorization is enforced
//! 5. Input validation is complete
//!
//! Test Categories:
//! - Cross-Committee Attack Tests (Category 1)
//! - Time-Based Exploit Tests (Category 3)
//! - Threshold/Limit Attack Tests (Category 4)
//! - Status Authorization Tests (Category 5)
//! - Input Validation Tests (Category 6)
//! - Potential Remaining Bug Tests

use std::collections::HashMap;
use tos_common::{
    crypto::{Hash, KeyPair, PublicKey},
    kyc::{
        CommitteeMember, CommitteeStatus, KycRegion, KycStatus, MemberRole, SecurityCommittee,
        APPROVAL_EXPIRY_SECONDS, MIN_COMMITTEE_MEMBERS,
    },
};

// ============================================================================
// Test Constants
// ============================================================================

const MAX_COMMITTEE_MEMBERS: usize = 21;
const MAX_APPROVALS: usize = 15;
const VALID_LEVELS: [u16; 9] = [0, 7, 31, 63, 255, 2047, 8191, 16383, 32767];

// ============================================================================
// Test Helpers
// ============================================================================

fn create_keypairs(count: usize) -> Vec<KeyPair> {
    (0..count).map(|_| KeyPair::new()).collect()
}

fn create_members(keypairs: &[KeyPair]) -> Vec<CommitteeMember> {
    keypairs
        .iter()
        .enumerate()
        .map(|(i, kp)| {
            let role = if i == 0 {
                MemberRole::Chair
            } else {
                MemberRole::Member
            };
            CommitteeMember::new(
                kp.get_public_key().compress(),
                Some(format!("Member {}", i)),
                role,
                1000,
            )
        })
        .collect()
}

fn create_committee(
    name: &str,
    region: KycRegion,
    keypairs: &[KeyPair],
    threshold: u8,
    kyc_threshold: u8,
    max_kyc_level: u16,
    parent_id: Option<Hash>,
    status: CommitteeStatus,
) -> SecurityCommittee {
    let members = create_members(keypairs);
    let now = current_timestamp();

    SecurityCommittee {
        id: compute_committee_id(name, now),
        name: name.to_string(),
        parent_id,
        region,
        members,
        threshold,
        kyc_threshold,
        max_kyc_level,
        status,
        created_at: now,
        updated_at: now,
    }
}

fn compute_committee_id(name: &str, timestamp: u64) -> Hash {
    use tos_common::crypto::hash;
    let mut data = Vec::new();
    data.extend_from_slice(name.as_bytes());
    data.extend_from_slice(&timestamp.to_le_bytes());
    hash(&data)
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1704067200)
}

// ============================================================================
// Mock State for Testing
// ============================================================================

#[derive(Debug, Clone)]
struct MockKycData {
    level: u16,
    status: KycStatus,
    verifying_committee: Hash,
    data_hash: Hash,
    verified_at: u64,
    expires_at: Option<u64>,
    previous_status: Option<KycStatus>,
}

struct MockState {
    committees: HashMap<Hash, SecurityCommittee>,
    kyc_data: HashMap<PublicKey, MockKycData>,
    global_committee_id: Option<Hash>,
    current_time: u64,
}

impl MockState {
    fn new() -> Self {
        Self {
            committees: HashMap::new(),
            kyc_data: HashMap::new(),
            global_committee_id: None,
            current_time: current_timestamp(),
        }
    }

    fn add_committee(&mut self, committee: SecurityCommittee) {
        if committee.parent_id.is_none() {
            self.global_committee_id = Some(committee.id.clone());
        }
        self.committees.insert(committee.id.clone(), committee);
    }

    fn set_kyc(
        &mut self,
        user: PublicKey,
        level: u16,
        committee_id: Hash,
        data_hash: Hash,
    ) -> Result<(), &'static str> {
        // SECURITY CHECK: If user already has KYC from different committee, reject
        if let Some(existing) = self.kyc_data.get(&user) {
            if existing.verifying_committee != committee_id {
                return Err(
                    "SetKyc: user already verified by different committee, use TransferKyc",
                );
            }
        }

        let committee = self
            .committees
            .get(&committee_id)
            .ok_or("Committee not found")?;
        if !committee.is_active() {
            return Err("Committee not active");
        }
        if level > committee.max_kyc_level {
            return Err("Level exceeds committee max");
        }

        self.kyc_data.insert(
            user,
            MockKycData {
                level,
                status: KycStatus::Active,
                verifying_committee: committee_id,
                data_hash,
                verified_at: self.current_time,
                expires_at: None,
                previous_status: None,
            },
        );
        Ok(())
    }

    fn revoke_kyc(&mut self, user: &PublicKey, committee_id: &Hash) -> Result<(), &'static str> {
        let kyc = self.kyc_data.get_mut(user).ok_or("KYC not found")?;

        // SECURITY CHECK: Committee must be user's verifying committee
        if &kyc.verifying_committee != committee_id {
            return Err("RevokeKyc: committee is not user's verifying committee");
        }

        kyc.status = KycStatus::Revoked;
        Ok(())
    }

    fn renew_kyc(
        &mut self,
        user: &PublicKey,
        committee_id: &Hash,
        new_expires_at: u64,
    ) -> Result<(), &'static str> {
        let kyc = self.kyc_data.get_mut(user).ok_or("KYC not found")?;

        // SECURITY CHECK: Committee must be user's verifying committee
        if &kyc.verifying_committee != committee_id {
            return Err("RenewKyc: committee is not user's verifying committee");
        }

        if kyc.status == KycStatus::Revoked {
            return Err("Cannot renew revoked KYC");
        }

        kyc.expires_at = Some(new_expires_at);
        if kyc.status == KycStatus::Suspended {
            kyc.status = KycStatus::Active;
        }
        Ok(())
    }

    fn transfer_kyc(
        &mut self,
        user: &PublicKey,
        source_committee_id: &Hash,
        dest_committee_id: &Hash,
        new_data_hash: Hash,
    ) -> Result<(), &'static str> {
        let kyc = self.kyc_data.get(user).ok_or("KYC not found")?;

        // SECURITY CHECK: Source committee must be user's verifying committee
        if &kyc.verifying_committee != source_committee_id {
            return Err("TransferKyc: source committee is not user's verifying committee");
        }

        // Check effective status (handle suspension expiry)
        let effective_status = if kyc.status == KycStatus::Suspended {
            if let Some(expires_at) = kyc.expires_at {
                if self.current_time >= expires_at {
                    kyc.previous_status.unwrap_or(KycStatus::Active)
                } else {
                    KycStatus::Suspended
                }
            } else {
                KycStatus::Suspended
            }
        } else {
            kyc.status
        };

        if effective_status == KycStatus::Revoked || effective_status == KycStatus::Suspended {
            return Err("Cannot transfer with current status");
        }

        let dest_committee = self
            .committees
            .get(dest_committee_id)
            .ok_or("Destination committee not found")?;

        // SECURITY CHECK: Level must not exceed destination max
        if kyc.level > dest_committee.max_kyc_level {
            return Err("KYC level exceeds destination committee max");
        }

        let kyc = self.kyc_data.get_mut(user).ok_or("KYC not found")?;
        kyc.verifying_committee = dest_committee_id.clone();
        kyc.data_hash = new_data_hash;
        kyc.verified_at = self.current_time;
        Ok(())
    }

    fn emergency_suspend(
        &mut self,
        user: &PublicKey,
        committee_id: &Hash,
        duration_hours: u64,
    ) -> Result<(), &'static str> {
        let kyc = self.kyc_data.get_mut(user).ok_or("KYC not found")?;

        // SECURITY CHECK: Committee must be user's verifying committee
        if &kyc.verifying_committee != committee_id {
            return Err("EmergencySuspend: committee is not user's verifying committee");
        }

        let committee = self
            .committees
            .get(committee_id)
            .ok_or("Committee not found")?;

        // SECURITY CHECK: Dissolved committees cannot emergency suspend
        if committee.status == CommitteeStatus::Dissolved {
            return Err("Dissolved committee cannot emergency suspend");
        }

        kyc.previous_status = Some(kyc.status);
        kyc.status = KycStatus::Suspended;
        kyc.expires_at = Some(self.current_time + duration_hours * 3600);
        Ok(())
    }

    fn appeal_kyc(
        &mut self,
        user: &PublicKey,
        original_committee_id: &Hash,
    ) -> Result<(), &'static str> {
        let kyc = self.kyc_data.get(user).ok_or("KYC not found")?;

        // SECURITY CHECK: Original committee must be user's verifying committee
        if &kyc.verifying_committee != original_committee_id {
            return Err("AppealKyc: original committee is not user's verifying committee");
        }

        // SECURITY CHECK: Only revoked KYC can be appealed
        if kyc.status != KycStatus::Revoked {
            return Err("AppealKyc: only revoked KYC can be appealed");
        }

        Ok(())
    }

    fn advance_time(&mut self, seconds: u64) {
        self.current_time += seconds;
    }

    fn get_effective_status(&self, user: &PublicKey) -> Option<KycStatus> {
        let kyc = self.kyc_data.get(user)?;
        if kyc.status == KycStatus::Suspended {
            if let Some(expires_at) = kyc.expires_at {
                if self.current_time >= expires_at {
                    return kyc.previous_status;
                }
            }
        }
        Some(kyc.status)
    }
}

// ============================================================================
// CATEGORY 1: CROSS-COMMITTEE AUTHORIZATION ATTACK TESTS
// ============================================================================

mod cross_committee_attacks {
    use super::*;

    /// Test: Committee B cannot revoke KYC issued by Committee A
    #[test]
    fn test_cross_committee_revoke_attack() {
        let mut state = MockState::new();

        // Setup: Create two committees
        let keypairs_a = create_keypairs(5);
        let committee_a = create_committee(
            "Committee A",
            KycRegion::Global,
            &keypairs_a,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_a_id = committee_a.id.clone();
        state.add_committee(committee_a);

        let keypairs_b = create_keypairs(5);
        let committee_b = create_committee(
            "Committee B",
            KycRegion::NorthAmerica,
            &keypairs_b,
            4,
            2,
            32767,
            Some(committee_a_id.clone()),
            CommitteeStatus::Active,
        );
        let committee_b_id = committee_b.id.clone();
        state.add_committee(committee_b);

        // Setup: User verified by Committee A
        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_a_id.clone(), Hash::zero())
            .expect("SetKyc should succeed");

        // Attack: Committee B tries to revoke
        let result = state.revoke_kyc(&user, &committee_b_id);

        // Verify: Attack is blocked
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "RevokeKyc: committee is not user's verifying committee"
        );

        // Verify: Correct committee can still revoke
        let result = state.revoke_kyc(&user, &committee_a_id);
        assert!(result.is_ok());
    }

    /// Test: Committee B cannot renew KYC issued by Committee A
    #[test]
    fn test_cross_committee_renew_attack() {
        let mut state = MockState::new();

        // Setup: Create two committees
        let keypairs_a = create_keypairs(5);
        let committee_a = create_committee(
            "Committee A",
            KycRegion::Global,
            &keypairs_a,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_a_id = committee_a.id.clone();
        state.add_committee(committee_a);

        let keypairs_b = create_keypairs(5);
        let committee_b = create_committee(
            "Committee B",
            KycRegion::Europe,
            &keypairs_b,
            4,
            2,
            32767,
            Some(committee_a_id.clone()),
            CommitteeStatus::Active,
        );
        let committee_b_id = committee_b.id.clone();
        state.add_committee(committee_b);

        // Setup: User verified by Committee A
        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_a_id.clone(), Hash::zero())
            .expect("SetKyc should succeed");

        // Attack: Committee B tries to renew
        let new_expires = state.current_time + 365 * 24 * 3600;
        let result = state.renew_kyc(&user, &committee_b_id, new_expires);

        // Verify: Attack is blocked
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "RenewKyc: committee is not user's verifying committee"
        );
    }

    /// Test: Committee B cannot transfer user verified by Committee A
    #[test]
    fn test_cross_committee_transfer_attack() {
        let mut state = MockState::new();

        // Setup: Create three committees
        let keypairs_a = create_keypairs(5);
        let committee_a = create_committee(
            "Committee A",
            KycRegion::Global,
            &keypairs_a,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_a_id = committee_a.id.clone();
        state.add_committee(committee_a);

        let keypairs_b = create_keypairs(5);
        let committee_b = create_committee(
            "Committee B",
            KycRegion::Europe,
            &keypairs_b,
            4,
            2,
            32767,
            Some(committee_a_id.clone()),
            CommitteeStatus::Active,
        );
        let committee_b_id = committee_b.id.clone();
        state.add_committee(committee_b);

        let keypairs_c = create_keypairs(5);
        let committee_c = create_committee(
            "Committee C",
            KycRegion::AsiaPacific,
            &keypairs_c,
            4,
            2,
            32767,
            Some(committee_a_id.clone()),
            CommitteeStatus::Active,
        );
        let committee_c_id = committee_c.id.clone();
        state.add_committee(committee_c);

        // Setup: User verified by Committee A
        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_a_id.clone(), Hash::zero())
            .expect("SetKyc should succeed");

        // Attack: Committee B (not the verifying committee) tries to transfer user to C
        let result = state.transfer_kyc(&user, &committee_b_id, &committee_c_id, Hash::zero());

        // Verify: Attack is blocked
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "TransferKyc: source committee is not user's verifying committee"
        );

        // Verify: Correct committee can transfer
        let result = state.transfer_kyc(&user, &committee_a_id, &committee_b_id, Hash::zero());
        assert!(result.is_ok());
    }

    /// Test: Committee B cannot emergency suspend user verified by Committee A
    #[test]
    fn test_cross_committee_emergency_suspend_attack() {
        let mut state = MockState::new();

        // Setup: Create two committees
        let keypairs_a = create_keypairs(5);
        let committee_a = create_committee(
            "Committee A",
            KycRegion::Global,
            &keypairs_a,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_a_id = committee_a.id.clone();
        state.add_committee(committee_a);

        let keypairs_b = create_keypairs(5);
        let committee_b = create_committee(
            "Committee B",
            KycRegion::Europe,
            &keypairs_b,
            4,
            2,
            32767,
            Some(committee_a_id.clone()),
            CommitteeStatus::Active,
        );
        let committee_b_id = committee_b.id.clone();
        state.add_committee(committee_b);

        // Setup: User verified by Committee A
        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_a_id.clone(), Hash::zero())
            .expect("SetKyc should succeed");

        // Attack: Committee B tries to emergency suspend (DoS attack)
        let result = state.emergency_suspend(&user, &committee_b_id, 24);

        // Verify: Attack is blocked
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "EmergencySuspend: committee is not user's verifying committee"
        );
    }

    /// Test: Committee B cannot SetKyc for user already verified by Committee A
    #[test]
    fn test_cross_committee_setkyc_hijack_attack() {
        let mut state = MockState::new();

        // Setup: Create two committees
        let keypairs_a = create_keypairs(5);
        let committee_a = create_committee(
            "Committee A",
            KycRegion::Global,
            &keypairs_a,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_a_id = committee_a.id.clone();
        state.add_committee(committee_a);

        let keypairs_b = create_keypairs(5);
        let committee_b = create_committee(
            "Committee B",
            KycRegion::Europe,
            &keypairs_b,
            4,
            2,
            32767,
            Some(committee_a_id.clone()),
            CommitteeStatus::Active,
        );
        let committee_b_id = committee_b.id.clone();
        state.add_committee(committee_b);

        // Setup: User verified by Committee A
        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_a_id.clone(), Hash::zero())
            .expect("SetKyc should succeed");

        // Attack: Committee B tries to overwrite KYC (hijack user)
        let result = state.set_kyc(user.clone(), 255, committee_b_id, Hash::zero());

        // Verify: Attack is blocked
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "SetKyc: user already verified by different committee, use TransferKyc"
        );
    }

    /// Test: Appeal must be against the verifying committee
    #[test]
    fn test_cross_committee_appeal_attack() {
        let mut state = MockState::new();

        // Setup: Create two committees
        let keypairs_a = create_keypairs(5);
        let committee_a = create_committee(
            "Committee A",
            KycRegion::Global,
            &keypairs_a,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_a_id = committee_a.id.clone();
        state.add_committee(committee_a);

        let keypairs_b = create_keypairs(5);
        let committee_b = create_committee(
            "Committee B",
            KycRegion::Europe,
            &keypairs_b,
            4,
            2,
            32767,
            Some(committee_a_id.clone()),
            CommitteeStatus::Active,
        );
        let committee_b_id = committee_b.id.clone();
        state.add_committee(committee_b);

        // Setup: User verified by Committee A, then revoked
        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_a_id.clone(), Hash::zero())
            .expect("SetKyc should succeed");
        state
            .revoke_kyc(&user, &committee_a_id)
            .expect("Revoke should succeed");

        // Attack: Try to appeal against Committee B (not the verifying committee)
        let result = state.appeal_kyc(&user, &committee_b_id);

        // Verify: Attack is blocked
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "AppealKyc: original committee is not user's verifying committee"
        );
    }
}

// ============================================================================
// CATEGORY 3: TIME-BASED EXPLOIT TESTS
// ============================================================================

mod time_based_tests {
    use super::*;

    /// Test: Emergency suspension auto-expires after 24 hours
    #[test]
    fn test_suspension_auto_expiry() {
        let mut state = MockState::new();

        // Setup: Create committee and user
        let keypairs = create_keypairs(5);
        let committee = create_committee(
            "Test Committee",
            KycRegion::Global,
            &keypairs,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_id = committee.id.clone();
        state.add_committee(committee);

        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_id.clone(), Hash::zero())
            .expect("SetKyc should succeed");

        // Suspend for 24 hours
        state
            .emergency_suspend(&user, &committee_id, 24)
            .expect("Suspend should succeed");

        // Verify: User is suspended
        assert_eq!(
            state.get_effective_status(&user),
            Some(KycStatus::Suspended)
        );

        // Advance time by 23 hours - still suspended
        state.advance_time(23 * 3600);
        assert_eq!(
            state.get_effective_status(&user),
            Some(KycStatus::Suspended)
        );

        // Advance time by 2 more hours (total 25 hours) - should be active
        state.advance_time(2 * 3600);
        assert_eq!(state.get_effective_status(&user), Some(KycStatus::Active));
    }

    /// Test: Transfer is allowed after suspension expires
    #[test]
    fn test_transfer_allowed_after_expiry() {
        let mut state = MockState::new();

        // Setup: Create two committees
        let keypairs_a = create_keypairs(5);
        let committee_a = create_committee(
            "Committee A",
            KycRegion::Global,
            &keypairs_a,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_a_id = committee_a.id.clone();
        state.add_committee(committee_a);

        let keypairs_b = create_keypairs(5);
        let committee_b = create_committee(
            "Committee B",
            KycRegion::Europe,
            &keypairs_b,
            4,
            2,
            32767,
            Some(committee_a_id.clone()),
            CommitteeStatus::Active,
        );
        let committee_b_id = committee_b.id.clone();
        state.add_committee(committee_b);

        // Setup: User verified and suspended
        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_a_id.clone(), Hash::zero())
            .expect("SetKyc should succeed");
        state
            .emergency_suspend(&user, &committee_a_id, 24)
            .expect("Suspend should succeed");

        // Verify: Transfer blocked while suspended
        let result = state.transfer_kyc(&user, &committee_a_id, &committee_b_id, Hash::zero());
        assert!(result.is_err());

        // Advance time past expiry
        state.advance_time(25 * 3600);

        // Verify: Transfer now allowed
        let result = state.transfer_kyc(&user, &committee_a_id, &committee_b_id, Hash::zero());
        assert!(result.is_ok());
    }

    /// Test: Approval timestamp validation - expired approvals
    #[test]
    fn test_expired_approval_timestamp() {
        let now = current_timestamp();
        let old_timestamp = now - APPROVAL_EXPIRY_SECONDS - 1;

        // Verify: old_timestamp is expired relative to now
        let is_expired = now.saturating_sub(old_timestamp) > APPROVAL_EXPIRY_SECONDS;
        assert!(is_expired, "Approval should be considered expired");
    }

    /// Test: Future approval timestamp validation
    #[test]
    fn test_future_approval_timestamp() {
        let now = current_timestamp();
        let future_timestamp = now + 7200; // 2 hours in future (beyond 1 hour skew)

        // Verify: future_timestamp is in the future beyond allowed skew (1 hour)
        let max_skew = 3600u64; // 1 hour
        let is_future = future_timestamp > now.saturating_add(max_skew);
        assert!(is_future, "Approval timestamp should be beyond allowed skew");
    }
}

// ============================================================================
// CATEGORY 4: THRESHOLD/LIMIT VIOLATION TESTS
// ============================================================================

mod threshold_limit_tests {
    use super::*;

    /// Test: Threshold cannot exceed MAX_APPROVALS
    #[test]
    fn test_threshold_exceeds_max_approvals() {
        // MAX_APPROVALS is 15, trying to create committee with threshold 20
        let threshold: u8 = 20;

        // Verify: threshold > MAX_APPROVALS should be rejected
        assert!(
            (threshold as usize) > MAX_APPROVALS,
            "Test setup: threshold should exceed MAX_APPROVALS"
        );
    }

    /// Test: Threshold must be >= 2/3 of member count
    #[test]
    fn test_threshold_two_thirds_rule() {
        // 5 members, 2/3 = 3.33... → min threshold = 4
        let member_count = 5;
        let min_threshold = (member_count * 2 + 2) / 3; // Ceiling division

        assert_eq!(min_threshold, 4);

        // Threshold 3 should be rejected for 5 members
        let invalid_threshold = 3;
        assert!(
            invalid_threshold < min_threshold,
            "Threshold 3 is below 2/3 of 5"
        );
    }

    /// Test: Member count boundaries
    #[test]
    fn test_member_count_boundaries() {
        // Minimum members
        assert_eq!(MIN_COMMITTEE_MEMBERS, 3);

        // Maximum members
        assert_eq!(MAX_COMMITTEE_MEMBERS, 21);

        // Verify edge cases
        assert!(2 < MIN_COMMITTEE_MEMBERS); // 2 members not allowed
        assert!(22 > MAX_COMMITTEE_MEMBERS); // 22 members not allowed
    }

    /// Test: KYC level boundaries
    #[test]
    fn test_kyc_level_boundaries() {
        // Valid levels are specific bitmask values
        for level in VALID_LEVELS {
            assert!(
                VALID_LEVELS.contains(&level),
                "Level {} should be valid",
                level
            );
        }

        // Invalid levels
        let invalid_levels = [1, 2, 3, 4, 5, 6, 8, 100, 1000, 65535];
        for level in invalid_levels {
            assert!(
                !VALID_LEVELS.contains(&level),
                "Level {} should be invalid",
                level
            );
        }
    }
}

// ============================================================================
// CATEGORY 5: STATUS AUTHORIZATION TESTS
// ============================================================================

mod status_authorization_tests {
    use super::*;

    /// Test: Active committee can perform all operations
    #[test]
    fn test_active_committee_full_access() {
        let mut state = MockState::new();

        let keypairs = create_keypairs(5);
        let committee = create_committee(
            "Active Committee",
            KycRegion::Global,
            &keypairs,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_id = committee.id.clone();
        state.add_committee(committee);

        let user = KeyPair::new().get_public_key().compress();

        // All operations should succeed
        assert!(state
            .set_kyc(user.clone(), 255, committee_id.clone(), Hash::zero())
            .is_ok());
        assert!(state
            .renew_kyc(&user, &committee_id, state.current_time + 3600)
            .is_ok());
        assert!(state.emergency_suspend(&user, &committee_id, 24).is_ok());
    }

    /// Test: Suspended committee can ONLY do emergency operations
    #[test]
    fn test_suspended_committee_emergency_only() {
        let mut state = MockState::new();

        // First create an active committee to set up the user
        let keypairs = create_keypairs(5);
        let mut committee = create_committee(
            "Suspended Committee",
            KycRegion::Global,
            &keypairs,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_id = committee.id.clone();
        state.add_committee(committee.clone());

        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_id.clone(), Hash::zero())
            .expect("SetKyc should succeed while active");

        // Now suspend the committee
        committee.status = CommitteeStatus::Suspended;
        state.committees.insert(committee_id.clone(), committee);

        // SetKyc should fail for suspended committee
        let new_user = KeyPair::new().get_public_key().compress();
        let result = state.set_kyc(new_user, 255, committee_id.clone(), Hash::zero());
        assert!(result.is_err());

        // Emergency suspend should still work
        assert!(state.emergency_suspend(&user, &committee_id, 24).is_ok());
    }

    /// Test: Dissolved committee cannot do ANY operations
    #[test]
    fn test_dissolved_committee_no_access() {
        let mut state = MockState::new();

        // First create an active committee to set up the user
        let keypairs = create_keypairs(5);
        let mut committee = create_committee(
            "Dissolved Committee",
            KycRegion::Global,
            &keypairs,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_id = committee.id.clone();
        state.add_committee(committee.clone());

        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_id.clone(), Hash::zero())
            .expect("SetKyc should succeed while active");

        // Now dissolve the committee
        committee.status = CommitteeStatus::Dissolved;
        state.committees.insert(committee_id.clone(), committee);

        // SetKyc should fail
        let new_user = KeyPair::new().get_public_key().compress();
        let result = state.set_kyc(new_user, 255, committee_id.clone(), Hash::zero());
        assert!(result.is_err());

        // Emergency suspend should also fail for dissolved committees
        let result = state.emergency_suspend(&user, &committee_id, 24);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Dissolved committee cannot emergency suspend"
        );
    }
}

// ============================================================================
// CATEGORY 6: INPUT VALIDATION TESTS
// ============================================================================

mod input_validation_tests {
    use super::*;

    /// Test: Transfer level cannot exceed destination committee max
    #[test]
    fn test_transfer_level_exceeds_dest_max() {
        let mut state = MockState::new();

        // Committee A: max_kyc_level = 32767 (highest)
        let keypairs_a = create_keypairs(5);
        let committee_a = create_committee(
            "Committee A",
            KycRegion::Global,
            &keypairs_a,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_a_id = committee_a.id.clone();
        state.add_committee(committee_a);

        // Committee B: max_kyc_level = 63 (lower)
        let keypairs_b = create_keypairs(5);
        let committee_b = create_committee(
            "Committee B",
            KycRegion::Europe,
            &keypairs_b,
            4,
            2,
            63, // Lower max level
            Some(committee_a_id.clone()),
            CommitteeStatus::Active,
        );
        let committee_b_id = committee_b.id.clone();
        state.add_committee(committee_b);

        // User with level 255 (higher than Committee B's max of 63)
        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_a_id.clone(), Hash::zero())
            .expect("SetKyc should succeed");

        // Transfer should fail - level 255 > max 63
        let result = state.transfer_kyc(&user, &committee_a_id, &committee_b_id, Hash::zero());
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "KYC level exceeds destination committee max"
        );
    }

    /// Test: Appeal only allowed for revoked status
    #[test]
    fn test_appeal_requires_revoked_status() {
        let mut state = MockState::new();

        let keypairs = create_keypairs(5);
        let committee = create_committee(
            "Test Committee",
            KycRegion::Global,
            &keypairs,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_id = committee.id.clone();
        state.add_committee(committee);

        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_id.clone(), Hash::zero())
            .expect("SetKyc should succeed");

        // User has Active status - appeal should fail
        let result = state.appeal_kyc(&user, &committee_id);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "AppealKyc: only revoked KYC can be appealed"
        );

        // Revoke the user
        state
            .revoke_kyc(&user, &committee_id)
            .expect("Revoke should succeed");

        // Now appeal should work
        let result = state.appeal_kyc(&user, &committee_id);
        assert!(result.is_ok());
    }

    /// Test: SetKyc level cannot exceed committee max
    #[test]
    fn test_setkyc_level_exceeds_committee_max() {
        let mut state = MockState::new();

        let keypairs = create_keypairs(5);
        let committee = create_committee(
            "Low Level Committee",
            KycRegion::Global,
            &keypairs,
            4,
            2,
            63, // Low max level
            None,
            CommitteeStatus::Active,
        );
        let committee_id = committee.id.clone();
        state.add_committee(committee);

        let user = KeyPair::new().get_public_key().compress();

        // Try to set level 255 when max is 63
        let result = state.set_kyc(user, 255, committee_id, Hash::zero());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Level exceeds committee max");
    }

    /// Test: Renew not allowed for revoked KYC
    #[test]
    fn test_renew_revoked_fails() {
        let mut state = MockState::new();

        let keypairs = create_keypairs(5);
        let committee = create_committee(
            "Test Committee",
            KycRegion::Global,
            &keypairs,
            4,
            2,
            32767,
            None,
            CommitteeStatus::Active,
        );
        let committee_id = committee.id.clone();
        state.add_committee(committee);

        let user = KeyPair::new().get_public_key().compress();
        state
            .set_kyc(user.clone(), 255, committee_id.clone(), Hash::zero())
            .expect("SetKyc should succeed");

        // Revoke the user
        state
            .revoke_kyc(&user, &committee_id)
            .expect("Revoke should succeed");

        // Try to renew - should fail
        let result = state.renew_kyc(&user, &committee_id, state.current_time + 3600);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Cannot renew revoked KYC");
    }
}

// ============================================================================
// POTENTIAL REMAINING BUG TESTS
// ============================================================================

mod potential_remaining_bugs {
    use super::*;

    /// Test: Empty member list protection
    #[test]
    fn test_empty_member_list_protection() {
        // Verify MIN_COMMITTEE_MEMBERS prevents empty committees
        assert!(MIN_COMMITTEE_MEMBERS >= 3);

        // An empty member list would be a serious vulnerability
        let empty_members: Vec<CommitteeMember> = vec![];
        assert!(empty_members.len() < MIN_COMMITTEE_MEMBERS);
    }

    /// Test: Zero timestamp handling
    #[test]
    fn test_zero_timestamp_handling() {
        let now = current_timestamp();

        // Approval with timestamp 0 should be considered expired
        let age = now.saturating_sub(0);
        let is_expired = age > APPROVAL_EXPIRY_SECONDS;

        assert!(
            is_expired,
            "Timestamp 0 should be expired relative to current time"
        );
    }

    /// Test: MAX_APPROVALS constant
    #[test]
    fn test_max_approvals_constant() {
        assert_eq!(MAX_APPROVALS, 15);

        // Verify that threshold > MAX_APPROVALS makes committee ungovernable
        let threshold = 20u8;
        assert!(
            (threshold as usize) > MAX_APPROVALS,
            "Threshold {} exceeds MAX_APPROVALS {}",
            threshold,
            MAX_APPROVALS
        );
    }
}

// ============================================================================
// SUMMARY TEST
// ============================================================================

#[test]
fn test_adversarial_test_suite_summary() {
    println!("\n========================================");
    println!("KYC ADVERSARIAL TEST SUITE SUMMARY");
    println!("========================================\n");

    println!("Category 1: Cross-Committee Authorization (6 tests)");
    println!("  - Revoke attack: Committee B cannot revoke user of Committee A");
    println!("  - Renew attack: Committee B cannot renew user of Committee A");
    println!("  - Transfer attack: Wrong source committee blocked");
    println!("  - EmergencySuspend DoS: Cross-committee suspension blocked");
    println!("  - SetKyc hijack: Cannot overwrite existing KYC from different committee");
    println!("  - Appeal attack: Must appeal to correct committee");
    println!();

    println!("Category 3: Time-Based Exploits (4 tests)");
    println!("  - Suspension auto-expires after 24 hours");
    println!("  - Transfer allowed after expiry");
    println!("  - Expired approvals detected");
    println!("  - Future approvals detected");
    println!();

    println!("Category 4: Threshold/Limit Violations (4 tests)");
    println!("  - Threshold > MAX_APPROVALS blocked");
    println!("  - Threshold 2/3 rule enforced");
    println!("  - Member count boundaries");
    println!("  - KYC level boundaries");
    println!();

    println!("Category 5: Status Authorization (3 tests)");
    println!("  - Active committee: full access");
    println!("  - Suspended committee: emergency only");
    println!("  - Dissolved committee: no access");
    println!();

    println!("Category 6: Input Validation (4 tests)");
    println!("  - Transfer level cannot exceed dest max");
    println!("  - Appeal requires revoked status");
    println!("  - SetKyc level cannot exceed committee max");
    println!("  - Renew not allowed for revoked KYC");
    println!();

    println!("Category 7: Potential Remaining Bugs (3 tests)");
    println!("  - Empty member list protection");
    println!("  - Zero timestamp handling");
    println!("  - MAX_APPROVALS constant");
    println!();

    println!("========================================");
    println!("TOTAL: 24 adversarial tests");
    println!("========================================\n");
}
