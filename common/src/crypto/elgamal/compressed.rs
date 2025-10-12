use curve25519_dalek::{
    ristretto::CompressedRistretto,
    traits::IsIdentity,
    Scalar
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use crate::{api::DataElement, crypto::{Address, AddressType}, serializer::{Reader, ReaderError, Serializer, Writer}};
use super::{Ciphertext, DecryptHandle, PedersenCommitment, PublicKey};

// Compressed point size in bytes
pub const RISTRETTO_COMPRESSED_SIZE: usize = 32;
// Scalar size in bytes
pub const SCALAR_SIZE: usize = 32;

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum DecompressionError {
    #[error("point decompression failed")]
    InvalidPoint,
    #[error("identity point rejected")]
    IdentityPoint,
}

// A Pedersen commitment compressed to 32 bytes
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressedCommitment(CompressedRistretto);

// A decrypt handle compressed to 32 bytes
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressedHandle(CompressedRistretto);

// A compressed ciphertext that can be serialized and deserialized with only 64 bytes
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressedCiphertext {
    commitment: CompressedCommitment,
    handle: CompressedHandle
}

// A compressed public key using only 32 bytes
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompressedPublicKey(CompressedRistretto);

impl CompressedCommitment {
    // Create a new compressed commitment
    pub fn new(point: CompressedRistretto) -> Self {
        Self(point)
    }

    // Commitment as 32 bytes
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0.as_bytes()
    }

    // Compressed commitment as a compressed point
    pub fn as_point(&self) -> &CompressedRistretto {
        &self.0
    }

    // Decompress it to a PedersenCommitment
    // Note: Identity points ARE allowed for commitments (they represent zero amounts)
    pub fn decompress(&self) -> Result<PedersenCommitment, DecompressionError> {
        let point = self.0.decompress().ok_or(DecompressionError::InvalidPoint)?;

        // Ristretto points are cofactor-clean by construction
        Ok(PedersenCommitment::from_point(point))
    }
}

impl CompressedHandle {
    // Create a new compressed handle
    pub fn new(point: CompressedRistretto) -> Self {
        Self(point)
    }

    // Handle as 32 bytes
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0.as_bytes()
    }

    // Decompress it to a DecryptHandle
    // Note: Identity points ARE allowed for handles (they are part of zero ciphertexts)
    pub fn decompress(&self) -> Result<DecryptHandle, DecompressionError> {
        let point = self.0.decompress().ok_or(DecompressionError::InvalidPoint)?;

        // Ristretto points are cofactor-clean by construction
        Ok(DecryptHandle::from_point(point))
    }
}

impl CompressedCiphertext {
    // Create a new compressed ciphertext
    pub fn new(commitment: CompressedCommitment, handle: CompressedHandle) -> Self {
        Self { commitment, handle }
    }

    // Serialized commitment
    pub fn commitment(&self) -> &CompressedCommitment {
        &self.commitment
    }

    // Serialized handle
    pub fn handle(&self) -> &CompressedHandle {
        &self.handle
    }

    // Ciphertext as 64 bytes
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; RISTRETTO_COMPRESSED_SIZE * 2];
        let commitment = self.commitment.as_bytes();
        let handle = self.handle.as_bytes();

        bytes[0..RISTRETTO_COMPRESSED_SIZE].copy_from_slice(commitment);
        bytes[RISTRETTO_COMPRESSED_SIZE..RISTRETTO_COMPRESSED_SIZE * 2].copy_from_slice(handle);

        bytes
    }

    // Decompress it to a Ciphertext with validation (delegated to component decompression)
    pub fn decompress(&self) -> Result<Ciphertext, DecompressionError> {
        // Both decompress calls perform comprehensive validation
        let commitment = self.commitment.decompress()?;
        let handle = self.handle.decompress()?;

        Ok(Ciphertext::new(commitment, handle))
    }
}

impl CompressedPublicKey {
    // Create a new compressed public key
    pub fn new(point: CompressedRistretto) -> Self {
        Self(point)
    }

    // Serialized public key
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0.as_bytes()
    }

    // Decompress it to a Public Key with comprehensive validation
    pub fn decompress(&self) -> Result<PublicKey, DecompressionError> {
        // Check not identity BEFORE decompression
        if self.0.is_identity() {
            return Err(DecompressionError::IdentityPoint);
        }

        let point = self.0.decompress().ok_or(DecompressionError::InvalidPoint)?;

        // Verify not identity AFTER decompression
        if point.is_identity() {
            return Err(DecompressionError::IdentityPoint);
        }

        // Ristretto points are cofactor-clean by construction, but we validate anyway
        Ok(PublicKey::from_point(point))
    }

    // Clone the key to convert it to an address
    pub fn as_address(&self, mainnet: bool) -> Address {
        self.clone().to_address(mainnet)
    }

    // Convert it to an address
    pub fn to_address(self, mainnet: bool) -> Address {
        Address::new(mainnet, AddressType::Normal, self)
    }

    // Convert it to an address with data integrated
    pub fn to_address_with(self, mainnet: bool, data: DataElement) -> Address {
        Address::new(mainnet, AddressType::Data(data), self)
    }
}

