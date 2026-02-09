// TnsProvider implementation for RocksDB storage

use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode, RocksStorage},
        NetworkProvider, TnsProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::crypto::{Hash, PublicKey};

#[async_trait]
impl TnsProvider for RocksStorage {
    // ===== Bootstrap Sync =====

    async fn list_all_tns_names(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(Hash, PublicKey)>, BlockchainError> {
        let iter = self.iter::<Hash, PublicKey>(Column::TnsNameToOwner, IteratorMode::Start)?;
        let mut out = Vec::new();
        let mut skipped = 0usize;
        for item in iter {
            let (key, value) = item?;
            if skipped < skip {
                skipped += 1;
                continue;
            }
            out.push((key, value));
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    // ===== Name Registration =====

    async fn is_name_registered(&self, name_hash: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("checking if name {} is registered", name_hash);
        }
        self.contains_data(Column::TnsNameToOwner, name_hash.as_bytes())
    }

    async fn get_name_owner(&self, name_hash: &Hash) -> Result<Option<PublicKey>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("getting owner for name {}", name_hash);
        }
        self.load_optional_from_disk(Column::TnsNameToOwner, name_hash.as_bytes())
    }

    async fn account_has_name(&self, owner: &PublicKey) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "checking if account {} has a name",
                owner.as_address(self.is_mainnet())
            );
        }
        self.contains_data(Column::TnsAccountToName, owner.as_bytes())
    }

    async fn get_account_name(&self, owner: &PublicKey) -> Result<Option<Hash>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting name for account {}",
                owner.as_address(self.is_mainnet())
            );
        }
        self.load_optional_from_disk(Column::TnsAccountToName, owner.as_bytes())
    }

    async fn register_name(
        &mut self,
        name_hash: Hash,
        owner: PublicKey,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "registering name {} for account {}",
                name_hash,
                owner.as_address(self.is_mainnet())
            );
        }

        // Check if name is already registered
        if self.is_name_registered(&name_hash).await? {
            return Err(BlockchainError::TnsNameAlreadyRegistered);
        }

        // Check if account already has a name
        if self.account_has_name(&owner).await? {
            return Err(BlockchainError::TnsAccountAlreadyHasName);
        }

        // Store name -> owner mapping
        self.insert_into_disk(Column::TnsNameToOwner, name_hash.as_bytes(), &owner)?;

        // Store owner -> name mapping (reverse index)
        self.insert_into_disk(Column::TnsAccountToName, owner.as_bytes(), &name_hash)?;

        Ok(())
    }

    // ===== Administrative Operations =====

    async fn delete_name_registration(&mut self, name_hash: &Hash) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("deleting name registration {}", name_hash);
        }

        // Get the owner first so we can delete the reverse mapping
        if let Some(owner) = self.get_name_owner(name_hash).await? {
            // Delete owner -> name mapping
            self.remove_from_disk(Column::TnsAccountToName, owner.as_bytes())?;
        }

        // Delete name -> owner mapping
        self.remove_from_disk(Column::TnsNameToOwner, name_hash.as_bytes())?;

        Ok(())
    }

    async fn delete_account_name(&mut self, owner: &PublicKey) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "deleting account name for {}",
                owner.as_address(self.is_mainnet())
            );
        }

        // Get the name hash first so we can delete the forward mapping
        if let Some(name_hash) = self.get_account_name(owner).await? {
            // Delete name -> owner mapping
            self.remove_from_disk(Column::TnsNameToOwner, name_hash.as_bytes())?;
        }

        // Delete owner -> name mapping
        self.remove_from_disk(Column::TnsAccountToName, owner.as_bytes())?;

        Ok(())
    }
}
