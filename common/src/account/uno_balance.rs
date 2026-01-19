//! UNO Privacy Balance
//!
//! This module provides versioned balance tracking for UNO (privacy balance) accounts.
//! UNO balances are stored as encrypted ciphertexts using twisted ElGamal encryption.

use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{
    block::TopoHeight,
    crypto::elgamal::{Ciphertext, CompressedCiphertext},
    error::BalanceError,
    serializer::{Reader, ReaderError, Serializer, Writer},
};

use super::{BalanceType, CiphertextCache};

/// Versioned UNO Balance
///
/// Tracks encrypted balance with version history for UNO privacy accounts.
/// Unlike plaintext balances, UNO balances are stored as ElGamal ciphertexts
/// that support homomorphic addition/subtraction.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct VersionedUnoBalance {
    /// Topoheight of the previous versioned balance
    /// None means this is the first version
    previous_topoheight: Option<TopoHeight>,
    /// Output balance for handling multiple TXs not in same block
    /// Used when building several TXs at the same time
    output_balance: Option<CiphertextCache>,
    /// Final encrypted balance (commitment + handle)
    /// This is the balance shown to user and used for building TXs
    final_balance: CiphertextCache,
    /// Indicates whether this version contains inputs, outputs, or both
    balance_type: BalanceType,
}

impl VersionedUnoBalance {
    /// Create a new versioned UNO balance
    pub const fn new(
        final_balance: CiphertextCache,
        previous_topoheight: Option<TopoHeight>,
    ) -> Self {
        Self {
            previous_topoheight,
            output_balance: None,
            final_balance,
            balance_type: BalanceType::Input,
        }
    }

    /// Create a zero balance (identity ciphertext)
    pub fn zero() -> Self {
        let zero = Ciphertext::zero();
        Self::new(CiphertextCache::Decompressed(zero), None)
    }

    /// Prepare for a new version at the given topoheight
    pub fn prepare_new(&mut self, previous_topoheight: Option<TopoHeight>) {
        self.previous_topoheight = previous_topoheight;
        self.output_balance = None;
        self.balance_type = BalanceType::Input;
    }

    /// Get reference to the final balance
    pub fn get_balance(&self) -> &CiphertextCache {
        &self.final_balance
    }

    /// Get mutable reference to the final balance
    pub fn get_mut_balance(&mut self) -> &mut CiphertextCache {
        &mut self.final_balance
    }

    /// Check if output balance exists
    pub fn has_output_balance(&self) -> bool {
        self.output_balance.is_some()
    }

    /// Take balance, preferring output balance if requested and available
    pub fn take_balance_with(self, output: bool) -> CiphertextCache {
        match self.output_balance {
            Some(balance) if output => balance,
            _ => self.final_balance,
        }
    }

    /// Take the final balance
    pub fn take_balance(self) -> CiphertextCache {
        self.final_balance
    }

    /// Take the output balance if present
    pub fn take_output_balance(self) -> Option<CiphertextCache> {
        self.output_balance
    }

    /// Set the output balance
    pub fn set_output_balance(&mut self, value: Option<CiphertextCache>) {
        self.output_balance = value;
    }

    /// Select balance for modification
    /// Returns tuple of (balance reference, is_output_balance)
    pub fn select_balance(&mut self, output: bool) -> (&mut CiphertextCache, bool) {
        match self.output_balance {
            Some(ref mut balance) if output => (balance, true),
            _ => (&mut self.final_balance, false),
        }
    }

    /// Set balance from compressed ciphertext
    pub fn set_compressed_balance(&mut self, value: CompressedCiphertext) {
        self.final_balance = CiphertextCache::Compressed(value);
    }

    /// Set the final balance
    pub fn set_balance(&mut self, value: CiphertextCache) {
        self.final_balance = value;
    }

    /// Add a ciphertext to the balance (homomorphic addition)
    pub fn add_ciphertext_to_balance(
        &mut self,
        ciphertext: &Ciphertext,
    ) -> Result<(), BalanceError> {
        let current = self
            .final_balance
            .computable()
            .map_err(|_| BalanceError::Decompression)?;
        let updated = current
            .checked_add(ciphertext)
            .ok_or(BalanceError::UnoOverflow)?;
        *current = updated;
        Ok(())
    }

    /// Subtract a ciphertext from the balance (homomorphic subtraction)
    pub fn sub_ciphertext_from_balance(
        &mut self,
        ciphertext: &Ciphertext,
    ) -> Result<(), BalanceError> {
        let current = self
            .final_balance
            .computable()
            .map_err(|_| BalanceError::Decompression)?;
        *current -= ciphertext;
        Ok(())
    }

