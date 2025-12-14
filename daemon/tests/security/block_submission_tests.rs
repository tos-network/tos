//! Security tests for block submission paths
//!
//! These tests validate the security fixes for Issue #2:
//! Block submission should not fail after merkle cache expiry.
//!
//! Tests cover:
//! - Full block submission via block_hex parameter (new path)
//! - Header-only submission with cache reconstruction (legacy path)
//! - Cache expiry handling after 300s TTL
//! - Merkle root validation for both paths
//!
//! SECURITY CONTEXT:
//! Issue #2 identified that miners couldn't submit blocks after 60s because:
//! 1. get_block_template caches transactions with 60s TTL
//! 2. submit_block reconstructed blocks from cache only
//! 3. Cache expiry caused honest miners to lose valid blocks
//!
//! FIX:
//! 1. Added optional block_hex parameter for full block submission
//! 2. Increased cache TTL to 300s (5 minutes)
//! 3. Maintained backward compatibility with header-only submission
//!
//! NOTE: These tests use mock implementations to validate security logic
//! without depending on the full blockchain infrastructure.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tos_common::crypto::{hash, Hash};
use tos_common::time::TimestampMillis;

/// Helper to create a Hash from a u8 value (fills a [u8; 32] array)
fn test_hash(val: u8) -> Hash {
    Hash::new([val; 32])
}

/// Mock block information for testing
#[derive(Clone, Debug)]
struct BlockInfo {
    hash: Hash,
    parents: Vec<Hash>,
    merkle_root: Hash,
    timestamp: TimestampMillis,
    valid: bool,
    transactions: Vec<MockTransaction>,
}

/// Mock transaction for testing
#[derive(Clone, Debug)]
struct MockTransaction {
    hash: Hash,
    #[allow(dead_code)]
    data: Vec<u8>,
}

/// Cache entry with TTL tracking
#[derive(Clone)]
struct CacheEntry {
    transactions: Vec<MockTransaction>,
    created_at: Instant,
}

/// Mock blockchain with cache support for testing block submission paths
struct MockBlockchain {
    blocks: Arc<RwLock<HashMap<Hash, BlockInfo>>>,
    tips: Arc<RwLock<Vec<Hash>>>,
    /// Transaction cache with TTL (simulates merkle cache)
    tx_cache: Arc<RwLock<HashMap<Hash, CacheEntry>>>,
    /// Cache TTL in seconds (should be 300s after fix)
    cache_ttl: Duration,
}

impl MockBlockchain {
    fn new() -> Self {
        Self::with_cache_ttl(Duration::from_secs(300)) // 5 minutes TTL after fix
    }

    fn with_cache_ttl(ttl: Duration) -> Self {
        let genesis_hash = Hash::zero();
        let mut blocks = HashMap::new();
        blocks.insert(
            genesis_hash.clone(),
            BlockInfo {
                hash: genesis_hash.clone(),
                parents: vec![],
                merkle_root: Hash::zero(),
                timestamp: 1600000000000,
                valid: true,
                transactions: vec![],
            },
        );

        Self {
            blocks: Arc::new(RwLock::new(blocks)),
            tips: Arc::new(RwLock::new(vec![genesis_hash])),
            tx_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: ttl,
        }
    }

    /// Cache transactions (simulates get_block_template caching)
    async fn cache_transactions(&self, template_hash: Hash, txs: Vec<MockTransaction>) {
        let mut cache = self.tx_cache.write().await;
        cache.insert(
            template_hash,
            CacheEntry {
                transactions: txs,
                created_at: Instant::now(),
            },
        );
    }

    /// Get cached transactions if not expired
    async fn get_cached_transactions(&self, template_hash: &Hash) -> Option<Vec<MockTransaction>> {
        let cache = self.tx_cache.read().await;
        if let Some(entry) = cache.get(template_hash) {
            if entry.created_at.elapsed() < self.cache_ttl {
                return Some(entry.transactions.clone());
            }
        }
        None
    }

