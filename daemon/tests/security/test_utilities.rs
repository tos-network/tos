//! Test utilities for security tests
//!
//! This module provides common utilities, helpers, and mock implementations
//! used across security tests.

use tos_common::crypto::Hash;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex};

/// Create a test hash from a single byte value
pub fn test_hash(value: u8) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[0] = value;
    Hash::new(bytes)
}

/// Create multiple test hashes
pub fn test_hashes(count: usize) -> Vec<Hash> {
    (0..count).map(|i| test_hash(i as u8)).collect()
}

/// Verify that two sets are disjoint (no overlap)
pub fn verify_disjoint<T: Eq + std::hash::Hash>(set1: &[T], set2: &[T]) -> bool {
    let s1: HashSet<_> = set1.iter().collect();
    let s2: HashSet<_> = set2.iter().collect();
    s1.is_disjoint(&s2)
}

/// Mock account state for testing
#[derive(Debug, Clone)]
pub struct MockAccount {
    pub address: String,
    pub balance: u64,
    pub nonce: u64,
}

impl MockAccount {
    pub fn new(address: String, balance: u64, nonce: u64) -> Self {
        Self { address, balance, nonce }
    }

    pub fn increment_nonce(&mut self) -> Result<(), String> {
        self.nonce = self.nonce.checked_add(1)
            .ok_or_else(|| "Nonce overflow".to_string())?;
        Ok(())
    }

    pub fn add_balance(&mut self, amount: u64) -> Result<(), String> {
        self.balance = self.balance.checked_add(amount)
            .ok_or_else(|| "Balance overflow".to_string())?;
        Ok(())
    }

    pub fn sub_balance(&mut self, amount: u64) -> Result<(), String> {
        self.balance = self.balance.checked_sub(amount)
            .ok_or_else(|| "Balance underflow".to_string())?;
        Ok(())
    }
}

/// Mock transaction for testing
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct MockTransaction {
    pub hash: Hash,
    pub sender: String,
    pub receiver: String,
    pub amount: u64,
    pub nonce: u64,
}

impl MockTransaction {
    pub fn new(sender: String, receiver: String, amount: u64, nonce: u64) -> Self {
        let mut hash_bytes = [0u8; 32];
        hash_bytes[0] = nonce as u8;
        hash_bytes[1] = (amount & 0xFF) as u8;

        Self {
            hash: Hash::new(hash_bytes),
            sender,
            receiver,
            amount,
            nonce,
        }
    }
}

/// Mock storage for testing
pub struct MockStorage {
    balances: Arc<RwLock<HashMap<String, u64>>>,
    nonces: Arc<RwLock<HashMap<String, u64>>>,
    blocks: Arc<RwLock<HashMap<Hash, MockBlock>>>,
}

