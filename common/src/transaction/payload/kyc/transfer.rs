// TransferKyc Transaction Payload
// Used to transfer KYC across regions (requires dual committee approval)
//
// Reference: TOS-KYC-Implementation-Details.md Section 3.7

use crate::{
    crypto::{elgamal::CompressedPublicKey, Hash, Signature},
    kyc::CommitteeApproval,
    serializer::*,
};
use serde::{Deserialize, Serialize};

/// TransferKycPayload is used to transfer a user's KYC across regions
///
/// This transaction requires approval from BOTH committees:
/// - Source committee: releases the KYC (kyc_threshold approvals)
/// - Destination committee: accepts the KYC (kyc_threshold approvals)
///
/// Use cases:
/// - User relocates from one region to another
/// - Regulatory requirement for jurisdiction change
///
/// Validation rules:
/// - User must have existing active KYC
/// - Source committee must be user's current KYC committee
/// - Destination committee max_kyc_level must >= user's current level
/// - Both committees must be active
///
/// Gas cost: 60,000 gas
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransferKycPayload {
    /// Target account public key
    account: CompressedPublicKey,

    /// Source committee ID (releasing)
    source_committee_id: Hash,

    /// Source committee approvals
    source_approvals: Vec<CommitteeApproval>,

    /// Destination committee ID (accepting)
    dest_committee_id: Hash,

    /// Destination committee approvals
    dest_approvals: Vec<CommitteeApproval>,

    /// New data hash (from destination committee's off-chain data)
    new_data_hash: Hash,

    /// Transfer timestamp
    transferred_at: u64,
}

impl TransferKycPayload {
    /// Create new TransferKyc payload
    pub fn new(
        account: CompressedPublicKey,
        source_committee_id: Hash,
        source_approvals: Vec<CommitteeApproval>,
        dest_committee_id: Hash,
        dest_approvals: Vec<CommitteeApproval>,
        new_data_hash: Hash,
        transferred_at: u64,
    ) -> Self {
        Self {
            account,
            source_committee_id,
            source_approvals,
            dest_committee_id,
            dest_approvals,
            new_data_hash,
            transferred_at,
        }
    }

    /// Get target account
    #[inline]
    pub fn get_account(&self) -> &CompressedPublicKey {
        &self.account
    }

    /// Get source committee ID
    #[inline]
    pub fn get_source_committee_id(&self) -> &Hash {
        &self.source_committee_id
    }

    /// Get source committee approvals
    #[inline]
    pub fn get_source_approvals(&self) -> &[CommitteeApproval] {
        &self.source_approvals
    }

    /// Get destination committee ID
    #[inline]
    pub fn get_dest_committee_id(&self) -> &Hash {
        &self.dest_committee_id
    }

    /// Get destination committee approvals
    #[inline]
    pub fn get_dest_approvals(&self) -> &[CommitteeApproval] {
        &self.dest_approvals
    }

    /// Get new data hash
    #[inline]
    pub fn get_new_data_hash(&self) -> &Hash {
        &self.new_data_hash
    }

    /// Get transfer timestamp
    #[inline]
    pub fn get_transferred_at(&self) -> u64 {
        self.transferred_at
    }

    /// Consume and return inner values
    pub fn consume(
        self,
    ) -> (
        CompressedPublicKey,
        Hash,
        Vec<CommitteeApproval>,
        Hash,
        Vec<CommitteeApproval>,
        Hash,
        u64,
    ) {
        (
            self.account,
            self.source_committee_id,
            self.source_approvals,
            self.dest_committee_id,
            self.dest_approvals,
            self.new_data_hash,
            self.transferred_at,
        )
    }
}

impl Serializer for TransferKycPayload {
    fn write(&self, writer: &mut Writer) {
        self.account.write(writer);
        self.source_committee_id.write(writer);

        // Write source approvals
        writer.write_u8(self.source_approvals.len() as u8);
        for approval in &self.source_approvals {
            approval.member_pubkey.write(writer);
            approval.signature.write(writer);
            writer.write_u64(&approval.timestamp);
        }

        self.dest_committee_id.write(writer);

        // Write dest approvals
        writer.write_u8(self.dest_approvals.len() as u8);
        for approval in &self.dest_approvals {
            approval.member_pubkey.write(writer);
            approval.signature.write(writer);
            writer.write_u64(&approval.timestamp);
        }

        self.new_data_hash.write(writer);
        writer.write_u64(&self.transferred_at);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let account = CompressedPublicKey::read(reader)?;
        let source_committee_id = Hash::read(reader)?;

        // Read source approvals
        let source_count = reader.read_u8()? as usize;
        let mut source_approvals = Vec::with_capacity(source_count);
        for _ in 0..source_count {
            let member_pubkey = CompressedPublicKey::read(reader)?;
            let signature = Signature::read(reader)?;
            let timestamp = reader.read_u64()?;
            source_approvals.push(CommitteeApproval::new(member_pubkey, signature, timestamp));
        }

        let dest_committee_id = Hash::read(reader)?;

        // Read dest approvals
        let dest_count = reader.read_u8()? as usize;
        let mut dest_approvals = Vec::with_capacity(dest_count);
        for _ in 0..dest_count {
            let member_pubkey = CompressedPublicKey::read(reader)?;
            let signature = Signature::read(reader)?;
            let timestamp = reader.read_u64()?;
            dest_approvals.push(CommitteeApproval::new(member_pubkey, signature, timestamp));
        }

        let new_data_hash = Hash::read(reader)?;
        let transferred_at = reader.read_u64()?;

        Ok(Self {
            account,
            source_committee_id,
            source_approvals,
            dest_committee_id,
            dest_approvals,
            new_data_hash,
            transferred_at,
        })
    }

