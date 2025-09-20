
use async_trait::async_trait;
use tos_common::{
    account::EnergyResource,
    crypto::PublicKey,
    block::TopoHeight,
};
use crate::core::error::BlockchainError;

/// Provider for energy resource storage operations
#[async_trait]
pub trait EnergyProvider {
    /// Get energy resource for an account
    async fn get_energy_resource(&self, account: &PublicKey) -> Result<Option<EnergyResource>, BlockchainError>;

    /// Set energy resource for an account at a specific topoheight
    async fn set_energy_resource(&mut self, account: &PublicKey, topoheight: TopoHeight, energy: &EnergyResource) -> Result<(), BlockchainError>;
}

// Simple implementation for testing
pub struct MockEnergyProvider;

#[async_trait::async_trait]
impl EnergyProvider for MockEnergyProvider {
    async fn get_energy_resource(&self, _account: &PublicKey) -> Result<Option<EnergyResource>, BlockchainError> {
        Ok(None) // Return None for now
    }
    
    async fn set_energy_resource(&mut self, _account: &PublicKey, _topoheight: TopoHeight, _energy_resource: &EnergyResource) -> Result<(), BlockchainError> {
        Ok(()) // Do nothing for now
    }
} 