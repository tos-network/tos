// Security Committee structures
// Defines the multi-sig committee governance for KYC operations
//
// Hierarchy:
// - Global Committee (11-15 members, max Tier 8)
//   - Regional Committees (7-11 members, max Tier 6)
//
// Reference: TOS-KYC-Level-Design.md Section 4

use crate::crypto::{Hash, PublicKey, Signature};
use crate::kyc::{KycError, KycRegion, KycResult};
use crate::serializer::{Reader, ReaderError, Serializer, Writer};
use serde::{Deserialize, Serialize};

/// Minimum number of active members required for a committee
pub const MIN_COMMITTEE_MEMBERS: usize = 3;

/// Default KYC approval threshold
pub const DEFAULT_KYC_THRESHOLD: u8 = 1;

/// Emergency suspension timeout in seconds (24 hours)
pub const EMERGENCY_SUSPENSION_TIMEOUT: u64 = 24 * 3600;

/// Approval expiry time in seconds (24 hours)
pub const APPROVAL_EXPIRY_SECONDS: u64 = 24 * 3600;

/// Security committee definition
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SecurityCommittee {
    /// Committee ID
    /// committee_id = blake3("TOS_COMMITTEE" || region || name || version)
    pub id: Hash,

    /// Committee region (NOT country - privacy protection)
    pub region: KycRegion,

    /// Committee name
    pub name: String,

    /// Committee members
    pub members: Vec<CommitteeMember>,

    /// Governance threshold (M-of-N for major operations)
    /// Must be >= 2/3 of active members
    pub threshold: u8,

    /// KYC approval threshold (configurable, default: 1)
    /// - SetKyc (Tier 0-4): kyc_threshold approvals
    /// - SetKyc (Tier 5+): kyc_threshold + 1 approvals
    ///
    /// Can be updated via UpdateKycThreshold (requires >= 2/3)
    pub kyc_threshold: u8,

    /// Maximum KYC level this committee can grant
    pub max_kyc_level: u16,

    /// Committee status
    pub status: CommitteeStatus,

    /// Parent committee ID (None for Global Committee)
    pub parent_id: Option<Hash>,

    /// Creation timestamp
    pub created_at: u64,

    /// Last update timestamp
    pub updated_at: u64,
}

