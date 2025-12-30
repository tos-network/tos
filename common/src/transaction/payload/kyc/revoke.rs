// RevokeKyc and EmergencySuspend Transaction Payloads

use crate::{
    crypto::{elgamal::CompressedPublicKey, Hash},
    kyc::CommitteeApproval,
    serializer::*,
};
use serde::{Deserialize, Serialize};

/// RevokeKycPayload is used to revoke a user's KYC
///
/// This transaction requires kyc_threshold approvals
/// Revoked KYC cannot be renewed - user must go through full re-verification
///
/// Gas cost: 30,000 gas
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RevokeKycPayload {
    /// Target account public key
    account: CompressedPublicKey,

    /// Revocation reason hash (full reason stored off-chain)
    reason_hash: Hash,

    /// Committee ID
    committee_id: Hash,

    /// Approver signatures
    approvals: Vec<CommitteeApproval>,
}

impl RevokeKycPayload {
    /// Create new RevokeKyc payload
    pub fn new(
        account: CompressedPublicKey,
        reason_hash: Hash,
        committee_id: Hash,
        approvals: Vec<CommitteeApproval>,
    ) -> Self {
        Self {
            account,
            reason_hash,
            committee_id,
            approvals,
        }
    }

    /// Get target account
    #[inline]
    pub fn get_account(&self) -> &CompressedPublicKey {
        &self.account
    }

    /// Get reason hash
    #[inline]
    pub fn get_reason_hash(&self) -> &Hash {
        &self.reason_hash
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
    pub fn consume(self) -> (CompressedPublicKey, Hash, Hash, Vec<CommitteeApproval>) {
        (
            self.account,
            self.reason_hash,
            self.committee_id,
            self.approvals,
        )
    }
}

impl Serializer for RevokeKycPayload {
    fn write(&self, writer: &mut Writer) {
        self.account.write(writer);
        self.reason_hash.write(writer);
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
        let reason_hash = Hash::read(reader)?;
        let committee_id = Hash::read(reader)?;

        let approval_count = reader.read_u8()? as usize;
        let mut approvals = Vec::with_capacity(approval_count);
        for _ in 0..approval_count {
            let member_pubkey = CompressedPublicKey::read(reader)?;
            let signature = crate::crypto::Signature::read(reader)?;
            let timestamp = reader.read_u64()?;
            approvals.push(CommitteeApproval::new(member_pubkey, signature, timestamp));
        }

        Ok(Self {
            account,
            reason_hash,
            committee_id,
            approvals,
        })
    }

    fn size(&self) -> usize {
        self.account.size()
            + self.reason_hash.size()
            + self.committee_id.size()
            + 1
            + self
                .approvals
                .iter()
                .map(|a| a.member_pubkey.size() + 64 + 8)
                .sum::<usize>()
    }
}

/// EmergencySuspendPayload is used for fast-track KYC suspension
///
/// This transaction requires only 2 members and has a 24-hour timeout
/// If not confirmed by full committee within 24 hours, suspension is lifted
///
/// Gas cost: 20,000 gas
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EmergencySuspendPayload {
    /// Target account public key
    account: CompressedPublicKey,

    /// Suspension reason hash (full reason stored off-chain)
    reason_hash: Hash,

    /// Committee ID
    committee_id: Hash,

    /// Approver signatures (minimum 2)
    approvals: Vec<CommitteeApproval>,

    /// Auto-expire timestamp (24 hours from submission)
    /// After this, suspension is lifted unless confirmed by full committee
    expires_at: u64,
}

impl EmergencySuspendPayload {
    /// Create new EmergencySuspend payload
    pub fn new(
        account: CompressedPublicKey,
        reason_hash: Hash,
        committee_id: Hash,
        approvals: Vec<CommitteeApproval>,
        expires_at: u64,
    ) -> Self {
        Self {
            account,
            reason_hash,
            committee_id,
            approvals,
            expires_at,
        }
    }

    /// Get target account
    #[inline]
    pub fn get_account(&self) -> &CompressedPublicKey {
        &self.account
    }

    /// Get reason hash
    #[inline]
    pub fn get_reason_hash(&self) -> &Hash {
        &self.reason_hash
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

    /// Get expiration timestamp
    #[inline]
    pub fn get_expires_at(&self) -> u64 {
        self.expires_at
    }

    /// Check if emergency suspension has expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        current_time >= self.expires_at
    }
}

impl Serializer for EmergencySuspendPayload {
    fn write(&self, writer: &mut Writer) {
        self.account.write(writer);
        self.reason_hash.write(writer);
        self.committee_id.write(writer);
        writer.write_u8(self.approvals.len() as u8);
        for approval in &self.approvals {
            approval.member_pubkey.write(writer);
            approval.signature.write(writer);
            writer.write_u64(&approval.timestamp);
        }
        writer.write_u64(&self.expires_at);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let account = CompressedPublicKey::read(reader)?;
        let reason_hash = Hash::read(reader)?;
        let committee_id = Hash::read(reader)?;

        let approval_count = reader.read_u8()? as usize;
        let mut approvals = Vec::with_capacity(approval_count);
        for _ in 0..approval_count {
            let member_pubkey = CompressedPublicKey::read(reader)?;
            let signature = crate::crypto::Signature::read(reader)?;
            let timestamp = reader.read_u64()?;
            approvals.push(CommitteeApproval::new(member_pubkey, signature, timestamp));
        }

        let expires_at = reader.read_u64()?;

        Ok(Self {
            account,
            reason_hash,
            committee_id,
            approvals,
            expires_at,
        })
    }

    fn size(&self) -> usize {
        self.account.size()
            + self.reason_hash.size()
            + self.committee_id.size()
            + 1
            + self
                .approvals
                .iter()
                .map(|a| a.member_pubkey.size() + 64 + 8)
                .sum::<usize>()
            + 8 // expires_at
    }
}
