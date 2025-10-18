use rocksdb::Direction;
use log::{debug, trace};
use tos_common::{
    block::TopoHeight,
    serializer::RawBytes,
    versioned_type::Versioned
};
use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode},
        RocksStorage,
        VersionedProvider
    }
};

mod balance;
mod contract;
mod multisig;
mod nonce;
mod registrations;
mod asset;
mod cache;
mod dag_order;

impl VersionedProvider for RocksStorage {}

impl RocksStorage {
    pub fn delete_versioned_at_topoheight(&mut self, column_pointer: Column, column_versioned: Column, topoheight: TopoHeight) -> Result<(), BlockchainError> {
        let prefix = topoheight.to_be_bytes();
        for res in Self::iter_owned_internal::<RawBytes, Option<TopoHeight>>(&self.db, self.snapshot.as_ref(), IteratorMode::WithPrefix(&prefix, Direction::Forward), column_versioned)? {
            let (key, prev_topo) = res?;

            Self::remove_from_disk_internal(&self.db, self.snapshot.as_mut(), column_versioned, &key)?;
            let pointer = self.load_optional_from_disk::<_, TopoHeight>(column_pointer, &key[8..])?;

            if let Some(pointer) = pointer {
                if pointer >= topoheight {
                    if let Some(prev_topo) = prev_topo {
                        Self::insert_into_disk_internal(&self.db, self.snapshot.as_mut(), column_pointer, &key[8..], &prev_topo.to_be_bytes(), false)?;
                    } else {
                        Self::remove_from_disk_internal(&self.db, self.snapshot.as_mut(), column_pointer, &key[8..])?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn delete_versioned_above_topoheight(&mut self, column_pointer: Column, column_versioned: Column, topoheight: TopoHeight) -> Result<(), BlockchainError> {
        let start = topoheight.to_be_bytes();
        for res in Self::iter_owned_internal::<RawBytes, Option<TopoHeight>>(&self.db, self.snapshot.as_ref(), IteratorMode::From(&start, Direction::Forward), column_versioned)? {
            let (key, prev_topo) = res?;

            Self::remove_from_disk_internal(&self.db, self.snapshot.as_mut(), column_versioned, &key)?;
            let pointer = self.load_optional_from_disk::<_, TopoHeight>(column_pointer, &key[8..])?;
            if pointer.is_none_or(|v| v > topoheight) {
                let filtered = prev_topo.filter(|v| *v <= topoheight);
                if filtered != pointer {
                    if let Some(pointer) = filtered {
                        Self::insert_into_disk_internal(&self.db, self.snapshot.as_mut(), column_pointer, &key[8..], &pointer.to_be_bytes(), false)?;
                    } else {
                        Self::remove_from_disk_internal(&self.db, self.snapshot.as_mut(), column_pointer, &key[8..])?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn delete_versioned_below_topoheight(&mut self, column_pointer: Column, column_versioned: Column, topoheight: TopoHeight, keep_last: bool) -> Result<(), BlockchainError> {
        if keep_last {
            // P1 Optimization Phase 2: Two-phase approach for keep_last=true
            // Phase 1: Find all affected accounts (those with versions < topoheight)
            use std::collections::HashSet;
            let mut affected_accounts: HashSet<Vec<u8>> = HashSet::new();

            // Scan versioned tree to find all entries below threshold
            for res in Self::iter_owned_internal::<RawBytes, ()>(&self.db, self.snapshot.as_ref(), IteratorMode::Start, column_versioned)? {
                let (key, _) = res?;
                // Key format: [topoheight(8)][account_key...]
                let key_topo = u64::from_be_bytes(key[0..8].try_into()
                    .map_err(|_| BlockchainError::CorruptedData)?);

                if key_topo >= topoheight {
                    break;  // Keys are sorted, early exit optimization
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
                let pointer: Option<TopoHeight> = self.load_optional_from_disk(column_pointer, &account_key)?;

                if let Some(pointer) = pointer {
                    // We fetch the last version to take its previous topoheight
                    // And we loop on it to delete them all until the end of the chained data
                    let mut prev_version = Some(pointer);
                    // If we are already below the threshold, we can directly erase without patching
                    let mut patched = pointer < topoheight;

                    // Craft by hand the key
                    let mut versioned_key = vec![0; account_key.len() + 8];
                    versioned_key[8..].copy_from_slice(&account_key);

                    while let Some(prev_topo) = prev_version {
                        versioned_key[0..8].copy_from_slice(&prev_topo.to_be_bytes());

                        // Delete this version from DB if its below the threshold
                        prev_version = self.load_from_disk(column_versioned, &versioned_key)?;
                        if patched {
                            Self::remove_from_disk_internal(&self.db, self.snapshot.as_mut(), column_versioned, &versioned_key)?;
                        } else if prev_version.is_some_and(|v| v < topoheight) {
                            if log::log_enabled!(log::Level::Trace) {
                                trace!("Patching versioned data at topoheight {}", topoheight);
                            }
                            patched = true;
                            let mut data: Versioned<RawBytes> = self.load_from_disk(column_versioned, &versioned_key)?;
                            data.set_previous_topoheight(None);

                            Self::insert_into_disk_internal(&self.db, self.snapshot.as_mut(), column_versioned, &versioned_key, &data, false)?;
                        }
                    }
                }
            }
        } else {
            // P1 Optimization Phase 1: Fix BUG + early exit for keep_last=false
            // BUG FIX: Was scanning FROM topoheight FORWARD (deleting >= topoheight)
            // Correct: Scan from START and stop at topoheight (delete < topoheight)
            for res in Self::iter_owned_internal::<RawBytes, ()>(&self.db, self.snapshot.as_ref(), IteratorMode::Start, column_versioned)? {
                let (key, _) = res?;
                // Key format: [topoheight(8)][data...]
                let key_topo = u64::from_be_bytes(key[0..8].try_into()
                    .map_err(|_| BlockchainError::CorruptedData)?);

                if key_topo >= topoheight {
                    break;  // Early exit: keys are sorted, stop when we reach threshold
                }

                Self::remove_from_disk_internal(&self.db, self.snapshot.as_mut(), column_versioned, &key)?;
            }
        }

        Ok(())
    }
}