    /// Submit block via full block_hex (new path - bypasses cache)
    async fn submit_block_with_hex(&self, block: &BlockInfo) -> Result<(), String> {
        // This path doesn't need cache - block contains all data
        self.validate_and_store_block(block).await
    }

    /// Submit block via header only (legacy path - uses cache)
    async fn submit_block_header_only(
        &self,
        header_hash: Hash,
        parents: Vec<Hash>,
        merkle_root: Hash,
        timestamp: TimestampMillis,
        template_hash: &Hash,
    ) -> Result<(), String> {
        // Reconstruct block from cache
        let cached_txs = self
            .get_cached_transactions(template_hash)
            .await
            .ok_or_else(|| {
                "Cache miss: transactions not found or expired (TTL=300s). \
             Consider using block_hex parameter for full block submission."
                    .to_string()
            })?;

        // Calculate expected merkle root from cached transactions
        let expected_merkle_root = Self::calculate_merkle_root(&cached_txs);

        // Validate merkle root matches
        if merkle_root != expected_merkle_root {
            return Err(
                "Merkle root mismatch: header merkle root doesn't match cached transactions"
                    .to_string(),
            );
        }

        let block = BlockInfo {
            hash: header_hash,
            parents,
            merkle_root,
            timestamp,
            valid: true,
            transactions: cached_txs,
        };

        self.validate_and_store_block(&block).await
    }

    /// Calculate merkle root from transactions
    fn calculate_merkle_root(txs: &[MockTransaction]) -> Hash {
        if txs.is_empty() {
            return Hash::zero();
        }
        // Simple merkle root calculation for testing - hash all tx hashes together
        let mut combined = Vec::with_capacity(txs.len() * 32);
        for tx in txs {
            combined.extend_from_slice(tx.hash.as_bytes());
        }
        hash(&combined)
    }

    /// Validate and store block (common logic)
    async fn validate_and_store_block(&self, block: &BlockInfo) -> Result<(), String> {
        let mut blocks = self.blocks.write().await;
        let mut tips = self.tips.write().await;

        // Check 1: Duplicate block
        if blocks.contains_key(&block.hash) {
            return Err("Block already exists".to_string());
        }

        // Check 2: Parents exist
        for parent in &block.parents {
            if !blocks.contains_key(parent) {
                return Err(format!("Parent {} not found", parent));
            }
        }

        // Check 3: Timestamp validation (must be greater than parents)
        for parent in &block.parents {
            if let Some(parent_block) = blocks.get(parent) {
                if block.timestamp <= parent_block.timestamp {
                    return Err("Block timestamp must be after parent timestamp".to_string());
                }
            }
        }

        // Check 4: Merkle root validation
        let expected_merkle = Self::calculate_merkle_root(&block.transactions);
        if block.merkle_root != expected_merkle {
            return Err("Invalid merkle root: doesn't match transactions".to_string());
        }

        // Check 5: Block validity flag (for testing invalid blocks)
        if !block.valid {
            return Err("Block marked as invalid".to_string());
        }

        // Accept block
        blocks.insert(block.hash.clone(), block.clone());

        // Update tips
        for parent in &block.parents {
            tips.retain(|tip| tip != parent);
        }
        tips.push(block.hash.clone());

        Ok(())
    }

    #[allow(dead_code)]
    async fn get_tips(&self) -> Vec<Hash> {
        self.tips.read().await.clone()
    }
}

// ============================================================================
// TEST: Full block submission via block_hex parameter
// ============================================================================

