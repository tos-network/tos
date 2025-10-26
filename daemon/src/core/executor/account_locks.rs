// Thread-Aware Account Locks for Parallel Transaction Execution
// Based on Solana agave thread_aware_account_locks.rs
//
// This module provides lock-free account locking for parallel transaction execution.
// Key features:
// - Supports up to 64 threads via u64 bitset
// - Read/write lock semantics per account
// - Nested lock support (lock count tracking)
// - Zero-copy lock/unlock operations
//
// Reference: ~/tos-network/agave/scheduling-utils/src/thread_aware_account_locks.rs

use std::collections::HashMap;
use tos_common::crypto::elgamal::CompressedPublicKey;
use tos_common::crypto::Hash;

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of threads supported (u64 bitset limit)
pub const MAX_THREADS: usize = 64;

// ============================================================================
// Type Aliases
// ============================================================================

/// Thread identifier (0-63)
pub type ThreadId = usize;

/// Lock count for nested locking support
pub type LockCount = u64;

// ============================================================================
// ThreadSet - Efficient thread tracking via bitset
// ============================================================================

/// Bit-set for tracking which threads hold locks on an account
///
/// Uses a single u64 to represent up to 64 threads:
/// - Bit N set = Thread N holds a lock
/// - Bit N clear = Thread N does not hold a lock
///
/// This provides O(1) operations for:
/// - Checking if a thread has a lock
/// - Adding/removing threads
/// - Counting active threads
/// - Finding available threads
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ThreadSet(u64);

impl ThreadSet {
    /// Create an empty ThreadSet
    #[inline]
    pub fn new() -> Self {
        ThreadSet(0)
    }

    /// Check if a thread is in the set
    #[inline]
    pub fn contains(&self, thread_id: ThreadId) -> bool {
        debug_assert!(thread_id < MAX_THREADS, "Thread ID {} exceeds maximum {}", thread_id, MAX_THREADS);
        (self.0 & (1u64 << thread_id)) != 0
    }

    /// Add a thread to the set
    #[inline]
    pub fn insert(&mut self, thread_id: ThreadId) {
        debug_assert!(thread_id < MAX_THREADS, "Thread ID {} exceeds maximum {}", thread_id, MAX_THREADS);
        self.0 |= 1u64 << thread_id;
    }

    /// Remove a thread from the set
    #[inline]
    pub fn remove(&mut self, thread_id: ThreadId) {
        debug_assert!(thread_id < MAX_THREADS, "Thread ID {} exceeds maximum {}", thread_id, MAX_THREADS);
        self.0 &= !(1u64 << thread_id);
    }

    /// Check if the set is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Count how many threads are in the set
    #[inline]
    pub fn count(&self) -> usize {
        self.0.count_ones() as usize
    }

    /// Check if this set intersects with another
    #[inline]
    pub fn intersects(&self, other: &ThreadSet) -> bool {
        (self.0 & other.0) != 0
    }

    /// Union of two sets
    #[inline]
    pub fn union(&self, other: &ThreadSet) -> ThreadSet {
        ThreadSet(self.0 | other.0)
    }

    /// Intersection of two sets
    #[inline]
    pub fn intersection(&self, other: &ThreadSet) -> ThreadSet {
        ThreadSet(self.0 & other.0)
    }

    /// Iterate over all thread IDs in the set
    pub fn iter(&self) -> impl Iterator<Item = ThreadId> + '_ {
        (0..MAX_THREADS).filter(move |&id| self.contains(id))
    }
}

// ============================================================================
// Lock State Tracking
// ============================================================================

/// Write lock state for an account
///
/// Only one thread can hold a write lock at a time, but nested locks
/// from the same thread are allowed (tracked via lock_count)
#[derive(Debug, Clone)]
struct AccountWriteLock {
    /// Thread ID that holds the write lock
    thread_id: ThreadId,

    /// Number of nested write locks (supports reentrant locking)
    lock_count: LockCount,
}

/// Read lock state for an account
///
/// Multiple threads can hold read locks simultaneously
/// Each thread can have multiple nested read locks
#[derive(Debug, Clone)]
struct AccountReadLocks {
    /// Set of threads that hold read locks
    thread_set: ThreadSet,

    /// Per-thread lock counts (for nested locking)
    lock_counts: [LockCount; MAX_THREADS],
}

