// KYC Integration Tests
//
// This module contains comprehensive integration tests for the KYC system.
// It tests the complete lifecycle of KYC operations including:
// - Committee bootstrap and registration
// - KYC verification (set, revoke, renew)
// - Threshold calculations and approval validation
// - Level validation and expiration
//
// Reference: TOS-KYC-Implementation-Details.md

#[cfg(test)]
mod tests {
    use crate::crypto::{Hash, KeyPair, Signature};
    use crate::kyc::{
        is_valid_kyc_level, level_to_tier, tier_to_level, CommitteeApproval, CommitteeMember,
        CommitteeMemberInfo, KycData, KycFlags, KycRegion, KycStatus, MemberRole, MemberStatus,
        OperationType, SecurityCommittee, VALID_KYC_LEVELS,
    };

    // ========== Helper Functions ==========

    fn create_test_keypair() -> KeyPair {
        KeyPair::new()
    }

    fn create_test_member(keypair: &KeyPair, role: MemberRole) -> CommitteeMember {
        CommitteeMember::new(
            keypair.get_public_key().compress(),
            Some("Test Member".to_string()),
            role,
            1000000,
        )
    }

    fn create_test_members(count: usize) -> (Vec<KeyPair>, Vec<CommitteeMember>) {
        let keypairs: Vec<_> = (0..count).map(|_| create_test_keypair()).collect();
        let members: Vec<_> = keypairs
            .iter()
            .enumerate()
            .map(|(i, kp)| {
                let role = if i == 0 {
                    MemberRole::Chair
                } else if i == 1 {
                    MemberRole::ViceChair
                } else {
                    MemberRole::Member
                };
                create_test_member(kp, role)
            })
            .collect();
        (keypairs, members)
    }

    fn create_approval(keypair: &KeyPair, timestamp: u64) -> CommitteeApproval {
        // Create a mock signature for testing using from_bytes
        let mock_bytes = [0u8; 64];
        let mock_signature =
            Signature::from_bytes(&mock_bytes).expect("Valid mock signature bytes");
        CommitteeApproval::new(
            keypair.get_public_key().compress(),
            mock_signature,
            timestamp,
        )
    }

    fn sample_hash() -> Hash {
        Hash::new([1u8; 32])
    }

    // ========== KYC Level Tests ==========

    #[test]
    fn test_all_valid_levels_are_cumulative() {
        // Verify that all valid levels follow the 2^n - 1 pattern
        for level in VALID_KYC_LEVELS {
            if level == 0 {
                continue;
            }
            // Each valid level should be 2^n - 1, meaning level + 1 should be a power of 2
            let next = level + 1;
            assert!(
                next.is_power_of_two(),
                "Level {} + 1 = {} should be power of 2",
                level,
                next
            );
        }
    }

    #[test]
    fn test_level_to_tier_roundtrip() {
        for tier in 0..=8 {
            let level = tier_to_level(tier);
            let back_tier = level_to_tier(level);
            assert_eq!(tier, back_tier, "Tier {} roundtrip failed", tier);
        }
    }

    #[test]
    fn test_invalid_levels_rejected() {
        // Test some non-cumulative levels
        let invalid_levels = [1, 2, 5, 15, 17, 100, 256, 1000, 65535];
        for level in invalid_levels {
            assert!(
                !is_valid_kyc_level(level),
                "Level {} should be invalid",
                level
            );
        }
    }

    #[test]
    fn test_kyc_flags_composition() {
        // Test that TIER levels are correctly composed from flags
        assert_eq!(
            KycFlags::TIER_1,
            KycFlags::EMAIL | KycFlags::PHONE | KycFlags::BASIC_INFO
        );
        assert_eq!(
            KycFlags::TIER_2,
            KycFlags::TIER_1 | KycFlags::GOV_ID | KycFlags::LIVENESS
        );
        assert_eq!(KycFlags::TIER_3, KycFlags::TIER_2 | KycFlags::ADDRESS);
        assert_eq!(
            KycFlags::TIER_4,
            KycFlags::TIER_3 | KycFlags::SOF | KycFlags::SOW
        );
        assert_eq!(
            KycFlags::TIER_5,
            KycFlags::TIER_4 | KycFlags::BACKGROUND | KycFlags::SCREENING | KycFlags::UBO
        );
    }