    /// Get the previous topoheight
    pub fn get_previous_topoheight(&self) -> Option<TopoHeight> {
        self.previous_topoheight
    }

    /// Set the previous topoheight
    pub fn set_previous_topoheight(&mut self, previous_topoheight: Option<TopoHeight>) {
        self.previous_topoheight = previous_topoheight;
    }

    /// Check if this version contains input transactions
    pub fn contains_input(&self) -> bool {
        self.balance_type != BalanceType::Output
    }

    /// Check if this version contains output transactions
    pub fn contains_output(&self) -> bool {
        self.balance_type.contains_output()
    }

    /// Set the balance type
    pub fn set_balance_type(&mut self, balance_type: BalanceType) {
        self.balance_type = balance_type;
    }

    /// Get the balance type
    pub fn get_balance_type(&self) -> BalanceType {
        self.balance_type
    }

    /// Consume and return all components
    pub fn consume(
        self,
    ) -> (
        CiphertextCache,
        Option<CiphertextCache>,
        BalanceType,
        Option<TopoHeight>,
    ) {
        (
            self.final_balance,
            self.output_balance,
            self.balance_type,
            self.previous_topoheight,
        )
    }

    /// Convert to UnoBalance with topoheight
    pub fn as_uno_balance(self, topoheight: TopoHeight) -> UnoBalance {
        UnoBalance {
            topoheight,
            output_balance: self.output_balance,
            final_balance: self.final_balance,
            balance_type: self.balance_type,
        }
    }
}

impl Default for VersionedUnoBalance {
    fn default() -> Self {
        Self::zero()
    }
}

impl Display for VersionedUnoBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "UnoBalance[{}, previous: {:?}]",
            self.final_balance, self.previous_topoheight
        )
    }
}

impl Serializer for VersionedUnoBalance {
    fn write(&self, writer: &mut Writer) {
        self.previous_topoheight.write(writer);
        self.balance_type.write(writer);
        self.final_balance.write(writer);
        self.output_balance.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let previous_topoheight = Option::read(reader)?;
        let balance_type = BalanceType::read(reader)?;
        let final_balance = CiphertextCache::read(reader)?;
        let output_balance = Option::read(reader)?;

        Ok(Self {
            output_balance,
            final_balance,
            previous_topoheight,
            balance_type,
        })
    }

    fn size(&self) -> usize {
        self.final_balance.size()
            + self.balance_type.size()
            + self.previous_topoheight.size()
            + self.output_balance.size()
    }
}

/// UNO Balance with topoheight
///
/// Represents a snapshot of UNO balance at a specific topoheight.
#[derive(Debug)]
pub struct UnoBalance {
    /// Topoheight at which this balance was stored
    pub topoheight: TopoHeight,
    /// Output balance for spending tracking
    pub output_balance: Option<CiphertextCache>,
    /// Final encrypted balance
    pub final_balance: CiphertextCache,
    /// Balance type (input/output/both)
    pub balance_type: BalanceType,
}

impl UnoBalance {
    /// Convert to versioned balance
    pub fn as_version(self) -> (TopoHeight, VersionedUnoBalance) {
        let version = VersionedUnoBalance {
            output_balance: self.output_balance,
            final_balance: self.final_balance,
            balance_type: self.balance_type,
            previous_topoheight: None,
        };
        (self.topoheight, version)
    }
}

impl Serializer for UnoBalance {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let topoheight = TopoHeight::read(reader)?;
        let output_balance = Option::read(reader)?;
        let final_balance = CiphertextCache::read(reader)?;
        let balance_type = BalanceType::read(reader)?;

        Ok(Self {
            topoheight,
            output_balance,
            final_balance,
            balance_type,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.topoheight.write(writer);
        self.output_balance.write(writer);
        self.final_balance.write(writer);
        self.balance_type.write(writer);
    }

    fn size(&self) -> usize {
        self.topoheight.size()
            + self.output_balance.size()
            + self.final_balance.size()
            + self.balance_type.size()
    }
}

/// Account summary for UNO balances
#[derive(Debug)]
pub struct UnoAccountSummary {
    /// Last output balance topoheight (if any)
    pub output_topoheight: Option<TopoHeight>,
    /// Last stable balance topoheight
    pub stable_topoheight: TopoHeight,
}

impl Serializer for UnoAccountSummary {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let output_topoheight = Option::read(reader)?;
        let stable_topoheight = TopoHeight::read(reader)?;