impl MockStorage {
    pub fn new() -> Self {
        Self {
            balances: Arc::new(RwLock::new(HashMap::new())),
            nonces: Arc::new(RwLock::new(HashMap::new())),
            blocks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn set_balance(&self, address: &str, balance: u64) {
        self.balances.write().await.insert(address.to_string(), balance);
    }

    pub async fn get_balance(&self, address: &str) -> Option<u64> {
        self.balances.read().await.get(address).copied()
    }

    pub async fn set_nonce(&self, address: &str, nonce: u64) {
        self.nonces.write().await.insert(address.to_string(), nonce);
    }

    pub async fn get_nonce(&self, address: &str) -> Option<u64> {
        self.nonces.read().await.get(address).copied()
    }

    pub async fn add_block(&self, block: MockBlock) {
        self.blocks.write().await.insert(block.hash.clone(), block);
    }

    pub async fn get_block(&self, hash: &Hash) -> Option<MockBlock> {
        self.blocks.read().await.get(hash).cloned()
    }

    pub async fn has_block(&self, hash: &Hash) -> bool {
        self.blocks.read().await.contains_key(hash)
    }
}

/// Mock block for testing
#[derive(Debug, Clone)]
pub struct MockBlock {
    pub hash: Hash,
    pub height: u64,
    pub timestamp: u64,
    pub transactions: Vec<MockTransaction>,
    pub parents: Vec<Hash>,
}

impl MockBlock {
    pub fn new(height: u64, timestamp: u64, parents: Vec<Hash>) -> Self {
        let mut hash_bytes = [0u8; 32];
        hash_bytes[0] = height as u8;
        hash_bytes[1] = (timestamp & 0xFF) as u8;

        Self {
            hash: Hash::new(hash_bytes),
            height,
            timestamp,
            transactions: Vec::new(),
            parents,
        }
    }

    pub fn add_transaction(&mut self, tx: MockTransaction) {
        self.transactions.push(tx);
    }
}

/// Mock mempool for testing
pub struct MockMempool {
    transactions: Arc<Mutex<HashMap<Hash, MockTransaction>>>,
    nonce_cache: Arc<Mutex<HashMap<String, HashSet<u64>>>>,
}

impl MockMempool {
    pub fn new() -> Self {
        Self {
            transactions: Arc::new(Mutex::new(HashMap::new())),
            nonce_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add_transaction(&self, tx: MockTransaction) -> Result<(), String> {
        let mut txs = self.transactions.lock().await;
        let mut nonces = self.nonce_cache.lock().await;

        // Check if nonce is already used
        if let Some(used_nonces) = nonces.get(&tx.sender) {
            if used_nonces.contains(&tx.nonce) {
                return Err(format!("Nonce {} already used", tx.nonce));
            }
        }

        // Add transaction
        txs.insert(tx.hash.clone(), tx.clone());

        // Update nonce cache
        nonces.entry(tx.sender.clone())
            .or_insert_with(HashSet::new)
            .insert(tx.nonce);

        Ok(())
    }

    pub async fn remove_transaction(&self, hash: &Hash) -> Option<MockTransaction> {
        let mut txs = self.transactions.lock().await;
        let tx = txs.remove(hash)?;

        // Update nonce cache
        let mut nonces = self.nonce_cache.lock().await;
        if let Some(used_nonces) = nonces.get_mut(&tx.sender) {
            used_nonces.remove(&tx.nonce);
        }

        Some(tx)
    }

    pub async fn get_transaction(&self, hash: &Hash) -> Option<MockTransaction> {
        self.transactions.lock().await.get(hash).cloned()
    }

    pub async fn has_transaction(&self, hash: &Hash) -> bool {
        self.transactions.lock().await.contains_key(hash)
    }

    pub async fn get_transaction_count(&self) -> usize {
        self.transactions.lock().await.len()
    }
}

/// Bounded collection that enforces size limits (for V-26 testing)
pub struct BoundedCollection<T> {
    items: Vec<T>,
    max_size: usize,
}

impl<T> BoundedCollection<T> {
    pub fn new(max_size: usize) -> Self {
        Self {
            items: Vec::with_capacity(max_size),
            max_size,
        }
    }

    pub fn try_insert(&mut self, item: T) -> Result<(), &'static str> {
        if self.items.len() >= self.max_size {
            Err("Collection is full")
        } else {
            self.items.push(item);
            Ok(())
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_full(&self) -> bool {
        self.items.len() >= self.max_size
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Atomic nonce checker for testing (V-11, V-19)
pub struct AtomicNonceChecker {
    nonces: Arc<RwLock<HashMap<String, u64>>>,
}

impl AtomicNonceChecker {
    pub fn new() -> Self {
        Self {
            nonces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn init_account(&self, address: String, nonce: u64) {
        self.nonces.write().await.insert(address, nonce);
    }

    pub async fn compare_and_swap(&self, address: &str, expected: u64, new: u64) -> Result<(), String> {
        let mut nonces = self.nonces.write().await;
        let current = nonces.get(address)
            .ok_or_else(|| format!("Account {} not found", address))?;

        if *current != expected {
            return Err(format!("Nonce mismatch: expected {}, got {}", expected, current));
        }

        nonces.insert(address.to_string(), new);
        Ok(())
    }

    pub async fn get_nonce(&self, address: &str) -> Option<u64> {
        self.nonces.read().await.get(address).copied()
    }

    pub async fn rollback_nonce(&self, address: &str) -> Result<(), String> {
        let mut nonces = self.nonces.write().await;
        let current = nonces.get_mut(address)
            .ok_or_else(|| format!("Account {} not found", address))?;

        *current = current.checked_sub(1)
            .ok_or_else(|| "Cannot rollback nonce below 0".to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_utilities() {
        let hash1 = test_hash(1);
        let hash2 = test_hash(2);
        assert_ne!(hash1, hash2);

        let hashes = test_hashes(5);
        assert_eq!(hashes.len(), 5);
    }

    #[test]
    fn test_disjoint_sets() {
        let set1 = vec![1, 2, 3];
        let set2 = vec![4, 5, 6];
        assert!(verify_disjoint(&set1, &set2));

        let set3 = vec![3, 4, 5];
        assert!(!verify_disjoint(&set1, &set3));
    }

    #[test]
    fn test_mock_account() {
        let mut account = MockAccount::new("test".to_string(), 1000, 0);

        // Test balance operations
        assert!(account.add_balance(500).is_ok());
        assert_eq!(account.balance, 1500);

        assert!(account.sub_balance(300).is_ok());
        assert_eq!(account.balance, 1200);

        // Test overflow
        account.balance = u64::MAX;
        assert!(account.add_balance(1).is_err());

        // Test underflow
        account.balance = 100;
        assert!(account.sub_balance(200).is_err());
    }

    #[test]
    fn test_bounded_collection() {
        let mut collection = BoundedCollection::new(3);

        assert!(collection.try_insert(1).is_ok());
        assert!(collection.try_insert(2).is_ok());
        assert!(collection.try_insert(3).is_ok());
        assert!(collection.try_insert(4).is_err()); // Should fail - full

        assert_eq!(collection.len(), 3);
        assert!(collection.is_full());
    }

    #[tokio::test]
    async fn test_mock_storage() {
        let storage = MockStorage::new();

        storage.set_balance("alice", 1000).await;
        assert_eq!(storage.get_balance("alice").await, Some(1000));

        storage.set_nonce("alice", 5).await;
        assert_eq!(storage.get_nonce("alice").await, Some(5));

        let block = MockBlock::new(1, 1000, vec![test_hash(0)]);
        storage.add_block(block.clone()).await;
        assert!(storage.has_block(&block.hash).await);
    }

    #[tokio::test]
    async fn test_mock_mempool() {
        let mempool = MockMempool::new();

        let tx1 = MockTransaction::new(
            "alice".to_string(),
            "bob".to_string(),
            100,
            1
        );

        // Add first transaction
        assert!(mempool.add_transaction(tx1.clone()).await.is_ok());
        assert_eq!(mempool.get_transaction_count().await, 1);

        // Try to add duplicate nonce
        let tx2 = MockTransaction::new(
            "alice".to_string(),
            "charlie".to_string(),
            200,
            1 // Same nonce!
        );
        assert!(mempool.add_transaction(tx2).await.is_err());

        // Remove transaction
        let removed = mempool.remove_transaction(&tx1.hash).await;
        assert!(removed.is_some());
        assert_eq!(mempool.get_transaction_count().await, 0);
    }

    #[tokio::test]
    async fn test_atomic_nonce_checker() {
        let checker = AtomicNonceChecker::new();
        checker.init_account("alice".to_string(), 10).await;

        // Compare and swap with correct expected value
        assert!(checker.compare_and_swap("alice", 10, 11).await.is_ok());
        assert_eq!(checker.get_nonce("alice").await, Some(11));

        // Compare and swap with wrong expected value
        assert!(checker.compare_and_swap("alice", 10, 12).await.is_err());
        assert_eq!(checker.get_nonce("alice").await, Some(11)); // Unchanged

        // Rollback nonce
        assert!(checker.rollback_nonce("alice").await.is_ok());
        assert_eq!(checker.get_nonce("alice").await, Some(10));
    }
}