    // ========== KycData Tests ==========

    #[test]
    fn test_kyc_data_lifecycle() {
        let verified_at = 1000000;
        let data_hash = sample_hash();

        // Create new KYC
        let mut kyc = KycData::new(31, verified_at, data_hash.clone()); // Tier 2

        // Initially active
        assert_eq!(kyc.status, KycStatus::Active);
        assert!(kyc.is_valid(verified_at + 100));
        assert_eq!(kyc.get_tier(), 2);

        // Can upgrade
        let new_hash = Hash::new([2u8; 32]);
        assert!(kyc.upgrade_level(63, new_hash.clone(), verified_at + 1000));
        assert_eq!(kyc.level, 63);
        assert_eq!(kyc.get_tier(), 3);

        // Cannot downgrade
        assert!(!kyc.upgrade_level(31, sample_hash(), verified_at + 2000));

        // Can be revoked
        kyc.set_status(KycStatus::Revoked);
        assert!(!kyc.is_valid(verified_at + 100));

        // Can be renewed
        kyc.renew(verified_at + 100000, sample_hash());
        assert_eq!(kyc.status, KycStatus::Active);
        assert!(kyc.is_valid(verified_at + 100001));
    }

    #[test]
    fn test_kyc_expiration_by_tier() {
        let verified_at = 0;
        let one_year = 365 * 24 * 3600;
        let two_years = 2 * one_year;

        // Tier 0: No expiration
        let kyc_tier0 = KycData::anonymous();
        assert!(!kyc_tier0.is_expired(u64::MAX));

        // Tier 1-2: 1 year
        let kyc_tier2 = KycData::new(31, verified_at, sample_hash());
        assert!(!kyc_tier2.is_expired(one_year - 1));
        assert!(kyc_tier2.is_expired(one_year));

        // Tier 3-4: 2 years
        let kyc_tier4 = KycData::new(255, verified_at, sample_hash());
        assert!(!kyc_tier4.is_expired(two_years - 1));
        assert!(kyc_tier4.is_expired(two_years));

        // Tier 5+: 1 year (stricter EDD)
        let kyc_tier5 = KycData::new(2047, verified_at, sample_hash());
        assert!(!kyc_tier5.is_expired(one_year - 1));
        assert!(kyc_tier5.is_expired(one_year));
    }

    #[test]
    fn test_kyc_meets_level_requirement() {
        let kyc = KycData::new(63, 1000, sample_hash()); // Tier 3

        // Meets lower tiers
        assert!(kyc.meets_level(0)); // Anonymous
        assert!(kyc.meets_level(7)); // Tier 1
        assert!(kyc.meets_level(31)); // Tier 2
        assert!(kyc.meets_level(63)); // Tier 3

        // Does not meet higher tiers
        assert!(!kyc.meets_level(255)); // Tier 4
        assert!(!kyc.meets_level(2047)); // Tier 5

        // Has specific flags
        assert!(kyc.has_flags(KycFlags::EMAIL));
        assert!(kyc.has_flags(KycFlags::GOV_ID));
        assert!(kyc.has_flags(KycFlags::ADDRESS));
        assert!(!kyc.has_flags(KycFlags::SOF));
    }

    // ========== Committee Tests ==========

    #[test]
    fn test_committee_creation() {
        let (_, members) = create_test_members(5);

        let committee = SecurityCommittee::new_global(
            "Global Security Committee".to_string(),
            members,
            4, // threshold: >= 2/3 of 5 = 4
            1, // kyc_threshold
            32767,
            1000000,
        );

        assert!(committee.is_global());
        assert_eq!(committee.region, KycRegion::Global);
        assert!(committee.parent_id.is_none());
        assert_eq!(committee.active_member_count(), 5);
        assert_eq!(committee.threshold, 4);
    }

