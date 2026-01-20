use crate::core::{
    error::BlockchainError,
    storage::{rocksdb::Column, snapshot::IteratorMode, A2ANonceProvider, RocksStorage},
};
use async_trait::async_trait;
use log::trace;
use tos_common::serializer::Serializer;

#[async_trait]
impl A2ANonceProvider for RocksStorage {
    async fn get_a2a_nonce_timestamp(&self, nonce: &str) -> Result<Option<u64>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get a2a nonce timestamp");
        }
        self.load_optional_from_disk(Column::A2ANonces, nonce.as_bytes())
    }

    async fn set_a2a_nonce_timestamp(
        &mut self,
        nonce: &str,
        timestamp: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set a2a nonce timestamp");
        }
        self.insert_into_disk(Column::A2ANonces, nonce.as_bytes(), &timestamp)
    }

    async fn remove_a2a_nonce(&mut self, nonce: &str) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("remove a2a nonce");
        }
        self.remove_from_disk(Column::A2ANonces, nonce.as_bytes())
    }

    async fn prune_a2a_nonces_older_than(
        &mut self,
        cutoff: u64,
        max_scan: usize,
    ) -> Result<usize, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "prune a2a nonces older than {} (max_scan={})",
                cutoff,
                max_scan
            );
        }

        let iter = RocksStorage::iter_raw_internal(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::A2ANonces,
        )?;

        let mut scanned = 0usize;
        let mut removed = 0usize;
        let mut keys_to_remove: Vec<Vec<u8>> = Vec::new();

        for result in iter {
            let (key, value) = result?;
            scanned = scanned.saturating_add(1);

            let timestamp = u64::from_bytes(value.as_ref())?;
            if timestamp < cutoff {
                keys_to_remove.push(key.as_ref().to_vec());
                removed = removed.saturating_add(1);
            }

            if scanned >= max_scan {
                break;
            }
        }

        for key in keys_to_remove {
            self.remove_from_disk(Column::A2ANonces, &key)?;
        }

        Ok(removed)
    }
}
