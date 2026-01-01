use rand::rngs::OsRng;
use tos_crypto::curve25519_dalek::{
    ristretto::CompressedRistretto,
    traits::{IsIdentity, VartimeMultiscalarMul},
    RistrettoPoint, Scalar,
};
use tos_crypto::merlin::Transcript;
use zeroize::Zeroize;

use super::{BatchCollector, ProofVerificationError, G, H};
use crate::{
    crypto::{
        elgamal::{
            DecompressionError, DecryptHandle, PedersenCommitment, PedersenOpening, PublicKey,
            RISTRETTO_COMPRESSED_SIZE, SCALAR_SIZE,
        },
        ProtocolTranscript,
    },
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// Shield Commitment Proof
///
/// This proof verifies that a Shield transfer commitment is correctly formed.
/// Given public amount `x`, commitment `C`, receiver handle `D`, and receiver public key `P`,
/// this proves that:
/// - C = x*G + r*H (commitment contains the claimed amount)
/// - D = r*P (handle uses the same opening r)
///
/// This is a DLOG equality proof (Chaum-Pedersen protocol) on:
/// - R = C - x*G = r*H
/// - D = r*P
///
/// It proves log_H(R) = log_P(D) without revealing r.
#[allow(non_snake_case)]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ShieldCommitmentProof {
    /// First commitment: k*H
    Y_H: CompressedRistretto,
    /// Second commitment: k*P
    Y_P: CompressedRistretto,
    /// Response: z = k + c*r
    z: Scalar,
}

#[allow(non_snake_case)]
impl ShieldCommitmentProof {
    /// Create a new Shield commitment proof
    ///
    /// # Arguments
    /// * `receiver_pubkey` - The receiver's public key P
    /// * `_amount` - The plaintext amount being shielded (not used in proof generation,
    ///   but kept for API consistency; verifier uses it to compute R = C - amount*G)
    /// * `opening` - The Pedersen opening (random scalar r)
    /// * `transcript` - Fiat-Shamir transcript for non-interactive proof
    pub fn new(
        receiver_pubkey: &PublicKey,
        _amount: u64,
        opening: &PedersenOpening,
        transcript: &mut Transcript,
    ) -> Self {
        transcript.shield_commitment_proof_domain_separator();

        let P = receiver_pubkey.as_point();
        let r = opening.as_scalar();

        // Generate random k
        let mut k = Scalar::random(&mut OsRng);

        // Compute commitments Y_H = k*H and Y_P = k*P
        let Y_H = (&k * &*H).compress();
        let Y_P = (&k * P).compress();

        // Append to transcript
        transcript.append_point(b"Y_H", &Y_H);
        transcript.append_point(b"Y_P", &Y_P);

        // Get challenge
        let c = transcript.challenge_scalar(b"c");

        // Compute response z = k + c*r
        let z = &k + &(&c * r);

        // Finalize transcript
        transcript.challenge_scalar(b"w");

        // Zeroize sensitive data
        k.zeroize();

        Self { Y_H, Y_P, z }
    }

    /// Pre-verify the Shield commitment proof (for batch verification)
    ///
    /// # Arguments
    /// * `commitment` - The Pedersen commitment C
    /// * `receiver_pubkey` - The receiver's public key P
    /// * `receiver_handle` - The receiver's decrypt handle D
    /// * `amount` - The claimed plaintext amount
    /// * `transcript` - Fiat-Shamir transcript
    /// * `batch_collector` - Collector for batch verification
    pub fn pre_verify(
        &self,
        commitment: &PedersenCommitment,
        receiver_pubkey: &PublicKey,
        receiver_handle: &DecryptHandle,
        amount: u64,
        transcript: &mut Transcript,
        batch_collector: &mut BatchCollector,
    ) -> Result<(), ProofVerificationError> {
        transcript.shield_commitment_proof_domain_separator();

        // Validate and append points
        transcript.validate_and_append_point(b"Y_H", &self.Y_H)?;
        transcript.validate_and_append_point(b"Y_P", &self.Y_P)?;

        // Get challenge
        let c = transcript.challenge_scalar(b"c");
        transcript.challenge_scalar(b"w");

        // Decompress points
        let Y_H = self.Y_H.decompress().ok_or(DecompressionError)?;
        let Y_P = self.Y_P.decompress().ok_or(DecompressionError)?;

        let C = commitment.as_point();
        let P = receiver_pubkey.as_point();
        let D = receiver_handle.as_point();

        // Compute R = C - amount*G
        let x = Scalar::from(amount);
        let R = C - &x * &*G;

        // Verification equations:
        // z*H = Y_H + c*R  =>  z*H - c*R - Y_H = 0
        // z*P = Y_P + c*D  =>  z*P - c*D - Y_P = 0

        let batch_factor = Scalar::random(&mut OsRng);

        // First equation: z*H - c*R - Y_H = 0
        batch_collector.h_scalar += self.z * batch_factor;
        batch_collector.dynamic_scalars.extend(
            [
                -c,           // -c (for R)
                -Scalar::ONE, // -1 (for Y_H)
            ]
            .map(|s| s * batch_factor),
        );
        batch_collector.dynamic_points.extend([&R, &Y_H]);

        // Second equation: z*P - c*D - Y_P = 0 (with weight w for independence)
        let w = Scalar::random(&mut OsRng);
        let w_batch = w * batch_factor;

        batch_collector.dynamic_scalars.extend([
            self.z * w_batch,       // z (for P)
            -c * w_batch,           // -c (for D)
            -Scalar::ONE * w_batch, // -1 (for Y_P)
        ]);
        batch_collector.dynamic_points.extend([P, D, &Y_P]);

        Ok(())
    }

