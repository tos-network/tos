use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode},
        RocksStorage, VersionedEnergyProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use std::collections::{HashMap, HashSet};
use tos_common::{block::TopoHeight, crypto::Address, serializer::RawBytes};

fn parse_energy_key(key: &[u8]) -> Result<(TopoHeight, String), BlockchainError> {
    let key_str = std::str::from_utf8(key).map_err(|_| BlockchainError::CorruptedData)?;
    let mut parts = key_str.splitn(2, '_');
    let topo_part = parts.next().ok_or(BlockchainError::CorruptedData)?;
    let address_part = parts.next().ok_or(BlockchainError::CorruptedData)?;
    let topoheight = topo_part
        .parse::<TopoHeight>()
        .map_err(|_| BlockchainError::CorruptedData)?;
    Ok((topoheight, address_part.to_string()))
}

impl RocksStorage {
    fn update_energy_pointer_for_address(
        &mut self,
        address: &str,
        new_pointer: Option<TopoHeight>,
    ) -> Result<(), BlockchainError> {
        let address = Address::from_string(address).map_err(|_| BlockchainError::CorruptedData)?;
        let account_key = address.to_public_key();
        let Some(mut account) = self.get_optional_account_type(&account_key)? else {
            return Ok(());
        };

        if account.energy_pointer != new_pointer {
            account.energy_pointer = new_pointer;
            Self::insert_into_disk_internal(
                &self.db,
                self.snapshot.as_mut(),
                Column::Account,
                account_key.as_bytes(),
                &account,
            )?;
        }

        Ok(())
    }
}

