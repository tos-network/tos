use crate::{
    crypto::{
        elgamal::{
            CompressedCiphertext, CompressedCommitment, CompressedHandle, CompressedPublicKey,
        },
        proofs::CiphertextValidityProof,
        Hash,
    },
    serializer::*,
    transaction::extra_data::UnknownExtraDataFormat,
};
use serde::{Deserialize, Serialize};

/// Role in a UNO transfer transaction
/// Used to select the appropriate DecryptHandle for ciphertext construction
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Sender,
    Receiver,
}

/// UnoTransferPayload is a privacy-preserving transfer payload
/// It contains encrypted amount using Twisted ElGamal encryption
/// The amount is hidden but the destination address is visible
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UnoTransferPayload {
    asset: Hash,
    destination: CompressedPublicKey,
    // Optional memo/extra data up to EXTRA_DATA_LIMIT_SIZE bytes
    extra_data: Option<UnknownExtraDataFormat>,
    /// Pedersen commitment to the transfer amount: C = amount * G + r * H
    commitment: CompressedCommitment,
    /// Sender's decrypt handle: D_s = r * P_sender
    sender_handle: CompressedHandle,
    /// Receiver's decrypt handle: D_r = r * P_receiver
    receiver_handle: CompressedHandle,
    /// Proof that the ciphertext is validly formed
    ct_validity_proof: CiphertextValidityProof,
}

impl UnoTransferPayload {
    /// Create a new UNO transfer payload
    pub fn new(
        asset: Hash,
        destination: CompressedPublicKey,
        extra_data: Option<UnknownExtraDataFormat>,
        commitment: CompressedCommitment,
        sender_handle: CompressedHandle,
        receiver_handle: CompressedHandle,
        ct_validity_proof: CiphertextValidityProof,
    ) -> Self {
        UnoTransferPayload {
            asset,
            destination,
            extra_data,
            commitment,
            sender_handle,
            receiver_handle,
            ct_validity_proof,
        }
    }

    /// Get the destination public key
    #[inline]
    pub fn get_destination(&self) -> &CompressedPublicKey {
        &self.destination
    }

    /// Get the asset hash
    #[inline]
    pub fn get_asset(&self) -> &Hash {
        &self.asset
    }

    /// Get the extra data if any
    #[inline]
    pub fn get_extra_data(&self) -> &Option<UnknownExtraDataFormat> {
        &self.extra_data
    }

    /// Get the Pedersen commitment
    #[inline]
    pub fn get_commitment(&self) -> &CompressedCommitment {
        &self.commitment
    }

    /// Get the receiver's decrypt handle
    #[inline]
    pub fn get_receiver_handle(&self) -> &CompressedHandle {
        &self.receiver_handle
    }

    /// Get the sender's decrypt handle
    #[inline]
    pub fn get_sender_handle(&self) -> &CompressedHandle {
        &self.sender_handle
    }

    /// Get the ciphertext validity proof
    #[inline]
    pub fn get_proof(&self) -> &CiphertextValidityProof {
        &self.ct_validity_proof
    }

    /// Get the ciphertext for the specified role
    /// Sender gets (commitment, sender_handle), Receiver gets (commitment, receiver_handle)
    #[inline]
    pub fn get_ciphertext(&self, role: Role) -> CompressedCiphertext {
        let handle = match role {
            Role::Receiver => self.receiver_handle.clone(),
            Role::Sender => self.sender_handle.clone(),
        };
        CompressedCiphertext::new(self.commitment.clone(), handle)
    }

    /// Consume and return all fields
    #[inline]
    pub fn consume(
        self,
    ) -> (
        Hash,
        CompressedPublicKey,
        Option<UnknownExtraDataFormat>,
        CompressedCommitment,
        CompressedHandle,
        CompressedHandle,
    ) {
        (
            self.asset,
            self.destination,
            self.extra_data,
            self.commitment,
            self.sender_handle,
            self.receiver_handle,
        )
    }
}

impl Serializer for UnoTransferPayload {
    fn write(&self, writer: &mut Writer) {
        self.asset.write(writer);
        self.destination.write(writer);
        self.extra_data.write(writer);
        self.commitment.write(writer);
        self.sender_handle.write(writer);
        self.receiver_handle.write(writer);
        self.ct_validity_proof.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<UnoTransferPayload, ReaderError> {
        let asset = Hash::read(reader)?;
        let destination = CompressedPublicKey::read(reader)?;
        let extra_data = Option::read(reader)?;
        let commitment = CompressedCommitment::read(reader)?;
        let sender_handle = CompressedHandle::read(reader)?;
        let receiver_handle = CompressedHandle::read(reader)?;
        let ct_validity_proof = CiphertextValidityProof::read(reader)?;

        Ok(UnoTransferPayload {
            asset,
            destination,
            extra_data,
            commitment,
            sender_handle,
            receiver_handle,
            ct_validity_proof,
        })
    }

    fn size(&self) -> usize {
        self.asset.size()
            + self.destination.size()
            + self.extra_data.size()
            + self.commitment.size()
            + self.sender_handle.size()
            + self.receiver_handle.size()
            + self.ct_validity_proof.size()
    }
}
