use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::Column, snapshot::Direction, snapshot::IteratorMode, A2ANonceProvider,
        PruneResult, RocksStorage,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::serializer::Serializer;

#[async_trait]
impl A2ANonceProvider for RocksStorage {
    async fn list_all_a2a_nonces(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, u64)>, BlockchainError> {
        let iter = RocksStorage::iter_raw_internal(
            &self.db,
            self.snapshot.as_ref(),
            IteratorMode::Start,
            Column::A2ANonces,
        )?;
        let mut out = Vec::new();
        let mut skipped = 0usize;
        for result in iter {
            let (key, value) = result?;
            if skipped < skip {
                skipped += 1;
                continue;
            }
            let timestamp = u64::from_bytes(value.as_ref())?;
            out.push((key.as_ref().to_vec(), timestamp));
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

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
        start_key: Option<&[u8]>,
    ) -> Result<PruneResult, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "prune a2a nonces older than {} (max_scan={}, start_key={:?})",
                cutoff,
                max_scan,
                start_key.map(|k| String::from_utf8_lossy(k).to_string())
            );
        }

        // Use continuation-based scanning: start from start_key if provided
        let mode = match start_key {
            Some(key) => IteratorMode::From(key, Direction::Forward),
            None => IteratorMode::Start,
        };

        let iter = RocksStorage::iter_raw_internal(
            &self.db,
            self.snapshot.as_ref(),
            mode,
            Column::A2ANonces,
        )?;

        let mut scanned = 0usize;
        let mut removed = 0usize;
        let mut keys_to_remove: Vec<Vec<u8>> = Vec::new();
        let mut last_key: Option<Vec<u8>> = None;
        let mut is_first = true;

        for result in iter {
            let (key, value) = result?;

            // Skip the first item ONLY if it exactly matches start_key (it was already processed)
            // This prevents skipping unprocessed keys when start_key was deleted and the iterator
            // starts from the next greater key instead
            if is_first {
                is_first = false;
                if let Some(sk) = start_key {
                    if key.as_ref() == sk {
                        continue;
                    }
                }
            }

            scanned = scanned.saturating_add(1);

            let timestamp = u64::from_bytes(value.as_ref())?;
            if timestamp < cutoff {
                keys_to_remove.push(key.as_ref().to_vec());
                removed = removed.saturating_add(1);
            }

            // Track the last key we saw for continuation
            last_key = Some(key.as_ref().to_vec());

            if scanned >= max_scan {
                break;
            }
        }

        for key in keys_to_remove {
            self.remove_from_disk(Column::A2ANonces, &key)?;
        }

        // If we scanned less than max_scan, we've reached the end - wrap around
        let next_key = if scanned < max_scan {
            None // Signal to wrap around to start
        } else {
            last_key
        };

        Ok((removed, next_key))
    }

    async fn check_and_store_a2a_nonce(
        &mut self,
        nonce: &str,
        timestamp: u64,
        cutoff: u64,
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("check and store a2a nonce atomically");
        }

        // Atomically check and store: if exists and not expired, return false (replay)
        // Note: RocksDB single-key operations are atomic within a single write batch
        let key = nonce.as_bytes();
        if let Some(stored_ts) =
            self.load_optional_from_disk::<[u8], u64>(Column::A2ANonces, key)?
        {
            if stored_ts >= cutoff {
                // Nonce exists and is not expired - replay detected
                return Ok(false);
            }
            // Nonce exists but is expired - will be overwritten
        }

        // Store the nonce with current timestamp
        self.insert_into_disk(Column::A2ANonces, key, &timestamp)?;
        Ok(true)
    }
}
