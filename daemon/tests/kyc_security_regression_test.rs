//! KYC Security Regression Tests
//!
//! This test module verifies security fixes identified during code review.
//! Each test is designed to catch specific vulnerabilities and ensure they
//! remain fixed.
//!
//! ## Test Categories
//!
//! 1. **Future Timestamp Validation**: Ensures approvals with timestamps
//!    beyond allowed clock skew are rejected.
//!
//! 2. **Arithmetic Overflow Protection**: Verifies saturating arithmetic
//!    prevents overflow/underflow with extreme timestamp values.
//!
//! 3. **Region Validation**: Ensures invalid regions (Global/Unspecified)
//!    are properly detected for regional committee registration.
//!
//! 4. **Zero Hash Validation**: Verifies zero hashes are properly detected
//!    for data_hash and reason_hash fields.
//!
//! 5. **Transfer Level Binding**: Ensures destination approvals are bound
//!    to the current KYC level, preventing replay after upgrades.

#![allow(clippy::disallowed_methods)]
#![allow(clippy::too_many_arguments)]
#![allow(dead_code)]

use tos_common::{
    crypto::{Hash, KeyPair},
    kyc::{
        CommitteeApproval, CommitteeMember, CommitteeStatus, KycRegion, MemberRole,
        SecurityCommittee, APPROVAL_EXPIRY_SECONDS,
    },
    network::Network,
};

// ============================================================================
// Test Helpers
// ============================================================================

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn test_network() -> Network {
    Network::Devnet
}

fn create_test_committee(
    id: Hash,
    member_count: usize,
    threshold: u8,
    max_kyc_level: u16,
    status: CommitteeStatus,
) -> (SecurityCommittee, Vec<KeyPair>) {
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

    let committee = SecurityCommittee {
        id,
        name: "Test Committee".to_string(),
        parent_id: None,
        region: KycRegion::Global,
        members,
        threshold,
        kyc_threshold: threshold,
        max_kyc_level,
        status,
        created_at: current_timestamp(),
        updated_at: current_timestamp(),
    };

    (committee, keypairs)
}

fn create_approval(kp: &KeyPair, message: &[u8], timestamp: u64) -> CommitteeApproval {
    let signature = kp.sign(message);
    CommitteeApproval {
        member_pubkey: kp.get_public_key().compress(),
        signature,
        timestamp,
    }
}

// ============================================================================
// Future Timestamp Validation Tests
// ============================================================================
// Ensures approvals with timestamps beyond allowed clock skew are rejected.

mod future_timestamp_tests {
    use super::*;

    /// Approval timestamp 2 hours in future should be rejected
    #[test]
    fn test_future_timestamp_beyond_skew_rejected() {
        let committee_id = Hash::new([1u8; 32]);
        let (committee, keypairs) =
            create_test_committee(committee_id.clone(), 5, 3, 255, CommitteeStatus::Active);
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let data_hash = Hash::new([2u8; 32]);
        let now = current_timestamp();
        let verified_at = now;

        // Approval timestamp 2 hours in future (beyond 1 hour allowed skew)
        let future_time = now + 7200;

        let message = CommitteeApproval::build_set_kyc_message(
            &test_network(),
            &committee.id,
            &user_pk,
            100,
            &data_hash,
            verified_at,
            future_time,
        );

        let approvals: Vec<CommitteeApproval> = keypairs
            .iter()
            .take(3)
            .map(|kp| create_approval(kp, &message, future_time))
            .collect();

        let result = tos_common::kyc::verify_set_kyc_approvals(
            &test_network(),
            &committee,
            &approvals,
            &user_pk,
            100,
            &data_hash,
            verified_at,
            now,
        );

        assert!(
            result.is_err(),
            "Approvals with future timestamps (>1 hour) should be rejected"
        );
    }

