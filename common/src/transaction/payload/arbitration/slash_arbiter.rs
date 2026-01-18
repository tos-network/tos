use serde::{Deserialize, Serialize};

use crate::{
    crypto::Hash,
    crypto::PublicKey,
    kyc::CommitteeApproval,
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// SlashArbiterPayload slashes arbiter stake via committee approvals.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SlashArbiterPayload {
    /// Committee ID authorizing the slash.
    committee_id: Hash,
    /// Target arbiter public key.
    arbiter_pubkey: PublicKey,
    /// Amount to slash.
    amount: u64,
    /// Reason hash for slashing.
    reason_hash: Hash,
    /// Committee approvals.
    approvals: Vec<CommitteeApproval>,
}

impl SlashArbiterPayload {
    pub fn new(
        committee_id: Hash,
        arbiter_pubkey: PublicKey,
        amount: u64,
        reason_hash: Hash,
        approvals: Vec<CommitteeApproval>,
    ) -> Self {
        Self {
            committee_id,
            arbiter_pubkey,
            amount,
            reason_hash,
            approvals,
        }
    }

    #[inline]
    pub fn get_committee_id(&self) -> &Hash {
        &self.committee_id
    }

    #[inline]
    pub fn get_arbiter_pubkey(&self) -> &PublicKey {
        &self.arbiter_pubkey
    }

    #[inline]
    pub fn get_amount(&self) -> u64 {
        self.amount
    }

    #[inline]
    pub fn get_reason_hash(&self) -> &Hash {
        &self.reason_hash
    }

    #[inline]
    pub fn get_approvals(&self) -> &[CommitteeApproval] {
        &self.approvals
    }

    pub fn consume(self) -> (Hash, PublicKey, u64, Hash, Vec<CommitteeApproval>) {
        (
            self.committee_id,
            self.arbiter_pubkey,
            self.amount,
            self.reason_hash,
            self.approvals,
        )
    }
}

impl Serializer for SlashArbiterPayload {
    fn write(&self, writer: &mut Writer) {
        self.committee_id.write(writer);
        self.arbiter_pubkey.write(writer);
        self.amount.write(writer);
        self.reason_hash.write(writer);
        self.approvals.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            committee_id: Hash::read(reader)?,
            arbiter_pubkey: PublicKey::read(reader)?,
            amount: u64::read(reader)?,
            reason_hash: Hash::read(reader)?,
            approvals: Vec::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        self.committee_id.size()
            + self.arbiter_pubkey.size()
            + self.amount.size()
            + self.reason_hash.size()
            + self.approvals.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;

    #[test]
    fn slash_arbiter_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let keypair = KeyPair::new();
        let payload = SlashArbiterPayload::new(
            Hash::zero(),
            keypair.get_public_key().compress(),
            100,
            Hash::zero(),
            Vec::new(),
        );
        let data = serde_json::to_vec(&payload)?;
        let decoded: SlashArbiterPayload = serde_json::from_slice(&data)?;
        assert_eq!(payload.get_amount(), decoded.get_amount());
        Ok(())
    }
}
