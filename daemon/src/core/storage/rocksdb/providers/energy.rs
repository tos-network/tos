use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode},
        EnergyProvider, NetworkProvider, RocksStorage,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    account::EnergyResource, block::TopoHeight, crypto::PublicKey, serializer::Serializer,
};

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

    async fn get_energy_resource_at_maximum_topoheight(
        &self,
        account: &PublicKey,
        maximum_topoheight: TopoHeight,
    ) -> Result<Option<EnergyResource>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get energy resource for account {} at maximum topoheight {}",
                account.as_address(self.is_mainnet()),
                maximum_topoheight
            );
        }

        let acc = self.get_optional_account_type(account)?;
        let Some(acc) = acc else {
            return Ok(None);
        };
        let Some(pointer) = acc.energy_pointer else {
            return Ok(None);
        };

        // Fast path: if the pointer is at or before maximum_topoheight,
        // the latest version IS the correct value
        if pointer <= maximum_topoheight {
            let key = format!("{}_{}", pointer, account.as_address(self.is_mainnet()));
            return self.load_optional_from_disk::<Vec<u8>, EnergyResource>(
                Column::VersionedEnergyResources,
                &key.as_bytes().to_vec(),
            );
        }

        // Slow path: pointer > maximum_topoheight, scan for the highest version
        // at or below maximum_topoheight for this account
        let address_str = account.as_address(self.is_mainnet()).to_string();
        let mut best: Option<(TopoHeight, Vec<u8>)> = None;

        let snapshot = self.snapshot.clone();
        for res in Self::iter_raw_internal(
            &self.db,
            snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedEnergyResources,
        )? {
            let (key, value) = res?;
            let key_str = std::str::from_utf8(&key).map_err(|_| BlockchainError::CorruptedData)?;
            let Some((topo_part, addr_part)) = key_str.split_once('_') else {
                continue;
            };
            if addr_part != address_str {
                continue;
            }
            let Ok(entry_topo) = topo_part.parse::<TopoHeight>() else {
                continue;
            };
            if entry_topo <= maximum_topoheight {
                if best.as_ref().map_or(true, |(t, _)| entry_topo > *t) {
                    best = Some((entry_topo, value.to_vec()));
                }
            }
        }

        match best {
            Some((_, data)) => {
                let energy = EnergyResource::from_bytes(&data)?;
                Ok(Some(energy))
            }
            None => Ok(None),
        }
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
