// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

use base58::{ToBase58, FromBase58};
use rand::rngs::OsRng;
use ed25519_dalek as dalek;
use ed25519_dalek::{Signer, Verifier};
use serde::{Deserialize, Serialize};
use std::convert::{TryFrom, TryInto};

use crate::error::TosError;

#[cfg(test)]
#[path = "unit_tests/base_types_tests.rs"]
mod base_types_tests;

#[derive(
    Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash, Default, Debug, Serialize, Deserialize,
)]
pub struct Amount(u64);
#[derive(
    Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash, Default, Debug, Serialize, Deserialize,
)]
pub struct Balance(i128);
#[derive(
    Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash, Default, Debug, Serialize, Deserialize,
)]
pub struct Nonce(u64);

pub type ShardId = u32;
pub type VersionNumber = Nonce;

#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Hash, Default, Debug, Serialize, Deserialize)]
pub struct UserData(pub Option<[u8; 32]>);

/// A signature key-pair.
#[derive(Clone)]
pub struct KeyPair(dalek::SigningKey);

#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash, Serialize, Deserialize)]
pub struct PublicKey(pub [u8; dalek::PUBLIC_KEY_LENGTH]);

pub type Address = PublicKey;
pub type ValidatorName = PublicKey;

pub fn get_key_pair() -> (Address, KeyPair) {
    let mut csprng = OsRng;
    let keypair = dalek::SigningKey::generate(&mut csprng);
    (PublicKey(keypair.verifying_key().to_bytes()), KeyPair(keypair))
}

pub fn address_as_base58<S>(key: &PublicKey, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::ser::Serializer,
{
    serializer.serialize_str(&encode_address(key))
}

pub fn address_from_base58<'de, D>(deserializer: D) -> Result<PublicKey, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let value = decode_address(&s).map_err(|err| serde::de::Error::custom(err.to_string()))?;
    Ok(value)
}

pub fn encode_address(key: &PublicKey) -> String {
    key.0[..].to_base58()
}

pub fn decode_address(s: &str) -> Result<PublicKey, failure::Error> {
    let value = s.from_base58().unwrap();
    let mut address = [0u8; dalek::PUBLIC_KEY_LENGTH];
    address.copy_from_slice(&value[..dalek::PUBLIC_KEY_LENGTH]);
    Ok(PublicKey(address))
}

#[cfg(test)]
pub fn dbg_addr(name: u8) -> Address {
    let addr = [name; dalek::PUBLIC_KEY_LENGTH];
    PublicKey(addr)
}

#[derive(Eq, PartialEq, Copy, Clone, Serialize, Deserialize)]
pub struct Signature(dalek::Signature);

impl KeyPair {
    pub fn copy(&self) -> KeyPair {
        KeyPair(self.0.clone())
     }
}

impl Serialize for KeyPair {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(&self.0.to_bytes().to_base58())
    }
}

impl<'de> Deserialize<'de> for KeyPair {
    fn deserialize<D>(deserializer: D) -> Result<KeyPair, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let value = s.from_base58().unwrap();
        let mut secret = [0u8; dalek::SECRET_KEY_LENGTH];
        secret.copy_from_slice(&value[..dalek::SECRET_KEY_LENGTH]);
        let keypair: dalek::SigningKey = dalek::SigningKey::from_bytes(&secret);
        Ok(KeyPair(keypair))
    }
}

impl std::fmt::Debug for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        let s = self.0.to_bytes().to_base58();
        write!(f, "{}", s)?;
        Ok(())
    }
}

impl std::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        let s = encode_address(&self);
        write!(f, "{}", s)?;
        Ok(())
    }
}

impl Amount {
    pub fn zero() -> Self {
        Amount(0)
    }

    pub fn try_add(self, other: Self) -> Result<Self, TosError> {
        let val = self.0.checked_add(other.0);
        match val {
            None => Err(TosError::AmountOverflow),
            Some(val) => Ok(Self(val)),
        }
    }

    pub fn try_sub(self, other: Self) -> Result<Self, TosError> {
        let val = self.0.checked_sub(other.0);
        match val {
            None => Err(TosError::AmountUnderflow),
            Some(val) => Ok(Self(val)),
        }
    }
}

impl Balance {
    pub fn zero() -> Self {
        Balance(0)
    }

    pub fn max() -> Self {
        Balance(std::i128::MAX)
    }

    pub fn try_add(&self, other: Self) -> Result<Self, TosError> {
        let val = self.0.checked_add(other.0);
        match val {
            None => Err(TosError::BalanceOverflow),
            Some(val) => Ok(Self(val)),
        }
    }

    pub fn try_sub(&self, other: Self) -> Result<Self, TosError> {
        let val = self.0.checked_sub(other.0);
        match val {
            None => Err(TosError::BalanceUnderflow),
            Some(val) => Ok(Self(val)),
        }
    }
}

impl std::fmt::Display for Balance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for Balance {
    type Err = std::num::ParseIntError;

