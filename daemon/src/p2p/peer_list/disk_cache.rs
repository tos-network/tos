use std::net::IpAddr;
use std::sync::Arc;

use log::info;
use rocksdb::{ColumnFamilyDescriptor, DBWithThreadMode, MultiThreaded, Options, WriteBatch};
use thiserror::Error;
use tokio::task::spawn_blocking;
use tos_common::serializer::{ReaderError, Serializer};

use super::PeerListEntry;

// Type alias for thread-safe RocksDB
type DB = DBWithThreadMode<MultiThreaded>;

const PEERLIST_CF: &str = "peerlist";

#[derive(Debug, Error)]
pub enum DiskError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("RocksDB error: {0}")]
    RocksDB(#[from] rocksdb::Error),
    #[error("Not found")]
    NotFound,
    #[error("Read error: {0}")]
    ReaderError(#[from] ReaderError),
}

// Previously, we were caching everything in the memory directly.
// But over time, the memory usage will grow and be a problem for low devices.
// DiskCache is a disk-based cache that stores the peerlist in the disk.
// It uses RocksDB as the underlying storage engine.
// This means IO operations instead of memory operations.
// Performance versus memory usage tradeoff.
pub struct DiskCache {
    // DB to use (wrapped in Arc for thread-safe access)
    db: Arc<DB>,
}

impl DiskCache {
    // Create a new disk cache
    pub fn new(filename: String) -> Result<Self, DiskError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        // Optimize for low memory usage
        opts.set_write_buffer_size(16 * 1024);
        opts.set_max_write_buffer_number(2);

        // Define column family for peerlist
        let cf_opts = Options::default();
        let cf_descriptor = ColumnFamilyDescriptor::new(PEERLIST_CF, cf_opts);

        let db = DB::open_cf_descriptors(&opts, &filename, vec![cf_descriptor])?;

        Ok(Self { db: Arc::new(db) })
    }

    // Check if a peerlist entry is present in DB
    pub fn has_peerlist_entry(&self, peer: &IpAddr) -> Result<bool, DiskError> {
        let cf = self
            .db
            .cf_handle(PEERLIST_CF)
            .ok_or_else(|| DiskError::NotFound)?;
        Ok(self.db.get_cf(&cf, peer.to_bytes())?.is_some())
    }

    // Set a peer state using its IP address
    pub fn set_peerlist_entry(&self, peer: &IpAddr, entry: PeerListEntry) -> Result<(), DiskError> {
        let cf = self
            .db
            .cf_handle(PEERLIST_CF)
            .ok_or_else(|| DiskError::NotFound)?;
        self.db.put_cf(&cf, peer.to_bytes(), entry.to_bytes())?;
        Ok(())
    }

    // Get a PeerListEntry using its IP address
    pub fn get_peerlist_entry(&self, peer: &IpAddr) -> Result<PeerListEntry, DiskError> {
        let cf = self
            .db
            .cf_handle(PEERLIST_CF)
            .ok_or_else(|| DiskError::NotFound)?;
        let v = self
            .db
            .get_cf(&cf, peer.to_bytes())?
            .map(|v| PeerListEntry::from_bytes(&v))
            .ok_or(DiskError::NotFound)??;

        Ok(v)
    }

    // Get all entries of peerlist
    // Returns a Vec to avoid lifetime issues with RocksDB iterators
    pub fn get_peerlist_entries(&self) -> Result<Vec<(IpAddr, PeerListEntry)>, DiskError> {
        let cf = self
            .db
            .cf_handle(PEERLIST_CF)
            .ok_or_else(|| DiskError::NotFound)?;
        let iter = self.db.iterator_cf(&cf, rocksdb::IteratorMode::Start);

        let mut entries = Vec::new();
        for item in iter {
            let (k, v) = item?;
            let ip = IpAddr::from_bytes(&k)?;
            let entry = PeerListEntry::from_bytes(&v)?;
            entries.push((ip, entry));
        }
        Ok(entries)
    }

    // Remove a peer from the peerlist
    pub fn remove_peerlist_entry(&self, peer: &IpAddr) -> Result<(), DiskError> {
        let cf = self
            .db
            .cf_handle(PEERLIST_CF)
            .ok_or_else(|| DiskError::NotFound)?;
        self.db.delete_cf(&cf, peer.to_bytes())?;
        Ok(())
    }

    // Clear the peerlist
    pub async fn clear_peerlist(&self) -> Result<(), DiskError> {
        let cf = self
            .db
            .cf_handle(PEERLIST_CF)
            .ok_or_else(|| DiskError::NotFound)?;

        // Collect all keys first
        let keys: Vec<Vec<u8>> = self
            .db
            .iterator_cf(&cf, rocksdb::IteratorMode::Start)
            .filter_map(|r| r.ok().map(|(k, _)| k.to_vec()))
            .collect();

        // Delete all keys using batch
        let mut batch = WriteBatch::default();
        for key in keys {
            batch.delete_cf(&cf, &key);
        }
        self.db.write(batch)?;

        // Flush to disk
        let db = self.db.clone();
        spawn_blocking(move || db.flush())
            .await
            .map_err(|e| DiskError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))??;
        Ok(())
    }

    // Flush the cache to disk
    pub async fn flush(&self) -> Result<(), DiskError> {
        info!("Flushing Disk Cache");
        let db = self.db.clone();
        spawn_blocking(move || db.flush())
            .await
            .map_err(|e| DiskError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))??;
        Ok(())
    }
}
