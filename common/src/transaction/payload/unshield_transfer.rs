use crate::{
    crypto::{
        elgamal::{CompressedCommitment, CompressedHandle, CompressedPublicKey},
        proofs::CiphertextValidityProof,
        Hash,
    },
    serializer::*,
    transaction::extra_data::UnknownExtraDataFormat,
};
use serde::{Deserialize, Serialize};

/// UnshieldTransferPayload converts encrypted UNO balance back to plaintext TOS balance.
///
/// The amount is publicly revealed in the transaction (exiting privacy mode).
/// Requires ZK proof that the sender has sufficient UNO balance.
///
/// # Fields
/// - `asset`: Asset being unshielded (must be TOS_ASSET for Phase 7)
/// - `destination`: Address to receive the plaintext TOS balance
/// - `amount`: Plaintext amount to unshield (publicly revealed)
/// - `commitment`: Pedersen commitment C = amount * G + r * H
/// - `sender_handle`: Decrypt handle D_s = r * P_sender
/// - `ct_validity_proof`: Proof that commitment encodes the claimed amount
///
/// # Verification
/// Unshield transfers require:
/// 1. CiphertextValidityProof - proves commitment matches the claimed amount
/// 2. Balance check - proves sender has sufficient UNO balance
/// 3. Range proof - proves remaining balance is non-negative
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UnshieldTransferPayload {
    asset: Hash,
    destination: CompressedPublicKey,
    /// Plaintext amount to unshield (publicly revealed)
    amount: u64,
    /// Optional memo/extra data
    extra_data: Option<UnknownExtraDataFormat>,
    /// Pedersen commitment to the amount: C = amount * G + r * H
    commitment: CompressedCommitment,
    /// Sender's decrypt handle: D_s = r * P_sender
    sender_handle: CompressedHandle,
    /// Proof that the commitment encodes the claimed amount
    ct_validity_proof: CiphertextValidityProof,
}

impl UnshieldTransferPayload {
    /// Create a new Unshield transfer payload
    pub fn new(
        asset: Hash,
        destination: CompressedPublicKey,
        amount: u64,
        extra_data: Option<UnknownExtraDataFormat>,
        commitment: CompressedCommitment,
        sender_handle: CompressedHandle,
        ct_validity_proof: CiphertextValidityProof,
    ) -> Self {
        UnshieldTransferPayload {
            asset,
            destination,
            amount,
            extra_data,
            commitment,
            sender_handle,
            ct_validity_proof,
        }
    }

    /// Get the asset hash
    #[inline]
    pub fn get_asset(&self) -> &Hash {
        &self.asset
    }

    /// Get the destination public key
    #[inline]
    pub fn get_destination(&self) -> &CompressedPublicKey {
        &self.destination
    }