impl SecurityCommittee {
    /// Create a new committee
    pub fn new(
        id: Hash,
        region: KycRegion,
        name: String,
        members: Vec<CommitteeMember>,
        threshold: u8,
        max_kyc_level: u16,
        parent_id: Option<Hash>,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            region,
            name,
            members,
            threshold,
            kyc_threshold: DEFAULT_KYC_THRESHOLD,
            max_kyc_level,
            status: CommitteeStatus::Active,
            parent_id,
            created_at,
            updated_at: created_at,
        }
    }

    /// Calculate required threshold for an operation
    pub fn required_threshold(&self, operation: &OperationType, tier: Option<u8>) -> u8 {
        match operation {
            // KYC operations: use kyc_threshold
            OperationType::SetKyc => match tier {
                Some(t) if t >= 5 => self.kyc_threshold.saturating_add(1),
                _ => self.kyc_threshold,
            },
            OperationType::RevokeKyc | OperationType::RenewKyc => self.kyc_threshold,

            // Transfer KYC: requires approval from both committees
            OperationType::TransferKyc => self.kyc_threshold,

            // Appeal: handled by parent committee (uses parent's threshold)
            OperationType::AppealKyc => self.threshold,

            // Emergency operations (fixed thresholds)
            OperationType::EmergencySuspend => 2,
            OperationType::EmergencyRemoveMember => 3,

            // Governance operations: >= 2/3 (use threshold)
            OperationType::AddMember
            | OperationType::RemoveMember
            | OperationType::UpdateThreshold
            | OperationType::UpdateKycThreshold
            | OperationType::UpdateRole
            | OperationType::Suspend
            | OperationType::Resume
            | OperationType::Dissolve
            | OperationType::RegisterCommittee => self.threshold,
        }
    }

    /// Check if member can approve KYC
    pub fn can_approve_kyc(&self, member_pubkey: &PublicKey) -> bool {
        self.members.iter().any(|m| {
            &m.public_key == member_pubkey
                && m.status == MemberStatus::Active
                && m.role != MemberRole::Observer
        })
    }

    /// Get active member count
    pub fn active_member_count(&self) -> usize {
        self.members
            .iter()
            .filter(|m| m.status == MemberStatus::Active)
            .count()
    }

    /// Get member by public key
    pub fn get_member(&self, pubkey: &PublicKey) -> Option<&CommitteeMember> {
        self.members.iter().find(|m| &m.public_key == pubkey)
    }

    /// Get mutable member by public key
    pub fn get_member_mut(&mut self, pubkey: &PublicKey) -> Option<&mut CommitteeMember> {
        self.members.iter_mut().find(|m| &m.public_key == pubkey)
    }

    /// Validate committee configuration
    pub fn validate(&self) -> KycResult<()> {
        let active_count = self.active_member_count();

        // Minimum 3 active members
        if active_count < MIN_COMMITTEE_MEMBERS {
            return Err(KycError::InsufficientMembers {
                required: MIN_COMMITTEE_MEMBERS,
                active: active_count,
            });
        }

        // Governance threshold must be >= 2/3 of active members
        let min_threshold = Self::calculate_min_threshold(active_count);
        if (self.threshold as usize) < min_threshold {
            return Err(KycError::InvalidThreshold);
        }

        // KYC threshold must be >= 1
        if self.kyc_threshold < 1 {
            return Err(KycError::InvalidKycThreshold);
        }

        Ok(())
    }

    /// Calculate minimum threshold (ceiling of 2/3)
    fn calculate_min_threshold(member_count: usize) -> usize {
        (member_count * 2).div_ceil(3)
    }

    /// Check if committee is active
    #[inline]
    pub fn is_active(&self) -> bool {
        self.status == CommitteeStatus::Active
    }

    /// Check if this is the global committee
    #[inline]
    pub fn is_global(&self) -> bool {
        self.region.is_global() && self.parent_id.is_none()
    }

    /// Check if this committee can manage a child region
    pub fn can_manage_region(&self, child_region: &KycRegion) -> bool {
        self.region.is_global() || &self.region == child_region
    }

    /// Check if level is within committee's allowed range
    #[inline]
    pub fn can_grant_level(&self, level: u16) -> bool {
        level <= self.max_kyc_level
    }

    /// Update last activity timestamp
    pub fn touch(&mut self, timestamp: u64) {
        self.updated_at = timestamp;
    }

    /// Add a new member to the committee
    pub fn add_member(&mut self, public_key: PublicKey, name: Option<String>, role: MemberRole) {
        let now = self.updated_at;
        let member = CommitteeMember::new(public_key, name, role, now);
        self.members.push(member);
    }

    /// Remove a member from the committee by public key
    /// Returns true if the member was found and removed
    pub fn remove_member(&mut self, pubkey: &PublicKey) -> bool {
        let initial_len = self.members.len();
        self.members.retain(|m| &m.public_key != pubkey);
        self.members.len() < initial_len
    }
}

/// Committee member definition
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CommitteeMember {
    /// Member's public key (primary identifier)
    pub public_key: PublicKey,

    /// Human-readable name (optional)
    pub name: Option<String>,

    /// Member role
    pub role: MemberRole,

    /// Member status
    pub status: MemberStatus,

    /// Join timestamp
    pub joined_at: u64,

    /// Last activity timestamp
    pub last_active_at: u64,
}

impl CommitteeMember {
    /// Create new committee member
    pub fn new(
        public_key: PublicKey,
        name: Option<String>,
        role: MemberRole,
        joined_at: u64,
    ) -> Self {
        Self {
            public_key,
            name,
            role,
            status: MemberStatus::Active,
            joined_at,
            last_active_at: joined_at,
        }
    }

    /// Check if member can vote/approve
    #[inline]
    pub fn can_vote(&self) -> bool {
        self.status == MemberStatus::Active && self.role != MemberRole::Observer
    }

    /// Update last activity
    pub fn touch(&mut self, timestamp: u64) {
        self.last_active_at = timestamp;
    }
}

