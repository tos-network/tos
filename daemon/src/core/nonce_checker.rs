use std::collections::HashMap;

use indexmap::IndexMap;
use log::{trace, warn};
use tos_common::{
    account::Nonce,
    block::TopoHeight,
    crypto::PublicKey
};

use super::{storage::Storage, error::BlockchainError, executor::ThreadId};

struct AccountEntry {
    initial_nonce: Nonce,
    expected_nonce: Nonce,
    used_nonces: IndexMap<Nonce, TopoHeight>
}

impl AccountEntry {
    pub fn new(nonce: Nonce) -> Self {
        Self {
            initial_nonce: nonce,
            expected_nonce: nonce,
            used_nonces: IndexMap::new()
        }
    }

    pub fn contains_nonce(&self, nonce: &Nonce) -> bool {
        self.used_nonces.contains_key(nonce)
    }

    pub fn insert_nonce_at_topoheight(&mut self, nonce: Nonce, topoheight: TopoHeight) -> bool {
        if log::log_enabled!(log::Level::Trace) {
            trace!("insert nonce {} at topoheight {}, (expected: {})", nonce, topoheight, self.expected_nonce);
        }

        // Check if nonce was already used
        if self.contains_nonce(&nonce) {
            return false;
        }

        // Check that nonce is not below the initial nonce (prevent rollback attacks)
        if nonce < self.initial_nonce {
            return false;
        }

        // Allow nonce jumps in case of DAG reorg, but update expected_nonce to the highest seen + 1
        if nonce >= self.expected_nonce {
            self.expected_nonce = nonce + 1;
        }

        self.used_nonces.insert(nonce, topoheight);

        true
    }
}

// ============================================================================
// Nonce Reservation for Parallel Execution (TIP-PE)
// ============================================================================

/// Reservation record for a nonce during parallel execution
///
/// Nonces must be reserved before parallel execution to prevent double-spend.
/// The reserve → execute → commit/cancel lifecycle ensures deterministic ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonceReservation {
    /// Account that owns this nonce
    pub account: PublicKey,
    /// The reserved nonce value
    pub nonce: Nonce,
    /// Thread ID that reserved this nonce
    pub thread_id: ThreadId,
    /// Topoheight at which reservation was made
    pub topoheight: TopoHeight,
}

// A simple cache that checks if a nonce has already been used
// Stores the topoheight of the block that used the nonce
//
// ENHANCEMENT (TIP-PE): Now supports nonce reservations for parallel execution
pub struct NonceChecker {
    cache: HashMap<PublicKey, AccountEntry>,
    // Reserved nonces: account → (nonce → thread_id)
    // Used during parallel execution to prevent conflicts
    reserved_nonces: HashMap<PublicKey, HashMap<Nonce, ThreadId>>,
}