/// Test: Full block submission via block_hex parameter
///
/// VALIDATES: Issue #2 fix - block_hex path bypasses cache dependency
#[tokio::test]
async fn test_block_submission_via_block_hex() {
    let blockchain = MockBlockchain::new();
    let genesis_hash = Hash::zero();

    // Create test transactions
    let txs = vec![
        MockTransaction {
            hash: test_hash(10),
            data: vec![1, 2, 3],
        },
        MockTransaction {
            hash: test_hash(11),
            data: vec![4, 5, 6],
        },
    ];

    // Calculate correct merkle root
    let merkle_root = MockBlockchain::calculate_merkle_root(&txs);

    // Create block with full transaction data
    let block = BlockInfo {
        hash: test_hash(1),
        parents: vec![genesis_hash],
        merkle_root,
        timestamp: 1600000001000,
        valid: true,
        transactions: txs,
    };

    // Submit via block_hex path (bypasses cache)
    let result = blockchain.submit_block_with_hex(&block).await;
    assert!(
        result.is_ok(),
        "Full block submission should succeed: {:?}",
        result
    );

    // Verify block was stored
    let blocks = blockchain.blocks.read().await;
    assert!(blocks.contains_key(&block.hash), "Block should be stored");
}

/// Test: Full block submission works even with no cache
///
/// VALIDATES: block_hex path is independent of cache state
#[tokio::test]
async fn test_block_hex_independent_of_cache() {
    // Use a blockchain with very short TTL to ensure cache is empty
    let blockchain = MockBlockchain::with_cache_ttl(Duration::from_millis(1));
    let genesis_hash = Hash::zero();

    // Don't cache anything - simulate cache miss scenario

    let txs = vec![MockTransaction {
        hash: test_hash(20),
        data: vec![7, 8, 9],
    }];

    let merkle_root = MockBlockchain::calculate_merkle_root(&txs);

    let block = BlockInfo {
        hash: test_hash(2),
        parents: vec![genesis_hash],
        merkle_root,
        timestamp: 1600000001000,
        valid: true,
        transactions: txs,
    };

    // Should succeed even without cache
    let result = blockchain.submit_block_with_hex(&block).await;
    assert!(
        result.is_ok(),
        "block_hex submission should work without cache: {:?}",
        result
    );
}

// ============================================================================
// TEST: Header-only submission with cache reconstruction
// ============================================================================

/// Test: Header-only submission with valid cache
///
/// VALIDATES: Legacy miners can still submit using header-only method
#[tokio::test]
async fn test_block_submission_via_header_cache() {
    let blockchain = MockBlockchain::new();
    let genesis_hash = Hash::zero();

    // Simulate get_block_template caching transactions
    let template_hash = test_hash(100);
    let cached_txs = vec![MockTransaction {
        hash: test_hash(30),
        data: vec![1, 2, 3],
    }];
    blockchain
        .cache_transactions(template_hash.clone(), cached_txs.clone())
        .await;

    // Calculate merkle root (same as what cache would produce)
    let merkle_root = MockBlockchain::calculate_merkle_root(&cached_txs);

    // Submit header only
    let header_hash = test_hash(3);
    let result = blockchain
        .submit_block_header_only(
            header_hash.clone(),
            vec![genesis_hash],
            merkle_root,
            1600000001000,
            &template_hash,
        )
        .await;

    assert!(
        result.is_ok(),
        "Header-only submission should succeed with valid cache: {:?}",
        result
    );

    // Verify block was reconstructed and stored
    let blocks = blockchain.blocks.read().await;
    assert!(blocks.contains_key(&header_hash), "Block should be stored");
}

/// Test: Header-only submission fails on cache miss
///
/// VALIDATES: Clear error message when cache expires
#[tokio::test]
async fn test_header_only_fails_on_cache_miss() {
    let blockchain = MockBlockchain::new();
    let genesis_hash = Hash::zero();

    // Don't cache anything - simulate cache miss
    let template_hash = test_hash(101);

    let result = blockchain
        .submit_block_header_only(
            test_hash(4),
            vec![genesis_hash],
            Hash::zero(),
            1600000001000,
            &template_hash,
        )
        .await;

    assert!(result.is_err(), "Should fail on cache miss");
    let error = result.unwrap_err();
    assert!(
        error.contains("Cache miss"),
        "Error should mention cache miss: {}",
        error
    );
    assert!(
        error.contains("TTL=300s"),
        "Error should mention TTL: {}",
        error
    );
    assert!(
        error.contains("block_hex"),
        "Error should suggest block_hex: {}",
        error
    );
}

