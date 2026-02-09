// DelegationProvider implementation for RocksDB storage

use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode, RocksStorage},
        DelegationProvider, NetworkProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    account::{DelegatedFreezeRecord, DelegatorState},
    crypto::PublicKey,
    serializer::Serializer,
};

/// Build delegation record key: {delegator_pubkey[32]}{record_index[4 BE]}
fn build_record_key(delegator: &PublicKey, record_index: u32) -> [u8; 36] {
    let mut key = [0u8; 36];
    key[..32].copy_from_slice(delegator.as_bytes());
    key[32..36].copy_from_slice(&record_index.to_be_bytes());
    key
}

#[async_trait]
impl DelegationProvider for RocksStorage {
    async fn get_delegation_record(
        &self,
        delegator: &PublicKey,
        record_index: u32,
    ) -> Result<Option<DelegatedFreezeRecord>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "loading delegation record {} for {}",
                record_index,
                delegator.as_address(self.is_mainnet())
            );
        }
        let key = build_record_key(delegator, record_index);
        self.load_optional_from_disk(Column::DelegationRecords, &key)
    }

    async fn set_delegation_record(
        &mut self,
        delegator: &PublicKey,
        record_index: u32,
        record: &DelegatedFreezeRecord,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "storing delegation record {} for {}",
                record_index,
                delegator.as_address(self.is_mainnet())
            );
        }
        let key = build_record_key(delegator, record_index);
        self.insert_into_disk(Column::DelegationRecords, &key, record)
    }

    async fn delete_delegation_record(
        &mut self,
        delegator: &PublicKey,
        record_index: u32,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "deleting delegation record {} for {}",
                record_index,
                delegator.as_address(self.is_mainnet())
            );
        }
        let key = build_record_key(delegator, record_index);
        self.remove_from_disk(Column::DelegationRecords, &key)
    }

    async fn get_delegator_state(
        &self,
        delegator: &PublicKey,
    ) -> Result<DelegatorState, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "loading delegator state for {}",
                delegator.as_address(self.is_mainnet())
            );
        }
        let result: Option<DelegatorState> =
            self.load_optional_from_disk(Column::DelegatorState, delegator.as_bytes())?;
        Ok(result.unwrap_or_default())
    }

    async fn set_delegator_state(
        &mut self,
        delegator: &PublicKey,
        state: &DelegatorState,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "storing delegator state for {} (record_count={})",
                delegator.as_address(self.is_mainnet()),
                state.record_count
            );
        }
        self.insert_into_disk(Column::DelegatorState, delegator.as_bytes(), state)
    }

    async fn delete_delegator_state(
        &mut self,
        delegator: &PublicKey,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "deleting delegator state for {}",
                delegator.as_address(self.is_mainnet())
            );
        }
        self.remove_from_disk(Column::DelegatorState, delegator.as_bytes())
    }

    async fn list_all_delegation_records(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(PublicKey, u32, DelegatedFreezeRecord)>, BlockchainError> {
        let snapshot = self.snapshot.clone();
        let iter = Self::iter_raw_internal(
            &self.db,
            snapshot.as_ref(),
            IteratorMode::Start,
            Column::DelegationRecords,
        )?;
        let mut out = Vec::new();
        let mut skipped = 0usize;
        for item in iter {
            let (key_bytes, value_bytes) = item?;
            if key_bytes.len() < 36 {
                continue;
            }
            if skipped < skip {
                skipped += 1;
                continue;
            }

            let mut pubkey_bytes = [0u8; 32];
            pubkey_bytes.copy_from_slice(&key_bytes[..32]);
            let mut idx_bytes = [0u8; 4];
            idx_bytes.copy_from_slice(&key_bytes[32..36]);
            let record_index = u32::from_be_bytes(idx_bytes);

            let pubkey = PublicKey::from_bytes(&pubkey_bytes)
                .map_err(|_| BlockchainError::InvalidPublicKey)?;
            let record = DelegatedFreezeRecord::from_bytes(&value_bytes)?;
            out.push((pubkey, record_index, record));
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
}