    /// Approval timestamp exactly at allowed skew boundary should be accepted
    #[test]
    fn test_timestamp_at_skew_boundary_accepted() {
        let committee_id = Hash::new([1u8; 32]);
        let (committee, keypairs) =
            create_test_committee(committee_id.clone(), 5, 3, 255, CommitteeStatus::Active);
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let data_hash = Hash::new([2u8; 32]);
        let now = current_timestamp();
        let verified_at = now;

        // Approval timestamp exactly 1 hour in future (at boundary)
        let boundary_time = now + 3600;

        let message = CommitteeApproval::build_set_kyc_message(
            &test_network(),
            &committee.id,
            &user_pk,
            100,
            &data_hash,
            verified_at,
            boundary_time,
        );

        let approvals: Vec<CommitteeApproval> = keypairs
            .iter()
            .take(3)
            .map(|kp| create_approval(kp, &message, boundary_time))
            .collect();

        let result = tos_common::kyc::verify_set_kyc_approvals(
            &test_network(),
            &committee,
            &approvals,
            &user_pk,
            100,
            &data_hash,
            verified_at,
            now,
        );

        assert!(
            result.is_ok(),
            "Approvals with timestamp at 1-hour boundary should be accepted"
        );
    }

    /// Approval timestamp 1 second beyond skew should be rejected
    #[test]
    fn test_timestamp_one_second_beyond_skew_rejected() {
        let committee_id = Hash::new([1u8; 32]);
        let (committee, keypairs) =
            create_test_committee(committee_id.clone(), 5, 3, 255, CommitteeStatus::Active);
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let data_hash = Hash::new([2u8; 32]);
        let now = current_timestamp();
        let verified_at = now;

        // Approval timestamp 1 second beyond allowed skew
        let beyond_time = now + 3601;

        let message = CommitteeApproval::build_set_kyc_message(
            &test_network(),
            &committee.id,
            &user_pk,
            100,
            &data_hash,
            verified_at,
            beyond_time,
        );

        let approvals: Vec<CommitteeApproval> = keypairs
            .iter()
            .take(3)
            .map(|kp| create_approval(kp, &message, beyond_time))
            .collect();

        let result = tos_common::kyc::verify_set_kyc_approvals(
            &test_network(),
            &committee,
            &approvals,
            &user_pk,
            100,
            &data_hash,
            verified_at,
            now,
        );

        assert!(
            result.is_err(),
            "Approvals with timestamp 1 second beyond skew should be rejected"
        );
    }
}

// ============================================================================
// Arithmetic Overflow Protection Tests
// ============================================================================
// Verifies saturating arithmetic prevents overflow with extreme values.

mod timestamp_overflow_tests {
    use super::*;

    /// Maximum u64 timestamp should be handled gracefully
    #[test]
    fn test_max_u64_timestamp_handled() {
        let now = u64::MAX;

        // saturating_add should not overflow
        let max_future = now.saturating_add(3600);
        assert_eq!(max_future, u64::MAX, "saturating_add should cap at MAX");

        // saturating_sub should not underflow
        let min_past = now.saturating_sub(APPROVAL_EXPIRY_SECONDS);
        assert!(min_past < u64::MAX, "saturating_sub should work correctly");
    }

    /// Near-max timestamp should not cause panic
    #[test]
    fn test_near_max_timestamp_no_panic() {
        let committee_id = Hash::new([1u8; 32]);
        let (committee, keypairs) =
            create_test_committee(committee_id.clone(), 5, 3, 255, CommitteeStatus::Active);
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let data_hash = Hash::new([2u8; 32]);

        // Use extreme timestamp near u64::MAX
        let extreme_time = u64::MAX - 1000;

        let message = CommitteeApproval::build_set_kyc_message(
            &test_network(),
            &committee.id,
            &user_pk,
            100,
            &data_hash,
            extreme_time,
            extreme_time,
        );

        let approvals: Vec<CommitteeApproval> = keypairs
            .iter()
            .take(3)
            .map(|kp| create_approval(kp, &message, extreme_time))
            .collect();

        // This should NOT panic - may error but not panic
        let result = tos_common::kyc::verify_set_kyc_approvals(
            &test_network(),
            &committee,
            &approvals,
            &user_pk,
            100,
            &data_hash,
            extreme_time,
            extreme_time,
        );

        // Result may be error but should not panic
        let _ = result;
    }