impl NonceChecker {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            reserved_nonces: HashMap::new(),
        }
    }

    // Undo the nonce usage
    // We remove it from the used nonces list and revert the expected nonce to the previous nonce if present.
    pub fn undo_nonce(&mut self, key: &PublicKey, nonce: Nonce) {
        if let Some(entry) = self.cache.get_mut(key) {
            entry.used_nonces.shift_remove(&nonce);

            if let Some((prev_nonce, _)) = entry.used_nonces.last() {
                entry.expected_nonce = *prev_nonce + 1;
            } else {
                entry.expected_nonce = entry.initial_nonce;
            }
        }
    }

    // Key may be cloned on first entry
    // Returns false if nonce is already used
    pub async fn use_nonce<S: Storage>(&mut self, storage: &S, key: &PublicKey, nonce: Nonce, topoheight: TopoHeight) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("use_nonce {} for {} at topoheight {}", nonce, key.as_address(storage.is_mainnet()), topoheight);
        }

        match self.cache.get_mut(key) {
            Some(entry) => {
                if !entry.insert_nonce_at_topoheight(nonce, topoheight) {
                    return Ok(false);
                }
            },
            None => {
                // Nonce must follows in increasing order
                let (_, version) = storage.get_nonce_at_maximum_topoheight(key, topoheight).await?.ok_or_else(|| BlockchainError::AccountNotFound(key.as_address(storage.is_mainnet())))?;
                let stored_nonce = version.get_nonce();

                let mut entry = AccountEntry::new(stored_nonce);
                let valid = entry.insert_nonce_at_topoheight(nonce, topoheight);

                // Insert the entry into the cache before returning
                // So we don't have to search nonce again
                self.cache.insert(key.clone(), entry);

                if !valid {
                    return Ok(false);
                }
            }
        };

        Ok(true)
    }

    // Get the next nonce needed for the account
    pub fn get_new_nonce(&self, key: &PublicKey, mainnet: bool) -> Result<u64, BlockchainError> {
        let entry = self.cache.get(key).ok_or_else(|| BlockchainError::AccountNotFound(key.as_address(mainnet)))?;
        Ok(entry.expected_nonce)
    }

    // ========================================================================
    // Nonce Reservation Methods (TIP-PE)
    // ========================================================================

    /// Reserve a nonce for parallel execution on a specific thread
    ///
    /// This method ensures that only one thread can reserve a specific nonce,
    /// preventing double-spend attacks during parallel execution.
    ///
    /// # Arguments
    ///
    /// * `key` - Account public key
    /// * `nonce` - Nonce to reserve
    /// * `thread_id` - Thread that will execute this transaction
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Nonce successfully reserved
    /// * `Err(BlockchainError::NonceAlreadyUsed)` - Nonce already reserved by another thread
    ///
    /// # Note
    ///
    /// Reservations must be committed or cancelled after execution to prevent leaks.
    pub fn reserve_nonce(
        &mut self,
        key: &PublicKey,
        nonce: Nonce,
        thread_id: ThreadId,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("reserve_nonce {} for account {:?} on thread {}", nonce, key, thread_id);
        }

        // Get or create reservation map for this account
        let account_reservations = self.reserved_nonces
            .entry(key.clone())
            .or_insert_with(HashMap::new);

        // Check if nonce is already reserved
        if let Some(existing_thread) = account_reservations.get(&nonce) {
            if log::log_enabled!(log::Level::Warn) {
                warn!(
                    "Nonce {} for account {:?} already reserved by thread {}",
                    nonce, key, existing_thread
                );
            }
            // Use placeholder hash since we don't have the transaction hash yet
            use tos_common::crypto::Hash;
            return Err(BlockchainError::TxNonceAlreadyUsed(nonce, Hash::zero()));
        }

        // Reserve the nonce for this thread
        account_reservations.insert(nonce, thread_id);

        Ok(())
    }

    /// Commit a nonce reservation after successful transaction execution
    ///
    /// This method finalizes the nonce reservation by marking it as used in the cache.
    /// The reservation is removed from the reservation map.
    ///
    /// # Arguments
    ///
    /// * `key` - Account public key
    /// * `nonce` - Nonce to commit
    /// * `topoheight` - Topoheight at which the transaction was executed
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Nonce successfully committed
    /// * `Err(BlockchainError)` - Nonce validation failed or account not found
    ///
    /// # Note
    ///
    /// This should be called sequentially (in nonce order) to maintain determinism.
    pub async fn commit_reservation<S: Storage>(
        &mut self,
        storage: &S,
        key: &PublicKey,
        nonce: Nonce,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "commit_reservation {} for account {} at topoheight {}",
                nonce, key.as_address(storage.is_mainnet()), topoheight
            );
        }

        // Remove reservation
        if let Some(account_reservations) = self.reserved_nonces.get_mut(key) {
            account_reservations.remove(&nonce);

            // Clean up empty reservation maps
            if account_reservations.is_empty() {
                self.reserved_nonces.remove(key);
            }
        }

        // Use the existing use_nonce logic to mark as used
        let valid = self.use_nonce(storage, key, nonce, topoheight).await?;

        if !valid {
            use tos_common::crypto::Hash;
            return Err(BlockchainError::TxNonceAlreadyUsed(nonce, Hash::zero()));
        }

        Ok(())
    }

    /// Cancel a nonce reservation after failed transaction execution
    ///
    /// This method removes the reservation without marking the nonce as used,
    /// allowing it to be reserved again by another transaction.
    ///
    /// # Arguments
    ///
    /// * `key` - Account public key
    /// * `nonce` - Nonce to cancel
    ///
    /// # Note
    ///
    /// This should be called when transaction validation fails or execution errors occur.
    pub fn cancel_reservation(&mut self, key: &PublicKey, nonce: Nonce) {
        if log::log_enabled!(log::Level::Trace) {
            trace!("cancel_reservation {} for account {:?}", nonce, key);
        }

        // Remove reservation
        if let Some(account_reservations) = self.reserved_nonces.get_mut(key) {
            account_reservations.remove(&nonce);

            // Clean up empty reservation maps
            if account_reservations.is_empty() {
                self.reserved_nonces.remove(key);
            }
        }
    }

    /// Check if a nonce is currently reserved
    ///
    /// # Arguments
    ///
    /// * `key` - Account public key
    /// * `nonce` - Nonce to check
    ///
    /// # Returns
    ///
    /// * `Some(thread_id)` - Nonce is reserved by this thread
    /// * `None` - Nonce is not reserved
    pub fn is_nonce_reserved(&self, key: &PublicKey, nonce: &Nonce) -> Option<ThreadId> {
        self.reserved_nonces
            .get(key)
            .and_then(|reservations| reservations.get(nonce).copied())
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::crypto::KeyPair;

    #[test]
    fn test_reservation_simple() {
        let mut checker = NonceChecker::new();
        let keypair = KeyPair::new();
        let key = keypair.get_public_key().compress();

        // Reserve nonce 10 for thread 0
        let result = checker.reserve_nonce(&key, 10, 0);
        assert!(result.is_ok(), "First reservation should succeed");

        // Check that nonce is reserved
        assert_eq!(checker.is_nonce_reserved(&key, &10), Some(0));

        // Try to reserve same nonce for different thread
        let result = checker.reserve_nonce(&key, 10, 1);
        assert!(result.is_err(), "Duplicate reservation should fail");
        assert!(matches!(result, Err(BlockchainError::TxNonceAlreadyUsed(10, _))));
    }

    #[test]
    fn test_reservation_cancel() {
        let mut checker = NonceChecker::new();
        let keypair = KeyPair::new();
        let key = keypair.get_public_key().compress();

        // Reserve nonce 10 for thread 0
        checker.reserve_nonce(&key, 10, 0).expect("Reservation should succeed");
        assert_eq!(checker.is_nonce_reserved(&key, &10), Some(0));

        // Cancel reservation
        checker.cancel_reservation(&key, 10);
        assert_eq!(checker.is_nonce_reserved(&key, &10), None);

        // Should be able to reserve again after cancellation
        let result = checker.reserve_nonce(&key, 10, 1);
        assert!(result.is_ok(), "Reservation after cancel should succeed");
        assert_eq!(checker.is_nonce_reserved(&key, &10), Some(1));
    }

    #[test]
    fn test_reservation_multiple_nonces() {
        let mut checker = NonceChecker::new();
        let keypair = KeyPair::new();
        let key = keypair.get_public_key().compress();

        // Reserve multiple nonces for different threads
        checker.reserve_nonce(&key, 10, 0).expect("Reservation should succeed");
        checker.reserve_nonce(&key, 11, 1).expect("Reservation should succeed");
        checker.reserve_nonce(&key, 12, 2).expect("Reservation should succeed");

        // Verify all reservations
        assert_eq!(checker.is_nonce_reserved(&key, &10), Some(0));
        assert_eq!(checker.is_nonce_reserved(&key, &11), Some(1));
        assert_eq!(checker.is_nonce_reserved(&key, &12), Some(2));

        // Cancel one reservation
        checker.cancel_reservation(&key, 11);
        assert_eq!(checker.is_nonce_reserved(&key, &11), None);

        // Other reservations should still be valid
        assert_eq!(checker.is_nonce_reserved(&key, &10), Some(0));
        assert_eq!(checker.is_nonce_reserved(&key, &12), Some(2));
    }

    #[test]
    fn test_reservation_multiple_accounts() {
        let mut checker = NonceChecker::new();
        let keypair1 = KeyPair::new();
        let keypair2 = KeyPair::new();
        let key1 = keypair1.get_public_key().compress();
        let key2 = keypair2.get_public_key().compress();

        // Reserve same nonce for different accounts
        checker.reserve_nonce(&key1, 10, 0).expect("Reservation should succeed");
        checker.reserve_nonce(&key2, 10, 1).expect("Reservation should succeed");

        // Both should be reserved independently
        assert_eq!(checker.is_nonce_reserved(&key1, &10), Some(0));
        assert_eq!(checker.is_nonce_reserved(&key2, &10), Some(1));

        // Cancel for one account
        checker.cancel_reservation(&key1, 10);
        assert_eq!(checker.is_nonce_reserved(&key1, &10), None);
        assert_eq!(checker.is_nonce_reserved(&key2, &10), Some(1));
    }

    #[test]
    fn test_reservation_cleanup() {
        let mut checker = NonceChecker::new();
        let keypair = KeyPair::new();
        let key = keypair.get_public_key().compress();

        // Reserve and cancel to trigger cleanup
        checker.reserve_nonce(&key, 10, 0).expect("Reservation should succeed");
        checker.cancel_reservation(&key, 10);

        // The account should be removed from reserved_nonces after cleanup
        assert!(!checker.reserved_nonces.contains_key(&key));
    }

    #[test]
    fn test_reservation_not_found() {
        let checker = NonceChecker::new();
        let keypair = KeyPair::new();
        let key = keypair.get_public_key().compress();

        // Check non-existent reservation
        assert_eq!(checker.is_nonce_reserved(&key, &10), None);
    }
}