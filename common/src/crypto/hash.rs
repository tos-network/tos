use crate::{
    block::Algorithm,
    serializer::{Reader, ReaderError, Serializer, Writer},
};
use blake3::hash as blake3_hash;
use serde::de::Error as SerdeError;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    convert::TryInto,
    fmt::{Display, Error, Formatter},
    hash::Hasher,
    str::FromStr,
};

pub use tos_hash::Error as TosHashError;
use tos_hash::{v1, v2};

pub const HASH_SIZE: usize = 32; // 32 bytes / 256 bits

#[derive(Eq, PartialEq, PartialOrd, Ord, Clone, Debug)]
pub struct Hash([u8; HASH_SIZE]);

impl Hash {
    pub const fn new(bytes: [u8; HASH_SIZE]) -> Self {
        Hash(bytes)
    }

    pub const fn zero() -> Self {
        Hash::new([0; HASH_SIZE])
    }

    pub const fn max() -> Self {
        Hash::new([u8::MAX; HASH_SIZE])
    }

    pub fn as_bytes(&self) -> &[u8; HASH_SIZE] {
        &self.0
    }

    pub fn to_bytes(self) -> [u8; HASH_SIZE] {
        self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl FromStr for Hash {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s).map_err(|_| "Invalid hex string")?;
        let bytes: [u8; HASH_SIZE] = bytes.try_into().map_err(|_| "Invalid hash")?;
        Ok(Hash::new(bytes))
    }
}

// Hash a byte array using the blake3 algorithm
#[inline(always)]
pub fn hash(value: &[u8]) -> Hash {
    let result: [u8; HASH_SIZE] = blake3_hash(value).into();
    Hash(result)
}

// Perform a PoW hash using the given algorithm
pub fn pow_hash(work: &[u8], algorithm: Algorithm) -> Result<Hash, TosHashError> {
    match algorithm {
        Algorithm::V1 => {
            let mut scratchpad = v1::ScratchPad::default();

            // Make sure the input has good alignment
            let mut input = v1::AlignedInput::default();
            let slice = input.as_mut_slice()?;
            slice[..work.len()].copy_from_slice(work);

            v1::tos_hash(slice, &mut scratchpad)
        }
        Algorithm::V2 => {
            let mut scratchpad = v2::ScratchPad::default();
            v2::tos_hash(work, &mut scratchpad)
        }
    }
    .map(Hash::new)
}

impl Serializer for Hash {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let hash = reader.read_hash()?;
        Ok(hash)
    }

    fn write(&self, writer: &mut Writer) {
        writer.write_hash(self);
    }

    fn size(&self) -> usize {
        HASH_SIZE
    }
}

impl std::hash::Hash for Hash {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl AsRef<Hash> for Hash {
    fn as_ref(&self) -> &Hash {
        self
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        write!(f, "{}", &self.to_hex())
    }
}

impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'a> Deserialize<'a> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let hex = String::deserialize(deserializer)?;
        if hex.len() != HASH_SIZE * 2 {
            return Err(SerdeError::custom("Invalid hex length"));
        }

        let decoded_hex = hex::decode(hex).map_err(SerdeError::custom)?;
        let bytes: [u8; 32] = decoded_hex
            .try_into()
            .map_err(|_| SerdeError::custom("Could not transform hex to bytes array for Hash"))?;
        Ok(Hash::new(bytes))
    }
}

pub trait Hashable: Serializer {
    #[inline(always)]
    fn hash(&self) -> Hash {
        let bytes = self.to_bytes();
        hash(&bytes)
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl<'a> Into<Cow<'a, Hash>> for Hash {
    fn into(self) -> Cow<'a, Hash> {
        Cow::Owned(self)
    }
}

impl<'a> Into<Cow<'a, Hash>> for &'a Hash {
    fn into(self) -> Cow<'a, Hash> {
        Cow::Borrowed(self)
    }
}