impl Default for AccountReadLocks {
    fn default() -> Self {
        Self {
            thread_set: ThreadSet::new(),
            lock_counts: [0; MAX_THREADS],
        }
    }
}

/// Combined lock state for an account
#[derive(Debug, Clone)]
enum AccountLocks {
    /// Account has write lock(s)
    Write(AccountWriteLock),

    /// Account has read lock(s)
    Read(AccountReadLocks),

    /// Account is unlocked
    Unlocked,
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur when trying to acquire locks
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TryLockError {
    /// Account already has a write lock held by another thread
    WriteConflict {
        account_pubkey: CompressedPublicKey,
        account_asset: Hash,
        holding_thread: ThreadId,
        requesting_thread: ThreadId,
    },

    /// Account has read locks, cannot acquire write lock
    ReadWriteConflict {
        account_pubkey: CompressedPublicKey,
        account_asset: Hash,
        read_threads: Vec<ThreadId>,
        requesting_thread: ThreadId,
    },

    /// Thread ID exceeds maximum supported threads
    InvalidThreadId {
        thread_id: ThreadId,
        max_threads: usize,
    },
}

impl std::fmt::Display for TryLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TryLockError::WriteConflict { account_pubkey, account_asset, holding_thread, requesting_thread } => {
                write!(
                    f,
                    "Write conflict on account {:?}:{:?} - thread {} holds write lock, thread {} cannot acquire",
                    account_pubkey, account_asset, holding_thread, requesting_thread
                )
            },
            TryLockError::ReadWriteConflict { account_pubkey, account_asset, read_threads, requesting_thread } => {
                write!(
                    f,
                    "Read-write conflict on account {:?}:{:?} - threads {:?} hold read locks, thread {} cannot acquire write lock",
                    account_pubkey, account_asset, read_threads, requesting_thread
                )
            },
            TryLockError::InvalidThreadId { thread_id, max_threads } => {
                write!(f, "Invalid thread ID {} (max: {})", thread_id, max_threads)
            },
        }
    }
}

impl std::error::Error for TryLockError {}

// ============================================================================
// ThreadAwareAccountLocks - Main Lock Manager
// ============================================================================

/// Thread-aware account lock manager for parallel transaction execution
///
/// Manages read/write locks on accounts across multiple worker threads.
/// Ensures conflict-free parallel execution by tracking which threads
/// hold locks on which accounts.
///
/// # Lock Semantics
///
/// - **Write locks**: Exclusive - only one thread can hold a write lock
/// - **Read locks**: Shared - multiple threads can hold read locks
/// - **Conflict**: Write lock conflicts with any other lock
/// - **Nested locks**: Same thread can acquire multiple locks (tracked via count)
///
/// # Performance
///
/// - Lock/unlock: O(1) average case (hash map lookup + bitset ops)
/// - Schedulable threads: O(accounts × threads) but optimized with bitsets
/// - Zero allocations for lock/unlock operations
pub struct ThreadAwareAccountLocks {
    /// Number of worker threads
    num_threads: usize,

    /// Lock state per account (pubkey + asset)
    /// Uses HashMap for fast lookup (ahash would be faster but adds dependency)
    locks: HashMap<(CompressedPublicKey, Hash), AccountLocks>,
}

impl ThreadAwareAccountLocks {
    /// Create a new lock manager
    ///
    /// # Arguments
    ///
    /// * `num_threads` - Number of worker threads (must be ≤ MAX_THREADS)
    ///
    /// # Panics
    ///
    /// Panics if `num_threads` exceeds MAX_THREADS (64)
    pub fn new(num_threads: usize) -> Self {
        assert!(
            num_threads <= MAX_THREADS,
            "Number of threads {} exceeds maximum supported {}",
            num_threads, MAX_THREADS
        );

        Self {
            num_threads,
            locks: HashMap::new(),
        }
    }

