use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

use super::ContractDeposit;
use crate::{crypto::Hash, serializer::*};

/// Wrapper type for contract deposits that ensures consistent u8 count serialization.
///
/// This type wraps `IndexMap<Hash, ContractDeposit>` and provides custom serialization
/// that always uses u8 for the deposits count, ensuring wire format consistency between
/// InvokeContractPayload and InvokeConstructorPayload.
///
/// Wire format:
/// - count: u8 (max 255 deposits per transaction)
/// - for each deposit:
///   - asset_id: Hash (32 bytes)
///   - deposit: ContractDeposit (type_tag u8 + amount u64 BE = 9 bytes)
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(transparent)]
pub struct Deposits(pub IndexMap<Hash, ContractDeposit>);

impl Deposits {
    /// Create a new empty Deposits collection
    pub fn new() -> Self {
        Self(IndexMap::new())
    }

    /// Create Deposits from an IndexMap
    pub fn from_map(map: IndexMap<Hash, ContractDeposit>) -> Self {
        Self(map)
    }

    /// Get the number of deposits
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Insert a deposit
    pub fn insert(&mut self, asset: Hash, deposit: ContractDeposit) {
        self.0.insert(asset, deposit);
    }

    /// Get a deposit by asset
    pub fn get(&self, asset: &Hash) -> Option<&ContractDeposit> {
        self.0.get(asset)
    }

    /// Iterate over deposits
    pub fn iter(&self) -> impl Iterator<Item = (&Hash, &ContractDeposit)> {
        self.0.iter()
    }

    /// Consume and return the inner IndexMap
    pub fn into_inner(self) -> IndexMap<Hash, ContractDeposit> {
        self.0
    }
}

impl Deref for Deposits {
    type Target = IndexMap<Hash, ContractDeposit>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Deposits {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<IndexMap<Hash, ContractDeposit>> for Deposits {
    fn from(map: IndexMap<Hash, ContractDeposit>) -> Self {
        Self(map)
    }
}

impl From<Deposits> for IndexMap<Hash, ContractDeposit> {
    fn from(deposits: Deposits) -> Self {
        deposits.0
    }
}

impl<'a> IntoIterator for &'a Deposits {
    type Item = (&'a Hash, &'a ContractDeposit);
    type IntoIter = indexmap::map::Iter<'a, Hash, ContractDeposit>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl IntoIterator for Deposits {
    type Item = (Hash, ContractDeposit);
    type IntoIter = indexmap::map::IntoIter<Hash, ContractDeposit>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Serializer for Deposits {
    fn write(&self, writer: &mut Writer) {
        // Always use u8 for count (max 255 deposits)
        writer.write_u8(self.0.len() as u8);
        for (asset, deposit) in &self.0 {
            asset.write(writer);
            deposit.write(writer);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let len = reader.read_u8()? as usize;
        let mut deposits = IndexMap::with_capacity(len);
        for _ in 0..len {
            let asset = Hash::read(reader)?;
            let deposit = ContractDeposit::read(reader)?;
            deposits.insert(asset, deposit);
        }
        Ok(Self(deposits))
    }

    fn size(&self) -> usize {
        // 1 byte for count (u8)
        let mut size = 1;
        for (asset, deposit) in &self.0 {
            size += asset.size() + deposit.size();
        }
        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deposits_serialization() {
        let mut deposits = Deposits::new();
        deposits.insert(Hash::zero(), ContractDeposit::new(1000));

        let bytes = deposits.to_bytes();
        let deserialized = Deposits::from_bytes(&bytes).unwrap();

        assert_eq!(deposits.len(), deserialized.len());
        assert_eq!(
            deposits.get(&Hash::zero()).unwrap().amount(),
            deserialized.get(&Hash::zero()).unwrap().amount()
        );
    }

    #[test]
    fn test_deposits_wire_format() {
        let mut deposits = Deposits::new();
        deposits.insert(Hash::zero(), ContractDeposit::new(1000));

        let bytes = deposits.to_bytes();

        // First byte should be count (u8), not u16
        assert_eq!(bytes[0], 1u8); // count = 1
                                   // bytes[1..33] = asset hash (32 bytes)
                                   // bytes[33] = type tag (0 for public)
                                   // bytes[34..42] = amount u64 BE
    }

    #[test]
    fn test_empty_deposits() {
        let deposits = Deposits::new();
        let bytes = deposits.to_bytes();

        assert_eq!(bytes.len(), 1); // Just the count byte
        assert_eq!(bytes[0], 0u8);

        let deserialized = Deposits::from_bytes(&bytes).unwrap();
        assert!(deserialized.is_empty());
    }
}
