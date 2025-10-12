use curve25519_dalek::{
    constants::RISTRETTO_BASEPOINT_POINT,
    ecdlp::{self, ECDLPArguments, ECDLPTablesFileView},
    ristretto::RistrettoPoint,
    Scalar
};
use rand::rngs::OsRng;
use serde::{Deserialize, Deserializer, Serialize};
use sha3::Sha3_512;
use thiserror::Error;
use zeroize::Zeroize;
use crate::{
    api::DataElement,
    config::MAXIMUM_SUPPLY,
    crypto::{
        proofs::H,
        Address,
        AddressType,
        Hash
    },
    serializer::{
        Reader,
        ReaderError,
        Serializer,
        Writer
    }
};
use super::{
    ciphertext::Ciphertext,
    hash_and_point_to_scalar,
    pedersen::{DecryptHandle, PedersenCommitment, PedersenOpening},
    CompressedPublicKey,
    Signature
};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum KeyError {
    #[error("scalar is zero")]
    ZeroScalar,
    #[error("weak entropy: scalar value too small")]
    WeakEntropy,
}

#[derive(Clone, PartialEq, Eq)]
pub struct PublicKey(RistrettoPoint);

#[derive(Clone, Zeroize)]
pub struct PrivateKey(Scalar);

#[derive(Clone)]
pub struct KeyPair {
    public_key: PublicKey,
    private_key: PrivateKey,
}

impl PublicKey {
    // Create a public key from a point
    pub fn from_point(p: RistrettoPoint) -> Self {
        Self(p)
    }

    // Create a public key from a 32 byte hash
    // The hash will be hashed again to output a 64 byte hash
    pub fn from_hash(hash: &Hash) -> Self {
        Self(RistrettoPoint::hash_from_bytes::<Sha3_512>(hash.as_bytes()))
    }

    // Create a new public key from a private key using STANDARD construction: P = s * G
    // Private key must not be zero and must have sufficient entropy
    pub fn new(secret: &PrivateKey) -> Result<Self, KeyError> {
        let s = secret.as_scalar();

        // Validate non-zero
        if s == &Scalar::ZERO {
            return Err(KeyError::ZeroScalar);
        }

        // Validate sufficient entropy (not a small value)
        // Check that scalar is at least 2^32 to prevent weak keys
        // We convert to bytes and check the value since Scalar doesn't implement PartialOrd
        let bytes = s.to_bytes();
        let mut is_weak = true;
        // Check if any of the upper 28 bytes are non-zero (scalar >= 2^32)
        for i in 4..32 {
            if bytes[i] != 0 {
                is_weak = false;
                break;
            }
        }
        // If all upper bytes are zero, check if lower 4 bytes represent a value >= 2^32
        if is_weak && (bytes[0] != 0 || bytes[1] != 0 || bytes[2] != 0 || bytes[3] != 0) {
            // Value is less than 2^32
            return Err(KeyError::WeakEntropy);
        } else if is_weak {
            // All bytes are zero which means zero scalar (already checked above)
            return Err(KeyError::WeakEntropy);
        }

        // Use STANDARD construction: P = s * G (not s^(-1) * H)
        Ok(Self(s * RISTRETTO_BASEPOINT_POINT))
    }

    // Encrypt an amount to a Ciphertext
    pub fn encrypt<T: Into<Scalar>>(&self, amount: T) -> Ciphertext {
        let (commitment, opening) = PedersenCommitment::new(amount);
        let handle = self.decrypt_handle(&opening);

        Ciphertext::new(commitment, handle)
    }

    // Encrypt an amount to a Ciphertext with a given opening
    pub fn encrypt_with_opening<T: Into<Scalar>>(&self, amount: T, opening: &PedersenOpening) -> Ciphertext {
        let commitment = PedersenCommitment::new_with_opening(amount, opening);
        let handle = self.decrypt_handle(opening);

        Ciphertext::new(commitment, handle)
    }

    // Create a new decrypt handle from a Pedersen opening
    pub fn decrypt_handle(&self, opening: &PedersenOpening) -> DecryptHandle {
        DecryptHandle::new(&self, opening)
    }

    // Get the public key as a compressed point
    pub fn compress(&self) -> CompressedPublicKey {
        CompressedPublicKey::new(self.0.compress())
    }

    // Get the public key as a point
    pub fn as_point(&self) -> &RistrettoPoint {
        &self.0
    }

    // Convert the public key to an address
    pub fn to_address(&self, mainnet: bool) -> Address {
        Address::new(mainnet, AddressType::Normal, self.compress())
    }

    // Convert the public key to an address with data integrated
    pub fn to_address_with(&self, mainnet: bool, data: DataElement) -> Address {
        Address::new(mainnet, AddressType::Data(data), self.compress())
    }
}

impl PrivateKey {
    // Create a new private key from a scalar
    // The scalar must not be zero
    pub fn from_scalar(scalar: Scalar) -> Self {
        assert!(scalar != Scalar::ZERO);

        Self(scalar)
    }

