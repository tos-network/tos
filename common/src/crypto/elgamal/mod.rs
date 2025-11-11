mod pedersen;
mod signature;

pub use pedersen::*;
pub use signature::*;

// Import tos-crypto's H generator (used in sign() methods)
pub use tos_crypto::proofs::G;

// Re-export constants
pub const RISTRETTO_COMPRESSED_SIZE: usize = 32;
pub const SCALAR_SIZE: usize = 32;

// signature module re-exports: Signature, SIGNATURE_SIZE, hash_and_point_to_scalar

// Re-export curve25519_dalek types that are needed
use curve25519_dalek::{ristretto::CompressedRistretto, RistrettoPoint};
use curve25519_dalek::traits::IsIdentity;

// Minimal types needed for Pedersen commitments and proofs
// These were in compressed.rs and key.rs but we keep minimal versions here

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompressedCommitment(CompressedRistretto);

impl CompressedCommitment {
    pub fn new(point: CompressedRistretto) -> Self {
        Self(point)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }

    pub const fn as_point(&self) -> &CompressedRistretto {
        &self.0
    }

    pub fn decompress(&self) -> Result<PedersenCommitment, DecompressionError> {
        let point = self
            .0
            .decompress()
            .ok_or(DecompressionError::InvalidPoint)?;
        if point.is_identity() {
            return Err(DecompressionError::IdentityPoint);
        }
        Ok(PedersenCommitment::from_point(point))
    }
}

impl SerializerTrait for CompressedCommitment {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(self.as_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let bytes =
            reader.read_bytes::<[u8; RISTRETTO_COMPRESSED_SIZE]>(RISTRETTO_COMPRESSED_SIZE)?;
        let compressed =
            CompressedRistretto::from_slice(&bytes).map_err(|_| ReaderError::InvalidValue)?;
        Ok(Self(compressed))
    }