    fn size(&self) -> usize {
        self.account.size()
            + self.source_committee_id.size()
            + 1  // source approval count
            + self.source_approvals.iter().map(|a| {
                a.member_pubkey.size() + 64 + 8  // pubkey + signature + timestamp
            }).sum::<usize>()
            + self.dest_committee_id.size()
            + 1  // dest approval count
            + self.dest_approvals.iter().map(|a| {
                a.member_pubkey.size() + 64 + 8
            }).sum::<usize>()
            + self.new_data_hash.size()
            + 8 // transferred_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;

    fn create_test_approval(keypair: &KeyPair, timestamp: u64) -> CommitteeApproval {
        let mock_bytes = [0u8; 64];
        let mock_signature =
            Signature::from_bytes(&mock_bytes).expect("Valid mock signature bytes");
        CommitteeApproval::new(
            keypair.get_public_key().compress(),
            mock_signature,
            timestamp,
        )
    }

    #[test]
    fn test_transfer_kyc_payload_creation() {
        let keypair = KeyPair::new();
        let account = keypair.get_public_key().compress();
        let source_committee_id = Hash::new([1u8; 32]);
        let dest_committee_id = Hash::new([2u8; 32]);
        let new_data_hash = Hash::new([3u8; 32]);

        let source_approver = KeyPair::new();
        let dest_approver = KeyPair::new();

        let payload = TransferKycPayload::new(
            account.clone(),
            source_committee_id.clone(),
            vec![create_test_approval(&source_approver, 1000)],
            dest_committee_id.clone(),
            vec![create_test_approval(&dest_approver, 1000)],
            new_data_hash.clone(),
            1000,
        );

        assert_eq!(payload.get_account(), &account);
        assert_eq!(payload.get_source_committee_id(), &source_committee_id);
        assert_eq!(payload.get_dest_committee_id(), &dest_committee_id);
        assert_eq!(payload.get_new_data_hash(), &new_data_hash);
        assert_eq!(payload.get_transferred_at(), 1000);
        assert_eq!(payload.get_source_approvals().len(), 1);
        assert_eq!(payload.get_dest_approvals().len(), 1);
    }

    #[test]
    fn test_transfer_kyc_payload_serialization() {
        let keypair = KeyPair::new();
        let account = keypair.get_public_key().compress();
        let source_committee_id = Hash::new([1u8; 32]);
        let dest_committee_id = Hash::new([2u8; 32]);
        let new_data_hash = Hash::new([3u8; 32]);

        let source_approver = KeyPair::new();
        let dest_approver = KeyPair::new();

        let payload = TransferKycPayload::new(
            account,
            source_committee_id,
            vec![create_test_approval(&source_approver, 1000)],
            dest_committee_id,
            vec![create_test_approval(&dest_approver, 2000)],
            new_data_hash,
            3000,
        );

        // Serialize
        let bytes = payload.to_bytes();

        // Deserialize
        let mut reader = Reader::new(&bytes);
        let restored = TransferKycPayload::read(&mut reader).expect("Should deserialize");

        // Verify
        assert_eq!(payload.get_account(), restored.get_account());
        assert_eq!(
            payload.get_source_committee_id(),
            restored.get_source_committee_id()
        );
        assert_eq!(
            payload.get_dest_committee_id(),
            restored.get_dest_committee_id()
        );
        assert_eq!(payload.get_new_data_hash(), restored.get_new_data_hash());
        assert_eq!(payload.get_transferred_at(), restored.get_transferred_at());
        assert_eq!(
            payload.get_source_approvals().len(),
            restored.get_source_approvals().len()
        );
        assert_eq!(
            payload.get_dest_approvals().len(),
            restored.get_dest_approvals().len()
        );
    }

    #[test]
    fn test_transfer_kyc_payload_size() {
        let keypair = KeyPair::new();
        let account = keypair.get_public_key().compress();
        let source_committee_id = Hash::new([1u8; 32]);
        let dest_committee_id = Hash::new([2u8; 32]);
        let new_data_hash = Hash::new([3u8; 32]);

        let payload = TransferKycPayload::new(
            account,
            source_committee_id,
            vec![], // No approvals
            dest_committee_id,
            vec![], // No approvals
            new_data_hash,
            1000,
        );

        // Size without approvals:
        // account (32) + source_committee_id (32) + 1 (count) +
        // dest_committee_id (32) + 1 (count) + new_data_hash (32) + 8 (transferred_at)
        // = 138 bytes
        let bytes = payload.to_bytes();
        assert_eq!(bytes.len(), payload.size());
    }
}
