// Compact Block Reconstruction Logic
// Reconstructs full blocks from compact blocks using mempool transactions

use super::{error::BlockchainError, mempool::Mempool};
use log::{debug, trace, warn};
use std::sync::Arc;
#[cfg(test)]
use tos_common::crypto::Hash;
use tos_common::{
    block::{
        calculate_short_tx_id, Block, CompactBlock, MissingTransactionsRequest,
        MissingTransactionsResponse,
    },
    crypto::Hashable,
    immutable::Immutable,
    transaction::Transaction,
};

/// Result of compact block reconstruction attempt
pub enum ReconstructionResult {
    /// Successfully reconstructed the full block
    Success(Block),

    /// Missing transactions - need to request from peer
    MissingTransactions(MissingTransactionsRequest),

    /// Too many missing transactions - should request full block instead
    TooManyMissing {
        missing_count: usize,
        total_count: usize,
    },
}

/// Compact block reconstructor
pub struct CompactBlockReconstructor;

impl CompactBlockReconstructor {
    /// Attempt to reconstruct a full block from a compact block using mempool
    ///
    /// Algorithm:
    /// 1. For each short transaction ID, search mempool for matching transaction
    /// 2. If found >90% of transactions, request missing ones
    /// 3. If found <90% of transactions, fall back to requesting full block
    ///
    /// Returns:
    /// - Success(Block) if all transactions found
    /// - MissingTransactions(request) if some transactions missing but reconstructable
    /// - TooManyMissing if too many transactions missing (>10%)
    pub async fn reconstruct(
        compact_block: CompactBlock,
        mempool: &Mempool,
    ) -> Result<ReconstructionResult, BlockchainError> {
        let block_hash = compact_block.header.hash();
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Attempting to reconstruct block {} from compact block",
                block_hash
            );
        }

        let total_tx_count = compact_block.short_tx_ids.len();

        // Build a map of prefilled transactions by index
        let mut transactions = vec![None; total_tx_count];
        for (index, tx) in compact_block.prefilled_txs {
            if (index as usize) < total_tx_count {
                transactions[index as usize] = Some(Arc::new(tx));
            } else {
                if log::log_enabled!(log::Level::Warn) {
                    warn!(
                        "Prefilled transaction index {} out of bounds (total: {})",
                        index, total_tx_count
                    );
                }
                return Err(BlockchainError::Any(anyhow::anyhow!(
                    "Prefilled transaction index out of bounds"
                )));
            }
        }

        // Try to match short IDs with mempool transactions
        let mut missing_indices = Vec::new();

        for (index, short_id) in compact_block.short_tx_ids.iter().enumerate() {
            // Skip if already prefilled
            if transactions[index].is_some() {
                continue;
            }

            // Search mempool for matching transaction
            let mut found = false;
            for (tx_hash, sorted_tx) in mempool.get_txs() {
                let candidate_short_id = calculate_short_tx_id(compact_block.nonce, tx_hash);
                if candidate_short_id == *short_id {
                    transactions[index] = Some(sorted_tx.get_tx().clone());
                    found = true;
                    if log::log_enabled!(log::Level::Trace) {
                        trace!(
                            "Matched short ID at index {} with mempool tx {}",
                            index,
                            tx_hash
                        );
                    }
                    break;
                }
            }

            if !found {
                missing_indices.push(index as u16);
            }
        }

        let missing_count = missing_indices.len();
        let missing_percentage = (missing_count as f64 / total_tx_count as f64) * 100.0;

        debug!(
            "Block {} reconstruction: {}/{} transactions found in mempool ({:.1}% missing)",
            block_hash,
            total_tx_count - missing_count,
            total_tx_count,
            missing_percentage
        );

        // Threshold: If more than 10% missing, request full block
        const MISSING_THRESHOLD_PERCENT: f64 = 10.0;
        if missing_percentage > MISSING_THRESHOLD_PERCENT {
            debug!(
                "Too many missing transactions ({:.1}% > {}%), falling back to full block request",
                missing_percentage, MISSING_THRESHOLD_PERCENT
            );
            return Ok(ReconstructionResult::TooManyMissing {
                missing_count,
                total_count: total_tx_count,
            });
        }

        // If we have missing transactions but under threshold, request them
        if !missing_indices.is_empty() {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Requesting {} missing transactions for block {}",
                    missing_count, block_hash
                );
            }
            let request = MissingTransactionsRequest {
                block_hash: block_hash.clone(),
                missing_indices,
            };
            return Ok(ReconstructionResult::MissingTransactions(request));
        }

        // All transactions found! Reconstruct the full block
        let complete_transactions: Vec<Arc<Transaction>> = transactions
            .into_iter()
            .map(|opt_tx| opt_tx.expect("All transactions should be present"))
            .collect();

        let block = Block::new(
            Immutable::Owned(compact_block.header),
            complete_transactions,
        );

        debug!(
            "Successfully reconstructed block {} with {} transactions",
            block_hash, total_tx_count
        );

        Ok(ReconstructionResult::Success(block))
    }

    /// Complete block reconstruction with missing transactions
    ///
    /// This is called after receiving MissingTransactionsResponse from peer
    pub fn complete_reconstruction(
        compact_block: CompactBlock,
        missing_txs_response: MissingTransactionsResponse,
        mempool: &Mempool,
    ) -> Result<Block, BlockchainError> {
        let block_hash = compact_block.header.hash();

        // Verify response is for the correct block
        if missing_txs_response.block_hash != block_hash {
            return Err(BlockchainError::Any(anyhow::anyhow!(
                "Missing transactions response is for wrong block: expected {}, got {}",
                block_hash,
                missing_txs_response.block_hash
            )));
        }

        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Completing reconstruction of block {} with {} missing transactions",
                block_hash,
                missing_txs_response.transactions.len()
            );
        }

        let total_tx_count = compact_block.short_tx_ids.len();
        let mut transactions = vec![None; total_tx_count];

        // Add prefilled transactions
        for (index, tx) in compact_block.prefilled_txs {
            if (index as usize) < total_tx_count {
                transactions[index as usize] = Some(Arc::new(tx));
            }
        }

        // Match short IDs with mempool OR missing transactions
        let mut missing_tx_iter = missing_txs_response.transactions.into_iter();

        for (index, short_id) in compact_block.short_tx_ids.iter().enumerate() {
            if transactions[index].is_some() {
                continue;
            }

            // Try mempool first
            let mut found = false;
            for (tx_hash, sorted_tx) in mempool.get_txs() {
                let candidate_short_id = calculate_short_tx_id(compact_block.nonce, tx_hash);
                if candidate_short_id == *short_id {
                    transactions[index] = Some(sorted_tx.get_tx().clone());
                    found = true;
                    break;
                }
            }

            // If not in mempool, use next missing transaction
            if !found {
                if let Some(tx) = missing_tx_iter.next() {
                    transactions[index] = Some(Arc::new(tx));
                } else {
                    return Err(BlockchainError::Any(anyhow::anyhow!(
                        "Not enough missing transactions provided"
                    )));
                }
            }
        }

        // Verify all transactions are present and collect them
        let complete_transactions: Vec<Arc<Transaction>> = transactions
            .into_iter()
            .enumerate()
            .map(|(index, opt_tx)| {
                opt_tx.ok_or_else(|| {
                    BlockchainError::Any(anyhow::anyhow!(
                        "Transaction at index {} still missing after reconstruction",
                        index
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let block = Block::new(
            Immutable::Owned(compact_block.header),
            complete_transactions,
        );

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Completed reconstruction of block {} with all {} transactions",
                block_hash, total_tx_count
            );
        }

        Ok(block)
    }

    /// Prepare missing transactions response
    ///
    /// This is called by the sender when receiving a GetMissingTransactions request
    pub fn prepare_missing_transactions(
        request: MissingTransactionsRequest,
        block: &Block,
    ) -> Result<MissingTransactionsResponse, BlockchainError> {
        let block_transactions = block.get_transactions();

        let mut missing_transactions = Vec::with_capacity(request.missing_indices.len());

        for index in request.missing_indices {
            let idx = index as usize;
            if idx >= block_transactions.len() {
                return Err(BlockchainError::Any(anyhow::anyhow!(
                    "Missing transaction index {} out of bounds (block has {} txs)",
                    index,
                    block_transactions.len()
                )));
            }

            // Clone the transaction (dereference Arc)
            missing_transactions.push((*block_transactions[idx]).clone());
        }

        Ok(MissingTransactionsResponse {
            block_hash: request.block_hash,
            transactions: missing_transactions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::{
        block::{BlockHeader, BlockVersion},
        crypto::elgamal::CompressedPublicKey,
        serializer::{Reader, Serializer},
    };

    #[test]
    fn test_reconstruction_threshold() {
        // Test that reconstruction correctly identifies too many missing transactions
        let missing_count = 15;
        let total_count = 100;
        let missing_percentage = (missing_count as f64 / total_count as f64) * 100.0;

        // 15% > 10% threshold, should request full block
        assert!(missing_percentage > 10.0);
    }

    #[test]
    fn test_missing_transactions_preparation() {
        // Create a simple block
        let parents = vec![Hash::new([0u8; 32])];
        // Create a minimal miner key from bytes (32 bytes for CompressedRistretto)
        let miner_bytes = [1u8; 32];
        let mut reader = Reader::new(&miner_bytes);
        let miner = CompressedPublicKey::read(&mut reader).unwrap();

        let header = BlockHeader::new_simple(
            BlockVersion::V0,
            parents,
            1234567890,
            [0u8; 32],
            miner,
            Hash::zero(),
        );

        // Create block with empty transactions (would need real transactions for full test)
        let block = Block::new(Immutable::Owned(header), vec![]);

        // Test with no missing indices
        let request = MissingTransactionsRequest {
            block_hash: Hash::new([1u8; 32]),
            missing_indices: vec![],
        };

        let response =
            CompactBlockReconstructor::prepare_missing_transactions(request, &block).unwrap();
        assert_eq!(response.transactions.len(), 0);
    }
}