    /// Try to acquire locks for a transaction's accounts
    ///
    /// # Arguments
    ///
    /// * `thread_id` - Thread attempting to acquire locks
    /// * `writable` - Accounts that need write locks
    /// * `readonly` - Accounts that need read locks
    ///
    /// # Returns
    ///
    /// * `Ok(())` - All locks acquired successfully
    /// * `Err(TryLockError)` - Lock conflict detected
    ///
    /// # Atomicity
    ///
    /// This operation is all-or-nothing: if any lock fails, NO locks are acquired.
    /// The caller must retry or skip the transaction.
    pub fn try_lock_accounts(
        &mut self,
        thread_id: ThreadId,
        writable: &[(CompressedPublicKey, Hash)],
        readonly: &[(CompressedPublicKey, Hash)],
    ) -> Result<(), TryLockError> {
        // Validate thread ID
        if thread_id >= self.num_threads {
            return Err(TryLockError::InvalidThreadId {
                thread_id,
                max_threads: self.num_threads,
            });
        }

        // Phase 1: Check if all locks can be acquired (no modifications)
        for (pubkey, asset) in writable {
            self.check_write_lock_available(thread_id, pubkey, asset)?;
        }

        for (pubkey, asset) in readonly {
            self.check_read_lock_available(thread_id, pubkey, asset)?;
        }

        // Phase 2: Acquire all locks (now safe)
        for (pubkey, asset) in writable {
            self.acquire_write_lock(thread_id, pubkey, asset);
        }

        for (pubkey, asset) in readonly {
            self.acquire_read_lock(thread_id, pubkey, asset);
        }

        Ok(())
    }

    /// Release locks for a transaction's accounts
    ///
    /// # Arguments
    ///
    /// * `thread_id` - Thread releasing locks
    /// * `writable` - Accounts to release write locks
    /// * `readonly` - Accounts to release read locks
    ///
    /// # Panics
    ///
    /// Panics if trying to unlock an account that isn't locked by this thread
    pub fn unlock_accounts(
        &mut self,
        thread_id: ThreadId,
        writable: &[(CompressedPublicKey, Hash)],
        readonly: &[(CompressedPublicKey, Hash)],
    ) {
        for (pubkey, asset) in writable {
            self.release_write_lock(thread_id, pubkey, asset);
        }

        for (pubkey, asset) in readonly {
            self.release_read_lock(thread_id, pubkey, asset);
        }
    }

    /// Find which threads can execute a transaction without conflicts
    ///
    /// # Arguments
    ///
    /// * `writable` - Accounts that need write locks
    /// * `readonly` - Accounts that need read locks
    ///
    /// # Returns
    ///
    /// ThreadSet of threads that can currently acquire all required locks
    pub fn schedulable_threads(
        &self,
        writable: &[(CompressedPublicKey, Hash)],
        readonly: &[(CompressedPublicKey, Hash)],
    ) -> ThreadSet {
        let mut available = ThreadSet::new();

        // Start with all threads available
        for thread_id in 0..self.num_threads {
            available.insert(thread_id);
        }

        // Remove threads that conflict with writable accounts
        for (pubkey, asset) in writable {
            let conflicts = self.write_lock_conflicts(pubkey, asset);
            for thread_id in conflicts.iter() {
                available.remove(thread_id);
            }
        }

        // Remove threads that conflict with readonly accounts
        for (pubkey, asset) in readonly {
            let conflicts = self.read_lock_conflicts(pubkey, asset);
            for thread_id in conflicts.iter() {
                available.remove(thread_id);
            }
        }

        available
    }

    // ========================================================================
    // Internal Helper Methods
    // ========================================================================

    /// Check if write lock can be acquired without modifying state
    fn check_write_lock_available(
        &self,
        thread_id: ThreadId,
        pubkey: &CompressedPublicKey,
        asset: &Hash,
    ) -> Result<(), TryLockError> {
        let key = (pubkey.clone(), asset.clone());

        match self.locks.get(&key) {
            None | Some(AccountLocks::Unlocked) => Ok(()), // No lock, can acquire

            Some(AccountLocks::Write(write_lock)) => {
                // Same thread can acquire nested write lock
                if write_lock.thread_id == thread_id {
                    Ok(())
                } else {
                    Err(TryLockError::WriteConflict {
                        account_pubkey: pubkey.clone(),
                        account_asset: asset.clone(),
                        holding_thread: write_lock.thread_id,
                        requesting_thread: thread_id,
                    })
                }
            },

            Some(AccountLocks::Read(read_locks)) => {
                // Write lock conflicts with any read locks
                let read_threads: Vec<ThreadId> = read_locks.thread_set.iter().collect();
                Err(TryLockError::ReadWriteConflict {
                    account_pubkey: pubkey.clone(),
                    account_asset: asset.clone(),
                    read_threads,
                    requesting_thread: thread_id,
                })
            },
        }
    }

