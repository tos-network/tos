// DelegationProvider: storage provider trait for energy delegation

use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    account::{DelegatedFreezeRecord, DelegatorState},
    crypto::PublicKey,
    serializer::Serializer,
};

/// Storage provider for energy delegation records and delegator state
#[async_trait]
pub trait DelegationProvider: Send + Sync {
    // ===== Delegation Records =====

    /// Get a delegation record by delegator public key and record index
    async fn get_delegation_record(
        &self,
        delegator: &PublicKey,
        record_index: u32,
    ) -> Result<Option<DelegatedFreezeRecord>, BlockchainError>;

    /// Store a delegation record
    async fn set_delegation_record(
        &mut self,
        delegator: &PublicKey,
        record_index: u32,
        record: &DelegatedFreezeRecord,
    ) -> Result<(), BlockchainError>;

    /// Delete a delegation record
    async fn delete_delegation_record(
        &mut self,
        delegator: &PublicKey,
        record_index: u32,
    ) -> Result<(), BlockchainError>;

    // ===== Delegator State =====

    /// Get delegator state (returns empty state if not found)
    async fn get_delegator_state(
        &self,
        delegator: &PublicKey,
    ) -> Result<DelegatorState, BlockchainError>;

    /// Store delegator state
    async fn set_delegator_state(
        &mut self,
        delegator: &PublicKey,
        state: &DelegatorState,
    ) -> Result<(), BlockchainError>;

    /// Delete delegator state
    async fn delete_delegator_state(
        &mut self,
        delegator: &PublicKey,
    ) -> Result<(), BlockchainError>;

    // ===== Bootstrap Sync =====

    /// List all delegation records with skip/limit pagination
    /// Returns (delegator, record_index, record) tuples
    async fn list_all_delegation_records(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(PublicKey, u32, DelegatedFreezeRecord)>, BlockchainError>;
}

// ============================================================================
// ConfigurableDelegationProvider - Test Infrastructure
// ============================================================================

/// Configurable in-memory delegation provider for testing
#[derive(Default)]
pub struct ConfigurableDelegationProvider {
    // Key: (delegator_bytes, record_index) -> DelegatedFreezeRecord
    records: std::collections::HashMap<([u8; 32], u32), DelegatedFreezeRecord>,
    // Key: delegator_bytes -> DelegatorState
    states: std::collections::HashMap<[u8; 32], DelegatorState>,

    // Fault injection flags
    fail_on_read: bool,
    fail_on_write: bool,
    fail_on_delete: bool,
}

impl ConfigurableDelegationProvider {
    /// Create a new empty provider
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable fault injection: reads will fail
    pub fn fail_on_read(mut self) -> Self {
        self.fail_on_read = true;
        self
    }

    /// Enable fault injection: writes will fail
    pub fn fail_on_write(mut self) -> Self {
        self.fail_on_write = true;
        self
    }

    /// Enable fault injection: deletes will fail
    pub fn fail_on_delete(mut self) -> Self {
        self.fail_on_delete = true;
        self
    }

    /// Get current record count (for testing)
    pub fn record_count(&self) -> usize {
        self.records.len()
    }
}

#[async_trait]
impl DelegationProvider for ConfigurableDelegationProvider {
    async fn get_delegation_record(
        &self,
        delegator: &PublicKey,
        record_index: u32,
    ) -> Result<Option<DelegatedFreezeRecord>, BlockchainError> {
        if self.fail_on_read {
            return Err(BlockchainError::Unknown);
        }
        let key = (*delegator.as_bytes(), record_index);
        Ok(self.records.get(&key).cloned())
    }

    async fn set_delegation_record(
        &mut self,
        delegator: &PublicKey,
        record_index: u32,
        record: &DelegatedFreezeRecord,
    ) -> Result<(), BlockchainError> {
        if self.fail_on_write {
            return Err(BlockchainError::Unknown);
        }
        let key = (*delegator.as_bytes(), record_index);
        self.records.insert(key, record.clone());
        Ok(())
    }

    async fn delete_delegation_record(
        &mut self,
        delegator: &PublicKey,
        record_index: u32,
    ) -> Result<(), BlockchainError> {
        if self.fail_on_delete {
            return Err(BlockchainError::Unknown);
        }
        let key = (*delegator.as_bytes(), record_index);
        self.records.remove(&key);
        Ok(())
    }

    async fn get_delegator_state(
        &self,
        delegator: &PublicKey,
    ) -> Result<DelegatorState, BlockchainError> {
        if self.fail_on_read {
            return Err(BlockchainError::Unknown);
        }
        Ok(self
            .states
            .get(delegator.as_bytes())
            .cloned()
            .unwrap_or_default())
    }

    async fn set_delegator_state(
        &mut self,
        delegator: &PublicKey,
        state: &DelegatorState,
    ) -> Result<(), BlockchainError> {
        if self.fail_on_write {
            return Err(BlockchainError::Unknown);
        }
        self.states.insert(*delegator.as_bytes(), state.clone());
        Ok(())
    }

    async fn delete_delegator_state(
        &mut self,
        delegator: &PublicKey,
    ) -> Result<(), BlockchainError> {
        if self.fail_on_delete {
            return Err(BlockchainError::Unknown);
        }
        self.states.remove(delegator.as_bytes());
        Ok(())
    }

    async fn list_all_delegation_records(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(PublicKey, u32, DelegatedFreezeRecord)>, BlockchainError> {
        if self.fail_on_read {
            return Err(BlockchainError::Unknown);
        }
        let mut out = Vec::new();
        let mut skipped = 0usize;
        for ((delegator_bytes, record_index), record) in &self.records {
            if skipped < skip {
                skipped += 1;
                continue;
            }
            let pubkey = PublicKey::from_bytes(delegator_bytes)
                .map_err(|_| BlockchainError::InvalidPublicKey)?;
            out.push((pubkey, *record_index, record.clone()));
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
}
