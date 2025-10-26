// Parallel Transaction Execution Module
//
// This module implements Solana-style parallel transaction execution
// based on pre-declared account dependencies in V2 transactions.
//
// Key concepts:
// - AccountMeta: Pre-declared account access (pubkey, asset, read/write)
// - Conflict Detection: Transactions conflict if they access the same (pubkey, asset) with at least one write
// - Parallel Batches: Group non-conflicting transactions for concurrent execution
// - Sequential Fallback: T0 transactions execute sequentially (no account_keys)

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use crate::crypto::{elgamal::CompressedPublicKey, Hash};
use super::{Transaction, AccountMeta, TxVersion};

/// Unique identifier for an account access (pubkey + asset)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountKey {
    pub pubkey: CompressedPublicKey,
    pub asset: Hash,
}

impl AccountKey {
    pub fn new(pubkey: CompressedPublicKey, asset: Hash) -> Self {
        Self { pubkey, asset }
    }

    pub fn from_meta(meta: &AccountMeta) -> Self {
        Self {
            pubkey: meta.pubkey.clone(),
            asset: meta.asset.clone(),
        }
    }
}

/// Access pattern for an account
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    Read,
    Write,
}

/// Conflict detection result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictStatus {
    /// Transactions can execute in parallel
    NoConflict,
    /// Transactions conflict on at least one account
    Conflict(AccountKey),
}

/// Analyze transaction dependencies for parallel execution
pub struct TransactionAnalyzer;

impl TransactionAnalyzer {
    /// Check if two transactions conflict
    ///
    /// Transactions conflict if they access the same (pubkey, asset) and at least one is a write.
    /// This follows Solana's model: read-read is safe, but read-write and write-write conflict.
    pub fn detect_conflict(tx1: &Transaction, tx2: &Transaction) -> ConflictStatus {
        // T0 transactions have no account_keys, treat as conflicting with everything
        if tx1.get_version() < TxVersion::V2 || tx2.get_version() < TxVersion::V2 {
            // Conservative: assume T0 transactions conflict
            return ConflictStatus::Conflict(AccountKey::new(
                tx1.get_source().clone(),
                crate::config::TOS_ASSET,
            ));
        }

        // Build access maps for both transactions
        let access1 = Self::build_access_map(tx1);
        let access2 = Self::build_access_map(tx2);

        // Check for conflicts: same account with at least one write
        for (key1, access_type1) in &access1 {
            if let Some(access_type2) = access2.get(key1) {
                // Conflict if at least one is a write
                if *access_type1 == AccessType::Write || *access_type2 == AccessType::Write {
                    return ConflictStatus::Conflict(key1.clone());
                }
            }
        }

        ConflictStatus::NoConflict
    }

    /// Build a map of account accesses for a transaction
    fn build_access_map(tx: &Transaction) -> HashMap<AccountKey, AccessType> {
        let mut map = HashMap::new();

        // For V2 transactions, use account_keys directly
        if tx.get_version() >= TxVersion::V2 {
            for meta in tx.get_account_keys() {
                let key = AccountKey::from_meta(meta);
                let access_type = if meta.is_writable {
                    AccessType::Write
                } else {
                    AccessType::Read
                };
                // If account appears multiple times, use most permissive (Write > Read)
                map.entry(key)
                    .and_modify(|existing| {
                        if access_type == AccessType::Write {
                            *existing = AccessType::Write;
                        }
                    })
                    .or_insert(access_type);
            }
        }

        map
    }

