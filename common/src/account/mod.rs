mod balance;
mod energy;
mod nonce;
mod uno_balance;

use std::{
    borrow::Cow,
    fmt::{self, Display, Formatter},
};

pub use balance::{AccountSummary, Balance, BalanceType, VersionedBalance};
pub use energy::{
    DelegateRecordEntry, DelegatedFreezeRecord, EnergyLease, EnergyResource, FreezeDuration,
    FreezeRecord, PendingUnfreeze,
};
pub use nonce::{Nonce, VersionedNonce};
pub use uno_balance::{UnoAccountSummary, UnoBalance, VersionedUnoBalance};

use crate::{
    crypto::elgamal::{
        Ciphertext, CompressedCiphertext, DecompressionError, RISTRETTO_COMPRESSED_SIZE,
    },
    serializer::{Reader, ReaderError, Serializer, Writer},
};
use serde::{Deserialize, Serialize};

/// Lazy compression/decompression cache for Ciphertext
/// Optimizes storage and computation by deferring operations
#[derive(Clone, Debug)]
pub enum CiphertextCache {
    Compressed(CompressedCiphertext),
    Decompressed(Ciphertext),
    /// Bool represents the flag "dirty" to know if the decompressed ciphertext has been modified
    Both(CompressedCiphertext, Ciphertext, bool),
}

impl CiphertextCache {
    pub fn computable(&mut self) -> Result<&mut Ciphertext, DecompressionError> {
        Ok(match self {
            Self::Compressed(c) => {
                let decompressed = c.decompress()?;
                *self = Self::Decompressed(decompressed);
                match self {
                    Self::Decompressed(e) => e,
                    _ => unreachable!(),
                }
            }
            Self::Decompressed(e) => e,
            Self::Both(_, e, dirty) => {
                *dirty = true;
                e
            }
        })
    }

    pub fn compress<'a>(&'a self) -> Cow<'a, CompressedCiphertext> {
        match self {
            Self::Compressed(c) => Cow::Borrowed(c),
            Self::Decompressed(e) => Cow::Owned(e.compress()),
            Self::Both(c, e, dirty) => {
                if *dirty {
                    Cow::Owned(e.compress())
                } else {
                    Cow::Borrowed(c)
                }
            }
        }
    }

    pub fn compressed(&mut self) -> &CompressedCiphertext {
        match self {
            Self::Compressed(c) => c,
            Self::Decompressed(e) => {
                *self = Self::Both(e.compress(), e.clone(), false);
                match self {
                    Self::Both(c, _, _) => c,
                    _ => unreachable!(),
                }
            }
            Self::Both(c, d, dirty) => {
                if *dirty {
                    *c = d.compress();
                }
                c
            }
        }
    }

    pub fn decompressed(&mut self) -> Result<&Ciphertext, DecompressionError> {
        match self {
            Self::Compressed(c) => {
                let decompressed = c.decompress()?;
                *self = Self::Both(c.clone(), decompressed, false);
                match self {
                    Self::Both(_, e, _) => Ok(e),
                    _ => unreachable!(),
                }
            }
            Self::Decompressed(e) => Ok(e),
            Self::Both(_, e, _) => Ok(e),
        }
    }

    pub fn both(&mut self) -> Result<(&CompressedCiphertext, &Ciphertext), DecompressionError> {
        match self {
            Self::Both(c, e, dirty) => {
                if *dirty {
                    *c = e.compress();
                }
                Ok((c, e))
            }
            Self::Compressed(c) => {
                let decompressed = c.decompress()?;
                *self = Self::Both(c.clone(), decompressed, false);
                match self {
                    Self::Both(c, e, _) => Ok((c, e)),
                    _ => unreachable!(),
                }
            }
            Self::Decompressed(e) => {
                let compressed = e.compress();
                *self = Self::Both(compressed, e.clone(), false);
                match self {
                    Self::Both(c, e, _) => Ok((c, e)),
                    _ => unreachable!(),
                }
            }
        }
    }

    pub fn take_ciphertext(self) -> Result<Ciphertext, DecompressionError> {
        Ok(match self {
            Self::Compressed(c) => c.decompress()?,
            Self::Decompressed(e) => e,
            Self::Both(_, e, _) => e,
        })
    }
}

