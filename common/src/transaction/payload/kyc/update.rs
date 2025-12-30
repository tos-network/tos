// UpdateCommittee Transaction Payload
// Used to modify committee configuration

use crate::{
    crypto::{elgamal::CompressedPublicKey, Hash, Signature},
    kyc::{CommitteeApproval, MemberRole, MemberStatus},
    serializer::*,
};
use serde::{Deserialize, Serialize};

/// Types of committee updates
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CommitteeUpdateType {
    /// Add a new member
    AddMember = 0,
    /// Remove a member
    RemoveMember = 1,
    /// Update member role
    UpdateMemberRole = 2,
    /// Update member status
    UpdateMemberStatus = 3,
    /// Update governance threshold
    UpdateThreshold = 4,
    /// Update KYC threshold
    UpdateKycThreshold = 5,
    /// Update committee name
    UpdateName = 6,
    /// Suspend committee
    SuspendCommittee = 7,
    /// Activate committee
    ActivateCommittee = 8,
}

impl CommitteeUpdateType {
    /// Convert from u8
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::AddMember),
            1 => Some(Self::RemoveMember),
            2 => Some(Self::UpdateMemberRole),
            3 => Some(Self::UpdateMemberStatus),
            4 => Some(Self::UpdateThreshold),
            5 => Some(Self::UpdateKycThreshold),
            6 => Some(Self::UpdateName),
            7 => Some(Self::SuspendCommittee),
            8 => Some(Self::ActivateCommittee),
            _ => None,
        }
    }
}

/// Update data for committee changes
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum CommitteeUpdateData {
    /// Add new member (public_key, name, role)
    AddMember {
        public_key: CompressedPublicKey,
        name: Option<String>,
        role: MemberRole,
    },
    /// Remove member by public key
    RemoveMember { public_key: CompressedPublicKey },
    /// Update member role
    UpdateMemberRole {
        public_key: CompressedPublicKey,
        new_role: MemberRole,
    },
    /// Update member status
    UpdateMemberStatus {
        public_key: CompressedPublicKey,
        new_status: MemberStatus,
    },
    /// Update governance threshold
    UpdateThreshold { new_threshold: u8 },
    /// Update KYC approval threshold
    UpdateKycThreshold { new_kyc_threshold: u8 },
    /// Update committee name
    UpdateName { new_name: String },
    /// Suspend committee (no data needed)
    SuspendCommittee,
    /// Activate committee (no data needed)
    ActivateCommittee,
}