    // Returns the private key as a scalar
    pub fn as_scalar(&self) -> &Scalar {
        &self.0
    }

    // Decrypt a Ciphertext to a point
    pub fn decrypt_to_point(&self, ciphertext: &Ciphertext) -> RistrettoPoint {
        let commitment = ciphertext.commitment().as_point();
        let handle = ciphertext.handle().as_point();

        commitment - &(self.0 * handle)
    }

    // Decode a point to a u64 with precomputed tables
    pub fn decode_point(&self, precomputed_tables: &ECDLPTablesFileView, point: RistrettoPoint) -> Option<u64> {
        self.decode_point_within_range(precomputed_tables, point, 0, MAXIMUM_SUPPLY as _)
    }

    // Decode a point to a u64 with precomputed tables within the requested range
    pub fn decode_point_within_range(
        &self,
        precomputed_tables: &ECDLPTablesFileView,
        point: RistrettoPoint,
        range_min: i64,
        range_max: i64
    ) -> Option<u64> {
        let args = ECDLPArguments::new_with_range(range_min, range_max);

        ecdlp::decode(precomputed_tables, point, args)
            .map(|x| x as u64)
    }

    // Decrypt a Ciphertext to a u64 with precomputed tables
    pub fn decrypt(&self, precomputed_tables: &ECDLPTablesFileView, ciphertext: &Ciphertext) -> Option<u64> {
        let point = self.decrypt_to_point(ciphertext);
        self.decode_point(precomputed_tables, point)
    }
}

impl KeyPair {
    // Generate a random new KeyPair
    pub fn new() -> Self {
        loop {
            let scalar = Scalar::random(&mut OsRng);
            let private_key = PrivateKey::from_scalar(scalar);

            // Random scalars from OsRng should always be valid, but handle it safely
            if let Ok(keypair) = Self::from_private_key(private_key) {
                return keypair;
            }
            // Extremely unlikely to reach here, but retry if we somehow got a weak key
        }
    }

    // Generate a key pair from a private key
    pub fn from_private_key(private_key: PrivateKey) -> Result<Self, KeyError> {
        let public_key = PublicKey::new(&private_key)?;
        Ok(Self {
            public_key,
            private_key,
        })
    }

    // Create a new key pair from a public and private key
    pub fn from_keys(public_key: PublicKey, private_key: PrivateKey) -> Self {
        KeyPair {
            public_key,
            private_key,
        }
    }

    // Decrypt a Ciphertext to a u64 with precomputed tables
    pub fn decrypt(&self, precomputed_tables: &ECDLPTablesFileView, ciphertext: &Ciphertext) -> Option<u64> {
        self.private_key.decrypt(precomputed_tables, ciphertext)
    }

    pub fn decrypt_to_point(&self, ciphertext: &Ciphertext) -> RistrettoPoint {
        self.private_key.decrypt_to_point(ciphertext)
    }

    // Sign a message with the private key
    pub fn sign(&self, message: &[u8]) -> Signature {
        let k = Scalar::random(&mut OsRng);
        let r = k * (*H);
        let e = hash_and_point_to_scalar(&self.public_key.compress(), message, &r);
        let s = self.private_key.as_scalar().invert() * e + k;
        Signature::new(s, e)
    }

    // Get the public key of the KeyPair
    pub fn get_public_key(&self) -> &PublicKey {
        &self.public_key
    }

    // Get the private key of the KeyPair
    pub fn get_private_key(&self) -> &PrivateKey {
        &self.private_key
    }

    // Split the KeyPair into its components
    pub fn split(self) -> (PublicKey, PrivateKey) {
        (self.public_key, self.private_key)
    }
}

impl Serializer for PrivateKey {
    fn write(&self, writer: &mut Writer) {
        self.0.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let scalar = Scalar::read(reader)?;
        Ok(PrivateKey::from_scalar(scalar))
    }

    fn size(&self) -> usize {
        self.0.size()
    }
}

impl Serialize for PrivateKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'a> Deserialize<'a> for PrivateKey {
    fn deserialize<D: Deserializer<'a>>(deserializer: D) -> Result<Self, D::Error> {
        let hex = String::deserialize(deserializer)?;
        PrivateKey::from_hex(&hex).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use curve25519_dalek::traits::Identity;

    use super::*;
    use super::super::G;

    // V-08 Security Tests: Test zero scalar rejection
    #[test]
    fn test_v08_zero_scalar_rejection() {
        let zero_key = PrivateKey(Scalar::ZERO);
        let result = PublicKey::new(&zero_key);
        assert!(matches!(result, Err(KeyError::ZeroScalar)));
    }

