use crate::core::error::BlockchainError;
use async_trait::async_trait;

/// Result of pruning nonces: (removed_count, next_key_to_scan)
/// If next_key is None, all entries have been scanned (wrap around to start)
pub type PruneResult = (usize, Option<Vec<u8>>);

#[async_trait]
pub trait A2ANonceProvider {
    // ===== Bootstrap Sync =====

    /// List all A2A nonces with skip/limit pagination
    /// Returns (nonce_bytes, timestamp) pairs
    async fn list_all_a2a_nonces(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(Vec<u8>, u64)>, BlockchainError>;

    async fn get_a2a_nonce_timestamp(&self, nonce: &str) -> Result<Option<u64>, BlockchainError>;
    async fn set_a2a_nonce_timestamp(
        &mut self,
        nonce: &str,
        timestamp: u64,
    ) -> Result<(), BlockchainError>;
    async fn remove_a2a_nonce(&mut self, nonce: &str) -> Result<(), BlockchainError>;
    /// Prune expired nonces older than cutoff timestamp.
    /// Scans up to max_scan entries starting from start_key (or beginning if None).
    /// Returns (removed_count, next_key) for continuation-based scanning.
    async fn prune_a2a_nonces_older_than(
        &mut self,
        cutoff: u64,
        max_scan: usize,
        start_key: Option<&[u8]>,
    ) -> Result<PruneResult, BlockchainError>;
    /// Atomically check if nonce is unique and store it if so.
    /// Returns Ok(true) if nonce was stored (was unique/expired).
    /// Returns Ok(false) if nonce already exists and is not expired (replay detected).
    /// This prevents TOCTOU race conditions between check and store.
    async fn check_and_store_a2a_nonce(
        &mut self,
        nonce: &str,
        timestamp: u64,
        cutoff: u64,
    ) -> Result<bool, BlockchainError>;
}
