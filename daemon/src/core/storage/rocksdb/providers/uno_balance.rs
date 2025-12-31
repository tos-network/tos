//! UNO Balance Provider implementation for RocksDB

use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{AccountId, Column},
        NetworkProvider, RocksStorage, UnoBalanceProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    account::{BalanceType, UnoAccountSummary, UnoBalance, VersionedUnoBalance},
    block::TopoHeight,
    crypto::PublicKey,
};

#[async_trait]
impl UnoBalanceProvider for RocksStorage {
    async fn has_uno_balance_for(&self, key: &PublicKey) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has uno balance for {}", key.as_address(self.is_mainnet()));
        }
        let account_id = self.get_account_id(key)?;
        let key = Self::get_uno_balance_key(account_id);
        self.contains_data(Column::UnoBalances, &key)
    }

    async fn has_uno_balance_at_exact_topoheight(
        &self,
        key: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "has uno balance at exact topoheight {} for {}",
                topoheight,
                key.as_address(self.is_mainnet())
            );
        }
        let account_id = self.get_account_id(key)?;
        let key = Self::get_versioned_uno_balance_key(account_id, topoheight);
        self.contains_data(Column::VersionedUnoBalances, &key)
    }

    async fn get_uno_balance_at_exact_topoheight(
        &self,
        key: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<VersionedUnoBalance, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get uno balance at exact topoheight {} for {}",
                topoheight,
                key.as_address(self.is_mainnet())
            );
        }
        let account_id = self.get_account_id(key)?;
        let key = Self::get_versioned_uno_balance_key(account_id, topoheight);
        self.load_from_disk(Column::VersionedUnoBalances, &key)
    }

    async fn get_uno_balance_at_maximum_topoheight(
        &self,
        key: &PublicKey,
        maximum_topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, VersionedUnoBalance)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get uno balance at maximum topoheight {} for {}",
                maximum_topoheight,
                key.as_address(self.is_mainnet())
            );
        }
        let Some(account_id) = self.get_optional_account_id(key)? else {
            if log::log_enabled!(log::Level::Trace) {
                trace!("no account found for {}", key.as_address(self.is_mainnet()));
            }
            return Ok(None);
        };

        let versioned_key = Self::get_versioned_uno_balance_key(account_id, maximum_topoheight);
        // Check if we have a balance at exact topoheight
        let mut topo = if self.contains_data(Column::VersionedUnoBalances, &versioned_key)? {
            if log::log_enabled!(log::Level::Trace) {
                trace!("using topoheight {}", maximum_topoheight);
            }
            Some(maximum_topoheight)
        } else {
            if log::log_enabled!(log::Level::Trace) {
                trace!("load latest version available");
            }
            // Skip the topoheight prefix (8 bytes), load the pointer
            self.load_optional_from_disk(Column::UnoBalances, &versioned_key[8..16])?
        };

        // Iterate over our linked list of versions
        while let Some(topoheight) = topo {
            let versioned_key = Self::get_versioned_uno_balance_key(account_id, topoheight);
            if topoheight <= maximum_topoheight {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "versioned uno balance of {} found at {}",
                        key.as_address(self.is_mainnet()),
                        topoheight
                    );
                }
                let version = self.load_from_disk(Column::VersionedUnoBalances, &versioned_key)?;
                return Ok(Some((topoheight, version)));
            }

            topo = self.load_from_disk(Column::VersionedUnoBalances, &versioned_key)?;
        }

        Ok(None)
    }

    async fn get_last_topoheight_for_uno_balance(
        &self,
        key: &PublicKey,
    ) -> Result<TopoHeight, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get last topoheight for uno balance {}",
                key.as_address(self.is_mainnet())
            );
        }
        let account_id = self.get_account_id(key)?;
        let key = Self::get_uno_balance_key(account_id);
        self.load_from_disk(Column::UnoBalances, &key)
    }

    async fn get_new_versioned_uno_balance(
        &self,
        key: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<(VersionedUnoBalance, bool), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get new versioned uno balance for {} at topoheight {}",
                key.as_address(self.is_mainnet()),
                topoheight
            );
        }
        match self
            .get_uno_balance_at_maximum_topoheight(key, topoheight)
            .await?
        {
            Some((topo, mut version)) => {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "Mark version as clean for {} at topoheight {}",
                        key.as_address(self.is_mainnet()),
                        topo
                    );
                }
                version.prepare_new(Some(topo));
                Ok((version, false))
            }
            None => {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "no uno balance found, new version for {}",
                        key.as_address(self.is_mainnet())
                    );
                }
                Ok((VersionedUnoBalance::zero(), true))
            }
        }
    }

    async fn get_last_uno_balance(
        &self,
        key: &PublicKey,
    ) -> Result<(TopoHeight, VersionedUnoBalance), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get last uno balance for {}",
                key.as_address(self.is_mainnet())
            );
        }
        let account_id = self.get_account_id(key)?;

        let key = Self::get_uno_balance_key(account_id);
        let topoheight = self.load_from_disk(Column::UnoBalances, &key)?;

        let versioned_key = Self::get_versioned_uno_balance_key(account_id, topoheight);
        let versioned_balance = self.load_from_disk(Column::VersionedUnoBalances, &versioned_key)?;

        Ok((topoheight, versioned_balance))
    }

    async fn get_uno_output_balance_at_maximum_topoheight(
        &self,
        key: &PublicKey,
        maximum_topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, VersionedUnoBalance)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get uno output balance at maximum topoheight {} for {}",
                maximum_topoheight,
                key.as_address(self.is_mainnet())
            );
        }
        self.get_uno_output_balance_in_range(key, 0, maximum_topoheight)
            .await
    }

    async fn get_uno_output_balance_in_range(
        &self,
        key: &PublicKey,
        minimum_topoheight: TopoHeight,
        maximum_topoheight: TopoHeight,
    ) -> Result<Option<(TopoHeight, VersionedUnoBalance)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get uno output balance in range {} - {} for {}",
                minimum_topoheight,
                maximum_topoheight,
                key.as_address(self.is_mainnet())
            );
        }
        let account_id = self.get_account_id(key)?;

        let versioned_key = Self::get_versioned_uno_balance_key(account_id, maximum_topoheight);
        let Some(pointer) =
            self.load_optional_from_disk::<_, TopoHeight>(Column::UnoBalances, &versioned_key[8..])?
        else {
            if log::log_enabled!(log::Level::Trace) {
                trace!("no uno balance pointer found");
            }
            return Ok(None);
        };

        let start_topo = if pointer > maximum_topoheight
            && self.contains_data(Column::VersionedUnoBalances, &versioned_key)?
        {
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "uno balance found at topoheight {}, using it",
                    maximum_topoheight
                );
            }
            maximum_topoheight
        } else {
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "uno balance not found at topoheight {}, using topoheight pointer {}",
                    maximum_topoheight,
                    pointer
                );
            }
            pointer
        };

        let mut topo = Some(start_topo);
        while let Some(topoheight) = topo {
            if topoheight < minimum_topoheight {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "uno balance reached minimum topoheight {}, stopping search",
                        minimum_topoheight
                    );
                }
                break;
            }

            let versioned_key = Self::get_versioned_uno_balance_key(account_id, topoheight);
            let (prev_topo, balance_type): (Option<u64>, BalanceType) =
                self.load_from_disk(Column::VersionedUnoBalances, &versioned_key)?;

            if topoheight <= maximum_topoheight && balance_type.contains_output() {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "uno balance of {} is updated at {}",
                        key.as_address(self.is_mainnet()),
                        topoheight
                    );
                }
                let version = self.load_from_disk(Column::VersionedUnoBalances, &versioned_key)?;
                return Ok(Some((topoheight, version)));
            }

            topo = prev_topo;
        }

        Ok(None)
    }

    fn set_last_topoheight_for_uno_balance(
        &mut self,
        key: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set last topoheight for uno balance {} to {}",
                key.as_address(self.is_mainnet()),
                topoheight
            );
        }
        let account_id = self.get_account_id(key)?;
        let key = Self::get_uno_balance_key(account_id);
        self.insert_into_disk(Column::UnoBalances, &key, &topoheight.to_be_bytes())
    }

    async fn set_last_uno_balance_to(
        &mut self,
        key: &PublicKey,
        topoheight: TopoHeight,
        version: &VersionedUnoBalance,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set last uno balance for {} to {}",
                key.as_address(self.is_mainnet()),
                topoheight
            );
        }
        let account_id = self.get_account_id(key)?;

        let versioned_key = Self::get_versioned_uno_balance_key(account_id, topoheight);
        self.insert_into_disk(
            Column::UnoBalances,
            &versioned_key[8..],
            &topoheight.to_be_bytes(),
        )?;
        self.insert_into_disk(Column::VersionedUnoBalances, &versioned_key, version)
    }

    async fn set_uno_balance_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
        key: &PublicKey,
        balance: &VersionedUnoBalance,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set uno balance at topoheight {} for {}",
                topoheight,
                key.as_address(self.is_mainnet())
            );
        }
        let account_id = self.get_account_id(key)?;

        let versioned_key = Self::get_versioned_uno_balance_key(account_id, topoheight);
        self.insert_into_disk(Column::VersionedUnoBalances, &versioned_key, balance)
    }

    async fn get_uno_account_summary_for(
        &self,
        key: &PublicKey,
        min_topoheight: TopoHeight,
        max_topoheight: TopoHeight,
    ) -> Result<Option<UnoAccountSummary>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get uno account summary for {} min topoheight {} max topoheight {}",
                key.as_address(self.is_mainnet()),
                min_topoheight,
                max_topoheight
            );
        }
        if let Some((topo, version)) = self
            .get_uno_balance_at_maximum_topoheight(key, max_topoheight)
            .await?
        {
            if topo < min_topoheight {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "uno balance found at topoheight {} below min topoheight {}, skipping",
                        topo,
                        min_topoheight
                    );
                }
                return Ok(None);
            }

            let mut account = UnoAccountSummary {
                output_topoheight: None,
                stable_topoheight: topo,
            };

            if version.contains_output() || version.get_previous_topoheight().is_none() {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "uno account summary found for {} at topoheight {}",
                        key.as_address(self.is_mainnet()),
                        topo
                    );
                }
                return Ok(Some(account));
            }

            let account_id = self.get_account_id(key)?;

            let mut previous = version.get_previous_topoheight();
            while let Some(topo) = previous {
                let versioned_key = Self::get_versioned_uno_balance_key(account_id, topo);
                let (previous_topo, balance_type): (Option<u64>, BalanceType) =
                    self.load_from_disk(Column::VersionedUnoBalances, &versioned_key)?;
                if balance_type.contains_output() {
                    if log::log_enabled!(log::Level::Trace) {
                        trace!(
                            "uno balance containing output found for {} at topoheight {}",
                            key.as_address(self.is_mainnet()),
                            topo
                        );
                    }
                    account.output_topoheight = Some(topo);
                    break;
                }

                previous = previous_topo;
            }

            return Ok(Some(account));
        }

        Ok(None)
    }

    async fn get_spendable_uno_balances_for(
        &self,
        key: &PublicKey,
        min_topoheight: TopoHeight,
        max_topoheight: TopoHeight,
        maximum: usize,
    ) -> Result<(Vec<UnoBalance>, Option<TopoHeight>), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get spendable uno balances for {} min topoheight {} max topoheight {}",
                key.as_address(self.is_mainnet()),
                min_topoheight,
                max_topoheight
            );
        }
        let account_id = self.get_account_id(key)?;

        let mut balances = Vec::new();
        let mut next_topo = Some(max_topoheight);

        while let Some(topo) = next_topo
            .take()
            .filter(|&t| t >= min_topoheight && balances.len() < maximum)
        {
            let versioned_key = Self::get_versioned_uno_balance_key(account_id, topo);
            let version = self.load_from_disk::<_, VersionedUnoBalance>(
                Column::VersionedUnoBalances,
                &versioned_key,
            )?;
            let has_output = version.contains_output();
            let previous_topoheight = version.get_previous_topoheight();

            balances.push(version.as_uno_balance(topo));

            if has_output {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "output uno balance found for {} at topoheight {}",
                        key.as_address(self.is_mainnet()),
                        topo
                    );
                }
                break;
            }

            next_topo = previous_topoheight;
        }

        Ok((balances, next_topo))
    }

    async fn delete_uno_balance_at_topoheight(
        &mut self,
        key: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "delete uno balance at topoheight {} for {}",
                topoheight,
                key.as_address(self.is_mainnet())
            );
        }
        let account_id = self.get_account_id(key)?;
        let versioned_key = Self::get_versioned_uno_balance_key(account_id, topoheight);
        self.remove_from_disk(Column::VersionedUnoBalances, &versioned_key)
    }
}

impl RocksStorage {
    /// Get the key for UNO balance pointer
    /// Format: {account_id} (8 bytes)
    pub fn get_uno_balance_key(account: AccountId) -> [u8; 8] {
        account.to_be_bytes()
    }

    /// Get the key for versioned UNO balance
    /// Format: {topoheight}{account_id} (16 bytes)
    pub fn get_versioned_uno_balance_key(account: AccountId, topoheight: TopoHeight) -> [u8; 16] {
        let mut buffer = [0; 16];
        buffer[0..8].copy_from_slice(&topoheight.to_be_bytes());
        buffer[8..16].copy_from_slice(&account.to_be_bytes());
        buffer
    }
}
