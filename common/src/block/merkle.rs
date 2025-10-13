// Merkle root calculation for transaction lists

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

    // Special case: single transaction pairs with itself
    if hashes.len() == 1 {
        return hash_pair(&hashes[0], &hashes[0]);
    }

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
    use crate::transaction::{
        Transaction, TransactionType, BurnPayload, TxVersion, FeeType, Reference,
    };
    use crate::crypto::elgamal::CompressedPublicKey;
    use crate::serializer::Serializer;
    use bulletproofs::RangeProof;
    use curve25519_dalek::scalar::Scalar;

    /// Create a mock transaction for testing merkle root calculation
    /// The amount parameter ensures different transactions have different hashes
    fn create_mock_transaction(amount: u64) -> Transaction {
        use crate::config::TOS_ASSET;

        // Create dummy values for required fields
        let mut source_bytes = [0u8; 32];
        source_bytes[0] = amount as u8;
        let source = CompressedPublicKey::from_bytes(&source_bytes).unwrap();

        let data = TransactionType::Burn(BurnPayload {
            amount,
            asset: TOS_ASSET,
        });
        let fee = 100;
        let fee_type = FeeType::TOS;
        let nonce = 0;
        let source_commitments = vec![];

        // Create minimal range proof for testing
        // Use a simple proof generation approach
        let range_proof = {
            use bulletproofs::{PedersenGens, BulletproofGens};
            let pc_gens = PedersenGens::default();
            let bp_gens = BulletproofGens::new(64, 1);
            let mut transcript = merlin::Transcript::new(b"test");
            let blinding = Scalar::from_bytes_mod_order([amount as u8; 32]);
            let (proof, _commitment) = RangeProof::prove_single(
                &bp_gens,
                &pc_gens,
                &mut transcript,
                amount,
                &blinding,
                64
            ).unwrap();
            proof
        };

        let reference = Reference {
            topoheight: 0,
            hash: Hash::zero(),
        };
        let multisig = None;

        // Create dummy signature
        let mut sig_bytes = [0u8; 64];
        sig_bytes[0] = amount as u8;
        let signature = crate::crypto::Signature::from_bytes(&sig_bytes).unwrap();

        Transaction::new(
            TxVersion::T0,
            source,
            data,
            fee,
            fee_type,
            nonce,
            source_commitments,
            range_proof,
            reference,
            multisig,
            signature,
        )
    }

    #[test]
    fn test_empty_merkle_root() {
        let txs = vec![];
        let root = calculate_merkle_root(&txs);
        assert_eq!(root, Hash::zero());
    }

    #[test]
    fn test_single_transaction() {
        let tx = Arc::new(create_mock_transaction(1000));
        let txs = vec![tx.clone()];

        let root = calculate_merkle_root(&txs);
        // Single tx: root should be tx hash paired with itself
        let expected = hash_pair(&tx.hash(), &tx.hash());
        assert_eq!(root, expected);
    }

    #[test]
    fn test_two_transactions() {
        let tx1 = Arc::new(create_mock_transaction(1000));
        let tx2 = Arc::new(create_mock_transaction(2000));
        let txs = vec![tx1.clone(), tx2.clone()];

        let root = calculate_merkle_root(&txs);
        // Two txs: root is hash(tx1 || tx2)
        let expected = hash_pair(&tx1.hash(), &tx2.hash());
        assert_eq!(root, expected);
    }

    #[test]
    fn test_three_transactions() {
        let tx1 = Arc::new(create_mock_transaction(1000));
        let tx2 = Arc::new(create_mock_transaction(2000));
        let tx3 = Arc::new(create_mock_transaction(3000));
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
        let tx1 = Arc::new(create_mock_transaction(1000));
        let tx2 = Arc::new(create_mock_transaction(2000));
        let txs = vec![tx1, tx2];

        let root1 = calculate_merkle_root(&txs);
        let root2 = calculate_merkle_root(&txs);

        assert_eq!(root1, root2, "Merkle root should be deterministic");
    }
}