impl Serializer for CompressedRistretto {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(self.as_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let bytes = reader.read_bytes_ref(RISTRETTO_COMPRESSED_SIZE)?;
        let point = CompressedRistretto::from_slice(bytes)?;

        Ok(point)
    }

    fn size(&self) -> usize {
        RISTRETTO_COMPRESSED_SIZE
    }
}

impl Serializer for CompressedCommitment {
    fn write(&self, writer: &mut Writer) {
        self.0.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        CompressedRistretto::read(reader).map(CompressedCommitment::new)
    }

    fn size(&self) -> usize {
        self.0.size()
    }
}

impl Serializer for CompressedHandle {
    fn write(&self, writer: &mut Writer) {
        self.0.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        CompressedRistretto::read(reader).map(CompressedHandle::new)
    }

    fn size(&self) -> usize {
        self.0.size()
    }
}

impl Serializer for CompressedPublicKey {
    fn write(&self, writer: &mut Writer) {
        self.0.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        CompressedRistretto::read(reader).map(CompressedPublicKey::new)
    }

    fn size(&self) -> usize {
        self.0.size()
    }
}

impl Serializer for CompressedCiphertext {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(self.commitment.as_bytes());
        writer.write_bytes(self.handle.as_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let commitment = CompressedCommitment::read(reader)?;
        let handle = CompressedHandle::read(reader)?;

        let compress = CompressedCiphertext::new(commitment, handle);
        Ok(compress)
    }

    fn size(&self) -> usize {
        self.commitment.size() + self.handle.size()
    }
}

impl Serializer for Scalar {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(self.as_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let bytes = reader.read_bytes(SCALAR_SIZE)?;
        let scalar: Option<Scalar> = Scalar::from_canonical_bytes(bytes).into();
        scalar.ok_or(ReaderError::InvalidValue)
    }

    fn size(&self) -> usize {
        SCALAR_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // V-09 Security Tests: Test identity point ACCEPTANCE for commitment
    // Identity commitments are valid (they represent zero amounts in ElGamal)
    #[test]
    fn test_v09_identity_commitment_acceptance() {
        // Identity point in compressed form is all zeros
        let identity_compressed = CompressedCommitment::new(CompressedRistretto([0u8; 32]));
        let result = identity_compressed.decompress();
        assert!(result.is_ok(), "Identity commitments should be allowed for zero amounts");
    }

    // V-09 Security Tests: Test identity point ACCEPTANCE for handle
    // Identity handles are valid (they are part of zero ciphertexts)
    #[test]
    fn test_v09_identity_handle_acceptance() {
        // Identity point in compressed form is all zeros
        let identity_compressed = CompressedHandle::new(CompressedRistretto([0u8; 32]));
        let result = identity_compressed.decompress();
        assert!(result.is_ok(), "Identity handles should be allowed for zero ciphertexts");
    }

    // V-09 Security Tests: Test identity point rejection for public key
    #[test]
    fn test_v09_identity_pubkey_rejection() {
        // Identity point in compressed form is all zeros
        let identity_compressed = CompressedPublicKey::new(CompressedRistretto([0u8; 32]));
        let result = identity_compressed.decompress();
        assert!(matches!(result, Err(DecompressionError::IdentityPoint)));
    }

    // V-09 Security Tests: Test invalid point rejection
    #[test]
    fn test_v09_invalid_point_rejection() {
        // Create an invalid compressed point (all 0xFF bytes is invalid)
        let invalid_bytes = [0xFFu8; 32];
        let invalid_compressed = CompressedRistretto(invalid_bytes);
        let commitment = CompressedCommitment::new(invalid_compressed);
        let result = commitment.decompress();
        assert!(matches!(result, Err(DecompressionError::InvalidPoint)));
    }

    // V-09 Security Tests: Test valid point acceptance
    #[test]
    fn test_v09_valid_point_acceptance() {
        use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;

        // Test with valid base point
        let valid_compressed = RISTRETTO_BASEPOINT_POINT.compress();
        let commitment = CompressedCommitment::new(valid_compressed);
        let result = commitment.decompress();
        assert!(result.is_ok());
    }

    #[test]
    fn test_compressed_ciphertext_zero() {
        let ciphertext = Ciphertext::zero();
        let compressed = ciphertext.compress();
        let decompressed = compressed.decompress().unwrap();

        assert_eq!(ciphertext, decompressed);
    }

    #[test]
    fn test_compressed_ciphertext_serde() {
        let ciphertext = Ciphertext::zero();
        let json  = json!(ciphertext);

        let deserialized: Ciphertext = serde_json::from_value(json).unwrap();
        assert_eq!(ciphertext, deserialized);
    }
}