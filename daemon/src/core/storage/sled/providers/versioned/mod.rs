mod asset;
mod balance;
mod cache;
mod contract;
mod dag_order;
mod multisig;
mod nonce;
mod registrations;

use crate::core::{
    error::{BlockchainError, DiskContext},
    storage::{sled::Snapshot, SledStorage, VersionedProvider},
};
use log::{debug, trace, warn};
use sled::Tree;
use tos_common::{
    block::TopoHeight,
    serializer::{NoTransform, Serializer},
    versioned_type::Versioned,
};

impl VersionedProvider for SledStorage {}

impl SledStorage {
    fn delete_versioned_tree_at_topoheight(
        snapshot: &mut Option<Snapshot>,
        tree_pointer: &Tree,
        tree_versioned: &Tree,
        topoheight: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned data at topoheight {}", topoheight);
        }
        for el in Self::scan_prefix(snapshot.as_ref(), tree_versioned, &topoheight.to_be_bytes()) {
            let prefixed_key = el?;

            // Delete this version from DB
            // We read the previous topoheight to check if we need to delete the balance
            let prev_topo = Self::remove_from_disk::<Option<TopoHeight>>(
                snapshot.as_mut(),
                tree_versioned,
                &prefixed_key,
            )?
            .ok_or(BlockchainError::CorruptedData)?;

            // Key without the topoheight
            let key = &prefixed_key[8..];
            if let Some(topo_pointer) = Self::load_optional_from_disk_internal::<TopoHeight>(
                snapshot.as_ref(),
                tree_pointer,
                key,
            )? {
                if topo_pointer >= topoheight {
                    if let Some(prev_topo) = prev_topo {
                        Self::insert_into_disk(
                            snapshot.as_mut(),
                            tree_pointer,
                            key,
                            &prev_topo.to_be_bytes(),
                        )?;
                    } else {
                        // FIX: Don't immediately delete pointer, search for earlier versions first
                        // Use reverse iterator from Sled to scan backwards efficiently
                        let mut found_earlier_version = None;

                        // Build starting key: [topoheight-1][account_asset_key]
                        let start_topo = topoheight.saturating_sub(1);
                        let mut start_key = Vec::with_capacity(prefixed_key.len());
                        start_key.extend_from_slice(&start_topo.to_be_bytes());
                        start_key.extend_from_slice(key);

                        // Iterate backwards from start_key to find the first (most recent) earlier version
                        // Sled's range() with .rev() iterates in reverse order
                        for res in tree_versioned.range(..=&start_key[..]).rev() {
                            let (iter_key, _) = res?;

                            // Check if this key matches our account+asset (skip topoheight prefix)
                            if iter_key.len() >= 8 && &iter_key[8..] == key {
                                // Extract topoheight from key
                                let iter_topo = u64::from_be_bytes(
                                    iter_key[0..8]
                                        .try_into()
                                        .map_err(|_| BlockchainError::CorruptedData)?,
                                );

                                // Must be strictly less than the deleted topoheight
                                if iter_topo < topoheight {
                                    found_earlier_version = Some(iter_topo);
                                    break;
                                }
                                // Continue searching backwards for earlier versions
                            } else {
                                // Key doesn't match our account+asset, skip this entry
                                // Don't break - there may be earlier versions of our account/asset
                                continue;
                            }
                        }

                        if let Some(earlier_topo) = found_earlier_version {
                            // Found earlier version, update pointer
                            if log::log_enabled!(log::Level::Warn) {
                                warn!("Balance pointer recovery: updated to topoheight {} after deleting {}", earlier_topo, topoheight);
                            }
                            Self::insert_into_disk(
                                snapshot.as_mut(),
                                tree_pointer,
                                key,
                                &earlier_topo.to_be_bytes(),
                            )?;
                        } else {
                            // No earlier version found, safe to delete pointer
                            Self::remove_from_disk_without_reading(
                                snapshot.as_mut(),
                                tree_pointer,
                                key,
                            )?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn delete_versioned_tree_above_topoheight(
        snapshot: &mut Option<Snapshot>,
        tree_pointer: &Tree,
        tree_versioned: &Tree,
        topoheight: u64,
        context: DiskContext,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned data above topoheight {}", topoheight);
        }
        for el in Self::iter(snapshot.as_ref(), tree_pointer) {
            let (key, value) = el?;
            let topo = u64::from_bytes(&value)?;

            if topo > topoheight {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "found pointer at {} above the requested topoheight {} with context {}",
                        topo, topoheight, context
                    );
                }

                // We fetch the last version to take its previous topoheight
                // And we loop on it to delete them all until the end of the chained data
                let mut prev_version = Self::remove_from_disk::<Option<u64>>(
                    snapshot.as_mut(),
                    tree_versioned,
                    &Self::get_versioned_key(&key, topo),
                )?
                .ok_or(BlockchainError::NotFoundOnDisk(context))?;

                // While we are above the threshold, we must delete versions to rewrite the correct topoheight
                let mut new_topo_pointer = None;
                while let Some(prev_topo) = prev_version {
                    if prev_topo <= topoheight {
                        new_topo_pointer = Some(prev_topo);
                        break;
                    }

                    if log::log_enabled!(log::Level::Trace) {
                        trace!("deleting versioned data at topoheight {}", prev_topo);
                    }
                    let key = Self::get_versioned_key(&key, prev_topo);
                    prev_version = Self::remove_from_disk::<Option<u64>>(
                        snapshot.as_mut(),
                        tree_versioned,
                        &key,
                    )?
                    .ok_or(BlockchainError::NotFoundOnDisk(context))?;
                }

                // If we don't have any previous versioned data, delete the pointer
                match new_topo_pointer {
                    Some(topo) => {
                        trace!("overwriting the topo pointer");
                        Self::insert_into_disk(
                            snapshot.as_mut(),
                            tree_pointer,
                            key,
                            topo.to_bytes(),
                        )?;
                    }
                    None => {
                        // FIX: Same as delete_versioned_tree_at_topoheight - use reverse iterator
                        let mut found_earlier_version = None;

                        // Build starting key: [topoheight][account_asset_key]
                        let start_key = Self::get_versioned_key(&key, topoheight);

                        // Iterate backwards from start_key to find the first (most recent) earlier version
                        for res in tree_versioned.range(..=&start_key[..]).rev() {
                            let (iter_key, _) = res?;

                            // Check if this key matches our account+asset (skip topoheight prefix)
                            if iter_key.len() >= 8 && &iter_key[8..] == key.as_ref() {
                                // Extract topoheight from key
                                let iter_topo = u64::from_be_bytes(
                                    iter_key[0..8]
                                        .try_into()
                                        .map_err(|_| BlockchainError::CorruptedData)?,
                                );

                                // Must be less than or equal to threshold
                                if iter_topo <= topoheight {
                                    found_earlier_version = Some(iter_topo);
                                    break;
                                }
                                // Continue searching backwards for earlier versions
                            } else {
                                // Key doesn't match our account+asset, skip this entry
                                // Don't break - there may be earlier versions of our account/asset
                                continue;
                            }
                        }

                        if let Some(earlier_topo) = found_earlier_version {
                            if log::log_enabled!(log::Level::Warn) {
                                warn!("Balance pointer recovery (delete_above): updated to topoheight {} after deleting above {}", earlier_topo, topoheight);
                            }
                            Self::insert_into_disk(
                                snapshot.as_mut(),
                                tree_pointer,
                                key,
                                earlier_topo.to_bytes(),
                            )?;
                        } else {
                            trace!("no new topo pointer to set, deleting the pointer from tree");
                            Self::remove_from_disk_internal(snapshot.as_mut(), tree_pointer, &key)?;
                        }
                    }
                };
            }
        }

        Ok(())
    }

    fn delete_versioned_tree_below_topoheight(
        snapshot: &mut Option<Snapshot>,
        tree_pointer: &Tree,
        tree_versioned: &Tree,
        topoheight: u64,
        keep_last: bool,
        context: DiskContext,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned data below topoheight {}", topoheight);
        }
        if keep_last {
            // P1 Optimization Phase 2: Two-phase approach for keep_last=true
            // Phase 1: Find all affected accounts (those with versions < topoheight)
            use std::collections::HashSet;
            let mut affected_accounts: HashSet<Vec<u8>> = HashSet::new();

            // Scan versioned tree to find all entries below threshold
            for el in Self::iter_keys(snapshot.as_ref(), tree_versioned) {
                let key = el?;
                // Key format: [topoheight(8)][account_key...]
                let topo = u64::from_bytes(&key[0..8])?;

                if topo >= topoheight {
                    break; // Keys are sorted, early exit optimization
                }

                // Extract account key (without topoheight prefix)
                let account_key = key[8..].to_vec();
                affected_accounts.insert(account_key);
            }

            if log::log_enabled!(log::Level::Debug) {
                debug!("Found {} affected accounts below topoheight {} (instead of scanning all accounts)",
                    affected_accounts.len(), topoheight);
            }

            // Phase 2: Only walk version chains for affected accounts
            for account_key in affected_accounts {
                // Load the current pointer for this account
                let value = Self::load_optional_from_disk_internal::<Vec<u8>>(
                    snapshot.as_ref(),
                    tree_pointer,
                    &account_key,
                )?;

                if let Some(value) = value {
                    let topo = u64::from_bytes(&value)?;

                    // We fetch the last version to take its previous topoheight
                    // And we loop on it to delete them all until the end of the chained data
                    let mut prev_version = Self::load_from_disk_internal::<Option<u64>>(
                        snapshot.as_ref(),
                        tree_versioned,
                        &Self::get_versioned_key(&account_key, topo),
                        context,
                    )?;
                    // If we are already below the threshold, we can directly erase without patching
                    let mut patched = topo < topoheight;
                    while let Some(prev_topo) = prev_version {
                        let key = Self::get_versioned_key(&account_key, prev_topo);

                        // Delete this version from DB if its below the threshold
                        if patched {
                            prev_version =
                                Self::remove_from_disk(snapshot.as_mut(), &tree_versioned, &key)?;
                        } else {
                            prev_version = Self::load_from_disk_internal(
                                snapshot.as_ref(),
                                tree_versioned,
                                &key,
                                context,
                            )?;
                            if prev_version.filter(|v| *v < topoheight).is_some() {
                                if log::log_enabled!(log::Level::Trace) {
                                    trace!("Patching versioned data at topoheight {}", topoheight);
                                }
                                patched = true;
                                let mut data: Versioned<NoTransform> =
                                    Self::load_from_disk_internal(
                                        snapshot.as_ref(),
                                        tree_versioned,
                                        &key,
                                        context,
                                    )?;
                                data.set_previous_topoheight(None);
                                Self::insert_into_disk(
                                    snapshot.as_mut(),
                                    tree_versioned,
                                    key,
                                    data.to_bytes(),
                                )?;
                            }
                        }
                    }
                }
            }
        } else {
            // P1 Optimization Phase 1: Early exit for keep_last=false
            for el in Self::iter_keys(snapshot.as_ref(), tree_versioned) {
                let key = el?;
                let topo = u64::from_bytes(&key[0..8])?;

                if topo >= topoheight {
                    break; // Early exit: keys are sorted, stop when we reach threshold
                }

                Self::remove_from_disk_without_reading(snapshot.as_mut(), tree_versioned, &key)?;
            }
        }
        Ok(())
    }

    // Versioned key is a key that starts with the topoheight
    pub fn get_versioned_key<T: AsRef<[u8]>>(data: T, topoheight: TopoHeight) -> Vec<u8> {
        let bytes = data.as_ref();
        let mut buf = Vec::with_capacity(8 + bytes.len());
        buf.extend(topoheight.to_be_bytes());
        buf.extend(bytes);
        buf
    }
}