    fn from_str(src: &str) -> Result<Self, Self::Err> {
        Ok(Self(i128::from_str(src)?))
    }
}

impl From<Amount> for u64 {
    fn from(val: Amount) -> Self {
        val.0
    }
}

impl From<Amount> for Balance {
    fn from(val: Amount) -> Self {
        Balance(val.0 as i128)
    }
}

impl TryFrom<Balance> for Amount {
    type Error = std::num::TryFromIntError;

    fn try_from(val: Balance) -> Result<Self, Self::Error> {
        Ok(Amount(val.0.try_into()?))
    }
}

impl Nonce {
    pub fn new() -> Self {
        Nonce(0)
    }

    pub fn max() -> Self {
        Nonce(0x7fff_ffff_ffff_ffff)
    }

    pub fn increment(self) -> Result<Nonce, TosError> {
        let val = self.0.checked_add(1);
        match val {
            None => Err(TosError::SequenceOverflow),
            Some(val) => Ok(Self(val)),
        }
    }

    pub fn decrement(self) -> Result<Nonce, TosError> {
        let val = self.0.checked_sub(1);
        match val {
            None => Err(TosError::SequenceUnderflow),
            Some(val) => Ok(Self(val)),
        }
    }
}

impl From<Nonce> for u64 {
    fn from(val: Nonce) -> Self {
        val.0
    }
}

impl From<u64> for Amount {
    fn from(value: u64) -> Self {
        Amount(value)
    }
}

impl From<i128> for Balance {
    fn from(value: i128) -> Self {
        Balance(value)
    }
}

impl From<u64> for Nonce {
    fn from(value: u64) -> Self {
        Nonce(value)
    }
}

impl From<Nonce> for usize {
    fn from(value: Nonce) -> Self {
        value.0 as usize
    }
}

/// Something that we know how to hash and sign.
pub trait Signable<Hasher> {
    fn write(&self, hasher: &mut Hasher);
}

/// Activate the blanket implementation of `Signable` based on serde and BCS.
/// * We use `serde_name` to extract a seed from the name of structs and enums.
/// * We use `BCS` to generate canonical bytes suitable for hashing and signing.
pub trait BcsSignable: Serialize + serde::de::DeserializeOwned {}

impl<T, Hasher> Signable<Hasher> for T
where
    T: BcsSignable,
    Hasher: std::io::Write,
{
    fn write(&self, hasher: &mut Hasher) {
        let name = serde_name::trace_name::<Self>().expect("Self must be a struct or an enum");
        // Note: This assumes that names never contain the separator `::`.
        write!(hasher, "{}::", name).expect("Hasher should not fail");
        bcs::serialize_into(hasher, &self).expect("Message serialization should not fail");
    }
}

impl Signature {
    pub fn new<T>(value: &T, keypair: &KeyPair) -> Self
    where
        T: Signable<Vec<u8>>,
    {
        let mut message = Vec::new();
        value.write(&mut message);
        let signing_key = keypair.0.clone();
        let signature = signing_key.sign(&message);
        Signature(signature)
    }

    fn check_internal<T>(
        &self,
        value: &T,
        author: Address,
    ) -> Result<(), dalek::SignatureError>
    where
        T: Signable<Vec<u8>>,
    {
        let mut message = Vec::new();
        value.write(&mut message);
        let verifying_key = dalek::VerifyingKey::from_bytes(&author.0)?;
        verifying_key.verify(&message, &self.0)
    }

    pub fn check<T>(&self, value: &T, author: Address) -> Result<(), TosError>
    where
        T: Signable<Vec<u8>>,
    {
        self.check_internal(value, author)
            .map_err(|error| TosError::InvalidSignature {
                error: format!("{}", error),
            })
    }

    fn verify_batch_internal<'a, T, I>(value: &'a T, votes: I) -> Result<(), dalek::SignatureError>
    where
        T: Signable<Vec<u8>>,
        I: IntoIterator<Item = &'a (Address, Signature)>,
    {
        let mut msg = Vec::new();
        value.write(&mut msg);
        let mut messages: Vec<&[u8]> = Vec::new();
        let mut signatures: Vec<dalek::Signature> = Vec::new();
        let mut public_keys: Vec<dalek::VerifyingKey> = Vec::new();
        for (addr, sig) in votes.into_iter() {
            messages.push(&msg);
            signatures.push(sig.0);
            public_keys.push(dalek::VerifyingKey::from_bytes(&addr.0)?);
        }
        dalek::verify_batch(&messages[..], &signatures[..], &public_keys[..])
    }

    pub fn verify_batch<'a, T, I>(value: &'a T, votes: I) -> Result<(), TosError>
    where
        T: Signable<Vec<u8>>,
        I: IntoIterator<Item = &'a (Address, Signature)>,
    {
        Signature::verify_batch_internal(value, votes).map_err(|error| {
            TosError::InvalidSignature {
                error: format!("{}", error),
            }
        })
    }
}
