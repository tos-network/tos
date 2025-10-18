use curve25519_dalek::{RistrettoPoint, Scalar};
use serde::{de::Error, Serialize};
use sha3::{Digest, Sha3_512};
use crate::{
    crypto::proofs::H,
    serializer::{
        Reader,
        ReaderError,
        Serializer,
        Writer
    }
};
use super::{CompressedPublicKey, PublicKey};

// SCALAR_SIZE moved to parent module
const SCALAR_SIZE: usize = 32;

pub const SIGNATURE_SIZE: usize = SCALAR_SIZE * 2;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Signature {
    s: Scalar,
    e: Scalar,
}

impl Signature {
    pub fn new(s: Scalar, e: Scalar) -> Self {
        Self { s, e }
    }

    // Verify the signature using the Public Key and the hash of the message
    pub fn verify(&self, message: &[u8], key: &PublicKey) -> bool {
        let r = (*H) * &self.s + key.as_point() * -self.e;
        let calculated = hash_and_point_to_scalar(&key.compress(), message, &r);
        self.e == calculated
    }
}

// Create a Scalar from Public Key, Hash of the message, and selected point
pub fn hash_and_point_to_scalar(key: &CompressedPublicKey, message: &[u8], point: &RistrettoPoint) -> Scalar {
    let mut hasher = Sha3_512::new();
    hasher.update(key.as_bytes());
    hasher.update(message);
    hasher.update(point.compress().as_bytes());

    let hash = hasher.finalize();
    Scalar::from_bytes_mod_order_wide(&hash.try_into().unwrap())
}

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer
    {
        serializer.serialize_str(&hex::encode(&self.to_bytes()))
    }
}

impl<'de> serde::Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from_hex(&s).map_err(D::Error::custom)?)
    }
}

impl Serializer for Signature {
    fn write(&self, writer: &mut Writer) {
        writer.write_bytes(&self.s.to_bytes());
        writer.write_bytes(&self.e.to_bytes());
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let s_bytes = reader.read_bytes::<[u8; SCALAR_SIZE]>(SCALAR_SIZE)?;
        let e_bytes = reader.read_bytes::<[u8; SCALAR_SIZE]>(SCALAR_SIZE)?;

        use curve25519_dalek::scalar::Scalar;
        let s = Scalar::from_canonical_bytes(s_bytes)
            .into_option()
            .ok_or(ReaderError::InvalidValue)?;
        let e = Scalar::from_canonical_bytes(e_bytes)
            .into_option()
            .ok_or(ReaderError::InvalidValue)?;
        Ok(Signature::new(s, e))
    }

    fn size(&self) -> usize {
        SIGNATURE_SIZE
    }
}