/// Member roles
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MemberRole {
    /// Chairman - can initiate governance votes
    Chair = 0,
    /// Vice chairman
    ViceChair = 1,
    /// Regular member
    Member = 2,
    /// Observer - no approval or voting rights
    Observer = 3,
}

impl MemberRole {
    /// Get role name
    pub fn as_str(&self) -> &'static str {
        match self {
            MemberRole::Chair => "Chair",
            MemberRole::ViceChair => "Vice Chair",
            MemberRole::Member => "Member",
            MemberRole::Observer => "Observer",
        }
    }

    /// Check if role can approve KYC
    #[inline]
    pub fn can_approve(&self) -> bool {
        !matches!(self, MemberRole::Observer)
    }

    /// Convert from u8
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(MemberRole::Chair),
            1 => Some(MemberRole::ViceChair),
            2 => Some(MemberRole::Member),
            3 => Some(MemberRole::Observer),
            _ => None,
        }
    }
}

/// Member status
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MemberStatus {
    Active = 0,
    Suspended = 1,
    Removed = 2,
}

impl MemberStatus {
    /// Get status name
    pub fn as_str(&self) -> &'static str {
        match self {
            MemberStatus::Active => "Active",
            MemberStatus::Suspended => "Suspended",
            MemberStatus::Removed => "Removed",
        }
    }

    /// Convert from u8
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(MemberStatus::Active),
            1 => Some(MemberStatus::Suspended),
            2 => Some(MemberStatus::Removed),
            _ => None,
        }
    }
}

/// Committee status
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CommitteeStatus {
    Active = 0,
    Suspended = 1,
    Dissolved = 2,
}

impl CommitteeStatus {
    /// Get status name
    pub fn as_str(&self) -> &'static str {
        match self {
            CommitteeStatus::Active => "Active",
            CommitteeStatus::Suspended => "Suspended",
            CommitteeStatus::Dissolved => "Dissolved",
        }
    }

    /// Convert from u8
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(CommitteeStatus::Active),
            1 => Some(CommitteeStatus::Suspended),
            2 => Some(CommitteeStatus::Dissolved),
            _ => None,
        }
    }
}

/// Operation types for threshold calculation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationType {
    // KYC Operations (kyc_threshold)
    SetKyc,
    RevokeKyc,
    RenewKyc,
    TransferKyc,

    // Appeal Operations (parent committee)
    AppealKyc,

    // Emergency Operations (fixed threshold)
    EmergencySuspend,      // 2 members
    EmergencyRemoveMember, // 3 members

    // Governance Operations (>= 2/3)
    AddMember,
    RemoveMember,
    UpdateThreshold,
    UpdateKycThreshold,
    UpdateRole,
    Suspend,
    Resume,
    Dissolve,
    RegisterCommittee,
}

impl OperationType {
    /// Get operation name
    pub fn as_str(&self) -> &'static str {
        match self {
            OperationType::SetKyc => "SetKyc",
            OperationType::RevokeKyc => "RevokeKyc",
            OperationType::RenewKyc => "RenewKyc",
            OperationType::TransferKyc => "TransferKyc",
            OperationType::AppealKyc => "AppealKyc",
            OperationType::EmergencySuspend => "EmergencySuspend",
            OperationType::EmergencyRemoveMember => "EmergencyRemoveMember",
            OperationType::AddMember => "AddMember",
            OperationType::RemoveMember => "RemoveMember",
            OperationType::UpdateThreshold => "UpdateThreshold",
            OperationType::UpdateKycThreshold => "UpdateKycThreshold",
            OperationType::UpdateRole => "UpdateRole",
            OperationType::Suspend => "Suspend",
            OperationType::Resume => "Resume",
            OperationType::Dissolve => "Dissolve",
            OperationType::RegisterCommittee => "RegisterCommittee",
        }
    }
}

/// Committee approval record
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CommitteeApproval {
    /// Approving member's public key
    pub member_pubkey: PublicKey,

    /// Signature over the approval message
    pub signature: Signature,

    /// Approval timestamp
    pub timestamp: u64,
}

impl CommitteeApproval {
    /// Create new approval
    pub fn new(member_pubkey: PublicKey, signature: Signature, timestamp: u64) -> Self {
        Self {
            member_pubkey,
            signature,
            timestamp,
        }
    }