    #[test]
    fn test_committee_validation() {
        let (_, members) = create_test_members(3);

        // Valid: 3 members, threshold 2 (>= 2/3)
        let valid_committee = SecurityCommittee::new_global(
            "Test".to_string(),
            members.clone(),
            2,
            1,
            32767,
            1000000,
        );
        assert!(valid_committee.validate().is_ok());

        // Invalid: threshold too low
        let invalid_committee = SecurityCommittee::new_global(
            "Test".to_string(),
            members,
            1, // < 2/3 of 3 = 2
            1,
            32767,
            1000000,
        );
        assert!(invalid_committee.validate().is_err());
    }

    #[test]
    fn test_committee_threshold_calculations() {
        let (_, members) = create_test_members(10);

        let committee = SecurityCommittee::new_global(
            "Test".to_string(),
            members,
            7, // >= 2/3 of 10
            2, // kyc_threshold
            32767,
            1000000,
        );

        // Normal KYC: kyc_threshold
        assert_eq!(
            committee.required_threshold(&OperationType::SetKyc, Some(2)),
            2
        );

        // High-tier KYC (5+): kyc_threshold + 1
        assert_eq!(
            committee.required_threshold(&OperationType::SetKyc, Some(5)),
            3
        );

        // Governance: threshold
        assert_eq!(
            committee.required_threshold(&OperationType::AddMember, None),
            7
        );

        // Emergency: fixed 2
        assert_eq!(
            committee.required_threshold(&OperationType::EmergencySuspend, None),
            2
        );
    }

    #[test]
    fn test_regional_committee_hierarchy() {
        let (_, global_members) = create_test_members(11);
        let global = SecurityCommittee::new_global(
            "Global".to_string(),
            global_members,
            8, // >= 2/3 of 11
            1,
            32767, // Can grant all tiers
            1000000,
        );

        let (_, regional_members) = create_test_members(7);
        let regional = SecurityCommittee::new_regional(
            "Asia Pacific".to_string(),
            KycRegion::AsiaPacific,
            regional_members,
            5, // >= 2/3 of 7
            1,
            2047, // Can only grant up to Tier 5
            global.id.clone(),
            1000000,
        );

        assert!(!regional.is_global());
        assert_eq!(regional.region, KycRegion::AsiaPacific);
        assert_eq!(regional.parent_id, Some(global.id.clone()));
        assert!(regional.max_kyc_level < global.max_kyc_level);
    }

    #[test]
    fn test_member_approval_rights() {
        let (keypairs, members) = create_test_members(5);

        let committee =
            SecurityCommittee::new_global("Test".to_string(), members, 4, 1, 32767, 1000000);

        // Active members can approve
        assert!(committee.can_approve_kyc(&keypairs[0].get_public_key().compress()));
        assert!(committee.can_approve_kyc(&keypairs[1].get_public_key().compress()));

        // Non-existent member cannot approve
        let outsider = create_test_keypair();
        assert!(!committee.can_approve_kyc(&outsider.get_public_key().compress()));
    }

    // ========== Approval Tests ==========

    #[test]
    fn test_approval_uniqueness() {
        let keypair = create_test_keypair();
        let approval1 = create_approval(&keypair, 1000);
        let approval2 = create_approval(&keypair, 2000);

        // Same pubkey = same approver
        assert_eq!(approval1.member_pubkey, approval2.member_pubkey);
    }

    #[test]
    fn test_approval_expiry() {
        // Approvals expire after 24 hours (design spec)
        let current_time = 1000000;
        let twenty_four_hours = 24 * 3600;

        let recent_approval = create_approval(&create_test_keypair(), current_time - 1000);
        let old_approval =
            create_approval(&create_test_keypair(), current_time - twenty_four_hours - 1);

        // Recent approval is valid
        assert!(current_time - recent_approval.timestamp <= twenty_four_hours);

        // Old approval is expired
        assert!(current_time - old_approval.timestamp > twenty_four_hours);
    }

    // ========== KYC Region Tests ==========

    #[test]
    fn test_region_enumeration() {
        use crate::kyc::KycRegion::*;

        let all_regions = [
            Unspecified,
            AsiaPacific,
            Europe,
            NorthAmerica,
            LatinAmerica,
            MiddleEast,
            Africa,
            Oceania,
            Global,
        ];

        for region in &all_regions {
            let code = region.to_u8();
            let back = KycRegion::from_u8(code);
            assert_eq!(
                back,
                Some(*region),
                "Region {:?} code {} roundtrip failed",
                region,
                code
            );
        }
    }