/// Compute deterministic contract address (CREATE2-style)
///
/// Formula: address = blake3(0xff || deployer || code_hash)
///
/// This enables pre-computing contract addresses before deployment,
/// similar to Ethereum's CREATE2, but without salt parameter.
///
/// # Arguments
/// * `deployer` - Public key of the deployer
/// * `bytecode` - Contract module bytecode (WASM/ELF)
///
/// # Returns
/// Deterministic 32-byte contract address
///
/// # Example
/// ```
/// use tos_common::crypto::{compute_deterministic_contract_address, elgamal::CompressedPublicKey};
/// use tos_common::serializer::Serializer;
///
/// let deployer = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
/// let bytecode = b"contract bytecode";
/// let address = compute_deterministic_contract_address(&deployer, bytecode);
/// ```
pub fn compute_deterministic_contract_address(
    deployer: &crate::crypto::elgamal::CompressedPublicKey,
    bytecode: &[u8],
) -> Hash {
    // Step 1: Compute code_hash = blake3(bytecode)
    let code_hash = hash(bytecode);

    // Step 2: Prepare data = 0xff || deployer || code_hash
    let mut data = Vec::with_capacity(1 + 32 + 32);
    data.push(0xff); // CREATE2 prefix
    data.extend_from_slice(deployer.as_bytes());
    data.extend_from_slice(code_hash.as_bytes());

    // Step 3: Return address = blake3(data)
    hash(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_address_computation() {
        // Create a test deployer public key
        let deployer = crate::crypto::elgamal::CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let bytecode = b"test contract bytecode";

        // Compute address
        let addr1 = compute_deterministic_contract_address(&deployer, bytecode);

        // Same inputs = same address (deterministic)
        let addr2 = compute_deterministic_contract_address(&deployer, bytecode);
        assert_eq!(addr1, addr2, "Same inputs should produce same address");

        // Different bytecode = different address
        let different_bytecode = b"different bytecode";
        let addr3 = compute_deterministic_contract_address(&deployer, different_bytecode);
        assert_ne!(
            addr1, addr3,
            "Different bytecode should produce different address"
        );

        // Different deployer = different address
        let different_deployer =
            crate::crypto::elgamal::CompressedPublicKey::from_bytes(&[2u8; 32]).unwrap();
        let addr4 = compute_deterministic_contract_address(&different_deployer, bytecode);
        assert_ne!(
            addr1, addr4,
            "Different deployer should produce different address"
        );

        // Same deployer + same bytecode = same address (idempotent)
        let addr5 = compute_deterministic_contract_address(&deployer, bytecode);
        assert_eq!(addr1, addr5, "Address computation should be idempotent");
    }

    #[test]
    fn test_deterministic_address_format() {
        let deployer = crate::crypto::elgamal::CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let bytecode = b"test";

        let addr = compute_deterministic_contract_address(&deployer, bytecode);

        // Verify address is 32 bytes
        assert_eq!(addr.as_bytes().len(), 32, "Address should be 32 bytes");

        // Verify it's different from simple hash of bytecode
        let simple_hash = hash(bytecode);
        assert_ne!(
            addr, simple_hash,
            "Address should not be simple hash of bytecode"
        );

        // Verify it includes CREATE2 prefix (0xff)
        // Manually compute and verify
        let code_hash = hash(bytecode);
        let mut manual_data = Vec::new();
        manual_data.push(0xff);
        manual_data.extend_from_slice(deployer.as_bytes());
        manual_data.extend_from_slice(code_hash.as_bytes());
        let manual_addr = hash(&manual_data);
        assert_eq!(addr, manual_addr, "Address should match manual computation");
    }

    #[test]
    fn test_deterministic_address_uniqueness() {
        // Test that different combinations produce different addresses
        let deployer1 =
            crate::crypto::elgamal::CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let deployer2 =
            crate::crypto::elgamal::CompressedPublicKey::from_bytes(&[2u8; 32]).unwrap();
        let bytecode1 = b"contract_v1";
        let bytecode2 = b"contract_v2";

        let addr_11 = compute_deterministic_contract_address(&deployer1, bytecode1);
        let addr_12 = compute_deterministic_contract_address(&deployer1, bytecode2);
        let addr_21 = compute_deterministic_contract_address(&deployer2, bytecode1);
        let addr_22 = compute_deterministic_contract_address(&deployer2, bytecode2);

        // All addresses should be different
        assert_ne!(addr_11, addr_12);
        assert_ne!(addr_11, addr_21);
        assert_ne!(addr_11, addr_22);
        assert_ne!(addr_12, addr_21);
        assert_ne!(addr_12, addr_22);
        assert_ne!(addr_21, addr_22);
    }
}