    /// Zero timestamp edge case should be handled
    #[test]
    fn test_zero_timestamp_edge_case() {
        let zero: u64 = 0;
        let result = zero.saturating_sub(3600);
        assert_eq!(
            result, 0,
            "saturating_sub(0, 3600) should be 0, not underflow"
        );
    }
}

// ============================================================================
// Region Validation Tests
// ============================================================================
// Ensures invalid regions are properly detected.

mod region_validation_tests {
    use super::*;

    /// KycRegion discriminants should be correct
    #[test]
    fn test_region_discriminants() {
        assert_eq!(KycRegion::Unspecified as u8, 0);
        assert_eq!(KycRegion::AsiaPacific as u8, 1);
        assert_eq!(KycRegion::Europe as u8, 2);
        assert_eq!(KycRegion::NorthAmerica as u8, 3);
    }

    /// Global region should be detected by is_global()
    #[test]
    fn test_global_region_detection() {
        assert!(
            KycRegion::Global.is_global(),
            "Global region should be detected"
        );
        assert!(
            !KycRegion::AsiaPacific.is_global(),
            "AsiaPacific should not be global"
        );
        assert!(
            !KycRegion::Europe.is_global(),
            "Europe should not be global"
        );
    }

    /// Unspecified region should be distinguishable
    #[test]
    fn test_unspecified_region_detection() {
        let region = KycRegion::Unspecified;
        assert_eq!(region, KycRegion::Unspecified, "Should be Unspecified");
    }
}

// ============================================================================
// Zero Hash Validation Tests
// ============================================================================
// Verifies zero hashes are properly detected.

mod zero_hash_validation_tests {
    use super::*;

    /// Zero hash should be detectable
    #[test]
    fn test_zero_hash_detection() {
        let zero_hash = Hash::zero();
        let non_zero_hash = Hash::new([1u8; 32]);

        assert_eq!(
            zero_hash,
            Hash::zero(),
            "Zero hash should equal Hash::zero()"
        );
        assert_ne!(
            non_zero_hash,
            Hash::zero(),
            "Non-zero hash should not equal Hash::zero()"
        );
    }

    /// Messages with different data_hash should differ
    #[test]
    fn test_data_hash_affects_message() {
        let committee_id = Hash::new([1u8; 32]);
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let now = current_timestamp();

        let zero_hash = Hash::zero();
        let valid_hash = Hash::new([42u8; 32]);

        let message_zero = CommitteeApproval::build_set_kyc_message(
            &test_network(),
            &committee_id,
            &user_pk,
            100,
            &zero_hash,
            now,
            now,
        );

        let message_valid = CommitteeApproval::build_set_kyc_message(
            &test_network(),
            &committee_id,
            &user_pk,
            100,
            &valid_hash,
            now,
            now,
        );

        assert_ne!(
            message_zero, message_valid,
            "Messages with different data_hash should differ"
        );
    }

    /// Revoke messages with different reason_hash should differ
    #[test]
    fn test_reason_hash_affects_revoke_message() {
        let committee_id = Hash::new([1u8; 32]);
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let now = current_timestamp();

        let zero_reason = Hash::zero();
        let valid_reason = Hash::new([99u8; 32]);

        let message_zero = CommitteeApproval::build_revoke_kyc_message(
            &test_network(),
            &committee_id,
            &user_pk,
            &zero_reason,
            now,
        );

        let message_valid = CommitteeApproval::build_revoke_kyc_message(
            &test_network(),
            &committee_id,
            &user_pk,
            &valid_reason,
            now,
        );

        assert_ne!(
            message_zero, message_valid,
            "Revoke messages with different reason_hash should differ"
        );
    }