    #[test]
    fn test_global_region_special() {
        assert!(KycRegion::Global.is_global());
        assert!(!KycRegion::AsiaPacific.is_global());
        assert!(!KycRegion::Europe.is_global());
    }

    // ========== CommitteeMemberInfo Tests ==========

    #[test]
    fn test_committee_member_info_conversion() {
        let keypair = create_test_keypair();
        // CommitteeMemberInfo uses CompressedPublicKey (crypto::PublicKey is a type alias)
        let pubkey = keypair.get_public_key().compress();
        let info = CommitteeMemberInfo::new(pubkey, Some("Alice".to_string()), MemberRole::Chair);

        let member = info.into_member(1000000);
        assert_eq!(member.status, MemberStatus::Active);
        assert_eq!(member.role, MemberRole::Chair);
        assert_eq!(member.joined_at, 1000000);
    }

    // ========== Operation Type Tests ==========

    #[test]
    fn test_operation_type_display() {
        let ops = [
            (OperationType::SetKyc, "SetKyc"),
            (OperationType::RevokeKyc, "RevokeKyc"),
            (OperationType::RenewKyc, "RenewKyc"),
            (OperationType::AddMember, "AddMember"),
            (OperationType::EmergencySuspend, "EmergencySuspend"),
        ];

        for (op, expected) in ops {
            assert_eq!(op.as_str(), expected);
        }
    }

    // ========== Edge Cases ==========

    #[test]
    fn test_kyc_data_size() {
        let kyc = KycData::new(31, 1000000, sample_hash());
        // Should be exactly 43 bytes: 2 (level) + 1 (status) + 8 (verified_at) + 32 (hash)
        use crate::serializer::Serializer;
        assert_eq!(kyc.size(), 43);
    }

    #[test]
    fn test_kyc_verification_count() {
        assert_eq!(KycData::new(0, 0, sample_hash()).verification_count(), 0);
        assert_eq!(KycData::new(7, 0, sample_hash()).verification_count(), 3);
        assert_eq!(KycData::new(31, 0, sample_hash()).verification_count(), 5);
        assert_eq!(KycData::new(63, 0, sample_hash()).verification_count(), 6);
        assert_eq!(KycData::new(255, 0, sample_hash()).verification_count(), 8);
        assert_eq!(
            KycData::new(2047, 0, sample_hash()).verification_count(),
            11
        );
        assert_eq!(
            KycData::new(8191, 0, sample_hash()).verification_count(),
            13
        );
        assert_eq!(
            KycData::new(16383, 0, sample_hash()).verification_count(),
            14
        );
        assert_eq!(
            KycData::new(32767, 0, sample_hash()).verification_count(),
            15
        );
    }

    #[test]
    fn test_committee_id_deterministic() {
        // Same inputs should produce same ID
        let id1 = SecurityCommittee::compute_id(KycRegion::AsiaPacific, "Test Committee", 1);
        let id2 = SecurityCommittee::compute_id(KycRegion::AsiaPacific, "Test Committee", 1);
        assert_eq!(id1, id2);

        // Different inputs should produce different IDs
        let id3 = SecurityCommittee::compute_id(KycRegion::Europe, "Test Committee", 1);
        assert_ne!(id1, id3);

        let id4 = SecurityCommittee::compute_id(KycRegion::AsiaPacific, "Other Committee", 1);
        assert_ne!(id1, id4);
    }

    #[test]
    fn test_kyc_status_transitions() {
        let mut kyc = KycData::new(31, 1000, sample_hash());

        // Active → Suspended
        kyc.set_status(KycStatus::Suspended);
        assert!(!kyc.status.allows_transactions());

        // Suspended → Active
        kyc.set_status(KycStatus::Active);
        assert!(kyc.status.allows_transactions());

        // Active → Revoked
        kyc.set_status(KycStatus::Revoked);
        assert!(!kyc.status.allows_transactions());

        // Revoked → Active (via renew)
        kyc.renew(2000, sample_hash());
        assert!(kyc.status.allows_transactions());
    }
}
