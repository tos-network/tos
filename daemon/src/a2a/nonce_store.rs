use std::sync::Arc;

use crate::a2a::auth::AuthError;
use crate::core::blockchain::Blockchain;
use crate::core::storage::{A2ANonceProvider, Storage};
use async_trait::async_trait;
use log::warn;

#[async_trait]
pub trait A2ANonceStore: Send + Sync {
    async fn get_nonce_timestamp(&self, nonce: &str) -> Result<Option<u64>, AuthError>;
    async fn set_nonce_timestamp(&self, nonce: &str, timestamp: u64) -> Result<(), AuthError>;
    async fn remove_nonce(&self, nonce: &str) -> Result<(), AuthError>;
    async fn prune_expired(&self, cutoff: u64, max_scan: usize) -> Result<usize, AuthError>;
}

pub struct StorageNonceStore<S: Storage> {
    blockchain: Arc<Blockchain<S>>,
}

impl<S: Storage> StorageNonceStore<S> {
    pub fn new(blockchain: Arc<Blockchain<S>>) -> Self {
        Self { blockchain }
    }
}

#[async_trait]
impl<S> A2ANonceStore for StorageNonceStore<S>
where
    S: Storage + A2ANonceProvider + Send + Sync,
{
    async fn get_nonce_timestamp(&self, nonce: &str) -> Result<Option<u64>, AuthError> {
        let storage = self.blockchain.get_storage().read().await;
        storage.get_a2a_nonce_timestamp(nonce).await.map_err(|e| {
            warn!("failed to load a2a nonce timestamp: {e}");
            AuthError::TosNonceInvalid
        })
    }

    async fn set_nonce_timestamp(&self, nonce: &str, timestamp: u64) -> Result<(), AuthError> {
        let mut storage = self.blockchain.get_storage().write().await;
        storage
            .set_a2a_nonce_timestamp(nonce, timestamp)
            .await
            .map_err(|e| {
                warn!("failed to store a2a nonce timestamp: {e}");
                AuthError::TosNonceInvalid
            })
    }

    async fn remove_nonce(&self, nonce: &str) -> Result<(), AuthError> {
        let mut storage = self.blockchain.get_storage().write().await;
        storage.remove_a2a_nonce(nonce).await.map_err(|e| {
            warn!("failed to remove a2a nonce: {e}");
            AuthError::TosNonceInvalid
        })
    }

    async fn prune_expired(&self, cutoff: u64, max_scan: usize) -> Result<usize, AuthError> {
        let mut storage = self.blockchain.get_storage().write().await;
        storage
            .prune_a2a_nonces_older_than(cutoff, max_scan)
            .await
            .map_err(|e| {
                warn!("failed to prune a2a nonces: {e}");
                AuthError::TosNonceInvalid
            })
    }
}
