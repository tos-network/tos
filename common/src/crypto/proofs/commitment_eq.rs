use curve25519_dalek::{
    ristretto::CompressedRistretto,
    traits::{Identity, MultiscalarMul},
    RistrettoPoint,
    Scalar
};
use merlin::Transcript;
use rand::rngs::OsRng;
use subtle::ConstantTimeEq;
use zeroize::Zeroize;
use crate::{
    crypto::{
        elgamal::{
            DecompressionError,
            PedersenCommitment,
            PedersenOpening,
            PublicKey,
            RISTRETTO_COMPRESSED_SIZE,
            SCALAR_SIZE
        },
        KeyPair,
        ProtocolTranscript
    },
    serializer::{Reader, ReaderError, Serializer, Writer}
};
use super::{
    BatchCollector,
    ProofVerificationError,
    PC_GENS,
    G,
    H,
};

/// Proof that a commitment and ciphertext are equal.
#[allow(non_snake_case)]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct CommitmentEqProof {
    Y_0: CompressedRistretto,
    Y_1: CompressedRistretto,
    Y_2: CompressedRistretto,
    z_s: Scalar,
    z_x: Scalar,
    z_r: Scalar,
}

#[allow(non_snake_case)]
impl CommitmentEqProof {
    // warning: caller must make sure not to forget to hash the public key, ciphertext, commitment in the transcript as it is not done here
    // TODO: This proof is no longer needed with plain balances - stub for compilation
    pub fn new(
        source_keypair: &KeyPair,
        source_balance: u64,
        opening: &PedersenOpening,
        amount: u64,
        transcript: &mut Transcript,
    ) -> Self {
        Self::new_with_scalar(source_keypair, source_balance, opening, Scalar::from(amount), transcript)
    }

    // warning: caller must make sure not to forget to hash the public key, ciphertext, commitment in the transcript as it is not done here
    // TODO: This proof is no longer needed with plain balances - stub for compilation
    pub fn new_with_scalar(
        source_keypair: &KeyPair,
        _source_balance: u64,
        opening: &PedersenOpening,
        x: Scalar,
        transcript: &mut Transcript,
    ) -> Self {
        transcript.equality_proof_domain_separator();

        // TODO: This proof logic needs to be updated for plain balances
        // For now, create dummy proof structures
        let P_source = source_keypair.get_public_key().as_point();
        // Dummy handle for compilation
        let D_source = P_source;

        let s = source_keypair.get_private_key().as_scalar();
        let r = opening.as_scalar();

        // generate random masking factors that also serves as nonces
        let mut y_s = Scalar::random(&mut OsRng);
        let mut y_x = Scalar::random(&mut OsRng);
        let mut y_r = Scalar::random(&mut OsRng);

        let Y_0 = (&y_s * P_source).compress();
        let Y_1 =
            RistrettoPoint::multiscalar_mul([&y_x, &y_s], [&G, D_source]).compress();
        let Y_2 = PC_GENS.commit(y_x, y_r).compress();

        // record masking factors in the transcript
        transcript.append_point(b"Y_0", &Y_0);
        transcript.append_point(b"Y_1", &Y_1);
        transcript.append_point(b"Y_2", &Y_2);

        let c = transcript.challenge_scalar(b"c");

        // compute the masked values
        let z_s = &(&c * s) + &y_s;
        let z_x = &(&c * x) + &y_x;
        let z_r = &(&c * r) + &y_r;

        // transcript.append_scalar(b"z_s", &z_s);
        // transcript.append_scalar(b"z_x", &z_x);
        // transcript.append_scalar(b"z_r", &z_r);

        transcript.challenge_scalar(b"w");

        // zeroize random scalars
        y_s.zeroize();
        y_x.zeroize();
        y_r.zeroize();

        Self {
            Y_0,
            Y_1,
            Y_2,
            z_s,
            z_x,
            z_r,
        }
    }