    /// Group transactions into non-conflicting batches for parallel execution
    ///
    /// Returns Vec<Vec<usize>> where each inner Vec contains indices of non-conflicting transactions
    pub fn create_parallel_batches(transactions: &[Arc<Transaction>]) -> Vec<Vec<usize>> {
        let mut batches: Vec<Vec<usize>> = Vec::new();
        let mut assigned = HashSet::new();

        for (i, tx) in transactions.iter().enumerate() {
            if assigned.contains(&i) {
                continue;
            }

            // T0 transactions execute sequentially (each in its own batch)
            if tx.get_version() < TxVersion::V2 {
                batches.push(vec![i]);
                assigned.insert(i);
                continue;
            }

            // Start a new batch with this transaction
            let mut batch = vec![i];
            assigned.insert(i);

            // Try to add more non-conflicting transactions to this batch
            for (j, _) in transactions.iter().enumerate() {
                if assigned.contains(&j) {
                    continue;
                }

                // Check if this transaction conflicts with any in the current batch
                let mut conflicts = false;
                for &batch_idx in &batch {
                    if Self::detect_conflict(&transactions[j], &transactions[batch_idx]) != ConflictStatus::NoConflict {
                        conflicts = true;
                        break;
                    }
                }

                if !conflicts {
                    batch.push(j);
                    assigned.insert(j);
                }
            }

            batches.push(batch);
        }

        batches
    }

    /// Estimate parallelism factor for a set of transactions
    ///
    /// Returns (num_batches, avg_batch_size, max_batch_size)
    pub fn estimate_parallelism(transactions: &[Arc<Transaction>]) -> (usize, f64, usize) {
        let batches = Self::create_parallel_batches(transactions);
        let num_batches = batches.len();
        let max_batch_size = batches.iter().map(|b| b.len()).max().unwrap_or(0);
        let avg_batch_size = if num_batches > 0 {
            transactions.len() as f64 / num_batches as f64
        } else {
            0.0
        };

        (num_batches, avg_batch_size, max_batch_size)
    }
}

/// Execution metrics for parallel transaction processing
#[derive(Debug, Clone, Default)]
pub struct ParallelExecutionMetrics {
    /// Total number of transactions processed
    pub total_transactions: usize,
    /// Number of parallel batches executed
    pub num_batches: usize,
    /// Maximum batch size (best-case parallelism)
    pub max_batch_size: usize,
    /// Average batch size
    /// SAFE: f64 for metrics display only, not consensus-critical
    pub avg_batch_size: f64,
    /// Parallelism efficiency (avg_batch_size / total_transactions)
    /// SAFE: f64 for metrics display only, not consensus-critical
    pub parallelism_efficiency: f64,
}

impl ParallelExecutionMetrics {
    /// Calculate metrics from transaction batches
    pub fn from_batches(batches: &[Vec<usize>], total_txs: usize) -> Self {
        let num_batches = batches.len();
        let max_batch_size = batches.iter().map(|b| b.len()).max().unwrap_or(0);
        let avg_batch_size = if num_batches > 0 {
            total_txs as f64 / num_batches as f64
        } else {
            0.0
        };
        let parallelism_efficiency = if total_txs > 0 {
            avg_batch_size / total_txs as f64
        } else {
            0.0
        };

        Self {
            total_transactions: total_txs,
            num_batches,
            max_batch_size,
            avg_batch_size,
            parallelism_efficiency,
        }
    }

