// BootstrapCommittee Transaction Payload
// Used to create the Global Committee (one-time operation)

use crate::{crypto::elgamal::CompressedPublicKey, kyc::MemberRole, serializer::*};
use serde::{Deserialize, Serialize};

/// Initial committee member for bootstrap
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CommitteeMemberInit {
    /// Member's public key
    pub public_key: CompressedPublicKey,
    /// Human-readable name (optional)
    pub name: Option<String>,
    /// Member role
    pub role: MemberRole,
}

impl CommitteeMemberInit {
    /// Create new member init
    pub fn new(public_key: CompressedPublicKey, name: Option<String>, role: MemberRole) -> Self {
        Self {
            public_key,
            name,
            role,
        }
    }
}

impl Serializer for CommitteeMemberInit {
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

/// BootstrapCommitteePayload is used to create the Global Committee
///
/// This is a one-time operation that can only be executed by BOOTSTRAP_ADDRESS
/// during chain initialization.
///
/// Requirements:
/// - Must be sent from BOOTSTRAP_ADDRESS
/// - Global Committee must not already exist
/// - Members should be 11-15 for Global Committee
/// - Threshold must be >= 2/3 of members
///
/// Gas cost: 100,000 gas
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BootstrapCommitteePayload {
    /// Committee name
    name: String,

    /// Initial members (11-15 recommended for Global Committee)
    members: Vec<CommitteeMemberInit>,

    /// Governance threshold (>= 2/3 of members)
    threshold: u8,

    /// KYC approval threshold (default: 1)
    kyc_threshold: u8,

    /// Maximum KYC level (Global Committee: 32767 = Tier 8)
    max_kyc_level: u16,
}

impl BootstrapCommitteePayload {
    /// Create new BootstrapCommittee payload
    pub fn new(
        name: String,
        members: Vec<CommitteeMemberInit>,
        threshold: u8,
        kyc_threshold: u8,
        max_kyc_level: u16,
    ) -> Self {
        Self {
            name,
            members,
            threshold,
            kyc_threshold,
            max_kyc_level,
        }
    }

    /// Get committee name
    #[inline]
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Get initial members
    #[inline]
    pub fn get_members(&self) -> &[CommitteeMemberInit] {
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

    /// Consume and return inner values
    pub fn consume(self) -> (String, Vec<CommitteeMemberInit>, u8, u8, u16) {
        (
            self.name,
            self.members,
            self.threshold,
            self.kyc_threshold,
            self.max_kyc_level,
        )
    }
}

impl Serializer for BootstrapCommitteePayload {
    fn write(&self, writer: &mut Writer) {
        self.name.write(writer);
        // Write members as vector
        writer.write_u8(self.members.len() as u8);
        for member in &self.members {
            member.write(writer);
        }
        writer.write_u8(self.threshold);
        writer.write_u8(self.kyc_threshold);
        writer.write_u16(self.max_kyc_level);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let name = String::read(reader)?;

        let member_count = reader.read_u8()? as usize;
        let mut members = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            members.push(CommitteeMemberInit::read(reader)?);
        }

        let threshold = reader.read_u8()?;
        let kyc_threshold = reader.read_u8()?;
        let max_kyc_level = reader.read_u16()?;

        Ok(Self {
            name,
            members,
            threshold,
            kyc_threshold,
            max_kyc_level,
        })
    }

    fn size(&self) -> usize {
        self.name.size()
            + 1  // member count
            + self.members.iter().map(|m| m.size()).sum::<usize>()
            + 1  // threshold
            + 1  // kyc_threshold
            + 2 // max_kyc_level
    }
}