    /// Check if approval has expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        current_time.saturating_sub(self.timestamp) > APPROVAL_EXPIRY_SECONDS
    }
}

// ===== Serializer Implementations =====

impl Serializer for MemberRole {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let value = u8::read(reader)?;
        MemberRole::from_u8(value).ok_or(ReaderError::InvalidValue)
    }

    fn write(&self, writer: &mut Writer) {
        (*self as u8).write(writer);
    }

    fn size(&self) -> usize {
        1
    }
}

impl Serializer for MemberStatus {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let value = u8::read(reader)?;
        MemberStatus::from_u8(value).ok_or(ReaderError::InvalidValue)
    }

    fn write(&self, writer: &mut Writer) {
        (*self as u8).write(writer);
    }

    fn size(&self) -> usize {
        1
    }
}

impl Serializer for CommitteeStatus {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let value = u8::read(reader)?;
        CommitteeStatus::from_u8(value).ok_or(ReaderError::InvalidValue)
    }

    fn write(&self, writer: &mut Writer) {
        (*self as u8).write(writer);
    }

    fn size(&self) -> usize {
        1
    }
}

impl Serializer for CommitteeMember {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let public_key = PublicKey::read(reader)?;
        let name = Option::<String>::read(reader)?;
        let role = MemberRole::read(reader)?;
        let status = MemberStatus::read(reader)?;
        let joined_at = u64::read(reader)?;
        let last_active_at = u64::read(reader)?;

        Ok(Self {
            public_key,
            name,
            role,
            status,
            joined_at,
            last_active_at,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.public_key.write(writer);
        self.name.write(writer);
        self.role.write(writer);
        self.status.write(writer);
        self.joined_at.write(writer);
        self.last_active_at.write(writer);
    }

    fn size(&self) -> usize {
        self.public_key.size()
            + self.name.size()
            + self.role.size()
            + self.status.size()
            + self.joined_at.size()
            + self.last_active_at.size()
    }
}

impl Serializer for SecurityCommittee {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let id = Hash::read(reader)?;
        let region = KycRegion::read(reader)?;
        let name = String::read(reader)?;
        let members = Vec::<CommitteeMember>::read(reader)?;
        let threshold = u8::read(reader)?;
        let kyc_threshold = u8::read(reader)?;
        let max_kyc_level = u16::read(reader)?;
        let status = CommitteeStatus::read(reader)?;
        let parent_id = Option::<Hash>::read(reader)?;
        let created_at = u64::read(reader)?;
        let updated_at = u64::read(reader)?;

        Ok(Self {
            id,
            region,
            name,
            members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            status,
            parent_id,
            created_at,
            updated_at,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.id.write(writer);
        self.region.write(writer);
        self.name.write(writer);
        self.members.write(writer);
        self.threshold.write(writer);
        self.kyc_threshold.write(writer);
        self.max_kyc_level.write(writer);
        self.status.write(writer);
        self.parent_id.write(writer);
        self.created_at.write(writer);
        self.updated_at.write(writer);
    }

    fn size(&self) -> usize {
        self.id.size()
            + self.region.size()
            + self.name.size()
            + self.members.size()
            + self.threshold.size()
            + self.kyc_threshold.size()
            + self.max_kyc_level.size()
            + self.status.size()
            + self.parent_id.size()
            + self.created_at.size()
            + self.updated_at.size()
    }
}

impl Serializer for CommitteeApproval {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let member_pubkey = PublicKey::read(reader)?;
        let signature = Signature::read(reader)?;
        let timestamp = u64::read(reader)?;