    /// Get a human-readable summary
    pub fn summary(&self) -> String {
        format!(
            "Parallel Execution: {} txs in {} batches (max={}, avg={:.2}, efficiency={:.2}%)",
            self.total_transactions,
            self.num_batches,
            self.max_batch_size,
            self.avg_batch_size,
            self.parallelism_efficiency * 100.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{TransactionType, BurnPayload, FeeType, Reference};
    use crate::crypto::Signature;
    use crate::config::TOS_ASSET;
    use crate::serializer::Serializer;

    fn create_test_transaction(version: TxVersion, source: CompressedPublicKey) -> Transaction {
        let data = TransactionType::Burn(BurnPayload {
            amount: 1000,
            asset: TOS_ASSET,
        });
        let reference = Reference {
            topoheight: 0,
            hash: Hash::zero(),
        };
        let signature = Signature::from_bytes(&[0u8; 64]).unwrap();

        Transaction::new(
            version,
            source,
            data,
            100,
            FeeType::TOS,
            0,
            reference,
            None,
            Vec::new(),
            signature,
        )
    }

    #[test]
    fn test_account_key_equality() {
        let pubkey1 = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let pubkey2 = CompressedPublicKey::from_bytes(&[2u8; 32]).unwrap();
        let asset = TOS_ASSET;

        let key1 = AccountKey::new(pubkey1.clone(), asset.clone());
        let key2 = AccountKey::new(pubkey1.clone(), asset.clone());
        let key3 = AccountKey::new(pubkey2, asset);

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_t0_transactions_conflict() {
        let source1 = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let source2 = CompressedPublicKey::from_bytes(&[2u8; 32]).unwrap();

        let tx1 = Arc::new(create_test_transaction(TxVersion::T0, source1));
        let tx2 = Arc::new(create_test_transaction(TxVersion::T0, source2));

        // T0 transactions should always conflict (conservative)
        assert_ne!(
            TransactionAnalyzer::detect_conflict(&tx1, &tx2),
            ConflictStatus::NoConflict
        );
    }

    #[test]
    fn test_parallel_batching_t0_only() {
        let source = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let transactions: Vec<Arc<Transaction>> = (0..5)
            .map(|_| Arc::new(create_test_transaction(TxVersion::T0, source.clone())))
            .collect();

        let batches = TransactionAnalyzer::create_parallel_batches(&transactions);

        // T0 transactions should each be in their own batch
        assert_eq!(batches.len(), 5);
        for batch in batches {
            assert_eq!(batch.len(), 1);
        }
    }

    #[test]
    fn test_estimate_parallelism() {
        let source = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let transactions: Vec<Arc<Transaction>> = (0..10)
            .map(|_| Arc::new(create_test_transaction(TxVersion::T0, source.clone())))
            .collect();

        let (num_batches, avg_batch_size, max_batch_size) =
            TransactionAnalyzer::estimate_parallelism(&transactions);

        assert_eq!(num_batches, 10); // T0 transactions: 1 per batch
        assert_eq!(avg_batch_size, 1.0);
        assert_eq!(max_batch_size, 1);
    }

    #[test]
    fn test_parallel_execution_metrics() {
        let batches = vec![
            vec![0, 1, 2],  // Batch 0: 3 transactions
            vec![3, 4],      // Batch 1: 2 transactions
            vec![5],         // Batch 2: 1 transaction
        ];
        let total_txs = 6;

        let metrics = ParallelExecutionMetrics::from_batches(&batches, total_txs);

        assert_eq!(metrics.total_transactions, 6);
        assert_eq!(metrics.num_batches, 3);
        assert_eq!(metrics.max_batch_size, 3);
        assert_eq!(metrics.avg_batch_size, 2.0); // 6 / 3
        assert!((metrics.parallelism_efficiency - 0.3333).abs() < 0.01); // 2.0 / 6.0

        // Verify summary format
        let summary = metrics.summary();
        assert!(summary.contains("6 txs"));
        assert!(summary.contains("3 batches"));
    }

    #[test]
    fn test_metrics_empty_batches() {
        let batches: Vec<Vec<usize>> = vec![];
        let metrics = ParallelExecutionMetrics::from_batches(&batches, 0);

        assert_eq!(metrics.total_transactions, 0);
        assert_eq!(metrics.num_batches, 0);
        assert_eq!(metrics.max_batch_size, 0);
        assert_eq!(metrics.avg_batch_size, 0.0);
        assert_eq!(metrics.parallelism_efficiency, 0.0);
    }

    fn create_v2_transaction_with_accounts(
        source: CompressedPublicKey,
        account_keys: Vec<AccountMeta>,
    ) -> Transaction {
        let data = TransactionType::Burn(BurnPayload {
            amount: 1000,
            asset: TOS_ASSET,
        });
        let reference = Reference {
            topoheight: 0,
            hash: Hash::zero(),
        };
        let signature = Signature::from_bytes(&[0u8; 64]).unwrap();

        Transaction::new(
            TxVersion::V2,
            source,
            data,
            100,
            FeeType::TOS,
            0,
            reference,
            None,
            account_keys,
            signature,
        )
    }

    #[test]
    fn test_v2_read_read_no_conflict() {
        // Two V2 transactions reading the same account should NOT conflict
        let source = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let account = CompressedPublicKey::from_bytes(&[2u8; 32]).unwrap();

        let account_meta = AccountMeta {
            pubkey: account.clone(),
            asset: TOS_ASSET,
            is_signer: false,
            is_writable: false, // Read-only
        };

        let tx1 = create_v2_transaction_with_accounts(source.clone(), vec![account_meta.clone()]);
        let tx2 = create_v2_transaction_with_accounts(source, vec![account_meta]);

        assert_eq!(
            TransactionAnalyzer::detect_conflict(&tx1, &tx2),
            ConflictStatus::NoConflict
        );
    }

    #[test]
    fn test_v2_read_write_conflict() {
        // V2 transaction reading + V2 transaction writing same account SHOULD conflict
        let source = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let account = CompressedPublicKey::from_bytes(&[2u8; 32]).unwrap();

        let read_meta = AccountMeta {
            pubkey: account.clone(),
            asset: TOS_ASSET,
            is_signer: false,
            is_writable: false, // Read-only
        };

        let write_meta = AccountMeta {
            pubkey: account.clone(),
            asset: TOS_ASSET,
            is_signer: false,
            is_writable: true, // Writable
        };

        let tx1 = create_v2_transaction_with_accounts(source.clone(), vec![read_meta]);
        let tx2 = create_v2_transaction_with_accounts(source, vec![write_meta]);

        match TransactionAnalyzer::detect_conflict(&tx1, &tx2) {
            ConflictStatus::Conflict(key) => {
                assert_eq!(key.pubkey, account);
                assert_eq!(key.asset, TOS_ASSET);
            }
            ConflictStatus::NoConflict => panic!("Expected conflict for read-write"),
        }
    }

    #[test]
    fn test_v2_write_write_conflict() {
        // Two V2 transactions writing the same account SHOULD conflict
        let source = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let account = CompressedPublicKey::from_bytes(&[2u8; 32]).unwrap();

        let write_meta = AccountMeta {
            pubkey: account.clone(),
            asset: TOS_ASSET,
            is_signer: false,
            is_writable: true,
        };

        let tx1 = create_v2_transaction_with_accounts(source.clone(), vec![write_meta.clone()]);
        let tx2 = create_v2_transaction_with_accounts(source, vec![write_meta]);

        match TransactionAnalyzer::detect_conflict(&tx1, &tx2) {
            ConflictStatus::Conflict(key) => {
                assert_eq!(key.pubkey, account);
            }
            ConflictStatus::NoConflict => panic!("Expected conflict for write-write"),
        }
    }

    #[test]
    fn test_v2_different_accounts_no_conflict() {
        // V2 transactions accessing different accounts should NOT conflict
        let source = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let account1 = CompressedPublicKey::from_bytes(&[2u8; 32]).unwrap();
        let account2 = CompressedPublicKey::from_bytes(&[3u8; 32]).unwrap();

        let meta1 = AccountMeta {
            pubkey: account1,
            asset: TOS_ASSET,
            is_signer: false,
            is_writable: true,
        };

        let meta2 = AccountMeta {
            pubkey: account2,
            asset: TOS_ASSET,
            is_signer: false,
            is_writable: true,
        };

        let tx1 = create_v2_transaction_with_accounts(source.clone(), vec![meta1]);
        let tx2 = create_v2_transaction_with_accounts(source, vec![meta2]);

        assert_eq!(
            TransactionAnalyzer::detect_conflict(&tx1, &tx2),
            ConflictStatus::NoConflict
        );
    }

    #[test]
    fn test_v2_parallel_batching_no_conflicts() {
        // V2 transactions with different accounts should batch together
        let source = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();

        let transactions: Vec<Arc<Transaction>> = (0..5)
            .map(|i| {
                let account = CompressedPublicKey::from_bytes(&[i as u8 + 10; 32]).unwrap();
                let meta = AccountMeta {
                    pubkey: account,
                    asset: TOS_ASSET,
                    is_signer: false,
                    is_writable: true,
                };
                Arc::new(create_v2_transaction_with_accounts(source.clone(), vec![meta]))
            })
            .collect();

        let batches = TransactionAnalyzer::create_parallel_batches(&transactions);

        // All non-conflicting V2 transactions should be in one batch
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 5);
    }

    #[test]
    fn test_v2_parallel_batching_with_conflicts() {
        // V2 transactions writing to same account should be in separate batches
        let source = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let shared_account = CompressedPublicKey::from_bytes(&[99u8; 32]).unwrap();

        let shared_meta = AccountMeta {
            pubkey: shared_account,
            asset: TOS_ASSET,
            is_signer: false,
            is_writable: true,
        };

        let transactions: Vec<Arc<Transaction>> = (0..3)
            .map(|_| {
                Arc::new(create_v2_transaction_with_accounts(
                    source.clone(),
                    vec![shared_meta.clone()],
                ))
            })
            .collect();

        let batches = TransactionAnalyzer::create_parallel_batches(&transactions);

        // All transactions conflict, so each should be in its own batch
        assert_eq!(batches.len(), 3);
        for batch in batches {
            assert_eq!(batch.len(), 1);
        }
    }

    #[test]
    fn test_v2_different_assets_no_conflict() {
        // Same pubkey but different assets should NOT conflict
        let source = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();
        let account = CompressedPublicKey::from_bytes(&[2u8; 32]).unwrap();
        let asset1 = Hash::from_bytes(&[1u8; 32]).unwrap();
        let asset2 = Hash::from_bytes(&[2u8; 32]).unwrap();

        let meta1 = AccountMeta {
            pubkey: account.clone(),
            asset: asset1,
            is_signer: false,
            is_writable: true,
        };

        let meta2 = AccountMeta {
            pubkey: account,
            asset: asset2,
            is_signer: false,
            is_writable: true,
        };

        let tx1 = create_v2_transaction_with_accounts(source.clone(), vec![meta1]);
        let tx2 = create_v2_transaction_with_accounts(source, vec![meta2]);

        assert_eq!(
            TransactionAnalyzer::detect_conflict(&tx1, &tx2),
            ConflictStatus::NoConflict
        );
    }

    #[test]
    fn test_mixed_t0_and_v2_batching() {
        // Mix of T0 and V2 transactions should batch correctly
        let source = CompressedPublicKey::from_bytes(&[1u8; 32]).unwrap();

        let mut transactions: Vec<Arc<Transaction>> = vec![];

        // Add 2 T0 transactions (will be sequential)
        transactions.push(Arc::new(create_test_transaction(TxVersion::T0, source.clone())));
        transactions.push(Arc::new(create_test_transaction(TxVersion::T0, source.clone())));

        // Add 2 V2 transactions with different accounts (can parallelize)
        for i in 0..2 {
            let account = CompressedPublicKey::from_bytes(&[i as u8 + 10; 32]).unwrap();
            let meta = AccountMeta {
                pubkey: account,
                asset: TOS_ASSET,
                is_signer: false,
                is_writable: true,
            };
            transactions.push(Arc::new(create_v2_transaction_with_accounts(
                source.clone(),
                vec![meta],
            )));
        }

        let batches = TransactionAnalyzer::create_parallel_batches(&transactions);

        // Should have: 2 T0 batches (1 tx each) + 1 V2 batch (2 txs)
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), 1); // T0 #1
        assert_eq!(batches[1].len(), 1); // T0 #2
        assert_eq!(batches[2].len(), 2); // V2 batch with both non-conflicting txs
    }
}