    /// Check if read lock can be acquired without modifying state
    fn check_read_lock_available(
        &self,
        thread_id: ThreadId,
        pubkey: &CompressedPublicKey,
        asset: &Hash,
    ) -> Result<(), TryLockError> {
        let key = (pubkey.clone(), asset.clone());

        match self.locks.get(&key) {
            None | Some(AccountLocks::Unlocked) => Ok(()), // No lock, can acquire

            Some(AccountLocks::Read(_)) => Ok(()), // Read locks are shared

            Some(AccountLocks::Write(write_lock)) => {
                // Read lock conflicts with write lock from different thread
                if write_lock.thread_id == thread_id {
                    Ok(()) // Same thread can downgrade write to read
                } else {
                    Err(TryLockError::WriteConflict {
                        account_pubkey: pubkey.clone(),
                        account_asset: asset.clone(),
                        holding_thread: write_lock.thread_id,
                        requesting_thread: thread_id,
                    })
                }
            },
        }
    }

    /// Acquire write lock (assumes check_write_lock_available passed)
    fn acquire_write_lock(
        &mut self,
        thread_id: ThreadId,
        pubkey: &CompressedPublicKey,
        asset: &Hash,
    ) {
        let key = (pubkey.clone(), asset.clone());

        let lock_state = self.locks.entry(key).or_insert(AccountLocks::Unlocked);

        match lock_state {
            AccountLocks::Unlocked => {
                *lock_state = AccountLocks::Write(AccountWriteLock {
                    thread_id,
                    lock_count: 1,
                });
            },
            AccountLocks::Write(ref mut write_lock) => {
                debug_assert_eq!(write_lock.thread_id, thread_id);
                write_lock.lock_count += 1;
            },
            AccountLocks::Read(_) => {
                panic!("Cannot acquire write lock while read locks exist");
            },
        }
    }

    /// Acquire read lock (assumes check_read_lock_available passed)
    fn acquire_read_lock(
        &mut self,
        thread_id: ThreadId,
        pubkey: &CompressedPublicKey,
        asset: &Hash,
    ) {
        let key = (pubkey.clone(), asset.clone());

        let lock_state = self.locks.entry(key).or_insert(AccountLocks::Unlocked);

        match lock_state {
            AccountLocks::Unlocked => {
                let mut read_locks = AccountReadLocks::default();
                read_locks.thread_set.insert(thread_id);
                read_locks.lock_counts[thread_id] = 1;
                *lock_state = AccountLocks::Read(read_locks);
            },
            AccountLocks::Read(ref mut read_locks) => {
                if !read_locks.thread_set.contains(thread_id) {
                    read_locks.thread_set.insert(thread_id);
                }
                read_locks.lock_counts[thread_id] += 1;
            },
            AccountLocks::Write(ref write_lock) => {
                debug_assert_eq!(write_lock.thread_id, thread_id);
                // Downgrade write to read is allowed for same thread
            },
        }
    }

    /// Release write lock
    fn release_write_lock(
        &mut self,
        thread_id: ThreadId,
        pubkey: &CompressedPublicKey,
        asset: &Hash,
    ) {
        let key = (pubkey.clone(), asset.clone());

        let should_remove = if let Some(lock_state) = self.locks.get_mut(&key) {
            match lock_state {
                AccountLocks::Write(ref mut write_lock) => {
                    assert_eq!(write_lock.thread_id, thread_id, "Unlocking write lock held by different thread");
                    write_lock.lock_count -= 1;
                    if write_lock.lock_count == 0 {
                        true
                    } else {
                        false
                    }
                },
                _ => panic!("Expected write lock on account"),
            }
        } else {
            panic!("Unlocking account that isn't locked");
        };

        if should_remove {
            self.locks.remove(&key);
        }
    }

    /// Release read lock
    fn release_read_lock(
        &mut self,
        thread_id: ThreadId,
        pubkey: &CompressedPublicKey,
        asset: &Hash,
    ) {
        let key = (pubkey.clone(), asset.clone());

        let should_remove = if let Some(lock_state) = self.locks.get_mut(&key) {
            match lock_state {
                AccountLocks::Read(ref mut read_locks) => {
                    assert!(read_locks.thread_set.contains(thread_id), "Unlocking read lock not held by thread");
                    read_locks.lock_counts[thread_id] -= 1;
                    if read_locks.lock_counts[thread_id] == 0 {
                        read_locks.thread_set.remove(thread_id);
                    }
                    read_locks.thread_set.is_empty()
                },
                _ => panic!("Expected read lock on account"),
            }
        } else {
            panic!("Unlocking account that isn't locked");
        };

        if should_remove {
            self.locks.remove(&key);
        }
    }

