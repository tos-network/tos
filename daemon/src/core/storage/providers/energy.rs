use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{account::AccountEnergy, block::TopoHeight, crypto::PublicKey};

/// Provider for account energy storage operations (Stake 2.0)
#[async_trait]
pub trait EnergyProvider {
    /// Get account energy for an account (Stake 2.0)
    async fn get_account_energy(
        &self,
        account: &PublicKey,
    ) -> Result<Option<AccountEnergy>, BlockchainError>;

    /// Set account energy for an account at a specific topoheight (Stake 2.0)
    async fn set_account_energy(
        &mut self,
        account: &PublicKey,
        topoheight: TopoHeight,
        energy: &AccountEnergy,
    ) -> Result<(), BlockchainError>;
}

// Simple implementation for testing
pub struct MockEnergyProvider;

#[async_trait::async_trait]
impl EnergyProvider for MockEnergyProvider {
    async fn get_account_energy(
        &self,
        _account: &PublicKey,
    ) -> Result<Option<AccountEnergy>, BlockchainError> {
        Ok(None) // Return None for now
    }

    async fn set_account_energy(
        &mut self,
        _account: &PublicKey,
        _topoheight: TopoHeight,
        _account_energy: &AccountEnergy,
    ) -> Result<(), BlockchainError> {
        Ok(()) // Do nothing for now
    }
}