    // V-08 Security Tests: Test weak entropy rejection
    #[test]
    fn test_v08_weak_entropy_rejection() {
        // Test scalar below 2^32 threshold
        let weak_key = PrivateKey(Scalar::from(1000u64));
        let result = PublicKey::new(&weak_key);
        assert!(matches!(result, Err(KeyError::WeakEntropy)));

        // Test scalar at boundary (should still fail)
        let boundary_key = PrivateKey(Scalar::from((1u64 << 32) - 1));
        let result = PublicKey::new(&boundary_key);
        assert!(matches!(result, Err(KeyError::WeakEntropy)));
    }

    // V-08 Security Tests: Test valid key generation
    #[test]
    fn test_v08_valid_key_generation() {
        // Random keys should always be valid
        let keypair = KeyPair::new();
        assert!(keypair.get_public_key().as_point() != &RistrettoPoint::identity());

        // Test key above threshold
        let strong_scalar = Scalar::from(1u64 << 32);
        let strong_key = PrivateKey(strong_scalar);
        let result = PublicKey::new(&strong_key);
        assert!(result.is_ok());
    }

    // V-08 Security Tests: Test standard construction (P = s * G)
    #[test]
    fn test_v08_standard_construction() {
        let scalar = Scalar::from(1u64 << 33); // Well above threshold
        let private_key = PrivateKey(scalar);
        let public_key = PublicKey::new(&private_key).unwrap();

        // Verify standard construction: P = s * G
        let expected = scalar * RISTRETTO_BASEPOINT_POINT;
        assert_eq!(public_key.as_point(), &expected);
    }

    #[test]
    fn test_signature() {
        let keypair = KeyPair::new();
        let public_key = keypair.get_public_key();

        let message = b"Hello, world!";
        let signature = keypair.sign(message);
        assert!(signature.verify(message, public_key));
    }

    #[test]
    fn test_encrypt_decrypt() {
        let keypair = KeyPair::new();
        let public_key = keypair.get_public_key();
        let private_key = keypair.get_private_key();

        let amount = Scalar::from(10u64);
        let ciphertext = public_key.encrypt(amount);

        let decrypted = private_key.decrypt_to_point(&ciphertext);
        assert_eq!(decrypted, amount * &G);
    }

    #[test]
    fn test_identity() {
        let keypair = KeyPair::new();
        let public_key = keypair.get_public_key();
        let private_key = keypair.get_private_key();

        let amount = Scalar::from(0u64);
        let ciphertext = public_key.encrypt(amount);
        let decrypted = private_key.decrypt_to_point(&ciphertext);
        assert_eq!(decrypted, RistrettoPoint::identity());
    }

    #[test]
    fn test_universal_identity() {
        let keypair = KeyPair::new();
        let private_key = keypair.get_private_key();

        let ciphertext = Ciphertext::zero();
        let decrypted = private_key.decrypt_to_point(&ciphertext);
        assert_eq!(decrypted, RistrettoPoint::identity());
    }

    #[test]
    fn test_homomorphic_add() {
        let keypair = KeyPair::new();
        let public_key = keypair.get_public_key();
        let private_key = keypair.get_private_key();

        let amount1 = Scalar::from(10u64);
        let amount2 = Scalar::from(20u64);
        let c1 = public_key.encrypt(amount1);
        let c2 = public_key.encrypt(amount2);

        let sum = c1 + c2;
        let decrypted = private_key.decrypt_to_point(&sum);
        assert_eq!(decrypted, (amount1 + amount2) * &G);
    }

    #[test]
    fn test_homomorphic_add_scalar() {
        let keypair = KeyPair::new();
        let public_key = keypair.get_public_key();
        let private_key = keypair.get_private_key();

        let amount1 = Scalar::from(10u64);
        let amount2 = Scalar::from(20u64);
        let c1 = public_key.encrypt(amount1);

        let sum = c1 + amount2;
        let decrypted = private_key.decrypt_to_point(&sum);
        assert_eq!(decrypted, (amount1 + amount2) * &G);
    }

    #[test]
    fn test_homomorphic_sub() {
        let keypair = KeyPair::new();
        let public_key = keypair.get_public_key();
        let private_key = keypair.get_private_key();

        let amount1 = Scalar::from(20u64);
        let amount2 = Scalar::from(10u64);
        let c1 = public_key.encrypt(amount1);
        let c2 = public_key.encrypt(amount2);

        let sub = c1 - c2;
        let decrypted = private_key.decrypt_to_point(&sub);
        assert_eq!(decrypted, (amount1 - amount2) * &G);
    }

    #[test]
    fn test_homomorphic_sub_scalar() {
        let keypair = KeyPair::new();
        let public_key = keypair.get_public_key();
        let private_key = keypair.get_private_key();

        let amount1 = Scalar::from(20u64);
        let amount2 = Scalar::from(10u64);
        let c1 = public_key.encrypt(amount1);

        let sub = c1 - amount2;
        let decrypted = private_key.decrypt_to_point(&sub);
        assert_eq!(decrypted, (amount1 - amount2) * &G);
    }
}