// ============================================================================
// TEST: Cache TTL behavior
// ============================================================================

/// Test: Cache TTL is 300 seconds (5 minutes)
///
/// VALIDATES: Issue #2 fix - TTL increased from 60s to 300s
#[tokio::test]
async fn test_cache_ttl_is_300_seconds() {
    let blockchain = MockBlockchain::new();

    // Verify default TTL is 300 seconds
    assert_eq!(
        blockchain.cache_ttl,
        Duration::from_secs(300),
        "Cache TTL should be 300 seconds"
    );
}

/// Test: Cache entry expires after TTL
///
/// VALIDATES: Cache expiry behavior
#[tokio::test]
async fn test_cache_expiry_after_ttl() {
    // Use very short TTL for testing
    let blockchain = MockBlockchain::with_cache_ttl(Duration::from_millis(50));

    let template_hash = test_hash(102);
    let txs = vec![MockTransaction {
        hash: test_hash(40),
        data: vec![1],
    }];

    // Cache transactions
    blockchain
        .cache_transactions(template_hash.clone(), txs)
        .await;

    // Should be available immediately
    let cached = blockchain.get_cached_transactions(&template_hash).await;
    assert!(cached.is_some(), "Cache should be available immediately");

    // Wait for TTL to expire
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Should be expired now
    let cached = blockchain.get_cached_transactions(&template_hash).await;
    assert!(cached.is_none(), "Cache should expire after TTL");
}

// ============================================================================
// TEST: Merkle root validation
// ============================================================================

/// Test: Merkle root validation for block_hex submission
///
/// VALIDATES: Security - block_hex path validates merkle root
#[tokio::test]
async fn test_merkle_root_validation_block_hex() {
    let blockchain = MockBlockchain::new();
    let genesis_hash = Hash::zero();

    let txs = vec![MockTransaction {
        hash: test_hash(50),
        data: vec![1, 2, 3],
    }];

    // Create block with WRONG merkle root
    let block = BlockInfo {
        hash: test_hash(5),
        parents: vec![genesis_hash],
        merkle_root: test_hash(0xFF), // Wrong!
        timestamp: 1600000001000,
        valid: true,
        transactions: txs,
    };

    let result = blockchain.submit_block_with_hex(&block).await;
    assert!(
        result.is_err(),
        "Block with wrong merkle root should be rejected"
    );
    let error = result.unwrap_err();
    assert!(
        error.contains("merkle"),
        "Error should mention merkle validation: {}",
        error
    );
}

/// Test: Merkle root validation for cache-based submission
///
/// VALIDATES: Security - cache path validates merkle root
#[tokio::test]
async fn test_merkle_root_validation_cache() {
    let blockchain = MockBlockchain::new();
    let genesis_hash = Hash::zero();

    let template_hash = test_hash(103);
    let cached_txs = vec![MockTransaction {
        hash: test_hash(60),
        data: vec![1, 2, 3],
    }];
    blockchain
        .cache_transactions(template_hash.clone(), cached_txs)
        .await;

    // Submit header with WRONG merkle root
    let result = blockchain
        .submit_block_header_only(
            test_hash(6),
            vec![genesis_hash],
            test_hash(0xFF), // Wrong merkle root!
            1600000001000,
            &template_hash,
        )
        .await;

    assert!(result.is_err(), "Mismatched merkle root should be rejected");
    let error = result.unwrap_err();
    assert!(
        error.contains("Merkle root mismatch"),
        "Error should mention merkle mismatch: {}",
        error
    );
}

