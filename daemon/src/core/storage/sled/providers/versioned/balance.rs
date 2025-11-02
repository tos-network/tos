use crate::core::{
    error::{BlockchainError, DiskContext},
    storage::{SledStorage, VersionedBalanceProvider},
};
use async_trait::async_trait;
use log::{debug, trace};
use tos_common::{account::BalanceType, block::TopoHeight, serializer::Serializer};

#[async_trait]
impl VersionedBalanceProvider for SledStorage {
    async fn delete_versioned_balances_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned balances at topoheight {}", topoheight);
        }
        Self::delete_versioned_tree_at_topoheight(
            &mut self.snapshot,
            &self.balances,
            &self.versioned_balances,
            topoheight,
        )?;
        Ok(())
    }

    async fn delete_versioned_balances_above_topoheight(
        &mut self,
        topoheight: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete versioned balances above topoheight {}!", topoheight);
        }
        Self::delete_versioned_tree_above_topoheight(
            &mut self.snapshot,
            &self.balances,
            &self.versioned_balances,
            topoheight,
            DiskContext::VersionedBalance,
        )
    }

    async fn delete_versioned_balances_below_topoheight(
        &mut self,
        topoheight: u64,
        keep_last: bool,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete versioned balances (keep last: {}) below topoheight {}!",
                keep_last,
                topoheight
            );
        }
        if !keep_last {
            Self::delete_versioned_tree_below_topoheight(
                &mut self.snapshot,
                &self.balances,
                &self.versioned_balances,
                topoheight,
                keep_last,
                DiskContext::VersionedBalance,
            )
        } else {
            // P1 Optimization: Two-phase approach for keep_last=true with output balance logic
            // We need to search until we find the latest output version and delete everything below it

            // Phase 1: Find all affected accounts (those with versions < topoheight)
            use std::collections::HashSet;
            let mut affected_accounts: HashSet<Vec<u8>> = HashSet::new();

            for el in Self::iter_keys(self.snapshot.as_ref(), &self.versioned_balances) {
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
                debug!("Found {} affected balance accounts below topoheight {} (instead of scanning all accounts)",
                    affected_accounts.len(), topoheight);
            }

            // Phase 2: Only check affected accounts for output balance logic
            for k in affected_accounts {
                let value = Self::load_optional_from_disk_internal::<Vec<u8>>(
                    self.snapshot.as_ref(),
                    &self.balances,
                    &k,
                )?;

                if let Some(value) = value {
                    let topo = TopoHeight::from_bytes(&value)?;

                    // We fetch the last version to take its previous topoheight
                    // And we loop on it to delete them all until the end of the chained data
                    // But before deleting, we need to find if we are below a output balance
                    let mut prev_version = self.load_from_disk(
                        &self.versioned_balances,
                        &Self::get_versioned_key(&k, topo),
                        DiskContext::BalanceAtTopoHeight(topo),
                    )?;
                    let mut delete = false;
                    while let Some(prev_topo) = prev_version {
                        let key = Self::get_versioned_key(&k, prev_topo);

                        // Delete this version from DB if its below the threshold
                        if delete {
                            prev_version = Self::remove_from_disk(
                                self.snapshot.as_mut(),
                                &self.versioned_balances,
                                &key,
                            )?;
                        } else {
                            let (prev_topo, ty) = self
                                .load_from_disk::<(Option<u64>, BalanceType)>(
                                    &self.versioned_balances,
                                    &key,
                                    DiskContext::BalanceAtTopoHeight(prev_topo),
                                )?;
                            // If this version contains an output, that means we can delete all others below
                            delete = ty.contains_output();
                            prev_version = prev_topo;
                        }
                    }
                }
            }

            Ok(())
        }
    }
}
