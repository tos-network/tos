use crate::core::{
    error::BlockchainError,
    storage::{rocksdb::Column, EnergyProvider, NetworkProvider, RocksStorage},
};
use async_trait::async_trait;
use log::trace;
use tos_common::{account::EnergyResource, block::TopoHeight, crypto::PublicKey};

#[async_trait]
impl EnergyProvider for RocksStorage {
    async fn get_energy_resource(
        &self,
        account: &PublicKey,
    ) -> Result<Option<EnergyResource>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get energy resource for account {}",
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
        let energy = self.load_optional_from_disk::<Vec<u8>, EnergyResource>(
            Column::VersionedEnergyResources,
            &key.as_bytes().to_vec(),
        )?;

        if log::log_enabled!(log::Level::Trace) {
            trace!("Found energy resource at topoheight {}: {:?}", topo, energy);
        }

        Ok(energy)
    }

    async fn set_energy_resource(
        &mut self,
        account: &PublicKey,
        topoheight: TopoHeight,
        energy: &EnergyResource,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set energy resource for account {} at topoheight {}: {:?}",
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