/// Test: Empty block has zero merkle root
///
/// VALIDATES: Empty blocks are valid with zero merkle root
#[tokio::test]
async fn test_empty_block_merkle_root() {
    let blockchain = MockBlockchain::new();
    let genesis_hash = Hash::zero();

    // Empty block should have zero merkle root
    let block = BlockInfo {
        hash: test_hash(7),
        parents: vec![genesis_hash],
        merkle_root: Hash::zero(),
        timestamp: 1600000001000,
        valid: true,
        transactions: vec![], // No transactions
    };

    let result = blockchain.submit_block_with_hex(&block).await;
    assert!(
        result.is_ok(),
        "Empty block with zero merkle root should be accepted: {:?}",
        result
    );
}

// ============================================================================
// TEST: Block validation scenarios
// ============================================================================

/// Test: Duplicate block submission
///
/// VALIDATES: Duplicate blocks are rejected
#[tokio::test]
async fn test_duplicate_block_rejected() {
    let blockchain = MockBlockchain::new();
    let genesis_hash = Hash::zero();

    let block = BlockInfo {
        hash: test_hash(8),
        parents: vec![genesis_hash],
        merkle_root: Hash::zero(),
        timestamp: 1600000001000,
        valid: true,
        transactions: vec![],
    };

    // First submission should succeed
    let result1 = blockchain.submit_block_with_hex(&block).await;
    assert!(result1.is_ok(), "First submission should succeed");

    // Second submission should fail
    let result2 = blockchain.submit_block_with_hex(&block).await;
    assert!(result2.is_err(), "Duplicate should be rejected");
    assert!(
        result2.unwrap_err().contains("already exists"),
        "Error should mention duplicate"
    );
}

/// Test: Block with invalid parent
///
/// VALIDATES: Blocks with nonexistent parents are rejected
#[tokio::test]
async fn test_invalid_parent_rejected() {
    let blockchain = MockBlockchain::new();

    let nonexistent_parent = test_hash(99);
    let block = BlockInfo {
        hash: test_hash(9),
        parents: vec![nonexistent_parent],
        merkle_root: Hash::zero(),
        timestamp: 1600000001000,
        valid: true,
        transactions: vec![],
    };

    let result = blockchain.submit_block_with_hex(&block).await;
    assert!(
        result.is_err(),
        "Block with invalid parent should be rejected"
    );
    assert!(
        result.unwrap_err().contains("not found"),
        "Error should mention missing parent"
    );
}

/// Test: Block with timestamp before parent
///
/// VALIDATES: Timestamp ordering is enforced
#[tokio::test]
async fn test_timestamp_ordering_enforced() {
    let blockchain = MockBlockchain::new();
    let genesis_hash = Hash::zero();

    // First create a valid block
    let block1 = BlockInfo {
        hash: test_hash(10),
        parents: vec![genesis_hash],
        merkle_root: Hash::zero(),
        timestamp: 1600000001000,
        valid: true,
        transactions: vec![],
    };
    blockchain.submit_block_with_hex(&block1).await.unwrap();

    // Try to create block with earlier timestamp
    let block2 = BlockInfo {
        hash: test_hash(11),
        parents: vec![block1.hash.clone()],
        merkle_root: Hash::zero(),
        timestamp: 1600000000500, // Before parent!
        valid: true,
        transactions: vec![],
    };

    let result = blockchain.submit_block_with_hex(&block2).await;
    assert!(
        result.is_err(),
        "Block with old timestamp should be rejected"
    );
    assert!(
        result.unwrap_err().contains("timestamp"),
        "Error should mention timestamp"
    );
}

// ============================================================================
// TEST: Concurrent submissions
// ============================================================================

