use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    account::{AccountEnergy, DelegatedResource, GlobalEnergyState},
    block::TopoHeight,
    crypto::PublicKey,
};

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

/// Provider for delegated resource storage operations (Stake 2.0)
#[async_trait]
pub trait DelegatedResourceProvider {
    /// Get a specific delegation from `from` to `to`
    async fn get_delegated_resource(
        &self,
        from: &PublicKey,
        to: &PublicKey,
    ) -> Result<Option<DelegatedResource>, BlockchainError>;

    /// Set or update a delegation from `from` to `to`
    /// Also stores a versioned record at the given topoheight for reorg support
    async fn set_delegated_resource(
        &mut self,
        delegation: &DelegatedResource,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Delete a delegation from `from` to `to`
    /// Also stores a versioned deletion record at the given topoheight for reorg support
    async fn delete_delegated_resource(
        &mut self,
        from: &PublicKey,
        to: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Get all delegations sent by an account (from -> [to1, to2, ...])
    async fn get_delegations_from(
        &self,
        from: &PublicKey,
    ) -> Result<Vec<DelegatedResource>, BlockchainError>;

    /// Get all delegations received by an account ([from1, from2, ...] -> to)
    async fn get_delegations_to(
        &self,
        to: &PublicKey,
    ) -> Result<Vec<DelegatedResource>, BlockchainError>;
}

/// Provider for global energy state storage (Stake 2.0)
#[async_trait]
pub trait GlobalEnergyProvider {
    /// Get the global energy state for the network
    async fn get_global_energy_state(&self) -> Result<GlobalEnergyState, BlockchainError>;

    /// Set the global energy state for the network
    /// Also stores a versioned record at the given topoheight for reorg support
    async fn set_global_energy_state(
        &mut self,
        state: &GlobalEnergyState,
        topoheight: TopoHeight,
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

#[async_trait::async_trait]
impl DelegatedResourceProvider for MockEnergyProvider {
    async fn get_delegated_resource(
        &self,
        _from: &PublicKey,
        _to: &PublicKey,
    ) -> Result<Option<DelegatedResource>, BlockchainError> {
        Ok(None)
    }

    async fn set_delegated_resource(
        &mut self,
        _delegation: &DelegatedResource,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        Ok(())
    }

    async fn delete_delegated_resource(
        &mut self,
        _from: &PublicKey,
        _to: &PublicKey,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        Ok(())
    }

    async fn get_delegations_from(
        &self,
        _from: &PublicKey,
    ) -> Result<Vec<DelegatedResource>, BlockchainError> {
        Ok(vec![])
    }

    async fn get_delegations_to(
        &self,
        _to: &PublicKey,
    ) -> Result<Vec<DelegatedResource>, BlockchainError> {
        Ok(vec![])
    }
}

#[async_trait::async_trait]
impl GlobalEnergyProvider for MockEnergyProvider {
    async fn get_global_energy_state(&self) -> Result<GlobalEnergyState, BlockchainError> {
        Ok(GlobalEnergyState::default())
    }

    async fn set_global_energy_state(
        &mut self,
        _state: &GlobalEnergyState,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        Ok(())
    }
}