    /// Get the plaintext amount being unshielded
    #[inline]
    pub fn get_amount(&self) -> u64 {
        self.amount
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

    /// Consume and return all fields
    #[inline]
    pub fn consume(
        self,
    ) -> (
        Hash,
        CompressedPublicKey,
        u64,
        Option<UnknownExtraDataFormat>,
        CompressedCommitment,
        CompressedHandle,
        CiphertextValidityProof,
    ) {
        (
            self.asset,
            self.destination,
            self.amount,
            self.extra_data,
            self.commitment,
            self.sender_handle,
            self.ct_validity_proof,
        )
    }
}

impl Serializer for UnshieldTransferPayload {
    fn write(&self, writer: &mut Writer) {
        self.asset.write(writer);
        self.destination.write(writer);
        self.amount.write(writer);
        self.extra_data.write(writer);
        self.commitment.write(writer);
        self.sender_handle.write(writer);
        self.ct_validity_proof.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<UnshieldTransferPayload, ReaderError> {
        let asset = Hash::read(reader)?;
        let destination = CompressedPublicKey::read(reader)?;
        let amount = reader.read_u64()?;
        let extra_data = Option::read(reader)?;
        let commitment = CompressedCommitment::read(reader)?;
        let sender_handle = CompressedHandle::read(reader)?;
        let ct_validity_proof = CiphertextValidityProof::read(reader)?;

        Ok(UnshieldTransferPayload {
            asset,
            destination,
            amount,
            extra_data,
            commitment,
            sender_handle,
            ct_validity_proof,
        })
    }

    fn size(&self) -> usize {
        self.asset.size()
            + self.destination.size()
            + self.amount.size()
            + self.extra_data.size()
            + self.commitment.size()
            + self.sender_handle.size()
            + self.ct_validity_proof.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::elgamal::{KeyPair, PedersenCommitment, PedersenOpening};
    use crate::transaction::TxVersion;
    use tos_crypto::merlin::Transcript;

    fn create_test_payload() -> UnshieldTransferPayload {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::zero();
        let amount = 100u64;

        // Create commitment and handle
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);

        // Create validity proof with T1 version (includes Y_2 for sender authentication)
        let mut transcript = Transcript::new(b"test_unshield_transfer");
        let proof = CiphertextValidityProof::new(
            sender_keypair.get_public_key(),
            receiver_keypair.get_public_key(),
            amount,
            &opening,
            TxVersion::T1,
            &mut transcript,
        );

        UnshieldTransferPayload::new(
            asset,
            destination,
            amount,
            None,
            commitment.compress(),
            sender_handle.compress(),
            proof,
        )
    }

    #[test]
    fn test_unshield_transfer_payload_creation() {
        let payload = create_test_payload();
        assert_eq!(payload.get_amount(), 100);
        assert_eq!(payload.get_asset(), &Hash::zero());
    }

    #[test]
    fn test_unshield_transfer_payload_serialization() {
        use crate::context::Context;
        use crate::transaction::TxVersion;

        let payload = create_test_payload();

        // Serialize
        let bytes = payload.to_bytes();

        // Deserialize with context (required by CiphertextValidityProof)
        let mut context = Context::new();
        context.store(TxVersion::T0);
        let mut reader = Reader::with_context(&bytes, context);
        let restored = UnshieldTransferPayload::read(&mut reader).expect("test");

        // Verify fields match
        assert_eq!(payload.get_asset(), restored.get_asset());
        assert_eq!(payload.get_destination(), restored.get_destination());
        assert_eq!(payload.get_amount(), restored.get_amount());
        assert_eq!(payload.get_commitment(), restored.get_commitment());
        assert_eq!(payload.get_sender_handle(), restored.get_sender_handle());
    }

    #[test]
    fn test_unshield_transfer_payload_size() {
        let payload = create_test_payload();

        // Verify size() matches actual serialized bytes
        let bytes = payload.to_bytes();
        assert_eq!(payload.size(), bytes.len());
    }

    #[test]
    fn test_unshield_transfer_payload_consume() {
        let payload = create_test_payload();

        let asset = payload.get_asset().clone();
        let destination = payload.get_destination().clone();
        let amount = payload.get_amount();
        let commitment = payload.get_commitment().clone();

        let (c_asset, c_dest, c_amount, _, c_commit, _, _) = payload.consume();

        assert_eq!(asset, c_asset);
        assert_eq!(destination, c_dest);
        assert_eq!(amount, c_amount);
        assert_eq!(commitment, c_commit);
    }

    #[test]
    fn test_unshield_transfer_different_amounts() {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::zero();

        // Test with various amounts
        for amount in [1u64, 100, 1000, 1_000_000, u64::MAX / 2] {
            let opening = PedersenOpening::generate_new();
            let commitment = PedersenCommitment::new_with_opening(amount, &opening);
            let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);

            let mut transcript = Transcript::new(b"test");
            let proof = CiphertextValidityProof::new(
                sender_keypair.get_public_key(),
                receiver_keypair.get_public_key(),
                amount,
                &opening,
                TxVersion::T1,
                &mut transcript,
            );

            let payload = UnshieldTransferPayload::new(
                asset.clone(),
                destination.clone(),
                amount,
                None,
                commitment.compress(),
                sender_handle.compress(),
                proof,
            );

            assert_eq!(payload.get_amount(), amount);
        }
    }

    #[test]
    fn test_unshield_transfer_payload_with_extra_data() {
        let sender_keypair = KeyPair::new();
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::zero();
        let amount = 500u64;

        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);

        let mut transcript = Transcript::new(b"test");
        let proof = CiphertextValidityProof::new(
            sender_keypair.get_public_key(),
            receiver_keypair.get_public_key(),
            amount,
            &opening,
            TxVersion::T1,
            &mut transcript,
        );

        // Create with extra data
        let extra_data = Some(UnknownExtraDataFormat(vec![1, 2, 3, 4, 5]));
        let payload = UnshieldTransferPayload::new(
            asset,
            destination,
            amount,
            extra_data,
            commitment.compress(),
            sender_handle.compress(),
            proof,
        );

        assert!(payload.get_extra_data().is_some());
    }
}