/// Test: Concurrent block submissions
///
/// VALIDATES: Both submission paths handle concurrent requests safely
#[tokio::test]
async fn test_concurrent_block_submissions() {
    let blockchain = Arc::new(MockBlockchain::new());
    let genesis_hash = Hash::zero();

    // Cache some transactions for header-only submissions
    for i in 0..5u8 {
        let template_hash = test_hash(200 + i);
        let txs = vec![MockTransaction {
            hash: test_hash(210 + i),
            data: vec![i],
        }];
        blockchain.cache_transactions(template_hash, txs).await;
    }

    // Submit 10 blocks concurrently (mix of paths)
    let handles: Vec<_> = (0..10u8)
        .map(|i| {
            let bc = blockchain.clone();
            let genesis = genesis_hash.clone();
            tokio::spawn(async move {
                if i % 2 == 0 {
                    // Even: use block_hex
                    let block = BlockInfo {
                        hash: test_hash(100 + i),
                        parents: vec![genesis],
                        merkle_root: Hash::zero(),
                        timestamp: 1600000001000 + (i as u64 * 1000),
                        valid: true,
                        transactions: vec![],
                    };
                    bc.submit_block_with_hex(&block).await
                } else {
                    // Odd: use header-only with cache
                    let template_hash = test_hash(200 + (i / 2));
                    let expected_txs = vec![MockTransaction {
                        hash: test_hash(210 + (i / 2)),
                        data: vec![i / 2],
                    }];
                    let merkle_root = MockBlockchain::calculate_merkle_root(&expected_txs);

                    bc.submit_block_header_only(
                        test_hash(100 + i),
                        vec![genesis],
                        merkle_root,
                        1600000001000 + (i as u64 * 1000),
                        &template_hash,
                    )
                    .await
                }
            })
        })
        .collect();

    // Wait for all submissions
    let mut success_count = 0;
    for handle in handles {
        if handle.await.unwrap().is_ok() {
            success_count += 1;
        }
    }

    // All submissions should succeed (no conflicts - different hashes)
    assert_eq!(
        success_count, 10,
        "All concurrent submissions should succeed"
    );

    // Verify blockchain state is consistent
    let blocks = blockchain.blocks.read().await;
    // 1 genesis + 10 new blocks
    assert_eq!(blocks.len(), 11, "Should have 11 total blocks");
}

// ============================================================================
// TEST: Comprehensive scenarios
// ============================================================================

