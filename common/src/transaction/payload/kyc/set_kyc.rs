// SetKyc Transaction Payload
// Used to set or update a user's KYC level

use crate::{
    crypto::{elgamal::CompressedPublicKey, Hash, Signature},
    kyc::CommitteeApproval,
    serializer::*,
};
use serde::{Deserialize, Serialize};

/// SetKycPayload is used to set a user's KYC level
///
/// This transaction requires committee approval:
/// - Tier 0-4: kyc_threshold approvals (default: 1)
/// - Tier 5+: kyc_threshold + 1 approvals
///
/// On-chain: Only stores 43 bytes (level, status, verified_at, data_hash)
/// Off-chain: Full KYC data stored by committee
///
/// Gas cost: 50,000 gas
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SetKycPayload {
    /// Target account public key
    account: CompressedPublicKey,

    /// KYC level bitmask (u16)
    /// Valid values: 0, 7, 31, 63, 255, 2047, 8191, 16383, 32767
    level: u16,

    /// Verification timestamp (Unix timestamp)
    verified_at: u64,

    /// SHA256 hash of full off-chain KycOffChainData
    data_hash: Hash,

    /// Committee ID that verified this KYC
    committee_id: Hash,

    /// Approver signatures
    approvals: Vec<CommitteeApproval>,
}

impl SetKycPayload {
    /// Create new SetKyc payload
    pub fn new(
        account: CompressedPublicKey,
        level: u16,
        verified_at: u64,
        data_hash: Hash,
        committee_id: Hash,
        approvals: Vec<CommitteeApproval>,
    ) -> Self {
        Self {
            account,
            level,
            verified_at,
            data_hash,
            committee_id,
            approvals,
        }
    }

    /// Get target account
    #[inline]
    pub fn get_account(&self) -> &CompressedPublicKey {
        &self.account
    }

    /// Get KYC level
    #[inline]
    pub fn get_level(&self) -> u16 {
        self.level
    }

    /// Get verification timestamp
    #[inline]
    pub fn get_verified_at(&self) -> u64 {
        self.verified_at
    }

    /// Get data hash
    #[inline]
    pub fn get_data_hash(&self) -> &Hash {
        &self.data_hash
    }

    /// Get committee ID
    #[inline]
    pub fn get_committee_id(&self) -> &Hash {
        &self.committee_id
    }

    /// Get approvals
    #[inline]
    pub fn get_approvals(&self) -> &[CommitteeApproval] {
        &self.approvals
    }

    /// Consume and return inner values
    pub fn consume(
        self,
    ) -> (
        CompressedPublicKey,
        u16,
        u64,
        Hash,
        Hash,
        Vec<CommitteeApproval>,
    ) {
        (
            self.account,
            self.level,
            self.verified_at,
            self.data_hash,
            self.committee_id,
            self.approvals,
        )
    }
}

impl Serializer for SetKycPayload {
    fn write(&self, writer: &mut Writer) {
        self.account.write(writer);
        writer.write_u16(self.level);
        writer.write_u64(&self.verified_at);
        self.data_hash.write(writer);
        self.committee_id.write(writer);
        // Write approvals as a vector
        writer.write_u8(self.approvals.len() as u8);
        for approval in &self.approvals {
            approval.member_pubkey.write(writer);
            approval.signature.write(writer);
            writer.write_u64(&approval.timestamp);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let account = CompressedPublicKey::read(reader)?;
        let level = reader.read_u16()?;
        let verified_at = reader.read_u64()?;
        let data_hash = Hash::read(reader)?;
        let committee_id = Hash::read(reader)?;

        // Read approvals
        let approval_count = reader.read_u8()? as usize;
        let mut approvals = Vec::with_capacity(approval_count);
        for _ in 0..approval_count {
            let member_pubkey = CompressedPublicKey::read(reader)?;
            let signature = Signature::read(reader)?;
            let timestamp = reader.read_u64()?;
            approvals.push(CommitteeApproval::new(member_pubkey, signature, timestamp));
        }

        Ok(Self {
            account,
            level,
            verified_at,
            data_hash,
            committee_id,
            approvals,
        })
    }

    fn size(&self) -> usize {
        self.account.size()
            + 2  // level
            + 8  // verified_at
            + self.data_hash.size()
            + self.committee_id.size()
            + 1  // approval count
            + self.approvals.iter().map(|a| {
                a.member_pubkey.size() + 64 + 8  // pubkey + signature + timestamp
            }).sum::<usize>()
    }
}

/// RenewKycPayload is used to renew an expiring KYC
///
/// This transaction requires kyc_threshold approvals
///
/// Gas cost: 30,000 gas
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RenewKycPayload {
    /// Target account public key
    account: CompressedPublicKey,

    /// New verification timestamp
    verified_at: u64,

    /// Updated data hash (may include re-verification)
    data_hash: Hash,

    /// Committee ID
    committee_id: Hash,

    /// Approver signatures
    approvals: Vec<CommitteeApproval>,
}

impl RenewKycPayload {
    /// Create new RenewKyc payload
    pub fn new(
        account: CompressedPublicKey,
        verified_at: u64,
        data_hash: Hash,
        committee_id: Hash,
        approvals: Vec<CommitteeApproval>,
    ) -> Self {
        Self {
            account,
            verified_at,
            data_hash,
            committee_id,
            approvals,
        }
    }

    /// Get target account
    #[inline]
    pub fn get_account(&self) -> &CompressedPublicKey {
        &self.account
    }

    /// Get verification timestamp
    #[inline]
    pub fn get_verified_at(&self) -> u64 {
        self.verified_at
    }

    /// Get data hash
    #[inline]
    pub fn get_data_hash(&self) -> &Hash {
        &self.data_hash
    }

    /// Get committee ID
    #[inline]
    pub fn get_committee_id(&self) -> &Hash {
        &self.committee_id
    }

    /// Get approvals
    #[inline]
    pub fn get_approvals(&self) -> &[CommitteeApproval] {
        &self.approvals
    }
}

impl Serializer for RenewKycPayload {
    fn write(&self, writer: &mut Writer) {
        self.account.write(writer);
        writer.write_u64(&self.verified_at);
        self.data_hash.write(writer);
        self.committee_id.write(writer);
        writer.write_u8(self.approvals.len() as u8);
        for approval in &self.approvals {
            approval.member_pubkey.write(writer);
            approval.signature.write(writer);
            writer.write_u64(&approval.timestamp);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let account = CompressedPublicKey::read(reader)?;
        let verified_at = reader.read_u64()?;
        let data_hash = Hash::read(reader)?;
        let committee_id = Hash::read(reader)?;

        let approval_count = reader.read_u8()? as usize;
        let mut approvals = Vec::with_capacity(approval_count);
        for _ in 0..approval_count {
            let member_pubkey = CompressedPublicKey::read(reader)?;
            let signature = Signature::read(reader)?;
            let timestamp = reader.read_u64()?;
            approvals.push(CommitteeApproval::new(member_pubkey, signature, timestamp));
        }

        Ok(Self {
            account,
            verified_at,
            data_hash,
            committee_id,
            approvals,
        })
    }

    fn size(&self) -> usize {
        self.account.size()
            + 8  // verified_at
            + self.data_hash.size()
            + self.committee_id.size()
            + 1  // approval count
            + self.approvals.iter().map(|a| {
                a.member_pubkey.size() + 64 + 8
            }).sum::<usize>()
    }
}