        Ok(Self {
            output_topoheight,
            stable_topoheight,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.output_topoheight.write(writer);
        self.stable_topoheight.write(writer);
    }

    fn size(&self) -> usize {
        self.output_topoheight.size() + self.stable_topoheight.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_versioned_uno_balance_zero() {
        let mut zero = VersionedUnoBalance::zero();
        zero.set_balance_type(BalanceType::Input);
        let zero_bis = VersionedUnoBalance::from_bytes(&zero.to_bytes()).unwrap();
        assert_eq!(zero, zero_bis);
    }

    #[test]
    fn test_versioned_uno_balance_previous_topo() {
        let mut zero = VersionedUnoBalance::zero();
        zero.set_balance_type(BalanceType::Input);
        zero.set_previous_topoheight(Some(42));
        let zero_bis = VersionedUnoBalance::from_bytes(&zero.to_bytes()).unwrap();
        assert_eq!(zero, zero_bis);
    }

    #[test]
    fn test_versioned_uno_balance_output() {
        let mut zero = VersionedUnoBalance::zero();
        zero.set_balance_type(BalanceType::Output);
        let zero_bis = VersionedUnoBalance::from_bytes(&zero.to_bytes()).unwrap();
        assert_eq!(zero, zero_bis);
    }

    #[test]
    fn test_versioned_uno_balance_both() {
        let mut zero = VersionedUnoBalance::zero();
        zero.set_balance_type(BalanceType::Both);
        zero.set_output_balance(Some(CiphertextCache::Decompressed(Ciphertext::zero())));
        let zero_bis = VersionedUnoBalance::from_bytes(&zero.to_bytes()).unwrap();
        assert_eq!(zero, zero_bis);
    }

    #[test]
    fn test_versioned_uno_balance_output_previous_topo() {
        let mut zero = VersionedUnoBalance::zero();
        zero.set_balance_type(BalanceType::Both);
        zero.set_output_balance(Some(CiphertextCache::Decompressed(Ciphertext::zero())));
        zero.set_previous_topoheight(Some(42));
        let zero_bis = VersionedUnoBalance::from_bytes(&zero.to_bytes()).unwrap();
        assert_eq!(zero, zero_bis);
    }

    #[test]
    fn test_uno_balance_serialization() {
        let balance = UnoBalance {
            topoheight: 100,
            output_balance: None,
            final_balance: CiphertextCache::Decompressed(Ciphertext::zero()),
            balance_type: BalanceType::Input,
        };

        let bytes = balance.to_bytes();
        let restored = UnoBalance::from_bytes(&bytes).unwrap();

        assert_eq!(restored.topoheight, 100);
        assert!(restored.output_balance.is_none());
        assert_eq!(restored.balance_type, BalanceType::Input);
    }

    #[test]
    fn test_uno_account_summary_serialization() {
        let summary = UnoAccountSummary {
            output_topoheight: Some(50),
            stable_topoheight: 100,
        };

        let bytes = summary.to_bytes();
        let restored = UnoAccountSummary::from_bytes(&bytes).unwrap();

        assert_eq!(restored.output_topoheight, Some(50));
        assert_eq!(restored.stable_topoheight, 100);
    }

    #[test]
    fn test_versioned_uno_balance_default() {
        let default = VersionedUnoBalance::default();
        assert!(default.previous_topoheight.is_none());
        assert!(default.output_balance.is_none());
        assert_eq!(default.balance_type, BalanceType::Input);
    }

    #[test]
    fn test_versioned_uno_balance_prepare_new() {
        let mut balance = VersionedUnoBalance::zero();
        balance.set_balance_type(BalanceType::Both);
        balance.set_output_balance(Some(CiphertextCache::Decompressed(Ciphertext::zero())));
        balance.set_previous_topoheight(Some(100));

        balance.prepare_new(Some(200));

        assert_eq!(balance.get_previous_topoheight(), Some(200));
        assert!(!balance.has_output_balance());
        assert_eq!(balance.get_balance_type(), BalanceType::Input);
    }

    #[test]
    fn test_versioned_uno_balance_consume() {
        let mut balance = VersionedUnoBalance::zero();
        balance.set_balance_type(BalanceType::Output);
        balance.set_previous_topoheight(Some(42));

        let (final_balance, output_balance, balance_type, prev_topo) = balance.consume();

        assert!(output_balance.is_none());
        assert_eq!(balance_type, BalanceType::Output);
        assert_eq!(prev_topo, Some(42));
        // final_balance should be zero ciphertext
        let _ = final_balance.take_ciphertext().unwrap();
    }
}
