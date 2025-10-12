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
        trace!("get reachability data for {}", hash);
        self.load_from_disk(Column::ReachabilityData, hash)
    }

    async fn has_reachability_data(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        trace!("has reachability data for {}", hash);
        self.contains_data(Column::ReachabilityData, hash)
    }

    async fn set_reachability_data(&mut self, hash: &Hash, data: &ReachabilityData) -> Result<(), BlockchainError> {
        trace!("set reachability data for {}", hash);
        self.insert_into_disk(Column::ReachabilityData, hash, data)
    }

    async fn delete_reachability_data(&mut self, hash: &Hash) -> Result<(), BlockchainError> {
        trace!("delete reachability data for {}", hash);
        self.remove_from_disk(Column::ReachabilityData, hash)
    }
}
