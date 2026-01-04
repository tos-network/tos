use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode},
        NetworkProvider, RocksStorage, VersionedEnergyProvider,
    },
};
use async_trait::async_trait;
use log::{debug, trace};
use tos_common::{
    account::{DelegatedResource, GlobalEnergyState},
    block::TopoHeight,
    crypto::{Address, PublicKey},
};

/// Parse a versioned delegation key: {topoheight}_{from_address}_{to_address}
fn parse_versioned_delegation_key(key: &[u8]) -> Option<(TopoHeight, String, String)> {
    let key_str = std::str::from_utf8(key).ok()?;
    let parts: Vec<&str> = key_str.splitn(3, '_').collect();
    if parts.len() != 3 {
        return None;
    }
    let topo = parts[0].parse::<TopoHeight>().ok()?;
    Some((topo, parts[1].to_string(), parts[2].to_string()))
}

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
        let is_mainnet = self.is_mainnet();

        // Collect keys to delete with their account addresses
        let keys_to_delete: Vec<(Vec<u8>, String)> = Self::iter_owned_internal::<Vec<u8>, ()>(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedEnergyResources,
        )?
        .filter_map(|res| {
            let (key, _) = res.ok()?;
            // Parse key as string "{topo}_{address}" to check topoheight prefix
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if key_str.starts_with(&target_prefix) {
                    // Extract address from key (after the topoheight prefix)
                    let address = key_str[target_prefix.len()..].to_string();
                    return Some((key, address));
                }
            }
            None
        })
        .collect();

        // Delete the collected keys and restore energy_pointer
        for (key, address) in keys_to_delete {
            Self::remove_from_disk_internal(
                &self.db,
                self.snapshot.as_mut(),
                Column::VersionedEnergyResources,
                &key,
            )?;

            // Restore Account.energy_pointer (follow nonce provider pattern)
            if let Ok(addr) = Address::from_string(&address) {
                let account_key: PublicKey = addr.into();
                if let Ok(mut account) = self.get_account_type(&account_key) {
                    if account
                        .energy_pointer
                        .is_some_and(|pointer| pointer >= topoheight)
                    {
                        // Find previous valid topoheight for this account
                        let prev_topo = self
                            .find_previous_energy_topoheight(&address, topoheight, is_mainnet)
                            .await?;
                        account.energy_pointer = prev_topo;

                        if log::log_enabled!(log::Level::Trace) {
                            trace!(
                                "updating account {} energy_pointer to {:?}",
                                address,
                                account.energy_pointer
                            );
                        }

                        Self::insert_into_disk_internal(
                            &self.db,
                            self.snapshot.as_mut(),
                            Column::Account,
                            account_key.as_bytes(),
                            &account,
                        )?;
                    }
                }
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

        let is_mainnet = self.is_mainnet();

        // Collect keys to delete with their topoheight and address
        let keys_to_delete: Vec<(Vec<u8>, TopoHeight, String)> =
            Self::iter_owned_internal::<Vec<u8>, ()>(
                &self.db,
                self.snapshot.as_ref(),
                IteratorMode::Start,
                Column::VersionedEnergyResources,
            )?
            .filter_map(|res| {
                let (key, _) = res.ok()?;
                // Parse key as string "{topo}_{address}" to extract topoheight and address
                if let Ok(key_str) = std::str::from_utf8(&key) {
                    if let Some(underscore_pos) = key_str.find('_') {
                        if let Ok(key_topo) = key_str[..underscore_pos].parse::<TopoHeight>() {
                            if key_topo > topoheight {
                                let address = key_str[underscore_pos + 1..].to_string();
                                return Some((key, key_topo, address));
                            }
                        }
                    }
                }
                None
            })
            .collect();

        // Delete the collected keys and restore energy_pointer
        for (key, key_topo, address) in keys_to_delete {
            Self::remove_from_disk_internal(
                &self.db,
                self.snapshot.as_mut(),
                Column::VersionedEnergyResources,
                &key,
            )?;

            // Restore Account.energy_pointer (follow nonce provider pattern)
            if let Ok(addr) = Address::from_string(&address) {
                let account_key: PublicKey = addr.into();
                if let Ok(mut account) = self.get_account_type(&account_key) {
                    // Update if pointer is None or above topoheight
                    if account.energy_pointer.is_none_or(|v| v > topoheight) {
                        // Find the highest valid topoheight <= target topoheight
                        let prev_topo = self
                            .find_previous_energy_topoheight(&address, key_topo, is_mainnet)
                            .await?;
                        let filtered = prev_topo.filter(|v| *v <= topoheight);

                        if filtered != account.energy_pointer {
                            account.energy_pointer = filtered;

                            if log::log_enabled!(log::Level::Trace) {
                                trace!(
                                    "updating account {} energy_pointer to {:?}",
                                    address,
                                    account.energy_pointer
                                );
                            }

                            Self::insert_into_disk_internal(
                                &self.db,
                                self.snapshot.as_mut(),
                                Column::Account,
                                account_key.as_bytes(),
                                &account,
                            )?;
                        }
                    }
                }
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

    async fn delete_versioned_delegations_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned delegations at topoheight {}", topoheight);
        }

        let target_prefix = format!("{}_", topoheight);

        // First, collect all versioned delegation records at this topoheight
        let records: Vec<(Vec<u8>, DelegatedResource)> =
            Self::iter_owned_internal::<Vec<u8>, DelegatedResource>(
                &self.db,
                self.snapshot.as_ref(),
                IteratorMode::Start,
                Column::VersionedDelegatedResources,
            )?
            .filter_map(|res| {
                let (key, record) = res.ok()?;
                if let Ok(key_str) = std::str::from_utf8(&key) {
                    if key_str.starts_with(&target_prefix) {
                        return Some((key, record));
                    }
                }
                None
            })
            .collect();

        // For each record, we need to restore the previous state
        // This is complex because we need to find the previous version
        for (key, delegation) in &records {
            // Find the most recent version before this topoheight
            let previous_version = self
                .find_previous_delegation_version(&delegation.from, &delegation.to, topoheight)
                .await?;

            match previous_version {
                Some(prev_delegation) => {
                    // Restore previous state
                    if prev_delegation.frozen_balance > 0 {
                        // Previous state was active, restore it
                        let del_key =
                            build_delegation_key(&prev_delegation.from, &prev_delegation.to);
                        self.insert_into_disk(
                            Column::DelegatedResources,
                            &del_key,
                            &prev_delegation,
                        )?;

                        let index_key =
                            build_delegation_index_key(&prev_delegation.to, &prev_delegation.from);
                        self.insert_into_disk(Column::DelegatedResourcesIndex, &index_key, &())?;
                    } else {
                        // Previous state was deleted, remove current
                        let del_key = build_delegation_key(&delegation.from, &delegation.to);
                        self.remove_from_disk(Column::DelegatedResources, &del_key)?;

                        let index_key =
                            build_delegation_index_key(&delegation.to, &delegation.from);
                        self.remove_from_disk(Column::DelegatedResourcesIndex, &index_key)?;
                    }
                }
                None => {
                    // No previous version, this was a new delegation, delete it
                    let del_key = build_delegation_key(&delegation.from, &delegation.to);
                    self.remove_from_disk(Column::DelegatedResources, &del_key)?;

                    let index_key = build_delegation_index_key(&delegation.to, &delegation.from);
                    self.remove_from_disk(Column::DelegatedResourcesIndex, &index_key)?;
                }
            }

            // Delete the versioned record
            Self::remove_from_disk_internal(
                &self.db,
                self.snapshot.as_mut(),
                Column::VersionedDelegatedResources,
                key,
            )?;
        }

        if log::log_enabled!(log::Level::Debug) && !records.is_empty() {
            debug!(
                "Reverted {} delegation changes at topoheight {}",
                records.len(),
                topoheight
            );
        }

        Ok(())
    }

    async fn delete_versioned_delegations_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned delegations above topoheight {}",
                topoheight
            );
        }

        // Collect all topoheights above the target, in descending order
        let mut topos_to_delete: Vec<TopoHeight> = Self::iter_owned_internal::<Vec<u8>, ()>(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedDelegatedResources,
        )?
        .filter_map(|res| {
            let (key, _) = res.ok()?;
            let (key_topo, _, _) = parse_versioned_delegation_key(&key)?;
            if key_topo > topoheight {
                Some(key_topo)
            } else {
                None
            }
        })
        .collect();

        // Sort descending to process newest first
        topos_to_delete.sort_by(|a, b| b.cmp(a));
        topos_to_delete.dedup();

        // Process each topoheight
        for topo in topos_to_delete {
            self.delete_versioned_delegations_at_topoheight(topo)
                .await?;
        }

        Ok(())
    }

    async fn delete_versioned_delegations_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
        keep_last: bool,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned delegations below topoheight {} (keep_last: {})",
                topoheight,
                keep_last
            );
        }

        if !keep_last {
            // Simple case: delete all versioned records below topoheight
            let keys_to_delete: Vec<Vec<u8>> = Self::iter_owned_internal::<Vec<u8>, ()>(
                &self.db,
                self.snapshot.as_ref(),
                IteratorMode::Start,
                Column::VersionedDelegatedResources,
            )?
            .filter_map(|res| {
                let (key, _) = res.ok()?;
                let (key_topo, _, _) = parse_versioned_delegation_key(&key)?;
                if key_topo < topoheight {
                    Some(key)
                } else {
                    None
                }
            })
            .collect();

            for key in keys_to_delete {
                Self::remove_from_disk_internal(
                    &self.db,
                    self.snapshot.as_mut(),
                    Column::VersionedDelegatedResources,
                    &key,
                )?;
            }
        }
        // When keep_last is true, keep versioned records for history

        Ok(())
    }

    async fn delete_versioned_global_energy_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned global energy at topoheight {}",
                topoheight
            );
        }

        // Find the previous version to restore
        let previous_version = self.find_previous_global_energy_version(topoheight).await?;

        // Delete the versioned record at this topoheight
        let key = topoheight.to_be_bytes();
        self.remove_from_disk(Column::VersionedGlobalEnergyState, &key)?;

        // Restore the previous state
        if let Some(prev_state) = previous_version {
            self.insert_into_disk(Column::GlobalEnergyState, b"GLOBAL", &prev_state)?;

            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Restored global energy state to topoheight {}: weight={}",
                    prev_state.last_update, prev_state.total_energy_weight
                );
            }
        } else {
            // No previous version, restore to default
            let default_state = GlobalEnergyState::default();
            self.insert_into_disk(Column::GlobalEnergyState, b"GLOBAL", &default_state)?;

            if log::log_enabled!(log::Level::Debug) {
                debug!("Restored global energy state to default (no previous version)");
            }
        }

        Ok(())
    }

    async fn delete_versioned_global_energy_above_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned global energy above topoheight {}",
                topoheight
            );
        }

        // Collect all topoheights above the target, in descending order
        let mut topos_to_delete: Vec<TopoHeight> = Self::iter_owned_internal::<Vec<u8>, ()>(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedGlobalEnergyState,
        )?
        .filter_map(|res| {
            let (key, _) = res.ok()?;
            if key.len() >= 8 {
                let topo_bytes: [u8; 8] = key[..8].try_into().ok()?;
                let key_topo = TopoHeight::from_be_bytes(topo_bytes);
                if key_topo > topoheight {
                    return Some(key_topo);
                }
            }
            None
        })
        .collect();

        // Sort descending to process newest first
        topos_to_delete.sort_by(|a, b| b.cmp(a));
        topos_to_delete.dedup();

        // Process each topoheight
        for topo in topos_to_delete {
            self.delete_versioned_global_energy_at_topoheight(topo)
                .await?;
        }

        Ok(())
    }

    async fn delete_versioned_global_energy_below_topoheight(
        &mut self,
        topoheight: TopoHeight,
        keep_last: bool,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned global energy below topoheight {} (keep_last: {})",
                topoheight,
                keep_last
            );
        }

        if !keep_last {
            // Simple case: delete all versioned records below topoheight
            let keys_to_delete: Vec<Vec<u8>> = Self::iter_owned_internal::<Vec<u8>, ()>(
                &self.db,
                self.snapshot.as_ref(),
                IteratorMode::Start,
                Column::VersionedGlobalEnergyState,
            )?
            .filter_map(|res| {
                let (key, _) = res.ok()?;
                if key.len() >= 8 {
                    let topo_bytes: [u8; 8] = key[..8].try_into().ok()?;
                    let key_topo = TopoHeight::from_be_bytes(topo_bytes);
                    if key_topo < topoheight {
                        return Some(key);
                    }
                }
                None
            })
            .collect();

            for key in keys_to_delete {
                Self::remove_from_disk_internal(
                    &self.db,
                    self.snapshot.as_mut(),
                    Column::VersionedGlobalEnergyState,
                    &key,
                )?;
            }
        }
        // When keep_last is true, keep versioned records for history

        Ok(())
    }
}

