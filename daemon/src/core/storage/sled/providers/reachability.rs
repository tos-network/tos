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
        trace!("get reachability data for {}", hash);
        self.load_from_disk(&self.reachability_data, hash.as_bytes(), DiskContext::ReachabilityData)
    }

    async fn has_reachability_data(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        trace!("has reachability data for {}", hash);
        self.contains_data(&self.reachability_data, hash)
    }

    async fn set_reachability_data(&mut self, hash: &Hash, data: &ReachabilityData) -> Result<(), BlockchainError> {
        trace!("set reachability data for {}", hash);

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
        trace!("delete reachability data for {}", hash);
        Self::remove_from_disk::<ReachabilityData>(self.snapshot.as_mut(), &self.reachability_data, hash.as_bytes())?;
        Ok(())
    }
}
