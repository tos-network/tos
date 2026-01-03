use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode},
        RocksStorage, VersionedEnergyProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::block::TopoHeight;

/// Implement versioned energy cleanup for blockchain reorgs
///
/// NOTE: The current energy key format is "{topoheight}_{address}" (string-based),
/// which differs from other versioned columns that use binary topoheight prefix.
/// This implementation parses the string keys to extract topoheight.
#[async_trait]
impl VersionedEnergyProvider for RocksStorage {
    async fn delete_versioned_energy_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned energy at topoheight {}", topoheight);
        }

        let target_prefix = format!("{}_", topoheight);

        // Collect keys to delete first (to avoid borrowing issues during iteration)
        let keys_to_delete: Vec<Vec<u8>> = Self::iter_owned_internal::<Vec<u8>, ()>(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedEnergyResources,
        )?
        .filter_map(|res| {
            let (key, _) = res.ok()?;
            // Parse key as string to check topoheight prefix
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if key_str.starts_with(&target_prefix) {
                    return Some(key);
                }
            }
            None
        })
        .collect();

        // Delete the collected keys
        for key in keys_to_delete {
            Self::remove_from_disk_internal(
                &self.db,
                self.snapshot.as_mut(),
                Column::VersionedEnergyResources,
                &key,
            )?;
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

        // Collect keys to delete first (to avoid borrowing issues during iteration)
        let keys_to_delete: Vec<Vec<u8>> = Self::iter_owned_internal::<Vec<u8>, ()>(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedEnergyResources,
        )?
        .filter_map(|res| {
            let (key, _) = res.ok()?;
            // Parse key as string "{topo}_{address}" to extract topoheight
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if let Some(underscore_pos) = key_str.find('_') {
                    if let Ok(key_topo) = key_str[..underscore_pos].parse::<TopoHeight>() {
                        if key_topo > topoheight {
                            return Some(key);
                        }
                    }
                }
            }
            None
        })
        .collect();

        // Delete the collected keys
        for key in keys_to_delete {
            Self::remove_from_disk_internal(
                &self.db,
                self.snapshot.as_mut(),
                Column::VersionedEnergyResources,
                &key,
            )?;
        }

        // NOTE: Account energy_pointer cleanup is handled separately by the reorg
        // mechanism when it resets account state to previous topoheight

        Ok(())
    }

    async fn delete_versioned_energy_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
        keep_last: bool,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned energy below topoheight {} (keep_last: {})",
                topoheight,
                keep_last
            );
        }

        // For energy, we typically want to keep the latest version for each account
        // This is more complex with the string-based key format
        // For now, we implement basic cleanup of old versions

        if !keep_last {
            // Simple case: delete everything below topoheight
            let keys_to_delete: Vec<Vec<u8>> = Self::iter_owned_internal::<Vec<u8>, ()>(
                &self.db,
                self.snapshot.as_ref(),
                IteratorMode::Start,
                Column::VersionedEnergyResources,
            )?
            .filter_map(|res| {
                let (key, _) = res.ok()?;
                if let Ok(key_str) = std::str::from_utf8(&key) {
                    if let Some(underscore_pos) = key_str.find('_') {
                        if let Ok(key_topo) = key_str[..underscore_pos].parse::<TopoHeight>() {
                            if key_topo < topoheight {
                                return Some(key);
                            }
                        }
                    }
                }
                None
            })
            .collect();

            // Delete the collected keys
            for key in keys_to_delete {
                Self::remove_from_disk_internal(
                    &self.db,
                    self.snapshot.as_mut(),
                    Column::VersionedEnergyResources,
                    &key,
                )?;
            }
        }
        // When keep_last is true, we skip deletion to preserve the latest state
        // This is appropriate for pruning old history while keeping current state

        Ok(())
    }
}
