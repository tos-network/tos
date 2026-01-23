//! UNO Balance Provider
//!
//! Provides storage operations for UNO (privacy) balances.

use super::{AccountProvider, NetworkProvider};
use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    account::{UnoAccountSummary, UnoBalance, VersionedUnoBalance},
    block::TopoHeight,
    crypto::{Hash, PublicKey},
};

#[async_trait]
pub trait UnoBalanceProvider: AccountProvider + NetworkProvider {
    /// Check if a UNO balance exists for the given key
    async fn has_uno_balance_for(
        &self,
        key: &PublicKey,
        asset: &Hash,
    ) -> Result<bool, BlockchainError>;

    /// Check if a UNO balance exists at a specific topoheight
    async fn has_uno_balance_at_exact_topoheight(
        &self,
        key: &PublicKey,
        asset: &Hash,
        topoheight: TopoHeight,
    ) -> Result<bool, BlockchainError>;

    /// Get the UNO balance at a specific topoheight
    async fn get_uno_balance_at_exact_topoheight(
        &self,
        key: &PublicKey,
        asset: &Hash,
        topoheight: TopoHeight,
    ) -> Result<VersionedUnoBalance, BlockchainError>;

    /// Get the UNO balance at or below the maximum topoheight
    async fn get_uno_balance_at_maximum_topoheight(
        &self,
        key: &PublicKey,
        asset: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, VersionedUnoBalance)>, BlockchainError>;

    /// Get the last topoheight for which the account has a UNO balance
    async fn get_last_topoheight_for_uno_balance(
        &self,
        key: &PublicKey,
        asset: &Hash,
    ) -> Result<TopoHeight, BlockchainError>;

    /// Get a new versioned UNO balance for the account
    /// Returns (balance, is_new) where is_new is true if no previous balance exists
    async fn get_new_versioned_uno_balance(
        &self,
        key: &PublicKey,
        asset: &Hash,
        topoheight: TopoHeight,
    ) -> Result<(VersionedUnoBalance, bool), BlockchainError>;

    /// Get the last UNO balance of the account
    async fn get_last_uno_balance(
        &self,
        key: &PublicKey,
        asset: &Hash,
    ) -> Result<(TopoHeight, VersionedUnoBalance), BlockchainError>;

    /// Search for the highest balance where we have an outgoing transaction
    async fn get_uno_output_balance_at_maximum_topoheight(
        &self,
        key: &PublicKey,
        asset: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, VersionedUnoBalance)>, BlockchainError>;

    /// Search for output balance in a topoheight range
    async fn get_uno_output_balance_in_range(
        &self,
        key: &PublicKey,
        asset: &Hash,
        min_topoheight: TopoHeight,
        max_topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, VersionedUnoBalance)>, BlockchainError>;

    /// Set the last topoheight for the UNO balance
    fn set_last_topoheight_for_uno_balance(
        &mut self,
        key: &PublicKey,
        asset: &Hash,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Set the last UNO balance and update the pointer
    async fn set_last_uno_balance_to(
        &mut self,
        key: &PublicKey,
        asset: &Hash,
        topoheight: TopoHeight,
        version: &VersionedUnoBalance,
    ) -> Result<(), BlockchainError>;

    /// Set the UNO balance at a specific topoheight
    async fn set_uno_balance_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
        key: &PublicKey,
        asset: &Hash,
        balance: &VersionedUnoBalance,
    ) -> Result<(), BlockchainError>;

    /// Get the UNO account summary for a topoheight range
    async fn get_uno_account_summary_for(
        &self,
        key: &PublicKey,
        asset: &Hash,
        min_topoheight: TopoHeight,
        max_topoheight: TopoHeight,
    ) -> Result<Option<UnoAccountSummary>, BlockchainError>;

    /// Get spendable UNO balances in a topoheight range
    async fn get_spendable_uno_balances_for(
        &self,
        key: &PublicKey,
        asset: &Hash,
        min_topoheight: TopoHeight,
        max_topoheight: TopoHeight,
        maximum: usize,
    ) -> Result<(Vec<UnoBalance>, Option<TopoHeight>), BlockchainError>;

    /// Delete UNO balance at a specific topoheight
    async fn delete_uno_balance_at_topoheight(
        &mut self,
        key: &PublicKey,
        asset: &Hash,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    // ===== Bootstrap Sync =====

    /// List all (key, asset) pairs that have UNO balances with skip/limit pagination
    async fn list_all_uno_balance_keys(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(PublicKey, Hash)>, BlockchainError>;

    /// Import a UNO balance version directly without validation (bootstrap sync)
    async fn import_uno_balance(
        &mut self,
        key: &PublicKey,
        asset: &Hash,
        topoheight: TopoHeight,
        balance: &VersionedUnoBalance,
    ) -> Result<(), BlockchainError>;
}