#[async_trait]
impl VersionedEnergyProvider for RocksStorage {
    async fn delete_versioned_energy_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned energy at topoheight {}", topoheight);
        }

        let mut max_below: HashMap<String, TopoHeight> = HashMap::new();
        let mut to_delete: Vec<(RawBytes, String)> = Vec::new();

        for res in Self::iter_owned_internal::<RawBytes, RawBytes>(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedEnergyResources,
        )? {
            let (key, _) = res?;
            let (entry_topo, address) = parse_energy_key(&key)?;
            if entry_topo == topoheight {
                to_delete.push((key, address));
            } else if entry_topo < topoheight {
                let entry = max_below.entry(address).or_insert(entry_topo);
                if entry_topo > *entry {
                    *entry = entry_topo;
                }
            }
        }

        for (key, address) in to_delete {
            Self::remove_from_disk_internal(
                &self.db,
                self.snapshot.as_mut(),
                Column::VersionedEnergyResources,
                &key,
            )?;

            let address_key =
                Address::from_string(&address).map_err(|_| BlockchainError::CorruptedData)?;
            let account_key = address_key.to_public_key();
            let Some(account) = self.get_optional_account_type(&account_key)? else {
                continue;
            };

            if account.energy_pointer == Some(topoheight) {
                let new_pointer = max_below.get(&address).copied();
                self.update_energy_pointer_for_address(&address, new_pointer)?;
            }
        }

        Ok(())
    }

    async fn delete_versioned_energy_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned energy above topoheight {}", topoheight);
        }

        let mut max_at_or_below: HashMap<String, TopoHeight> = HashMap::new();
        let mut to_delete: Vec<(RawBytes, String)> = Vec::new();

        for res in Self::iter_owned_internal::<RawBytes, RawBytes>(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedEnergyResources,
        )? {
            let (key, _) = res?;
            let (entry_topo, address) = parse_energy_key(&key)?;
            if entry_topo > topoheight {
                to_delete.push((key, address));
            } else {
                let entry = max_at_or_below.entry(address).or_insert(entry_topo);
                if entry_topo > *entry {
                    *entry = entry_topo;
                }
            }
        }

        for (key, address) in to_delete {
            Self::remove_from_disk_internal(
                &self.db,
                self.snapshot.as_mut(),
                Column::VersionedEnergyResources,
                &key,
            )?;

            let address_key =
                Address::from_string(&address).map_err(|_| BlockchainError::CorruptedData)?;
            let account_key = address_key.to_public_key();
            let Some(account) = self.get_optional_account_type(&account_key)? else {
                continue;
            };

            if account
                .energy_pointer
                .is_some_and(|pointer| pointer > topoheight)
            {
                let new_pointer = max_at_or_below.get(&address).copied();
                self.update_energy_pointer_for_address(&address, new_pointer)?;
            }
        }

        Ok(())
    }

    async fn delete_versioned_energy_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
        keep_last: bool,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned energy below topoheight {}", topoheight);
        }

        let mut max_below: HashMap<String, TopoHeight> = HashMap::new();
        let mut candidates: Vec<(RawBytes, String, TopoHeight)> = Vec::new();
        let mut addresses_with_deletions: HashSet<String> = HashSet::new();

        for res in Self::iter_owned_internal::<RawBytes, RawBytes>(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedEnergyResources,
        )? {
            let (key, _) = res?;
            let (entry_topo, address) = parse_energy_key(&key)?;
            if entry_topo < topoheight {
                if keep_last {
                    let entry = max_below.entry(address.clone()).or_insert(entry_topo);
                    if entry_topo > *entry {
                        *entry = entry_topo;
                    }
                }
                addresses_with_deletions.insert(address.clone());
                candidates.push((key, address, entry_topo));
            }
        }

        for (key, address, entry_topo) in candidates {
            if keep_last
                && max_below
                    .get(&address)
                    .is_some_and(|max_topo| *max_topo == entry_topo)
            {
                continue;
            }

            Self::remove_from_disk_internal(
                &self.db,
                self.snapshot.as_mut(),
                Column::VersionedEnergyResources,
                &key,
            )?;
        }

        if keep_last {
            for (address, new_pointer) in max_below.iter() {
                let address_key =
                    Address::from_string(address).map_err(|_| BlockchainError::CorruptedData)?;
                let account_key = address_key.to_public_key();
                let Some(account) = self.get_optional_account_type(&account_key)? else {
                    continue;
                };

                if account
                    .energy_pointer
                    .is_some_and(|pointer| pointer < topoheight)
                {
                    self.update_energy_pointer_for_address(address, Some(*new_pointer))?;
                }
            }
        } else {
            for address in addresses_with_deletions {
                let address_key =
                    Address::from_string(&address).map_err(|_| BlockchainError::CorruptedData)?;
                let account_key = address_key.to_public_key();
                let Some(account) = self.get_optional_account_type(&account_key)? else {
                    continue;
                };

                if account
                    .energy_pointer
                    .is_some_and(|pointer| pointer < topoheight)
                {
                    self.update_energy_pointer_for_address(&address, None)?;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::RocksDBConfig;
    use crate::core::storage::{
        EnergyProvider, NetworkProvider, RocksStorage, VersionedEnergyProvider,
    };
    use tempdir::TempDir;
    use tos_common::{account::EnergyResource, crypto::KeyPair, network::Network};

    async fn create_storage() -> (TempDir, RocksStorage) {
        let temp_dir = TempDir::new("versioned_energy_tests").unwrap();
        let config = RocksDBConfig::default();
        let storage =
            RocksStorage::new(&temp_dir.path().to_string_lossy(), Network::Devnet, &config);
        (temp_dir, storage)
    }

    #[tokio::test]
    async fn test_reorg_rewind_restores_previous_energy_version() {
        let (_temp_dir, mut storage) = create_storage().await;
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();

        let mut energy_v1 = EnergyResource::new();
        energy_v1.energy = 10;
        storage
            .set_energy_resource(&pubkey, 10, &energy_v1)
            .await
            .unwrap();

        let mut energy_v2 = EnergyResource::new();
        energy_v2.energy = 20;
        storage
            .set_energy_resource(&pubkey, 11, &energy_v2)
            .await
            .unwrap();

        storage
            .delete_versioned_energy_at_topoheight(11)
            .await
            .unwrap();

        let current = storage.get_energy_resource(&pubkey).await.unwrap().unwrap();
        assert_eq!(current.energy, 10);

        let account = storage.get_optional_account_type(&pubkey).unwrap().unwrap();
        assert_eq!(account.energy_pointer, Some(10));
    }

    #[tokio::test]
    async fn test_prune_below_keeps_latest_energy_pointer() {
        let (_temp_dir, mut storage) = create_storage().await;
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();

        let mut energy_v1 = EnergyResource::new();
        energy_v1.energy = 5;
        storage
            .set_energy_resource(&pubkey, 5, &energy_v1)
            .await
            .unwrap();

        let mut energy_v2 = EnergyResource::new();
        energy_v2.energy = 8;
        storage
            .set_energy_resource(&pubkey, 8, &energy_v2)
            .await
            .unwrap();

        storage
            .delete_versioned_energy_below_topoheight(10, true)
            .await
            .unwrap();

        let account = storage.get_optional_account_type(&pubkey).unwrap().unwrap();
        assert_eq!(account.energy_pointer, Some(8));

        let addr = pubkey.as_address(storage.is_mainnet());
        let key_v1 = format!("{}_{}", 5, addr);
        let key_v2 = format!("{}_{}", 8, addr);
        let v1 = storage
            .load_optional_from_disk::<Vec<u8>, EnergyResource>(
                Column::VersionedEnergyResources,
                &key_v1.as_bytes().to_vec(),
            )
            .unwrap();
        let v2 = storage
            .load_optional_from_disk::<Vec<u8>, EnergyResource>(
                Column::VersionedEnergyResources,
                &key_v2.as_bytes().to_vec(),
            )
            .unwrap();

        assert!(v1.is_none());
        assert!(v2.is_some());
    }

    #[tokio::test]
    async fn test_consistency_after_reorg_cleanup() {
        let (_temp_dir_a, mut storage_a) = create_storage().await;
        let (_temp_dir_b, mut storage_b) = create_storage().await;
        let keypair = KeyPair::new();
        let pubkey = keypair.get_public_key().compress();

        let mut energy_v1 = EnergyResource::new();
        energy_v1.energy = 12;
        storage_a
            .set_energy_resource(&pubkey, 5, &energy_v1)
            .await
            .unwrap();
        storage_b
            .set_energy_resource(&pubkey, 5, &energy_v1)
            .await
            .unwrap();

        let mut energy_v2 = EnergyResource::new();
        energy_v2.energy = 24;
        storage_a
            .set_energy_resource(&pubkey, 7, &energy_v2)
            .await
            .unwrap();

        storage_a
            .delete_versioned_energy_above_topoheight(5)
            .await
            .unwrap();

        let a_energy = storage_a
            .get_energy_resource(&pubkey)
            .await
            .unwrap()
            .unwrap();
        let b_energy = storage_b
            .get_energy_resource(&pubkey)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(a_energy.energy, b_energy.energy);
    }
}