    /// Get threads that conflict with acquiring a write lock
    fn write_lock_conflicts(
        &self,
        pubkey: &CompressedPublicKey,
        asset: &Hash,
    ) -> ThreadSet {
        let key = (pubkey.clone(), asset.clone());

        match self.locks.get(&key) {
            None | Some(AccountLocks::Unlocked) => ThreadSet::new(),

            Some(AccountLocks::Write(write_lock)) => {
                let mut conflicts = ThreadSet::new();
                // All threads except the holding thread are blocked
                for thread_id in 0..self.num_threads {
                    if thread_id != write_lock.thread_id {
                        conflicts.insert(thread_id);
                    }
                }
                conflicts
            },

            Some(AccountLocks::Read(read_locks)) => {
                // All threads that don't hold read locks are blocked
                let mut conflicts = ThreadSet::new();
                for thread_id in 0..self.num_threads {
                    if !read_locks.thread_set.contains(thread_id) {
                        conflicts.insert(thread_id);
                    }
                }
                conflicts
            },
        }
    }

    /// Get threads that conflict with acquiring a read lock
    fn read_lock_conflicts(
        &self,
        pubkey: &CompressedPublicKey,
        asset: &Hash,
    ) -> ThreadSet {
        let key = (pubkey.clone(), asset.clone());

        match self.locks.get(&key) {
            None | Some(AccountLocks::Unlocked) => ThreadSet::new(),

            Some(AccountLocks::Read(_)) => ThreadSet::new(), // Read locks don't conflict

            Some(AccountLocks::Write(write_lock)) => {
                let mut conflicts = ThreadSet::new();
                // All threads except the holding thread are blocked
                for thread_id in 0..self.num_threads {
                    if thread_id != write_lock.thread_id {
                        conflicts.insert(thread_id);
                    }
                }
                conflicts
            },
        }
    }

    /// Get number of currently locked accounts
    pub fn locked_account_count(&self) -> usize {
        self.locks.len()
    }