// Helper functions for delegation key building (used in reorg cleanup)
fn build_delegation_key(from: &PublicKey, to: &PublicKey) -> Vec<u8> {
    let mut key = Vec::with_capacity(64);
    key.extend_from_slice(from.as_bytes());
    key.extend_from_slice(to.as_bytes());
    key
}

fn build_delegation_index_key(to: &PublicKey, from: &PublicKey) -> Vec<u8> {
    let mut key = Vec::with_capacity(64);
    key.extend_from_slice(to.as_bytes());
    key.extend_from_slice(from.as_bytes());
    key
}

impl RocksStorage {
    /// Find the most recent delegation version before the given topoheight
    async fn find_previous_delegation_version(
        &self,
        from: &PublicKey,
        to: &PublicKey,
        before_topoheight: TopoHeight,
    ) -> Result<Option<DelegatedResource>, BlockchainError> {
        let from_addr = from.as_address(self.is_mainnet()).to_string();
        let to_addr = to.as_address(self.is_mainnet()).to_string();

        // Find the highest topoheight less than before_topoheight for this delegation pair
        let mut best_topo: Option<TopoHeight> = None;

        for result in Self::iter_owned_internal::<Vec<u8>, DelegatedResource>(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedDelegatedResources,
        )? {
            let (key, _) = result?;
            if let Some((key_topo, key_from, key_to)) = parse_versioned_delegation_key(&key) {
                if key_from == from_addr && key_to == to_addr && key_topo < before_topoheight {
                    match best_topo {
                        Some(best) if key_topo > best => best_topo = Some(key_topo),
                        None => best_topo = Some(key_topo),
                        _ => {}
                    }
                }
            }
        }

        // Load the record at the best topoheight
        if let Some(topo) = best_topo {
            let key = format!("{}_{}_{}", topo, from_addr, to_addr);
            let record = self.load_optional_from_disk::<Vec<u8>, DelegatedResource>(
                Column::VersionedDelegatedResources,
                &key.into_bytes(),
            )?;
            return Ok(record);
        }

        Ok(None)
    }

