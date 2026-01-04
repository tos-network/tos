use crate::{
    crypto::{
        elgamal::{CompressedCommitment, CompressedHandle, CompressedPublicKey},
        proofs::ShieldCommitmentProof,
        Hash,
    },
    serializer::*,
    transaction::extra_data::UnknownExtraDataFormat,
};
use serde::{Deserialize, Serialize};

/// ShieldTransferPayload converts plaintext TOS balance to encrypted UNO balance.
///
/// The amount is publicly visible in the transaction (no hiding needed),
/// but after the transfer, the destination's UNO balance is encrypted.
///
/// # Fields
/// - `asset`: Asset being shielded (must be TOS_ASSET for Phase 7)
/// - `destination`: Address to receive the encrypted UNO balance
/// - `amount`: Plaintext amount to shield (publicly visible)
/// - `commitment`: Pedersen commitment C = amount * G + r * H
/// - `receiver_handle`: Decrypt handle D_r = r * P_receiver
/// - `proof`: ShieldCommitmentProof proving commitment correctness
///
/// # Verification
/// Shield transfers require ShieldCommitmentProof to verify:
/// 1. The commitment contains the claimed amount
/// 2. The commitment and receiver_handle use the same opening
/// 3. This prevents inflation attacks via forged commitments
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShieldTransferPayload {
    asset: Hash,
    destination: CompressedPublicKey,
    /// Plaintext amount to shield (publicly visible)
    amount: u64,
    /// Optional memo/extra data
    extra_data: Option<UnknownExtraDataFormat>,
    /// Pedersen commitment to the amount: C = amount * G + r * H
    commitment: CompressedCommitment,
    /// Receiver's decrypt handle: D_r = r * P_receiver
    receiver_handle: CompressedHandle,
    /// Proof that commitment is correctly formed for the claimed amount
    proof: ShieldCommitmentProof,
}

impl ShieldTransferPayload {
    /// Create a new Shield transfer payload
    pub fn new(
        asset: Hash,
        destination: CompressedPublicKey,
        amount: u64,
        extra_data: Option<UnknownExtraDataFormat>,
        commitment: CompressedCommitment,
        receiver_handle: CompressedHandle,
        proof: ShieldCommitmentProof,
    ) -> Self {
        ShieldTransferPayload {
            asset,
            destination,
            amount,
            extra_data,
            commitment,
            receiver_handle,
            proof,
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

    /// Get the plaintext amount being shielded
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

    /// Get the receiver's decrypt handle
    #[inline]
    pub fn get_receiver_handle(&self) -> &CompressedHandle {
        &self.receiver_handle
    }

    /// Get the Shield commitment proof
    #[inline]
    pub fn get_proof(&self) -> &ShieldCommitmentProof {
        &self.proof
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
        ShieldCommitmentProof,
    ) {
        (
            self.asset,
            self.destination,
            self.amount,
            self.extra_data,
            self.commitment,
            self.receiver_handle,
            self.proof,
        )
    }
}

impl Serializer for ShieldTransferPayload {
    fn write(&self, writer: &mut Writer) {
        self.asset.write(writer);
        self.destination.write(writer);
        self.amount.write(writer);
        self.extra_data.write(writer);
        self.commitment.write(writer);
        self.receiver_handle.write(writer);
        self.proof.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<ShieldTransferPayload, ReaderError> {
        let asset = Hash::read(reader)?;
        let destination = CompressedPublicKey::read(reader)?;
        let amount = reader.read_u64()?;
        let extra_data = Option::read(reader)?;
        let commitment = CompressedCommitment::read(reader)?;
        let receiver_handle = CompressedHandle::read(reader)?;
        let proof = ShieldCommitmentProof::read(reader)?;

        Ok(ShieldTransferPayload {
            asset,
            destination,
            amount,
            extra_data,
            commitment,
            receiver_handle,
            proof,
        })
    }

    fn size(&self) -> usize {
        self.asset.size()
            + self.destination.size()
            + self.amount.size()
            + self.extra_data.size()
            + self.commitment.size()
            + self.receiver_handle.size()
            + self.proof.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::elgamal::{KeyPair, PedersenCommitment, PedersenOpening};
    use tos_crypto::merlin::Transcript;

    fn create_test_payload() -> ShieldTransferPayload {
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::zero();
        let amount = 100u64;

        // Create commitment and handle
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

        // Create proof
        let mut transcript = Transcript::new(b"test");
        let proof = ShieldCommitmentProof::new(
            receiver_keypair.get_public_key(),
            amount,
            &opening,
            &mut transcript,
        );

        ShieldTransferPayload::new(
            asset,
            destination,
            amount,
            None,
            commitment.compress(),
            receiver_handle.compress(),
            proof,
        )
    }

    #[test]
    fn test_shield_transfer_payload_creation() {
        let payload = create_test_payload();
        assert_eq!(payload.get_amount(), 100);
        assert_eq!(payload.get_asset(), &Hash::zero());
    }

    #[test]
    fn test_shield_transfer_payload_serialization() {
        let payload = create_test_payload();

        // Serialize
        let bytes = payload.to_bytes();

        // Deserialize
        let mut reader = Reader::new(&bytes);
        let restored = ShieldTransferPayload::read(&mut reader).expect("test");

        // Verify fields match
        assert_eq!(payload.get_asset(), restored.get_asset());
        assert_eq!(payload.get_destination(), restored.get_destination());
        assert_eq!(payload.get_amount(), restored.get_amount());
        assert_eq!(payload.get_commitment(), restored.get_commitment());
        assert_eq!(
            payload.get_receiver_handle(),
            restored.get_receiver_handle()
        );
    }

    #[test]
    fn test_shield_transfer_payload_size() {
        let payload = create_test_payload();

        // Verify size() matches actual serialized bytes
        let bytes = payload.to_bytes();
        assert_eq!(payload.size(), bytes.len());
    }

    #[test]
    fn test_shield_transfer_payload_consume() {
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
    fn test_shield_transfer_payload_with_extra_data() {
        let receiver_keypair = KeyPair::new();
        let destination = receiver_keypair.get_public_key().compress();
        let asset = Hash::zero();
        let amount = 500u64;

        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

        // Create proof
        let mut transcript = Transcript::new(b"test");
        let proof = ShieldCommitmentProof::new(
            receiver_keypair.get_public_key(),
            amount,
            &opening,
            &mut transcript,
        );

        // Create with extra data
        let extra_data = Some(UnknownExtraDataFormat(vec![1, 2, 3, 4, 5]));
        let payload = ShieldTransferPayload::new(
            asset,
            destination,
            amount,
            extra_data.clone(),
            commitment.compress(),
            receiver_handle.compress(),
            proof,
        );

        assert!(payload.get_extra_data().is_some());

        // Verify serialization roundtrip with extra data
        let bytes = payload.to_bytes();
        let mut reader = Reader::new(&bytes);
        let restored = ShieldTransferPayload::read(&mut reader).expect("test");
        assert!(restored.get_extra_data().is_some());
    }
}
