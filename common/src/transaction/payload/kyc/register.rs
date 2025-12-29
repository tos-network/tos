// RegisterCommittee Transaction Payload
// Used to create regional committees under parent committee supervision

use crate::{
    crypto::{elgamal::CompressedPublicKey, Hash, Signature},
    kyc::{CommitteeApproval, KycRegion, MemberRole},
    serializer::*,
};
use serde::{Deserialize, Serialize};

/// Initial member for new committee registration
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NewCommitteeMember {
    /// Member's public key
    pub public_key: CompressedPublicKey,
    /// Human-readable name (optional)
    pub name: Option<String>,
    /// Member role
    pub role: MemberRole,
}

impl NewCommitteeMember {
    /// Create new member
    pub fn new(public_key: CompressedPublicKey, name: Option<String>, role: MemberRole) -> Self {
        Self {
            public_key,
            name,
            role,
        }
    }
}

impl Serializer for NewCommitteeMember {
    fn write(&self, writer: &mut Writer) {
        self.public_key.write(writer);
        self.name.write(writer);
        writer.write_u8(self.role as u8);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let public_key = CompressedPublicKey::read(reader)?;
        let name = Option::read(reader)?;
        let role_u8 = reader.read_u8()?;
        let role = MemberRole::from_u8(role_u8).ok_or(ReaderError::InvalidValue)?;

        Ok(Self {
            public_key,
            name,
            role,
        })
    }

    fn size(&self) -> usize {
        self.public_key.size() + self.name.size() + 1
    }
}

/// RegisterCommitteePayload is used to create a regional committee
///
/// # Threshold Requirements
///
/// This transaction requires approval from the **parent committee** based on the
/// parent's governance threshold. The threshold calculation uses `required_threshold()`
/// with `OperationType::RegisterCommittee`.
///
/// ## Approval Rules
///
/// | Parent Committee | Required Threshold | Example |
/// |-----------------|-------------------|---------|
/// | Global (11 members, threshold=8) | 8 approvals | 8/11 (73%) |
/// | Regional (7 members, threshold=5) | 5 approvals | 5/7 (71%) |
///
/// The required threshold is the parent committee's **governance threshold**
/// (the `threshold` field), NOT the `kyc_threshold`. This ensures that creating
/// new committees requires strong consensus from the parent committee.
///
/// ## Verification Flow
///
/// 1. **Stateless verification** (`verify_register_committee`):
///    - Validates at least 1 approval is provided
///    - Validates committee name (non-empty, max 64 chars)
///    - Validates member count (3-15 members)
///    - Validates member roles (at least one Chair)
///    - Validates governance threshold (>= 2/3 of members)
///    - Validates approval timestamps (not expired, not in future)
///
/// 2. **Stateful verification** (`verify_register_committee_approvals`):
///    - Loads parent committee from state
///    - Verifies parent committee is active
///    - Verifies each approver is an active parent committee member
///    - Verifies cryptographic signatures using domain-separated messages
///    - Enforces parent's governance threshold
///
/// ## Security Considerations
///
/// - **Domain separation**: Signatures include operation type prefix
///   (`TOS_REGISTER_COMMITTEE:`) to prevent cross-operation replay attacks
/// - **Timestamp binding**: Each approval includes a timestamp that is verified
///   against the block timestamp to prevent replay attacks
/// - **Member validation**: Only active parent committee members can approve
/// - **Threshold enforcement**: Cannot create a committee with fewer approvals
///   than the parent's governance threshold
///
/// # Other Requirements
///
/// - Parent committee must exist and be active
/// - Parent's max_kyc_level must be >= new committee's max_kyc_level
/// - Regional committees should have 7-11 members (3-15 allowed)
/// - New committee's threshold must be >= 2/3 of its member count
///
/// # Gas Cost
///
/// 80,000 gas
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RegisterCommitteePayload {
    /// Committee name
    name: String,

    /// Region this committee covers
    region: KycRegion,

    /// Initial members (7-11 recommended for regional committees)
    members: Vec<NewCommitteeMember>,

    /// Governance threshold (>= 2/3 of members)
    threshold: u8,

    /// KYC approval threshold (default: 1)
    kyc_threshold: u8,

    /// Maximum KYC level this committee can approve
    /// Must be <= parent's max_kyc_level
    /// Regional committees: typically 8191 (Tier 6)
    max_kyc_level: u16,

    /// Parent committee ID (must be Global Committee or valid regional)
    parent_id: Hash,

    /// Parent committee approvals
    approvals: Vec<CommitteeApproval>,
}

