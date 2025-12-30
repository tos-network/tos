// AppealKyc Transaction Payload
// Used to appeal rejected or revoked KYC to parent committee
//
// Reference: TOS-KYC-Implementation-Details.md Section 3.9

use crate::{
    crypto::{elgamal::CompressedPublicKey, Hash},
    serializer::*,
};
use serde::{Deserialize, Serialize};

/// AppealKycPayload is used to appeal a rejected or revoked KYC decision
///
/// This transaction initiates an appeal process with the parent committee.
/// The parent committee reviews the appeal and can overturn the original
/// committee's decision.
///
/// Use cases:
/// - User's KYC was wrongly revoked
/// - User's KYC application was rejected without valid reason
/// - User has new evidence to support their KYC application
///
/// Validation rules:
/// - User must have existing KYC record (revoked or rejected)
/// - Original committee must exist and be active
/// - Parent committee must exist and be active
/// - Original committee must be a child of parent committee
/// - Appeal must be submitted within appeal window (e.g., 30 days)
///
/// Gas cost: 40,000 gas
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppealKycPayload {
    /// Target account public key (the user appealing)
    account: CompressedPublicKey,

    /// Original committee that rejected/revoked the KYC
    original_committee_id: Hash,

    /// Parent committee ID (reviewer/arbiter)
    parent_committee_id: Hash,

    /// Hash of appeal reason (full reason stored off-chain)
    reason_hash: Hash,

    /// Hash of supporting documents (documents stored off-chain)
    documents_hash: Hash,

    /// Appeal submission timestamp
    submitted_at: u64,
}

impl AppealKycPayload {
    /// Create new AppealKyc payload
    pub fn new(
        account: CompressedPublicKey,
        original_committee_id: Hash,
        parent_committee_id: Hash,
        reason_hash: Hash,
        documents_hash: Hash,
        submitted_at: u64,
    ) -> Self {
        Self {
            account,
            original_committee_id,
            parent_committee_id,
            reason_hash,
            documents_hash,
            submitted_at,
        }
    }

    /// Get target account
    #[inline]
    pub fn get_account(&self) -> &CompressedPublicKey {
        &self.account
    }

    /// Get original committee ID (the one that rejected/revoked)
    #[inline]
    pub fn get_original_committee_id(&self) -> &Hash {
        &self.original_committee_id
    }

    /// Get parent committee ID (the arbiter)
    #[inline]
    pub fn get_parent_committee_id(&self) -> &Hash {
        &self.parent_committee_id
    }

    /// Get appeal reason hash
    #[inline]
    pub fn get_reason_hash(&self) -> &Hash {
        &self.reason_hash
    }

    /// Get supporting documents hash
    #[inline]
    pub fn get_documents_hash(&self) -> &Hash {
        &self.documents_hash
    }

    /// Get submission timestamp
    #[inline]
    pub fn get_submitted_at(&self) -> u64 {
        self.submitted_at
    }

    /// Consume and return inner values
    pub fn consume(self) -> (CompressedPublicKey, Hash, Hash, Hash, Hash, u64) {
        (
            self.account,
            self.original_committee_id,
            self.parent_committee_id,
            self.reason_hash,
            self.documents_hash,
            self.submitted_at,
        )
    }
}

impl Serializer for AppealKycPayload {
    fn write(&self, writer: &mut Writer) {
        self.account.write(writer);
        self.original_committee_id.write(writer);
        self.parent_committee_id.write(writer);
        self.reason_hash.write(writer);
        self.documents_hash.write(writer);
        writer.write_u64(&self.submitted_at);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let account = CompressedPublicKey::read(reader)?;
        let original_committee_id = Hash::read(reader)?;
        let parent_committee_id = Hash::read(reader)?;
        let reason_hash = Hash::read(reader)?;
        let documents_hash = Hash::read(reader)?;
        let submitted_at = reader.read_u64()?;

        Ok(Self {
            account,
            original_committee_id,
            parent_committee_id,
            reason_hash,
            documents_hash,
            submitted_at,
        })
    }

    fn size(&self) -> usize {
        self.account.size()
            + self.original_committee_id.size()
            + self.parent_committee_id.size()
            + self.reason_hash.size()
            + self.documents_hash.size()
            + 8 // submitted_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;

    #[test]
    fn test_appeal_kyc_payload_creation() {
        let keypair = KeyPair::new();
        let account = keypair.get_public_key().compress();
        let original_committee_id = Hash::new([1u8; 32]);
        let parent_committee_id = Hash::new([2u8; 32]);
        let reason_hash = Hash::new([3u8; 32]);
        let documents_hash = Hash::new([4u8; 32]);

        let payload = AppealKycPayload::new(
            account.clone(),
            original_committee_id.clone(),
            parent_committee_id.clone(),
            reason_hash.clone(),
            documents_hash.clone(),
            1000,
        );

        assert_eq!(payload.get_account(), &account);
        assert_eq!(payload.get_original_committee_id(), &original_committee_id);
        assert_eq!(payload.get_parent_committee_id(), &parent_committee_id);
        assert_eq!(payload.get_reason_hash(), &reason_hash);
        assert_eq!(payload.get_documents_hash(), &documents_hash);
        assert_eq!(payload.get_submitted_at(), 1000);
    }

    #[test]
    fn test_appeal_kyc_payload_serialization() {
        let keypair = KeyPair::new();
        let account = keypair.get_public_key().compress();
        let original_committee_id = Hash::new([1u8; 32]);
        let parent_committee_id = Hash::new([2u8; 32]);
        let reason_hash = Hash::new([3u8; 32]);
        let documents_hash = Hash::new([4u8; 32]);

        let payload = AppealKycPayload::new(
            account,
            original_committee_id,
            parent_committee_id,
            reason_hash,
            documents_hash,
            2000,
        );

        // Serialize
        let bytes = payload.to_bytes();

        // Deserialize
        let mut reader = Reader::new(&bytes);
        let restored = AppealKycPayload::read(&mut reader).expect("Should deserialize");

        // Verify
        assert_eq!(payload.get_account(), restored.get_account());
        assert_eq!(
            payload.get_original_committee_id(),
            restored.get_original_committee_id()
        );
        assert_eq!(
            payload.get_parent_committee_id(),
            restored.get_parent_committee_id()
        );
        assert_eq!(payload.get_reason_hash(), restored.get_reason_hash());
        assert_eq!(payload.get_documents_hash(), restored.get_documents_hash());
        assert_eq!(payload.get_submitted_at(), restored.get_submitted_at());
    }

    #[test]
    fn test_appeal_kyc_payload_size() {
        let keypair = KeyPair::new();
        let account = keypair.get_public_key().compress();
        let original_committee_id = Hash::new([1u8; 32]);
        let parent_committee_id = Hash::new([2u8; 32]);
        let reason_hash = Hash::new([3u8; 32]);
        let documents_hash = Hash::new([4u8; 32]);

        let payload = AppealKycPayload::new(
            account,
            original_committee_id,
            parent_committee_id,
            reason_hash,
            documents_hash,
            1000,
        );

        // Size:
        // account (32) + original_committee_id (32) + parent_committee_id (32) +
        // reason_hash (32) + documents_hash (32) + submitted_at (8) = 168 bytes
        let bytes = payload.to_bytes();
        assert_eq!(bytes.len(), payload.size());
    }
}
