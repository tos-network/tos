// TOS Reachability RocksDB Storage Implementation
// Implements ReachabilityDataProvider trait for RocksDB backend

use async_trait::async_trait;
use log::trace;
use tos_common::crypto::Hash;

use crate::core::{
    error::BlockchainError,
    reachability::ReachabilityData,
    storage::{
        rocksdb::{Column, RocksStorage},
        ReachabilityDataProvider,
    },
};

#[async_trait]
impl ReachabilityDataProvider for RocksStorage {
    async fn get_reachability_data(&self, hash: &Hash) -> Result<ReachabilityData, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get reachability data for {}", hash);
        }
        self.load_from_disk(Column::ReachabilityData, hash)
    }

    async fn has_reachability_data(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has reachability data for {}", hash);
        }
        self.contains_data(Column::ReachabilityData, hash)
    }

    async fn set_reachability_data(&mut self, hash: &Hash, data: &ReachabilityData) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set reachability data for {}", hash);
        }
        self.insert_into_disk(Column::ReachabilityData, hash, data)
    }

    async fn delete_reachability_data(&mut self, hash: &Hash) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete reachability data for {}", hash);
        }
        self.remove_from_disk(Column::ReachabilityData, hash)
    }

    async fn get_reindex_root(&self) -> Result<Hash, BlockchainError> {
        trace!("get reindex root");
        self.load_from_disk(Column::Common, b"reindex_root")
    }

    async fn set_reindex_root(&mut self, root: Hash) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set reindex root to {}", root);
        }
        self.insert_into_disk(Column::Common, b"reindex_root", &root)
    }
}
