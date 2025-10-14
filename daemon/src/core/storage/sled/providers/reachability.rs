// TOS Reachability Sled Storage Implementation
// Implements ReachabilityDataProvider trait for Sled backend

use async_trait::async_trait;
use log::trace;
use tos_common::{crypto::Hash, serializer::Serializer};

use crate::core::{
    error::{BlockchainError, DiskContext},
    reachability::ReachabilityData,
    storage::{ReachabilityDataProvider, SledStorage},
};

#[async_trait]
impl ReachabilityDataProvider for SledStorage {
    async fn get_reachability_data(&self, hash: &Hash) -> Result<ReachabilityData, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get reachability data for {}", hash);
        }
        self.load_from_disk(&self.reachability_data, hash.as_bytes(), DiskContext::ReachabilityData)
    }

    async fn has_reachability_data(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has reachability data for {}", hash);
        }
        self.contains_data(&self.reachability_data, hash)
    }

    async fn set_reachability_data(&mut self, hash: &Hash, data: &ReachabilityData) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set reachability data for {}", hash);
        }

        // Serialize using Serializer trait
        let data_bytes = data.to_bytes();
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.reachability_data,
            hash.as_bytes(),
            data_bytes,
        )?;

        Ok(())
    }

    async fn delete_reachability_data(&mut self, hash: &Hash) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("delete reachability data for {}", hash);
        }
        Self::remove_from_disk::<ReachabilityData>(self.snapshot.as_mut(), &self.reachability_data, hash.as_bytes())?;
        Ok(())
    }

    async fn get_reindex_root(&self) -> Result<Hash, BlockchainError> {
        trace!("get reindex root");
        self.load_from_disk(&self.extra, b"reindex_root", DiskContext::ReachabilityData)
    }

    async fn set_reindex_root(&mut self, root: Hash) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set reindex root to {}", root);
        }
        let root_bytes = root.to_bytes();
        Self::insert_into_disk(
            self.snapshot.as_mut(),
            &self.extra,
            b"reindex_root",
            root_bytes.to_vec(),
        )?;
        Ok(())
    }
}