    /// Find the most recent global energy state version before the given topoheight
    async fn find_previous_global_energy_version(
        &self,
        before_topoheight: TopoHeight,
    ) -> Result<Option<GlobalEnergyState>, BlockchainError> {
        // Find the highest topoheight less than before_topoheight
        let mut best_topo: Option<TopoHeight> = None;

        for result in Self::iter_owned_internal::<Vec<u8>, GlobalEnergyState>(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedGlobalEnergyState,
        )? {
            let (key, _) = result?;
            if key.len() >= 8 {
                if let Ok(topo_bytes) = key[..8].try_into() {
                    let key_topo = TopoHeight::from_be_bytes(topo_bytes);
                    if key_topo < before_topoheight {
                        match best_topo {
                            Some(best) if key_topo > best => best_topo = Some(key_topo),
                            None => best_topo = Some(key_topo),
                            _ => {}
                        }
                    }
                }
            }
        }

        // Load the record at the best topoheight
        if let Some(topo) = best_topo {
            let key = topo.to_be_bytes();
            let state = self.load_optional_from_disk::<[u8; 8], GlobalEnergyState>(
                Column::VersionedGlobalEnergyState,
                &key,
            )?;
            return Ok(state);
        }

        Ok(None)
    }

    /// Find the most recent energy topoheight before the given topoheight for an account
    ///
    /// Used during reorg to restore Account.energy_pointer to the previous valid state.
    /// Key format: "{topoheight}_{address}"
    async fn find_previous_energy_topoheight(
        &self,
        address: &str,
        before_topoheight: TopoHeight,
        _is_mainnet: bool,
    ) -> Result<Option<TopoHeight>, BlockchainError> {
        let mut best_topo: Option<TopoHeight> = None;

        for result in Self::iter_owned_internal::<Vec<u8>, ()>(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::VersionedEnergyResources,
        )? {
            let (key, _) = result?;
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if let Some(underscore_pos) = key_str.find('_') {
                    let key_address = &key_str[underscore_pos + 1..];
                    if key_address == address {
                        if let Ok(key_topo) = key_str[..underscore_pos].parse::<TopoHeight>() {
                            if key_topo < before_topoheight {
                                match best_topo {
                                    Some(best) if key_topo > best => best_topo = Some(key_topo),
                                    None => best_topo = Some(key_topo),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(best_topo)
    }
}
