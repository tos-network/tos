use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{account::EnergyResource, block::TopoHeight, crypto::PublicKey};

/// Provider for energy resource storage operations
#[async_trait]
pub trait EnergyProvider {
    /// Get energy resource for an account (latest version via pointer)
    async fn get_energy_resource(
        &self,
        account: &PublicKey,
    ) -> Result<Option<EnergyResource>, BlockchainError>;

    /// Get energy resource for an account at or before the given topoheight
    async fn get_energy_resource_at_maximum_topoheight(
        &self,
        account: &PublicKey,
        maximum_topoheight: TopoHeight,
    ) -> Result<Option<EnergyResource>, BlockchainError>;

    /// Set energy resource for an account at a specific topoheight
    async fn set_energy_resource(
        &mut self,
        account: &PublicKey,
        topoheight: TopoHeight,
        energy: &EnergyResource,
    ) -> Result<(), BlockchainError>;
}

// Simple implementation for testing
pub struct MockEnergyProvider;

#[async_trait::async_trait]
impl EnergyProvider for MockEnergyProvider {
    async fn get_energy_resource(
        &self,
        _account: &PublicKey,
    ) -> Result<Option<EnergyResource>, BlockchainError> {
        Ok(None)
    }

    async fn get_energy_resource_at_maximum_topoheight(
        &self,
        _account: &PublicKey,
        _maximum_topoheight: TopoHeight,
    ) -> Result<Option<EnergyResource>, BlockchainError> {
        Ok(None)
    }

    async fn set_energy_resource(
        &mut self,
        _account: &PublicKey,
        _topoheight: TopoHeight,
        _energy_resource: &EnergyResource,
    ) -> Result<(), BlockchainError> {
        Ok(())
    }
}