    /// Clear all locks (for testing/reset)
    pub fn clear(&mut self) {
        self.locks.clear();
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create test account keys
    fn test_account(id: u8) -> (CompressedPublicKey, Hash) {
        let mut pubkey_bytes = [0u8; 32];
        pubkey_bytes[0] = id;
        let pubkey = CompressedPublicKey::from_bytes(&pubkey_bytes).unwrap();

        let mut asset_bytes = [0u8; 32];
        asset_bytes[0] = id;
        let asset = Hash::new(asset_bytes);

        (pubkey, asset)
    }

    #[test]
    fn test_threadset_basic() {
        let mut set = ThreadSet::new();
        assert!(set.is_empty());
        assert_eq!(set.count(), 0);

        set.insert(0);
        assert!(!set.is_empty());
        assert_eq!(set.count(), 1);
        assert!(set.contains(0));

        set.insert(5);
        assert_eq!(set.count(), 2);
        assert!(set.contains(5));

        set.remove(0);
        assert_eq!(set.count(), 1);
        assert!(!set.contains(0));
        assert!(set.contains(5));
    }

    #[test]
    fn test_threadset_operations() {
        let mut set1 = ThreadSet::new();
        set1.insert(0);
        set1.insert(1);

        let mut set2 = ThreadSet::new();
        set2.insert(1);
        set2.insert(2);

        assert!(set1.intersects(&set2));

        let union = set1.union(&set2);
        assert_eq!(union.count(), 3);
        assert!(union.contains(0));
        assert!(union.contains(1));
        assert!(union.contains(2));

        let intersection = set1.intersection(&set2);
        assert_eq!(intersection.count(), 1);
        assert!(intersection.contains(1));
    }

    #[test]
    fn test_simple_write_lock() {
        let mut locks = ThreadAwareAccountLocks::new(4);
        let account = test_account(1);

        // Thread 0 acquires write lock
        assert!(locks.try_lock_accounts(0, &[account.clone()], &[]).is_ok());

        // Thread 1 cannot acquire write lock
        assert!(locks.try_lock_accounts(1, &[account.clone()], &[]).is_err());

        // Release lock
        locks.unlock_accounts(0, &[account.clone()], &[]);

        // Now thread 1 can acquire
        assert!(locks.try_lock_accounts(1, &[account.clone()], &[]).is_ok());
    }

    #[test]
    fn test_simple_read_lock() {
        let mut locks = ThreadAwareAccountLocks::new(4);
        let account = test_account(1);

        // Thread 0 acquires read lock
        assert!(locks.try_lock_accounts(0, &[], &[account.clone()]).is_ok());

        // Thread 1 can also acquire read lock
        assert!(locks.try_lock_accounts(1, &[], &[account.clone()]).is_ok());

        // Thread 2 cannot acquire write lock
        assert!(locks.try_lock_accounts(2, &[account.clone()], &[]).is_err());

        // Release read locks
        locks.unlock_accounts(0, &[], &[account.clone()]);
        locks.unlock_accounts(1, &[], &[account.clone()]);

        // Now thread 2 can acquire write lock
        assert!(locks.try_lock_accounts(2, &[account.clone()], &[]).is_ok());
    }

    #[test]
    fn test_nested_write_locks() {
        let mut locks = ThreadAwareAccountLocks::new(4);
        let account = test_account(1);

        // Thread 0 acquires write lock twice
        assert!(locks.try_lock_accounts(0, &[account.clone()], &[]).is_ok());
        assert!(locks.try_lock_accounts(0, &[account.clone()], &[]).is_ok());

        // Release once - still locked
        locks.unlock_accounts(0, &[account.clone()], &[]);
        assert!(locks.try_lock_accounts(1, &[account.clone()], &[]).is_err());

        // Release again - now unlocked
        locks.unlock_accounts(0, &[account.clone()], &[]);
        assert!(locks.try_lock_accounts(1, &[account.clone()], &[]).is_ok());
    }

    #[test]
    fn test_schedulable_threads() {
        let mut locks = ThreadAwareAccountLocks::new(4);
        let account1 = test_account(1);
        let account2 = test_account(2);

        // Initially all threads can schedule
        let schedulable = locks.schedulable_threads(&[account1.clone()], &[]);
        assert_eq!(schedulable.count(), 4);

        // Thread 0 locks account1
        locks.try_lock_accounts(0, &[account1.clone()], &[]).unwrap();

        // Now only thread 0 can schedule account1
        let schedulable = locks.schedulable_threads(&[account1.clone()], &[]);
        assert_eq!(schedulable.count(), 1);
        assert!(schedulable.contains(0));

        // All threads can still schedule account2
        let schedulable = locks.schedulable_threads(&[account2.clone()], &[]);
        assert_eq!(schedulable.count(), 4);
    }

    #[test]
    fn test_multi_account_lock() {
        let mut locks = ThreadAwareAccountLocks::new(4);
        let account1 = test_account(1);
        let account2 = test_account(2);

        // Thread 0 locks both accounts
        assert!(locks.try_lock_accounts(0, &[account1.clone(), account2.clone()], &[]).is_ok());

        // Thread 1 cannot lock either account
        assert!(locks.try_lock_accounts(1, &[account1.clone()], &[]).is_err());
        assert!(locks.try_lock_accounts(1, &[account2.clone()], &[]).is_err());

        // Release both
        locks.unlock_accounts(0, &[account1.clone(), account2.clone()], &[]);

        // Now thread 1 can lock both
        assert!(locks.try_lock_accounts(1, &[account1.clone(), account2.clone()], &[]).is_ok());
    }

    #[test]
    fn test_mixed_read_write_locks() {
        let mut locks = ThreadAwareAccountLocks::new(4);
        let account1 = test_account(1);
        let account2 = test_account(2);

        // Thread 0: write account1, read account2
        assert!(locks.try_lock_accounts(0, &[account1.clone()], &[account2.clone()]).is_ok());

        // Thread 1: read account1 (blocked), write account2 (blocked)
        assert!(locks.try_lock_accounts(1, &[], &[account1.clone()]).is_err());
        assert!(locks.try_lock_accounts(1, &[account2.clone()], &[]).is_err());

        // Thread 1: read account2 (ok - shared read)
        assert!(locks.try_lock_accounts(1, &[], &[account2.clone()]).is_ok());
    }
}
