// Merkle root calculation for transaction lists
// Based on standard Bitcoin/Kaspa merkle tree implementation

use crate::crypto::{Hash, Hashable};
use crate::transaction::Transaction;
use std::sync::Arc;

/// Calculate merkle root from a list of transactions
///
/// This creates a binary merkle tree where:
/// - Leaves are transaction hashes
/// - Parent nodes are hash(left || right)
/// - If odd number of nodes, last node is paired with itself
///
/// # Security Note
/// This must match the header's hash_merkle_root to prevent
/// malicious blocks with mismatched transaction bodies
pub fn calculate_merkle_root(transactions: &[Arc<Transaction>]) -> Hash {
    if transactions.is_empty() {
        // Empty merkle root (all zeros)
        return Hash::zero();
    }

    // Create leaf hashes
    let mut hashes: Vec<Hash> = transactions
        .iter()
        .map(|tx| tx.hash())
        .collect();

    // Build merkle tree bottom-up
    while hashes.len() > 1 {
        let mut next_level = Vec::new();

        // Process pairs
        for chunk in hashes.chunks(2) {
            let left = &chunk[0];
            let right = if chunk.len() == 2 {
                &chunk[1]
            } else {
                // Odd number: pair with itself
                &chunk[0]
            };

            // Combine hashes: hash(left || right)
            let combined = hash_pair(left, right);
            next_level.push(combined);
        }

        hashes = next_level;
    }

    hashes[0].clone()
}

/// Hash a pair of hashes
fn hash_pair(left: &Hash, right: &Hash) -> Hash {
    use blake3::Hasher;

    let mut hasher = Hasher::new();
    hasher.update(left.as_bytes());
    hasher.update(right.as_bytes());

    let result = hasher.finalize();
    Hash::new(*result.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::{Transaction, TransactionType};

    #[test]
    fn test_empty_merkle_root() {
        let txs = vec![];
        let root = calculate_merkle_root(&txs);
        assert_eq!(root, Hash::zero());
    }

    #[test]
    fn test_single_transaction() {
        let tx = Arc::new(Transaction::coinbase(
            1000,
            vec![],
            vec![],
        ));
        let txs = vec![tx.clone()];

        let root = calculate_merkle_root(&txs);
        // Single tx: root should be tx hash paired with itself
        let expected = hash_pair(&tx.hash(), &tx.hash());
        assert_eq!(root, expected);
    }

    #[test]
    fn test_two_transactions() {
        let tx1 = Arc::new(Transaction::coinbase(1000, vec![], vec![]));
        let tx2 = Arc::new(Transaction::coinbase(2000, vec![], vec![]));
        let txs = vec![tx1.clone(), tx2.clone()];

        let root = calculate_merkle_root(&txs);
        // Two txs: root is hash(tx1 || tx2)
        let expected = hash_pair(&tx1.hash(), &tx2.hash());
        assert_eq!(root, expected);
    }

    #[test]
    fn test_three_transactions() {
        let tx1 = Arc::new(Transaction::coinbase(1000, vec![], vec![]));
        let tx2 = Arc::new(Transaction::coinbase(2000, vec![], vec![]));
        let tx3 = Arc::new(Transaction::coinbase(3000, vec![], vec![]));
        let txs = vec![tx1.clone(), tx2.clone(), tx3.clone()];

        let root = calculate_merkle_root(&txs);

        // Level 0: [tx1, tx2, tx3]
        // Level 1: [hash(tx1||tx2), hash(tx3||tx3)]
        // Level 2: hash(hash(tx1||tx2) || hash(tx3||tx3))
        let h12 = hash_pair(&tx1.hash(), &tx2.hash());
        let h33 = hash_pair(&tx3.hash(), &tx3.hash());
        let expected = hash_pair(&h12, &h33);

        assert_eq!(root, expected);
    }

    #[test]
    fn test_merkle_root_deterministic() {
        let tx1 = Arc::new(Transaction::coinbase(1000, vec![], vec![]));
        let tx2 = Arc::new(Transaction::coinbase(2000, vec![], vec![]));
        let txs = vec![tx1, tx2];

        let root1 = calculate_merkle_root(&txs);
        let root2 = calculate_merkle_root(&txs);

        assert_eq!(root1, root2, "Merkle root should be deterministic");
    }
}