impl RegisterCommitteePayload {
    /// Create new RegisterCommittee payload
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        region: KycRegion,
        members: Vec<NewCommitteeMember>,
        threshold: u8,
        kyc_threshold: u8,
        max_kyc_level: u16,
        parent_id: Hash,
        approvals: Vec<CommitteeApproval>,
    ) -> Self {
        Self {
            name,
            region,
            members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            parent_id,
            approvals,
        }
    }

    /// Get committee name
    #[inline]
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Get region
    #[inline]
    pub fn get_region(&self) -> KycRegion {
        self.region
    }

    /// Get initial members
    #[inline]
    pub fn get_members(&self) -> &[NewCommitteeMember] {
        &self.members
    }

    /// Get governance threshold
    #[inline]
    pub fn get_threshold(&self) -> u8 {
        self.threshold
    }

    /// Get KYC threshold
    #[inline]
    pub fn get_kyc_threshold(&self) -> u8 {
        self.kyc_threshold
    }

    /// Get maximum KYC level
    #[inline]
    pub fn get_max_kyc_level(&self) -> u16 {
        self.max_kyc_level
    }

    /// Get parent committee ID
    #[inline]
    pub fn get_parent_id(&self) -> &Hash {
        &self.parent_id
    }

    /// Get approvals
    #[inline]
    pub fn get_approvals(&self) -> &[CommitteeApproval] {
        &self.approvals
    }

    /// Consume and return inner values
    #[allow(clippy::type_complexity)]
    pub fn consume(
        self,
    ) -> (
        String,
        KycRegion,
        Vec<NewCommitteeMember>,
        u8,
        u8,
        u16,
        Hash,
        Vec<CommitteeApproval>,
    ) {
        (
            self.name,
            self.region,
            self.members,
            self.threshold,
            self.kyc_threshold,
            self.max_kyc_level,
            self.parent_id,
            self.approvals,
        )
    }
}

impl Serializer for RegisterCommitteePayload {
    fn write(&self, writer: &mut Writer) {
        self.name.write(writer);
        writer.write_u8(self.region as u8);
        // Write members
        writer.write_u8(self.members.len() as u8);
        for member in &self.members {
            member.write(writer);
        }
        writer.write_u8(self.threshold);
        writer.write_u8(self.kyc_threshold);
        writer.write_u16(self.max_kyc_level);
        self.parent_id.write(writer);
        // Write approvals
        writer.write_u8(self.approvals.len() as u8);
        for approval in &self.approvals {
            approval.member_pubkey.write(writer);
            approval.signature.write(writer);
            writer.write_u64(&approval.timestamp);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let name = String::read(reader)?;
        let region_u8 = reader.read_u8()?;
        let region = KycRegion::from_u8(region_u8).ok_or(ReaderError::InvalidValue)?;

        let member_count = reader.read_u8()? as usize;
        let mut members = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            members.push(NewCommitteeMember::read(reader)?);
        }

        let threshold = reader.read_u8()?;
        let kyc_threshold = reader.read_u8()?;
        let max_kyc_level = reader.read_u16()?;
        let parent_id = Hash::read(reader)?;

        let approval_count = reader.read_u8()? as usize;
        let mut approvals = Vec::with_capacity(approval_count);
        for _ in 0..approval_count {
            let member_pubkey = CompressedPublicKey::read(reader)?;
            let signature = Signature::read(reader)?;
            let timestamp = reader.read_u64()?;
            approvals.push(CommitteeApproval::new(member_pubkey, signature, timestamp));
        }

        Ok(Self {
            name,
            region,
            members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            parent_id,
            approvals,
        })
    }

    fn size(&self) -> usize {
        self.name.size()
            + 1 // region
            + 1 // member count
            + self.members.iter().map(|m| m.size()).sum::<usize>()
            + 1 // threshold
            + 1 // kyc_threshold
            + 2 // max_kyc_level
            + self.parent_id.size()
            + 1 // approval count
            + self.approvals.iter().map(|a| {
                a.member_pubkey.size() + 64 + 8
            }).sum::<usize>()
    }
}
