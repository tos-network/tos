use crate::core::error::BlockchainError;
use async_trait::async_trait;

#[async_trait]
pub trait A2ANonceProvider {
    async fn get_a2a_nonce_timestamp(&self, nonce: &str) -> Result<Option<u64>, BlockchainError>;
    async fn set_a2a_nonce_timestamp(
        &mut self,
        nonce: &str,
        timestamp: u64,
    ) -> Result<(), BlockchainError>;
    async fn remove_a2a_nonce(&mut self, nonce: &str) -> Result<(), BlockchainError>;
    async fn prune_a2a_nonces_older_than(
        &mut self,
        cutoff: u64,
        max_scan: usize,
    ) -> Result<usize, BlockchainError>;
}