impl Serializer for CommitteeUpdateData {
    fn write(&self, writer: &mut Writer) {
        match self {
            CommitteeUpdateData::AddMember {
                public_key,
                name,
                role,
            } => {
                writer.write_u8(CommitteeUpdateType::AddMember as u8);
                public_key.write(writer);
                name.write(writer);
                writer.write_u8(*role as u8);
            }
            CommitteeUpdateData::RemoveMember { public_key } => {
                writer.write_u8(CommitteeUpdateType::RemoveMember as u8);
                public_key.write(writer);
            }
            CommitteeUpdateData::UpdateMemberRole {
                public_key,
                new_role,
            } => {
                writer.write_u8(CommitteeUpdateType::UpdateMemberRole as u8);
                public_key.write(writer);
                writer.write_u8(*new_role as u8);
            }
            CommitteeUpdateData::UpdateMemberStatus {
                public_key,
                new_status,
            } => {
                writer.write_u8(CommitteeUpdateType::UpdateMemberStatus as u8);
                public_key.write(writer);
                writer.write_u8(*new_status as u8);
            }
            CommitteeUpdateData::UpdateThreshold { new_threshold } => {
                writer.write_u8(CommitteeUpdateType::UpdateThreshold as u8);
                writer.write_u8(*new_threshold);
            }
            CommitteeUpdateData::UpdateKycThreshold { new_kyc_threshold } => {
                writer.write_u8(CommitteeUpdateType::UpdateKycThreshold as u8);
                writer.write_u8(*new_kyc_threshold);
            }
            CommitteeUpdateData::UpdateName { new_name } => {
                writer.write_u8(CommitteeUpdateType::UpdateName as u8);
                new_name.write(writer);
            }
            CommitteeUpdateData::SuspendCommittee => {
                writer.write_u8(CommitteeUpdateType::SuspendCommittee as u8);
            }
            CommitteeUpdateData::ActivateCommittee => {
                writer.write_u8(CommitteeUpdateType::ActivateCommittee as u8);
            }
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let update_type =
            CommitteeUpdateType::from_u8(reader.read_u8()?).ok_or(ReaderError::InvalidValue)?;

        match update_type {
            CommitteeUpdateType::AddMember => {
                let public_key = CompressedPublicKey::read(reader)?;
                let name = Option::read(reader)?;
                let role =
                    MemberRole::from_u8(reader.read_u8()?).ok_or(ReaderError::InvalidValue)?;
                Ok(CommitteeUpdateData::AddMember {
                    public_key,
                    name,
                    role,
                })
            }
            CommitteeUpdateType::RemoveMember => {
                let public_key = CompressedPublicKey::read(reader)?;
                Ok(CommitteeUpdateData::RemoveMember { public_key })
            }
            CommitteeUpdateType::UpdateMemberRole => {
                let public_key = CompressedPublicKey::read(reader)?;
                let new_role =
                    MemberRole::from_u8(reader.read_u8()?).ok_or(ReaderError::InvalidValue)?;
                Ok(CommitteeUpdateData::UpdateMemberRole {
                    public_key,
                    new_role,
                })
            }
            CommitteeUpdateType::UpdateMemberStatus => {
                let public_key = CompressedPublicKey::read(reader)?;
                let new_status =
                    MemberStatus::from_u8(reader.read_u8()?).ok_or(ReaderError::InvalidValue)?;
                Ok(CommitteeUpdateData::UpdateMemberStatus {
                    public_key,
                    new_status,
                })
            }
            CommitteeUpdateType::UpdateThreshold => {
                let new_threshold = reader.read_u8()?;
                Ok(CommitteeUpdateData::UpdateThreshold { new_threshold })
            }
            CommitteeUpdateType::UpdateKycThreshold => {
                let new_kyc_threshold = reader.read_u8()?;
                Ok(CommitteeUpdateData::UpdateKycThreshold { new_kyc_threshold })
            }
            CommitteeUpdateType::UpdateName => {
                let new_name = String::read(reader)?;
                Ok(CommitteeUpdateData::UpdateName { new_name })
            }
            CommitteeUpdateType::SuspendCommittee => Ok(CommitteeUpdateData::SuspendCommittee),
            CommitteeUpdateType::ActivateCommittee => Ok(CommitteeUpdateData::ActivateCommittee),
        }
    }

    fn size(&self) -> usize {
        1 + match self {
            CommitteeUpdateData::AddMember {
                public_key, name, ..
            } => public_key.size() + name.size() + 1,
            CommitteeUpdateData::RemoveMember { public_key } => public_key.size(),
            CommitteeUpdateData::UpdateMemberRole { public_key, .. } => public_key.size() + 1,
            CommitteeUpdateData::UpdateMemberStatus { public_key, .. } => public_key.size() + 1,
            CommitteeUpdateData::UpdateThreshold { .. } => 1,
            CommitteeUpdateData::UpdateKycThreshold { .. } => 1,
            CommitteeUpdateData::UpdateName { new_name } => new_name.size(),
            CommitteeUpdateData::SuspendCommittee => 0,
            CommitteeUpdateData::ActivateCommittee => 0,
        }
    }
}

/// UpdateCommitteePayload is used to modify committee configuration
///
/// This transaction requires threshold approvals from the committee
///
/// Gas cost: 40,000 gas
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UpdateCommitteePayload {
    /// Committee ID to update
    committee_id: Hash,

    /// Update data
    update: CommitteeUpdateData,

    /// Approver signatures (>= threshold)
    approvals: Vec<CommitteeApproval>,
}

impl UpdateCommitteePayload {
    /// Create new UpdateCommittee payload
    pub fn new(
        committee_id: Hash,
        update: CommitteeUpdateData,
        approvals: Vec<CommitteeApproval>,
    ) -> Self {
        Self {
            committee_id,
            update,
            approvals,
        }
    }

    /// Get committee ID
    #[inline]
    pub fn get_committee_id(&self) -> &Hash {
        &self.committee_id
    }

    /// Get update data
    #[inline]
    pub fn get_update(&self) -> &CommitteeUpdateData {
        &self.update
    }

    /// Get approvals
    #[inline]
    pub fn get_approvals(&self) -> &[CommitteeApproval] {
        &self.approvals
    }

    /// Consume and return inner values
    pub fn consume(self) -> (Hash, CommitteeUpdateData, Vec<CommitteeApproval>) {
        (self.committee_id, self.update, self.approvals)
    }
}

impl Serializer for UpdateCommitteePayload {
    fn write(&self, writer: &mut Writer) {
        self.committee_id.write(writer);
        self.update.write(writer);
        writer.write_u8(self.approvals.len() as u8);
        for approval in &self.approvals {
            approval.member_pubkey.write(writer);
            approval.signature.write(writer);
            writer.write_u64(&approval.timestamp);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let committee_id = Hash::read(reader)?;
        let update = CommitteeUpdateData::read(reader)?;

        let approval_count = reader.read_u8()? as usize;
        let mut approvals = Vec::with_capacity(approval_count);
        for _ in 0..approval_count {
            let member_pubkey = CompressedPublicKey::read(reader)?;
            let signature = Signature::read(reader)?;
            let timestamp = reader.read_u64()?;
            approvals.push(CommitteeApproval::new(member_pubkey, signature, timestamp));
        }

        Ok(Self {
            committee_id,
            update,
            approvals,
        })
    }

    fn size(&self) -> usize {
        self.committee_id.size()
            + self.update.size()
            + 1
            + self
                .approvals
                .iter()
                .map(|a| a.member_pubkey.size() + 64 + 8)
                .sum::<usize>()
    }
}