impl Serializer for CiphertextCache {
    fn write(&self, writer: &mut Writer) {
        self.compress().write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let compressed = CompressedCiphertext::read(reader)?;
        Ok(Self::Compressed(compressed))
    }

    fn size(&self) -> usize {
        RISTRETTO_COMPRESSED_SIZE + RISTRETTO_COMPRESSED_SIZE
    }
}

impl Serialize for CiphertextCache {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.compress().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CiphertextCache {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        CompressedCiphertext::deserialize(deserializer).map(Self::Compressed)
    }
}

impl Display for CiphertextCache {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CiphertextCache[{}]",
            match self {
                Self::Compressed(c) => format!("Compressed({})", hex::encode(c.to_bytes())),
                Self::Decompressed(e) =>
                    format!("Decompressed({})", hex::encode(e.compress().to_bytes())),
                Self::Both(c, d, dirty) => format!(
                    "Both(c: {}, d: {}, dirty: {dirty})",
                    hex::encode(c.to_bytes()),
                    hex::encode(d.compress().to_bytes())
                ),
            }
        )
    }
}

impl PartialEq for CiphertextCache {
    fn eq(&self, other: &Self) -> bool {
        let a = self.compress();
        let b = other.compress();
        a == b
    }
}

impl Eq for CiphertextCache {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::elgamal::{KeyPair, PedersenCommitment, PedersenOpening};

    /// Helper to create a valid ciphertext for testing
    fn create_test_ciphertext(amount: u64) -> Ciphertext {
        let keypair = KeyPair::new();
        let opening = PedersenOpening::generate_new();
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let handle = keypair.get_public_key().decrypt_handle(&opening);
        Ciphertext::new(commitment, handle)
    }

    #[test]
    fn test_ciphertext_cache_from_compressed() {
        let ct = create_test_ciphertext(100);
        let compressed = ct.compress();
        let mut cache = CiphertextCache::Compressed(compressed.clone());

        // Should be able to get compressed without decompression
        assert!(matches!(cache, CiphertextCache::Compressed(_)));

        // Decompress and verify state transition
        let decompressed = cache.decompressed().unwrap();
        assert_eq!(decompressed.compress(), compressed);

        // After decompression, should be in Both state
        assert!(matches!(cache, CiphertextCache::Both(_, _, false)));
    }

    #[test]
    fn test_ciphertext_cache_from_decompressed() {
        let ct = create_test_ciphertext(200);
        let mut cache = CiphertextCache::Decompressed(ct.clone());

        // Should be able to get decompressed directly
        let decompressed = cache.decompressed().unwrap();
        assert_eq!(decompressed.compress(), ct.compress());

        // After compress(), should transition to Both
        let _ = cache.compressed();
        assert!(matches!(cache, CiphertextCache::Both(_, _, false)));
    }

    #[test]
    fn test_ciphertext_cache_computable_sets_dirty() {
        let ct = create_test_ciphertext(300);
        let compressed = ct.compress();
        let mut cache = CiphertextCache::Both(compressed.clone(), ct.clone(), false);

        // computable() should mark as dirty
        let _ = cache.computable().unwrap();
        assert!(matches!(cache, CiphertextCache::Both(_, _, true)));
    }