        Ok(Self {
            member_pubkey,
            signature,
            timestamp,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.member_pubkey.write(writer);
        self.signature.write(writer);
        self.timestamp.write(writer);
    }

    fn size(&self) -> usize {
        self.member_pubkey.size() + self.signature.size() + self.timestamp.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::ristretto::CompressedRistretto;
    use curve25519_dalek::scalar::Scalar;

    fn create_test_pubkey(seed: u8) -> PublicKey {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        // Create a valid compressed ristretto point
        PublicKey::new(CompressedRistretto::from_slice(&bytes).expect("Valid bytes"))
    }

    fn sample_member(seed: u8, role: MemberRole) -> CommitteeMember {
        CommitteeMember::new(
            create_test_pubkey(seed),
            Some(format!("Member {}", seed)),
            role,
            1000,
        )
    }

    fn sample_committee() -> SecurityCommittee {
        let members = vec![
            sample_member(1, MemberRole::Chair),
            sample_member(2, MemberRole::ViceChair),
            sample_member(3, MemberRole::Member),
            sample_member(4, MemberRole::Member),
            sample_member(5, MemberRole::Member),
        ];

        SecurityCommittee::new(
            Hash::zero(),
            KycRegion::AsiaPacific,
            "Test Committee".to_string(),
            members,
            4,                  // 4/5 threshold
            8191,               // Tier 6 max
            Some(Hash::zero()), // Has parent
            1000,
        )
    }

    fn create_test_signature() -> Signature {
        Signature::new(Scalar::ZERO, Scalar::ZERO)
    }

    #[test]
    fn test_committee_validation() {
        let committee = sample_committee();
        assert!(committee.validate().is_ok());
        assert_eq!(committee.active_member_count(), 5);
    }

    #[test]
    fn test_required_threshold() {
        let committee = sample_committee();

        // KYC operations use kyc_threshold (default 1)
        assert_eq!(
            committee.required_threshold(&OperationType::SetKyc, Some(2)),
            1
        );

        // Tier 5+ requires kyc_threshold + 1
        assert_eq!(
            committee.required_threshold(&OperationType::SetKyc, Some(5)),
            2
        );

        // Governance uses main threshold
        assert_eq!(
            committee.required_threshold(&OperationType::AddMember, None),
            4
        );

        // Emergency operations have fixed thresholds
        assert_eq!(
            committee.required_threshold(&OperationType::EmergencySuspend, None),
            2
        );
    }

    #[test]
    fn test_min_threshold_calculation() {
        // 2/3 ceiling calculation
        assert_eq!(SecurityCommittee::calculate_min_threshold(3), 2);
        assert_eq!(SecurityCommittee::calculate_min_threshold(5), 4);
        assert_eq!(SecurityCommittee::calculate_min_threshold(7), 5);
        assert_eq!(SecurityCommittee::calculate_min_threshold(10), 7);
        assert_eq!(SecurityCommittee::calculate_min_threshold(11), 8);
        assert_eq!(SecurityCommittee::calculate_min_threshold(15), 10);
    }

    #[test]
    fn test_invalid_committee() {
        // Too few members
        let mut committee = sample_committee();
        committee.members = vec![sample_member(1, MemberRole::Chair)];
        assert!(matches!(
            committee.validate(),
            Err(KycError::InsufficientMembers { .. })
        ));

        // Threshold too low
        let mut committee = sample_committee();
        committee.threshold = 1; // Less than 2/3 of 5
        assert!(matches!(
            committee.validate(),
            Err(KycError::InvalidThreshold)
        ));
    }

    #[test]
    fn test_can_approve_kyc() {
        let committee = sample_committee();

        // Active members can approve
        assert!(committee.can_approve_kyc(&create_test_pubkey(1)));
        assert!(committee.can_approve_kyc(&create_test_pubkey(2)));

        // Non-existent member cannot approve
        assert!(!committee.can_approve_kyc(&create_test_pubkey(99)));
    }

    #[test]
    fn test_member_role() {
        assert!(MemberRole::Chair.can_approve());
        assert!(MemberRole::ViceChair.can_approve());
        assert!(MemberRole::Member.can_approve());
        assert!(!MemberRole::Observer.can_approve());
    }

    #[test]
    fn test_approval_expiry() {
        let approval = CommitteeApproval::new(create_test_pubkey(1), create_test_signature(), 1000);

        // Not expired within 24 hours
        assert!(!approval.is_expired(1000 + APPROVAL_EXPIRY_SECONDS - 1));

        // Expired after 24 hours
        assert!(approval.is_expired(1000 + APPROVAL_EXPIRY_SECONDS + 1));
    }
}
