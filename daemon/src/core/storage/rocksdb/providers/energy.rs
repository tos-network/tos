use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode},
        DelegatedResourceProvider, EnergyProvider, GlobalEnergyProvider, NetworkProvider,
        RocksStorage,
    },
};
use async_trait::async_trait;
use log::trace;
use rocksdb::Direction;
use tos_common::{
    account::{AccountEnergy, DelegatedResource, GlobalEnergyState},
    block::TopoHeight,
    crypto::PublicKey,
    serializer::Serializer,
};

#[async_trait]
impl EnergyProvider for RocksStorage {
    async fn get_account_energy(
        &self,
        account: &PublicKey,
    ) -> Result<Option<AccountEnergy>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get account energy for account {}",
                account.as_address(self.is_mainnet())
            );
        }

        // Read pointer from Account struct (like nonce_pointer)
        let acc = self.get_optional_account_type(account)?;
        let Some(acc) = acc else {
            return Ok(None);
        };
        let Some(topo) = acc.energy_pointer else {
            return Ok(None);
        };

        // Read energy data from VersionedEnergyResources
        let key = format!("{}_{}", topo, account.as_address(self.is_mainnet()));
        let energy = self.load_optional_from_disk::<Vec<u8>, AccountEnergy>(
            Column::VersionedEnergyResources,
            &key.as_bytes().to_vec(),
        )?;

        if log::log_enabled!(log::Level::Trace) {
            trace!("Found account energy at topoheight {}: {:?}", topo, energy);
        }

        Ok(energy)
    }

    async fn set_account_energy(
        &mut self,
        account: &PublicKey,
        topoheight: TopoHeight,
        energy: &AccountEnergy,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set account energy for account {} at topoheight {}: {:?}",
                account.as_address(self.is_mainnet()),
                topoheight,
                energy
            );
        }

        // Store versioned energy resource
        let key = format!("{}_{}", topoheight, account.as_address(self.is_mainnet()));
        self.insert_into_disk(Column::VersionedEnergyResources, key.as_bytes(), energy)?;

        // Update Account.energy_pointer (follow nonce pattern in nonce.rs:154-162)
        let mut acc = self.get_or_create_account_type(account)?;
        acc.energy_pointer = Some(topoheight);
        self.insert_into_disk(Column::Account, account.as_bytes(), &acc)?;

        Ok(())
    }
}

/// Build delegation key: {from_pubkey (32 bytes)}{to_pubkey (32 bytes)}
fn build_delegation_key(from: &PublicKey, to: &PublicKey) -> Vec<u8> {
    let mut key = Vec::with_capacity(64);
    key.extend_from_slice(from.as_bytes());
    key.extend_from_slice(to.as_bytes());
    key
}

/// Build delegation index key: {to_pubkey (32 bytes)}{from_pubkey (32 bytes)}
fn build_delegation_index_key(to: &PublicKey, from: &PublicKey) -> Vec<u8> {
    let mut key = Vec::with_capacity(64);
    key.extend_from_slice(to.as_bytes());
    key.extend_from_slice(from.as_bytes());
    key
}

#[async_trait]
impl DelegatedResourceProvider for RocksStorage {
    async fn get_delegated_resource(
        &self,
        from: &PublicKey,
        to: &PublicKey,
    ) -> Result<Option<DelegatedResource>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get delegated resource from {} to {}",
                from.as_address(self.is_mainnet()),
                to.as_address(self.is_mainnet())
            );
        }

        let key = build_delegation_key(from, to);
        self.load_optional_from_disk::<Vec<u8>, DelegatedResource>(Column::DelegatedResources, &key)
    }

    async fn set_delegated_resource(
        &mut self,
        delegation: &DelegatedResource,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set delegated resource from {} to {}: {} TOS",
                delegation.from.as_address(self.is_mainnet()),
                delegation.to.as_address(self.is_mainnet()),
                delegation.frozen_balance
            );
        }

        // Store delegation record
        let key = build_delegation_key(&delegation.from, &delegation.to);
        self.insert_into_disk(Column::DelegatedResources, &key, delegation)?;

        // Store index for reverse lookup (to -> from)
        let index_key = build_delegation_index_key(&delegation.to, &delegation.from);
        // Index value is empty - we just need to know the key exists
        self.insert_into_disk(Column::DelegatedResourcesIndex, &index_key, &())?;

        Ok(())
    }

    async fn delete_delegated_resource(
        &mut self,
        from: &PublicKey,
        to: &PublicKey,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete delegated resource from {} to {}",
                from.as_address(self.is_mainnet()),
                to.as_address(self.is_mainnet())
            );
        }

        // Delete delegation record
        let key = build_delegation_key(from, to);
        self.remove_from_disk(Column::DelegatedResources, &key)?;

        // Delete index
        let index_key = build_delegation_index_key(to, from);
        self.remove_from_disk(Column::DelegatedResourcesIndex, &index_key)?;

        Ok(())
    }

    async fn get_delegations_from(
        &self,
        from: &PublicKey,
    ) -> Result<Vec<DelegatedResource>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get all delegations from {}",
                from.as_address(self.is_mainnet())
            );
        }

        let prefix = from.as_bytes();
        let mode = IteratorMode::WithPrefix(prefix, Direction::Forward);
        let iter = self.iter::<Vec<u8>, DelegatedResource>(Column::DelegatedResources, mode)?;

        let mut delegations = Vec::new();
        for result in iter {
            let (_, delegation) = result?;
            delegations.push(delegation);
        }

        Ok(delegations)
    }

    async fn get_delegations_to(
        &self,
        to: &PublicKey,
    ) -> Result<Vec<DelegatedResource>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get all delegations to {}",
                to.as_address(self.is_mainnet())
            );
        }

        // Use the index to find all delegators
        let prefix = to.as_bytes();
        let mode = IteratorMode::WithPrefix(prefix, Direction::Forward);
        let iter = self.iter_keys::<Vec<u8>>(Column::DelegatedResourcesIndex, mode)?;

        let mut delegations = Vec::new();
        for result in iter {
            let index_key = result?;
            // Index key format: {to (32 bytes)}{from (32 bytes)}
            if index_key.len() >= 64 {
                let from_bytes = &index_key[32..64];
                if let Ok(from) = PublicKey::from_bytes(from_bytes) {
                    let delegation_key = build_delegation_key(&from, to);
                    if let Some(delegation) = self
                        .load_optional_from_disk::<Vec<u8>, DelegatedResource>(
                            Column::DelegatedResources,
                            &delegation_key,
                        )?
                    {
                        delegations.push(delegation);
                    }
                }
            }
        }

        Ok(delegations)
    }
}

/// Global energy state key
const GLOBAL_ENERGY_STATE_KEY: &[u8] = b"GLOBAL";

#[async_trait]
impl GlobalEnergyProvider for RocksStorage {
    async fn get_global_energy_state(&self) -> Result<GlobalEnergyState, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get global energy state");
        }

        // Try to load from storage, return default if not found
        let state = self.load_optional_from_disk::<&[u8], GlobalEnergyState>(
            Column::GlobalEnergyState,
            &GLOBAL_ENERGY_STATE_KEY,
        )?;

        Ok(state.unwrap_or_default())
    }

    async fn set_global_energy_state(
        &mut self,
        state: &GlobalEnergyState,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set global energy state: total_weight={}, last_update={}",
                state.total_energy_weight,
                state.last_update
            );
        }

        self.insert_into_disk(Column::GlobalEnergyState, GLOBAL_ENERGY_STATE_KEY, state)?;

        Ok(())
    }
}
