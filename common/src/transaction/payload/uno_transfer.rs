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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::elgamal::{KeyPair, PedersenOpening};
    use crate::transaction::TxVersion;
    use tos_crypto::merlin::Transcript;

    fn create_test_payload() -> UnoTransferPayload {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::zero();

        // Create a proper ciphertext with opening for proof generation
        let amount = 100u64;
        let opening = PedersenOpening::generate_new();

        // Create commitment and handles
        let commitment =
            crate::crypto::elgamal::PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);
        let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

        // Create a valid proof with T1 version (includes Y_2 for sender authentication)
        let mut transcript = Transcript::new(b"test_uno_transfer");
        let proof = CiphertextValidityProof::new(
            receiver_keypair.get_public_key(),
            sender_keypair.get_public_key(),
            amount,
            &opening,
            TxVersion::T1,
            &mut transcript,
        );

        UnoTransferPayload::new(
            asset,
            destination,
            None,
            commitment.compress(),
            sender_handle.compress(),
            receiver_handle.compress(),
            proof,
        )
    }

    #[test]
    fn test_uno_transfer_payload_serialization() {
        use crate::context::Context;
        use crate::serializer::Reader;
        use crate::transaction::TxVersion;

        let payload = create_test_payload();

        // Serialize
        let bytes = payload.to_bytes();

        // Deserialize with context containing TxVersion (required by CiphertextValidityProof)
        let mut context = Context::new();
        context.store(TxVersion::T1);
        let mut reader = Reader::with_context(&bytes, context);
        let restored = UnoTransferPayload::read(&mut reader).unwrap();

        // Verify fields match
        assert_eq!(payload.get_asset(), restored.get_asset());
        assert_eq!(payload.get_destination(), restored.get_destination());
        assert_eq!(payload.get_commitment(), restored.get_commitment());
        assert_eq!(payload.get_sender_handle(), restored.get_sender_handle());
        assert_eq!(
            payload.get_receiver_handle(),
            restored.get_receiver_handle()
        );
    }

    #[test]
    fn test_uno_transfer_payload_size() {
        let payload = create_test_payload();

        // Verify size() matches actual serialized bytes
        let bytes = payload.to_bytes();
        assert_eq!(payload.size(), bytes.len());
    }

    #[test]
    fn test_uno_transfer_payload_get_ciphertext() {
        let payload = create_test_payload();

        // Get ciphertext for sender
        let sender_ct = payload.get_ciphertext(Role::Sender);
        assert_eq!(sender_ct.commitment(), payload.get_commitment());
        assert_eq!(sender_ct.handle(), payload.get_sender_handle());

        // Get ciphertext for receiver
        let receiver_ct = payload.get_ciphertext(Role::Receiver);
        assert_eq!(receiver_ct.commitment(), payload.get_commitment());
        assert_eq!(receiver_ct.handle(), payload.get_receiver_handle());
    }

    #[test]
    fn test_uno_transfer_payload_consume() {
        let payload = create_test_payload();

        let asset = payload.get_asset().clone();
        let destination = payload.get_destination().clone();
        let commitment = payload.get_commitment().clone();

        let (consumed_asset, consumed_dest, _, consumed_commitment, _, _) = payload.consume();

        assert_eq!(asset, consumed_asset);
        assert_eq!(destination, consumed_dest);
        assert_eq!(commitment, consumed_commitment);
    }
}