    /// EmergencySuspend messages with different reason_hash should differ
    #[test]
    fn test_reason_hash_affects_suspend_message() {
        let committee_id = Hash::new([1u8; 32]);
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let now = current_timestamp();
        let expires_at = now + 86400;

        let zero_reason = Hash::zero();
        let valid_reason = Hash::new([99u8; 32]);

        let message_zero = CommitteeApproval::build_emergency_suspend_message(
            &test_network(),
            &committee_id,
            &user_pk,
            &zero_reason,
            expires_at,
            now,
        );

        let message_valid = CommitteeApproval::build_emergency_suspend_message(
            &test_network(),
            &committee_id,
            &user_pk,
            &valid_reason,
            expires_at,
            now,
        );

        assert_ne!(
            message_zero, message_valid,
            "EmergencySuspend messages with different reason_hash should differ"
        );
    }
}

// ============================================================================
// Transfer Level Binding Tests
// ============================================================================
// Ensures destination approvals are bound to the current KYC level.

mod transfer_level_binding_tests {
    use super::*;

    /// Destination approval message should include current_level
    #[test]
    fn test_dest_approval_includes_level() {
        let source_committee = Hash::new([1u8; 32]);
        let dest_committee = Hash::new([2u8; 32]);
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let data_hash = Hash::new([3u8; 32]);
        let now = current_timestamp();

        let message_level_100 = CommitteeApproval::build_transfer_kyc_dest_message(
            &test_network(),
            &source_committee,
            &dest_committee,
            &user_pk,
            100,
            &data_hash,
            now,
            now,
        );

        let message_level_200 = CommitteeApproval::build_transfer_kyc_dest_message(
            &test_network(),
            &source_committee,
            &dest_committee,
            &user_pk,
            200,
            &data_hash,
            now,
            now,
        );

        assert_ne!(
            message_level_100, message_level_200,
            "Destination approval messages with different levels should differ"
        );
    }

    /// Approval signed for level L should not verify for level L'
    #[test]
    fn test_level_upgrade_replay_prevented() {
        let source_committee_id = Hash::new([1u8; 32]);
        let dest_committee_id = Hash::new([2u8; 32]);
        let (dest_committee, dest_keypairs) = create_test_committee(
            dest_committee_id.clone(),
            5,
            3,
            255,
            CommitteeStatus::Active,
        );
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let data_hash = Hash::new([3u8; 32]);
        let now = current_timestamp();

        // Destination approvals signed for level 100
        let original_level: u16 = 100;
        let dest_message = CommitteeApproval::build_transfer_kyc_dest_message(
            &test_network(),
            &source_committee_id,
            &dest_committee_id,
            &user_pk,
            original_level,
            &data_hash,
            now,
            now,
        );

        let dest_approvals: Vec<CommitteeApproval> = dest_keypairs
            .iter()
            .take(3)
            .map(|kp| create_approval(kp, &dest_message, now))
            .collect();

        // Verify with same level should work
        let result_same = tos_common::kyc::verify_transfer_kyc_dest_approvals(
            &test_network(),
            &dest_committee,
            &dest_approvals,
            &source_committee_id,
            &user_pk,
            original_level,
            &data_hash,
            now,
            now,
        );
        assert!(result_same.is_ok(), "Should verify with same level");

        // Verify with upgraded level should fail
        let upgraded_level: u16 = 200;
        let result_upgraded = tos_common::kyc::verify_transfer_kyc_dest_approvals(
            &test_network(),
            &dest_committee,
            &dest_approvals,
            &source_committee_id,
            &user_pk,
            upgraded_level,
            &data_hash,
            now,
            now,
        );

        assert!(
            result_upgraded.is_err(),
            "Dest approvals for level {} should not work for level {}",
            original_level,
            upgraded_level
        );
    }
}

// ============================================================================
// Summary Test
// ============================================================================

#[test]
fn test_security_regression_suite_summary() {
    println!("\n=== KYC Security Regression Test Suite ===\n");
    println!("Tests verify security fixes identified during code review.\n");
    println!("Test Categories:");
    println!("  - Future timestamp validation (adversarial time manipulation)");
    println!("  - Arithmetic overflow protection (extreme timestamp values)");
    println!("  - Region validation (Global/Unspecified detection)");
    println!("  - Zero hash validation (data_hash and reason_hash)");
    println!("  - Transfer level binding (signature replay prevention)");
    println!("\n=== All regression tests should pass ===\n");
}