    /// Verify that the commitment and ciphertext are equal.
    /// This function is used for batch verification.
    /// TODO: This proof is no longer needed with plain balances - stub for compilation
    pub fn pre_verify(
        &self,
        source_pubkey: &PublicKey,
        _source_balance: u64,
        destination_commitment: &PedersenCommitment,
        transcript: &mut Transcript,
        batch_collector: &mut BatchCollector,
    ) -> Result<(), ProofVerificationError> {
        transcript.equality_proof_domain_separator();

        // TODO: This proof logic needs to be updated for plain balances
        // extract the relevant scalar and Ristretto points from the inputs
        let P_source = source_pubkey.as_point();
        // Dummy commitment/handle for compilation
        let C_source = destination_commitment.as_point();
        let D_source = P_source;
        let C_destination = destination_commitment.as_point();

        // include Y_0, Y_1, Y_2 to transcript and extract challenges
        transcript.validate_and_append_point(b"Y_0", &self.Y_0)?;
        transcript.validate_and_append_point(b"Y_1", &self.Y_1)?;
        transcript.validate_and_append_point(b"Y_2", &self.Y_2)?;

        let c = transcript.challenge_scalar(b"c");

        let mut cloned = transcript.clone();

        cloned.append_scalar(b"z_s", &self.z_s);
        cloned.append_scalar(b"z_x", &self.z_x);
        cloned.append_scalar(b"z_r", &self.z_r);

        let w = cloned.challenge_scalar(b"w"); // w used for batch verification
        transcript.challenge_scalar(b"w");

        let ww = &w * &w;

        let w_negated = -&w;
        let ww_negated = -&ww;

        // check that the required algebraic condition holds
        let Y_0 = self
            .Y_0
            .decompress()
            .ok_or(DecompressionError::InvalidPoint)?;
        let Y_1 = self
            .Y_1
            .decompress()
            .ok_or(DecompressionError::InvalidPoint)?;
        let Y_2 = self
            .Y_2
            .decompress()
            .ok_or(DecompressionError::InvalidPoint)?;

        let batch_factor = Scalar::random(&mut OsRng);

        // w * z_x * G + ww * z_x * G
        batch_collector.g_scalar += (w * self.z_x + ww * self.z_x) * batch_factor;
        // -c * H + ww * z_r * H
        batch_collector.h_scalar += (-c + ww * self.z_r) * batch_factor;

        batch_collector.dynamic_scalars.extend(
            [
                self.z_s,       // z_s
                -Scalar::ONE,   // -identity
                w * self.z_s,   // w * z_s
                w_negated * c,  // -w * c
                w_negated,      // -w
                ww_negated * c, // -ww * c
                ww_negated,     // -ww
            ]
            .map(|s| s * batch_factor),
        );
        batch_collector.dynamic_points.extend([
            P_source,      // P_source
            &Y_0,          // Y_0
            D_source,      // D_source
            C_source,      // C_source
            &Y_1,          // Y_1
            C_destination, // C_destination
            &Y_2,          // Y_2
        ]);

        Ok(())
    }