    fn size(&self) -> usize {
        RISTRETTO_COMPRESSED_SIZE
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompressedHandle(CompressedRistretto);

impl Default for CompressedHandle {
    fn default() -> Self {
        use curve25519_dalek::traits::Identity;
        Self(RistrettoPoint::identity().compress())
    }
}

impl CompressedHandle {
    pub fn new(point: CompressedRistretto) -> Self {
        Self(point)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }

    pub fn decompress(&self) -> Result<DecryptHandle, DecompressionError> {
        let point = self
            .0
            .decompress()
            .ok_or(DecompressionError::InvalidPoint)?;
        if point.is_identity() {
            return Err(DecompressionError::IdentityPoint);
        }
        Ok(DecryptHandle::from_point(point))
    }
}

impl SerializerTrait for CompressedHandle {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(self.as_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let bytes =
            reader.read_bytes::<[u8; RISTRETTO_COMPRESSED_SIZE]>(RISTRETTO_COMPRESSED_SIZE)?;
        let compressed =
            CompressedRistretto::from_slice(&bytes).map_err(|_| ReaderError::InvalidValue)?;
        Ok(Self(compressed))
    }

    fn size(&self) -> usize {
        RISTRETTO_COMPRESSED_SIZE
    }
}

// Import types for TOS-specific extensions
use crate::{
    crypto::{Address, AddressType, Hash},
    serializer::{Reader, ReaderError, Serializer as SerializerTrait, Writer},
};
use curve25519_dalek::Scalar;
use rand::rngs::OsRng;
use sha3::Sha3_512;
use zeroize::Zeroize;

// Schnorr signature key types (kept in tos-common for Address integration)

#[derive(Clone, Debug, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompressedPublicKey(CompressedRistretto);

impl CompressedPublicKey {
    pub fn new(point: CompressedRistretto) -> Self {
        Self(point)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }

    pub fn decompress(&self) -> Result<PublicKey, DecompressionError> {
        let point = self
            .0
            .decompress()
            .ok_or(DecompressionError::InvalidPoint)?;
        if point.is_identity() {
            return Err(DecompressionError::IdentityPoint);
        }
        Ok(PublicKey::from_point(point))
    }

    /// Convert to address for display/serialization
    pub fn as_address(&self, mainnet: bool) -> Address {
        Address::new(mainnet, AddressType::Normal, self.clone())
    }

    /// Create Address (alternative method if needed)
    pub fn to_address(&self, mainnet: bool) -> Address {
        self.as_address(mainnet)
    }
}

// Minimal PublicKey type needed for signatures and pedersen
#[derive(Clone, PartialEq, Eq)]
pub struct PublicKey(RistrettoPoint);

impl PublicKey {
    pub fn from_point(p: RistrettoPoint) -> Self {
        Self(p)
    }

    pub fn as_point(&self) -> &RistrettoPoint {
        &self.0
    }

    pub fn compress(&self) -> CompressedPublicKey {
        CompressedPublicKey::new(self.0.compress())
    }

    pub fn from_hash(hash: &Hash) -> Self {
        Self(RistrettoPoint::hash_from_bytes::<Sha3_512>(hash.as_bytes()))
    }

    pub fn to_address(&self, mainnet: bool) -> Address {
        Address::new(mainnet, AddressType::Normal, self.compress())
    }

    /// Create a decrypt handle using a Pedersen opening
    /// This is a convenience method that calls DecryptHandle::new
    pub fn decrypt_handle(&self, opening: &PedersenOpening) -> DecryptHandle {
        DecryptHandle::new(self, opening)
    }
}

// Error type for decompression
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecompressionError {
    #[error("point decompression failed")]
    InvalidPoint,
    #[error("identity point rejected")]
    IdentityPoint,
}

// Minimal PrivateKey implementation (for signatures only, no encryption)
#[derive(Clone, Zeroize, serde::Serialize, serde::Deserialize)]
pub struct PrivateKey(Scalar);

impl Default for PrivateKey {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivateKey {
    pub fn new() -> Self {
        Self(Scalar::random(&mut OsRng))
    }

    pub fn from_scalar(scalar: Scalar) -> Self {
        Self(scalar)
    }

    pub fn as_scalar(&self) -> &Scalar {
        &self.0
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self, ()> {
        let scalar = Scalar::from_canonical_bytes(*bytes)
            .into_option()
            .ok_or(())?;
        Ok(Self(scalar))
    }

    pub fn from_hex(hex: &str) -> Result<Self, hex::FromHexError> {
        let bytes: [u8; 32] = hex::decode(hex)?
            .as_slice()
            .try_into()
            .map_err(|_| hex::FromHexError::InvalidStringLength)?;
        Self::from_bytes(&bytes).map_err(|_| hex::FromHexError::InvalidStringLength)
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.to_bytes())
    }

    // Generate signature using tos-crypto's H generator
    #[allow(non_snake_case)]
    pub fn sign(&self, message: &[u8], public_key: &PublicKey) -> Signature {
        use crate::crypto::proofs::H;

        let r = Scalar::random(&mut OsRng);
        let R = (*H) * r;
        let e = hash_and_point_to_scalar(&public_key.compress(), message, &R);
        let s = r + (e * self.0);

        Signature::new(s, e)
    }
}

// Minimal KeyPair implementation
#[derive(Clone)]
pub struct KeyPair {
    public_key: PublicKey,
    private_key: PrivateKey,
}

impl Default for KeyPair {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyPair {
    pub fn new() -> Self {
        use crate::crypto::proofs::H;

        let private_key = PrivateKey::new();
        // Public key: P = H * private_key (standard Schnorr signature)
        let public_key = PublicKey::from_point((*H) * private_key.as_scalar());

        Self {
            public_key,
            private_key,
        }
    }

    pub fn from_private_key(private_key: PrivateKey) -> Result<Self, ()> {
        use crate::crypto::proofs::H;

        // Validate non-zero
        if private_key.as_scalar() == &Scalar::ZERO {
            return Err(());
        }

        // Public key: P = H * private_key (standard Schnorr signature)
        let public_key = PublicKey::from_point((*H) * private_key.as_scalar());

        Ok(Self {
            public_key,
            private_key,
        })
    }

    pub fn get_public_key(&self) -> &PublicKey {
        &self.public_key
    }

    pub fn get_private_key(&self) -> &PrivateKey {
        &self.private_key
    }

    pub fn split(self) -> (PublicKey, PrivateKey) {
        (self.public_key, self.private_key)
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        self.private_key.sign(message, &self.public_key)
    }
}

// Serializer implementation for CompressedPublicKey
impl SerializerTrait for CompressedPublicKey {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(self.as_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let bytes = reader.read_bytes::<[u8; 32]>(32)?;
        let compressed =
            CompressedRistretto::from_slice(&bytes).map_err(|_| ReaderError::InvalidValue)?;
        Ok(Self(compressed))
    }

    fn size(&self) -> usize {
        32
    }
}

// Serializer implementation for PrivateKey
impl SerializerTrait for PrivateKey {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(&self.0.to_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let bytes = reader.read_bytes::<[u8; 32]>(32)?;
        let scalar = Scalar::from_canonical_bytes(bytes)
            .into_option()
            .ok_or(ReaderError::InvalidValue)?;
        Ok(PrivateKey(scalar))
    }

    fn size(&self) -> usize {
        32
    }
}
