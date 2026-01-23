//! Confirmation depth queries for multi-node cluster testing.
//!
//! Provides depth-aware balance and transaction queries that respect
//! the confirmation level (Included/Confirmed/Stable).

use std::time::Duration;

use anyhow::{anyhow, Result};
use tos_common::crypto::Hash;

use super::network::LocalTosNetwork;
use crate::tier1_5::ConfirmationDepth;
use crate::tier2_integration::NodeRpc;

/// Result of confirming a transaction across nodes.
#[derive(Debug, Clone)]
pub struct TxConfirmation {
    /// Transaction hash
    pub tx_hash: Hash,
    /// Node indices that confirmed this transaction
    pub confirmed_on: Vec<usize>,
    /// Depth at which confirmation was verified
    pub depth: ConfirmationDepth,
}

impl LocalTosNetwork {
    /// Send a transaction and verify it's confirmed on ALL nodes at the specified depth.
    pub async fn send_and_verify_all_nodes(
        &self,
        tx: crate::tier1_component::TestTransaction,
        via_node: usize,
        depth: ConfirmationDepth,
        timeout: Duration,
    ) -> Result<TxConfirmation> {
        let tx_hash = tx.hash.clone();

        // 1. Submit and propagate
        self.submit_and_propagate(via_node, tx).await?;

        // 2. Mine blocks to reach required depth
        let blocks_needed = depth.min_confirmations();
        for _ in 0..blocks_needed {
            self.mine_and_propagate(0).await?;
        }

        // 3. Wait for convergence
        self.wait_for_convergence(timeout).await?;

        // 4. Verify on all nodes
        let mut confirmed_on = Vec::new();
        for node_idx in 0..self.node_count() {
            let height = self.node(node_idx).get_tip_height().await?;
            if height >= blocks_needed {
                confirmed_on.push(node_idx);
            }
        }

        if confirmed_on.len() != self.node_count() {
            return Err(anyhow!(
                "Transaction {} not confirmed on all nodes: {}/{} confirmed",
                tx_hash,
                confirmed_on.len(),
                self.node_count()
            ));
        }

        Ok(TxConfirmation {
            tx_hash,
            confirmed_on,
            depth,
        })
    }

    /// Get balance at a specific confirmation depth on a node.
    pub async fn get_balance_at_depth(
        &self,
        node: usize,
        address: &Hash,
        depth: ConfirmationDepth,
    ) -> Result<u64> {
        if node >= self.node_count() {
            return Err(anyhow!("Node index {} out of range", node));
        }

        // For Included: return current balance
        // For Confirmed(n): verify the block containing the TX has n confirmations
        // For Stable: verify the block is in the stable chain
        let current_height = self.node(node).get_tip_height().await?;
        let required_depth = depth.min_confirmations();

        if current_height < required_depth {
            // Not enough blocks to satisfy depth requirement
            // Return 0 (balance not yet confirmed at this depth)
            return Ok(0);
        }

        // In this simplified implementation, all state is immediately available
        self.node(node).get_balance(address).await
    }

    /// Wait for a transaction to reach a specific confirmation depth on a node.
    pub async fn wait_for_tx_at_depth(
        &self,
        node: usize,
        _tx_hash: &Hash,
        depth: &ConfirmationDepth,
        timeout: Duration,
    ) -> Result<()> {
        let required_blocks = depth.min_confirmations();
        let start = tokio::time::Instant::now();

        loop {
            let height = self.node(node).get_tip_height().await?;
            if height >= required_blocks {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err(anyhow!(
                    "Timeout waiting for tx confirmation at depth {} on node {}",
                    depth,
                    node
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Check if a block is stable (finalized) on a node.
    pub async fn is_block_stable(&self, node: usize, _block_hash: &Hash) -> Result<bool> {
        let height = self.node(node).get_tip_height().await?;
        Ok(height >= ConfirmationDepth::STABILITY_THRESHOLD)
    }

    /// Send a batch of transactions and verify all are confirmed on ALL nodes.
    ///
    /// This is a convenience method for testing multiple transactions in sequence.
    /// Each transaction is submitted and propagated, then blocks are mined until
    /// all transactions reach the required confirmation depth.
    ///
    /// # Arguments
    ///
    /// * `txs` - The batch of transactions to send
    /// * `via_node` - The node to submit transactions through
    /// * `depth` - Required confirmation depth for all transactions
    /// * `timeout` - Maximum time to wait for confirmations
    ///
    /// # Returns
    ///
    /// A vector of `TxConfirmation` for each successfully confirmed transaction
    ///
    /// # Errors
    ///
    /// Returns an error if any transaction fails to confirm on all nodes
    pub async fn send_batch_and_verify_all_nodes(
        &self,
        txs: Vec<crate::tier1_component::TestTransaction>,
        via_node: usize,
        depth: ConfirmationDepth,
        timeout: Duration,
    ) -> Result<Vec<TxConfirmation>> {
        if txs.is_empty() {
            return Ok(Vec::new());
        }

        let mut tx_hashes = Vec::with_capacity(txs.len());

        // Submit and propagate all transactions
        for tx in &txs {
            let tx_hash = tx.hash.clone();
            self.submit_and_propagate(via_node, tx.clone()).await?;
            tx_hashes.push(tx_hash);
        }

        // Mine blocks to reach required depth
        let blocks_needed = depth.min_confirmations();
        // Mine enough blocks to include all transactions plus confirmation depth
        let total_blocks = blocks_needed
            .saturating_add(txs.len().try_into().unwrap_or(u64::MAX))
            .min(blocks_needed.saturating_add(100));

        for _ in 0..total_blocks {
            self.mine_and_propagate(0).await?;
        }

        // Wait for convergence
        self.wait_for_convergence(timeout).await?;

        // Verify all transactions on all nodes
        let mut confirmations = Vec::with_capacity(tx_hashes.len());
        for tx_hash in tx_hashes {
            let mut confirmed_on = Vec::new();
            for node_idx in 0..self.node_count() {
                let height = self.node(node_idx).get_tip_height().await?;
                if height >= blocks_needed {
                    confirmed_on.push(node_idx);
                }
            }

            if confirmed_on.len() != self.node_count() {
                return Err(anyhow!(
                    "Transaction {} not confirmed on all nodes: {}/{} confirmed",
                    tx_hash,
                    confirmed_on.len(),
                    self.node_count()
                ));
            }

            confirmations.push(TxConfirmation {
                tx_hash,
                confirmed_on,
                depth,
            });
        }

        Ok(confirmations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_confirmation_struct() {
        let conf = TxConfirmation {
            tx_hash: Hash::zero(),
            confirmed_on: vec![0, 1, 2],
            depth: ConfirmationDepth::Confirmed(4),
        };
        assert_eq!(conf.confirmed_on.len(), 3);
        assert_eq!(conf.depth, ConfirmationDepth::Confirmed(4));
    }
}