    /// Verify that the commitment and ciphertext are equal.
    /// This function is used for individual verification without batch collector.
    /// TODO: This proof is no longer needed with plain balances - stub for compilation
    pub fn verify(
        &self,
        source_pubkey: &PublicKey,
        _balance: u64,
        commitment: &PedersenCommitment,
        transcript: &mut Transcript,
    ) -> Result<(), ProofVerificationError> {
        transcript.equality_proof_domain_separator();

        // TODO: This proof logic needs to be updated for plain balances
        // extract the relevant scalar and Ristretto points from the inputs
        let P = source_pubkey.as_point();
        // Dummy commitment/handle for compilation
        let C_ciphertext = commitment.as_point();
        let D = P;
        let C_commitment = commitment.as_point();

        // include Y_0, Y_1, Y_2 to transcript and extract challenges
        transcript.validate_and_append_point(b"Y_0", &self.Y_0)?;
        transcript.validate_and_append_point(b"Y_1", &self.Y_1)?;
        transcript.validate_and_append_point(b"Y_2", &self.Y_2)?;

        let c = transcript.challenge_scalar(b"c");

        transcript.append_scalar(b"z_s", &self.z_s);
        transcript.append_scalar(b"z_x", &self.z_x);
        transcript.append_scalar(b"z_r", &self.z_r);

        let w = transcript.challenge_scalar(b"w");
        let ww = &w * &w;

        let w_negated = -&w;
        let ww_negated = -&ww;

        // check that the required algebraic condition holds
        let Y_0 = self
            .Y_0
            .decompress()
            .ok_or(DecompressionError::InvalidPoint)?;
        let Y_1 = self
            .Y_1
            .decompress()
            .ok_or(DecompressionError::InvalidPoint)?;
        let Y_2 = self
            .Y_2
            .decompress()
            .ok_or(DecompressionError::InvalidPoint)?;

        // Use constant-time multiscalar multiplication to prevent timing attacks
        let check = RistrettoPoint::multiscalar_mul(
            [
                self.z_s,           // z_s
                -c,                 // -c
                -Scalar::ONE,       // -identity
                w * self.z_x,       // w * z_x
                w * self.z_s,       // w * z_s
                w_negated * c,      // -w * c
                w_negated,          // -w
                ww * self.z_x,      // ww * z_x
                ww * self.z_r,      // ww * z_r
                ww_negated * c,     // -ww * c
                ww_negated,         // -ww
            ],
            [
                *P,           // P
                *H,           // H
                Y_0,          // Y_0
                *G,           // G
                *D,           // D
                *C_ciphertext, // C_ciphertext
                Y_1,          // Y_1
                *G,           // G
                *H,           // H
                *C_commitment, // C_commitment
                Y_2,          // Y_2
            ],
        );

        // Use constant-time comparison to prevent timing leaks
        if bool::from(check.ct_eq(&RistrettoPoint::identity())) {
            Ok(())
        } else {
            Err(ProofVerificationError::CommitmentEqProof)
        }
    }
}

#[allow(non_snake_case)]
impl Serializer for CommitmentEqProof {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(self.Y_0.as_bytes());
        writer.write_bytes(self.Y_1.as_bytes());
        writer.write_bytes(self.Y_2.as_bytes());
        writer.write_bytes(&self.z_s.to_bytes());
        writer.write_bytes(&self.z_x.to_bytes());
        writer.write_bytes(&self.z_r.to_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let Y_0_bytes = reader.read_bytes::<[u8; 32]>(32)?;
        let Y_0 = CompressedRistretto::from_slice(&Y_0_bytes).map_err(|_| ReaderError::InvalidValue)?;
        let Y_1_bytes = reader.read_bytes::<[u8; 32]>(32)?;
        let Y_1 = CompressedRistretto::from_slice(&Y_1_bytes).map_err(|_| ReaderError::InvalidValue)?;
        let Y_2_bytes = reader.read_bytes::<[u8; 32]>(32)?;
        let Y_2 = CompressedRistretto::from_slice(&Y_2_bytes).map_err(|_| ReaderError::InvalidValue)?;
        let z_s_bytes = reader.read_bytes::<[u8; 32]>(32)?;
        let z_s = Scalar::from_canonical_bytes(z_s_bytes)
            .into_option()
            .ok_or(ReaderError::InvalidValue)?;
        let z_x_bytes = reader.read_bytes::<[u8; 32]>(32)?;
        let z_x = Scalar::from_canonical_bytes(z_x_bytes)
            .into_option()
            .ok_or(ReaderError::InvalidValue)?;
        let z_r_bytes = reader.read_bytes::<[u8; 32]>(32)?;
        let z_r = Scalar::from_canonical_bytes(z_r_bytes)
            .into_option()
            .ok_or(ReaderError::InvalidValue)?;

        Ok(Self { Y_0, Y_1, Y_2, z_s, z_x, z_r })
    }

    fn size(&self) -> usize {
        RISTRETTO_COMPRESSED_SIZE * 3 + SCALAR_SIZE * 3
    }    
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Disabled: encrypt() method removed after balance simplification
    fn test_commitment_eq_proof() {
        // TODO: Rewrite this test for plain u64 balances without encryption
        // This test verified commitment equality proofs for encrypted balances
        // After balance simplification, this needs to be refactored or removed
    }
}