    #[test]
    fn test_ciphertext_cache_dirty_recompression() {
        let ct = create_test_ciphertext(400);
        let compressed = ct.compress();
        let mut cache = CiphertextCache::Both(compressed.clone(), ct.clone(), false);

        // Mark as dirty by calling computable
        {
            let computable = cache.computable().unwrap();

            // Modify the ciphertext (add zero to it)
            let ct2 = create_test_ciphertext(0);
            *computable = computable.clone() + ct2;
        }

        // Should be marked as dirty
        assert!(matches!(cache, CiphertextCache::Both(_, _, true)));

        // compressed() recomputes but dirty flag stays true (by design)
        let new_compressed = cache.compressed().clone();
        // The value is updated but dirty flag is not reset by compressed()
        assert!(matches!(cache, CiphertextCache::Both(_, _, true)));
        assert_ne!(&new_compressed, &compressed);

        // compress() (immutable) should also return the new value when dirty
        let cow_compressed = cache.compress();
        assert_ne!(cow_compressed.as_ref(), &compressed);
    }

    #[test]
    fn test_ciphertext_cache_both_returns_both() {
        let ct = create_test_ciphertext(500);
        let mut cache = CiphertextCache::Decompressed(ct.clone());

        // both() should transition to Both state
        let (c, d) = cache.both().unwrap();
        assert_eq!(c, &ct.compress());
        assert_eq!(d.compress(), ct.compress());
        assert!(matches!(cache, CiphertextCache::Both(_, _, false)));
    }

    #[test]
    fn test_ciphertext_cache_take_ciphertext() {
        let ct = create_test_ciphertext(600);
        let compressed = ct.compress();

        // Test take from Compressed
        let cache = CiphertextCache::Compressed(compressed.clone());
        let taken = cache.take_ciphertext().unwrap();
        assert_eq!(taken.compress(), compressed);

        // Test take from Decompressed
        let cache = CiphertextCache::Decompressed(ct.clone());
        let taken = cache.take_ciphertext().unwrap();
        assert_eq!(taken.compress(), ct.compress());

        // Test take from Both
        let cache = CiphertextCache::Both(compressed.clone(), ct.clone(), false);
        let taken = cache.take_ciphertext().unwrap();
        assert_eq!(taken.compress(), compressed);
    }

    #[test]
    fn test_ciphertext_cache_serialization_roundtrip() {
        use crate::serializer::Reader;

        let ct = create_test_ciphertext(700);
        let cache = CiphertextCache::Decompressed(ct.clone());

        // Serialize
        let bytes = cache.to_bytes();

        // Deserialize
        let mut reader = Reader::new(&bytes);
        let restored = CiphertextCache::read(&mut reader).unwrap();

        // Restored should be Compressed variant
        assert!(matches!(restored, CiphertextCache::Compressed(_)));

        // Values should be equal
        assert_eq!(cache, restored);
    }

    #[test]
    fn test_ciphertext_cache_equality() {
        let ct = create_test_ciphertext(800);
        let compressed = ct.compress();

        // Different variants with same value should be equal
        let cache1 = CiphertextCache::Compressed(compressed.clone());
        let cache2 = CiphertextCache::Decompressed(ct.clone());
        let cache3 = CiphertextCache::Both(compressed.clone(), ct.clone(), false);
        let cache4 = CiphertextCache::Both(compressed.clone(), ct.clone(), true);

        assert_eq!(cache1, cache2);
        assert_eq!(cache2, cache3);
        assert_eq!(cache3, cache4); // dirty flag doesn't affect equality
    }

    #[test]
    fn test_ciphertext_cache_display() {
        let ct = create_test_ciphertext(900);
        let compressed = ct.compress();

        let cache1 = CiphertextCache::Compressed(compressed.clone());
        let cache2 = CiphertextCache::Decompressed(ct.clone());
        let cache3 = CiphertextCache::Both(compressed.clone(), ct.clone(), true);

        // Just verify display works and contains expected substrings
        let s1 = format!("{}", cache1);
        let s2 = format!("{}", cache2);
        let s3 = format!("{}", cache3);

        assert!(s1.contains("Compressed"));
        assert!(s2.contains("Decompressed"));
        assert!(s3.contains("Both"));
        assert!(s3.contains("dirty: true"));
    }
}