    /// Verify the Shield commitment proof directly (non-batch)
    ///
    /// # Arguments
    /// * `commitment` - The Pedersen commitment C
    /// * `receiver_pubkey` - The receiver's public key P
    /// * `receiver_handle` - The receiver's decrypt handle D
    /// * `amount` - The claimed plaintext amount
    /// * `transcript` - Fiat-Shamir transcript
    pub fn verify(
        &self,
        commitment: &PedersenCommitment,
        receiver_pubkey: &PublicKey,
        receiver_handle: &DecryptHandle,
        amount: u64,
        transcript: &mut Transcript,
    ) -> Result<(), ProofVerificationError> {
        transcript.shield_commitment_proof_domain_separator();

        // Validate and append points
        transcript.validate_and_append_point(b"Y_H", &self.Y_H)?;
        transcript.validate_and_append_point(b"Y_P", &self.Y_P)?;

        // Get challenge
        let c = transcript.challenge_scalar(b"c");
        transcript.challenge_scalar(b"w");

        // Decompress points
        let Y_H = self.Y_H.decompress().ok_or(DecompressionError)?;
        let Y_P = self.Y_P.decompress().ok_or(DecompressionError)?;

        let C = commitment.as_point();
        let P = receiver_pubkey.as_point();
        let D = receiver_handle.as_point();

        // Compute R = C - amount*G
        let x = Scalar::from(amount);
        let R = C - &x * &*G;

        // Verification equations:
        // z*H = Y_H + c*R  (equation 1)
        // z*P = Y_P + c*D  (equation 2)

        let check = RistrettoPoint::vartime_multiscalar_mul(
            vec![
                &self.z,         // z (for H)
                &(-&c),          // -c (for R)
                &(-Scalar::ONE), // -1 (for Y_H)
                &self.z,         // z (for P)
                &(-&c),          // -c (for D)
                &(-Scalar::ONE), // -1 (for Y_P)
            ],
            vec![&(*H), &R, &Y_H, P, D, &Y_P],
        );

        if check.is_identity() {
            Ok(())
        } else {
            Err(ProofVerificationError::GenericProof)
        }
    }
}

#[allow(non_snake_case)]
impl Serializer for ShieldCommitmentProof {
    fn write(&self, writer: &mut Writer) {
        self.Y_H.write(writer);
        self.Y_P.write(writer);
        self.z.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let Y_H = CompressedRistretto::read(reader)?;
        let Y_P = CompressedRistretto::read(reader)?;
        let z = Scalar::read(reader)?;

        Ok(Self { Y_H, Y_P, z })
    }

    fn size(&self) -> usize {
        RISTRETTO_COMPRESSED_SIZE * 2 + SCALAR_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;

    #[test]
    fn test_shield_commitment_proof_valid() {
        let mut transcript = Transcript::new(b"test");
        let receiver_keypair = KeyPair::new();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 1000u64;
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let receiver_handle = receiver_pubkey.decrypt_handle(&opening);

        // Generate proof
        let proof = ShieldCommitmentProof::new(receiver_pubkey, amount, &opening, &mut transcript);

        // Verify proof
        let mut verify_transcript = Transcript::new(b"test");
        let result = proof.verify(
            &commitment,
            receiver_pubkey,
            &receiver_handle,
            amount,
            &mut verify_transcript,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_shield_commitment_proof_wrong_amount() {
        let mut transcript = Transcript::new(b"test");
        let receiver_keypair = KeyPair::new();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 1000u64;
        let wrong_amount = 2000u64;
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let receiver_handle = receiver_pubkey.decrypt_handle(&opening);

        // Generate proof with correct amount
        let proof = ShieldCommitmentProof::new(receiver_pubkey, amount, &opening, &mut transcript);

        // Verify with wrong amount should fail
        let mut verify_transcript = Transcript::new(b"test");
        let result = proof.verify(
            &commitment,
            receiver_pubkey,
            &receiver_handle,
            wrong_amount, // Wrong amount
            &mut verify_transcript,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_shield_commitment_proof_batch_verify() {
        let mut transcript = Transcript::new(b"test");
        let receiver_keypair = KeyPair::new();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 500u64;
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let receiver_handle = receiver_pubkey.decrypt_handle(&opening);

        // Generate proof
        let proof = ShieldCommitmentProof::new(receiver_pubkey, amount, &opening, &mut transcript);

        // Batch verify
        let mut verify_transcript = Transcript::new(b"test");
        let mut batch_collector = BatchCollector::default();

        let result = proof.pre_verify(
            &commitment,
            receiver_pubkey,
            &receiver_handle,
            amount,
            &mut verify_transcript,
            &mut batch_collector,
        );
        assert!(result.is_ok());
        assert!(batch_collector.verify().is_ok());
    }

    #[test]
    fn test_shield_commitment_proof_serialization() {
        let mut transcript = Transcript::new(b"test");
        let receiver_keypair = KeyPair::new();
        let receiver_pubkey = receiver_keypair.get_public_key();

        let amount = 100u64;
        let opening = PedersenOpening::generate_new();

        let proof = ShieldCommitmentProof::new(receiver_pubkey, amount, &opening, &mut transcript);

        // Serialize
        let bytes = proof.to_bytes();

        // Deserialize
        let mut reader = Reader::new(&bytes);
        let restored = ShieldCommitmentProof::read(&mut reader).unwrap();

        // Verify size matches
        assert_eq!(proof.size(), bytes.len());

        // Verify content matches
        assert_eq!(proof.Y_H, restored.Y_H);
        assert_eq!(proof.Y_P, restored.Y_P);
        assert_eq!(proof.z, restored.z);
    }
}