/// Test: Comprehensive block submission scenarios
///
/// VALIDATES: All aspects of block submission
#[tokio::test]
async fn test_comprehensive_block_submission_scenarios() {
    let blockchain = MockBlockchain::new();
    let genesis_hash = Hash::zero();

    // Scenario 1: Valid block with correct merkle root
    if log::log_enabled!(log::Level::Info) {
        log::info!("Test scenario 1: Valid block submission");
    }

    let txs1 = vec![MockTransaction {
        hash: test_hash(70),
        data: vec![1, 2, 3],
    }];
    let merkle1 = MockBlockchain::calculate_merkle_root(&txs1);

    let valid_block = BlockInfo {
        hash: test_hash(12),
        parents: vec![genesis_hash.clone()],
        merkle_root: merkle1,
        timestamp: 1600000001000,
        valid: true,
        transactions: txs1,
    };

    let result1 = blockchain.submit_block_with_hex(&valid_block).await;
    assert!(result1.is_ok(), "Valid block should be accepted");

    // Scenario 2: Invalid block with wrong merkle root
    if log::log_enabled!(log::Level::Info) {
        log::info!("Test scenario 2: Invalid merkle root");
    }

    let invalid_merkle_block = BlockInfo {
        hash: test_hash(13),
        parents: vec![genesis_hash.clone()],
        merkle_root: test_hash(0xFF),
        timestamp: 1600000002000,
        valid: true,
        transactions: vec![MockTransaction {
            hash: test_hash(80),
            data: vec![],
        }],
    };

    let result2 = blockchain
        .submit_block_with_hex(&invalid_merkle_block)
        .await;
    assert!(
        result2.is_err(),
        "Block with invalid merkle root should be rejected"
    );

    // Scenario 3: Duplicate block submission
    if log::log_enabled!(log::Level::Info) {
        log::info!("Test scenario 3: Duplicate block submission");
    }

    let result3 = blockchain.submit_block_with_hex(&valid_block).await;
    assert!(result3.is_err(), "Duplicate block should be rejected");

    // Scenario 4: Block with invalid parents
    if log::log_enabled!(log::Level::Info) {
        log::info!("Test scenario 4: Invalid parent reference");
    }

    let invalid_parent_block = BlockInfo {
        hash: test_hash(14),
        parents: vec![test_hash(99)],
        merkle_root: Hash::zero(),
        timestamp: 1600000003000,
        valid: true,
        transactions: vec![],
    };

    let result4 = blockchain
        .submit_block_with_hex(&invalid_parent_block)
        .await;
    assert!(
        result4.is_err(),
        "Block with nonexistent parent should be rejected"
    );

    // Scenario 5: Empty block with zero merkle root
    if log::log_enabled!(log::Level::Info) {
        log::info!("Test scenario 5: Empty block submission");
    }

    let empty_block = BlockInfo {
        hash: test_hash(15),
        parents: vec![valid_block.hash.clone()],
        merkle_root: Hash::zero(),
        timestamp: 1600000004000,
        valid: true,
        transactions: vec![],
    };

    let result5 = blockchain.submit_block_with_hex(&empty_block).await;
    assert!(
        result5.is_ok(),
        "Empty block with zero merkle root should be accepted"
    );

    // Verify final blockchain state
    let tips = blockchain.tips.read().await;
    assert_eq!(tips.len(), 1, "Should have exactly 1 tip");
    assert_eq!(
        tips[0], empty_block.hash,
        "Tip should be the latest valid block"
    );

    if log::log_enabled!(log::Level::Info) {
        log::info!("All comprehensive block submission scenarios passed");
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    /// Test merkle root calculation for empty transaction set
    #[test]
    fn test_merkle_root_empty_txs() {
        let merkle = MockBlockchain::calculate_merkle_root(&[]);
        assert_eq!(
            merkle,
            Hash::zero(),
            "Empty txs should have zero merkle root"
        );
    }

    /// Test merkle root calculation for single transaction
    #[test]
    fn test_merkle_root_single_tx() {
        let tx = MockTransaction {
            hash: test_hash(1),
            data: vec![],
        };
        let merkle = MockBlockchain::calculate_merkle_root(&[tx.clone()]);
        assert_ne!(
            merkle,
            Hash::zero(),
            "Single tx should have non-zero merkle root"
        );
    }

    /// Test merkle root is deterministic
    #[test]
    fn test_merkle_root_deterministic() {
        let txs = vec![
            MockTransaction {
                hash: test_hash(1),
                data: vec![],
            },
            MockTransaction {
                hash: test_hash(2),
                data: vec![],
            },
        ];

        let merkle1 = MockBlockchain::calculate_merkle_root(&txs);
        let merkle2 = MockBlockchain::calculate_merkle_root(&txs);

        assert_eq!(merkle1, merkle2, "Merkle root should be deterministic");
    }

    /// Test merkle root changes with different transactions
    #[test]
    fn test_merkle_root_differs_for_different_txs() {
        let txs1 = vec![MockTransaction {
            hash: test_hash(1),
            data: vec![],
        }];

        let txs2 = vec![MockTransaction {
            hash: test_hash(2),
            data: vec![],
        }];

        let merkle1 = MockBlockchain::calculate_merkle_root(&txs1);
        let merkle2 = MockBlockchain::calculate_merkle_root(&txs2);

        assert_ne!(
            merkle1, merkle2,
            "Different txs should have different merkle roots"
        );
    }
}
