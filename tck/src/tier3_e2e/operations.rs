//! Cluster-level operations for multi-node testing.
//!
//! Provides high-level operations that coordinate across multiple nodes
//! in a test cluster, such as mining, transferring, and verifying
//! transactions across all nodes.

use anyhow::Result;
use std::time::Duration;
use tos_common::crypto::Hash;

use crate::tier1_component::TestTransaction;
use crate::tier2_integration::NodeRpc;

use super::network::LocalTosNetwork;

/// Create a test transaction for transfer operations.
///
/// Builds a `TestTransaction` with a deterministic hash derived from
/// the transfer parameters.
pub fn create_transfer_tx(
    sender: &Hash,
    recipient: &Hash,
    amount: u64,
    fee: u64,
    nonce: u64,
) -> TestTransaction {
    let mut hash_bytes = [0u8; 32];
    hash_bytes[0..8].copy_from_slice(&amount.to_le_bytes());
    hash_bytes[8..16].copy_from_slice(&fee.to_le_bytes());
    hash_bytes[16..24].copy_from_slice(&nonce.to_le_bytes());
    // Mix in sender bytes for uniqueness
    for (i, b) in sender.as_bytes().iter().enumerate().take(8) {
        hash_bytes[24 + (i % 8)] ^= b;
    }
    let hash = Hash::new(hash_bytes);

    TestTransaction {
        hash,
        sender: sender.clone(),
        recipient: recipient.clone(),
        amount,
        fee,
        nonce,
    }
}

/// Mine blocks on the specified node and wait for propagation.
///
/// Mines `count` blocks on the miner node and waits for all other nodes
/// to receive and validate the blocks.
pub async fn mine_and_propagate(
    network: &LocalTosNetwork,
    miner_index: usize,
    count: u64,
    propagation_timeout: Duration,
) -> Result<Vec<Hash>> {
    let mut block_hashes = Vec::with_capacity(count as usize);

    for _ in 0..count {
        let hash = network.mine_and_propagate(miner_index).await?;
        block_hashes.push(hash);
    }

    // Wait for all nodes to converge on same height
    network
        .wait_for_height_convergence(propagation_timeout)
        .await?;

    Ok(block_hashes)
}

/// Send a transfer transaction and verify it on all nodes.
///
/// Submits the transaction to the specified node, mines a block,
/// and then verifies the balance changes are reflected on all nodes.
pub async fn send_and_verify_transfer(
    network: &LocalTosNetwork,
    submit_node: usize,
    from: &Hash,
    to: &Hash,
    amount: u64,
    timeout: Duration,
) -> Result<()> {
    let initial_sender_balance = network.node(submit_node).get_balance(from).await?;
    let initial_receiver_balance = network.node(submit_node).get_balance(to).await?;

    // Get nonce for the sender and create transaction
    let nonce = network.node(submit_node).get_nonce(from).await?;
    let tx = create_transfer_tx(from, to, amount, 1, nonce.saturating_add(1));

    // Submit, propagate, and mine
    network.submit_and_propagate(submit_node, tx).await?;
    network.mine_and_propagate(submit_node).await?;

    // Wait for propagation
    network.wait_for_height_convergence(timeout).await?;

    // Verify on all nodes
    let node_count = network.node_count();
    for i in 0..node_count {
        let sender_balance = network.node(i).get_balance(from).await?;
        let receiver_balance = network.node(i).get_balance(to).await?;

        if sender_balance >= initial_sender_balance {
            anyhow::bail!(
                "Node {}: sender balance did not decrease (was {}, now {})",
                i,
                initial_sender_balance,
                sender_balance
            );
        }

        let expected_receiver = initial_receiver_balance.saturating_add(amount);
        if receiver_balance != expected_receiver {
            anyhow::bail!(
                "Node {}: receiver balance mismatch (expected {}, got {})",
                i,
                expected_receiver,
                receiver_balance
            );
        }
    }

    Ok(())
}

/// Verify all nodes have consistent state at the current height.
pub async fn verify_cluster_consistency(network: &LocalTosNetwork) -> Result<()> {
    let node_count = network.node_count();
    if node_count < 2 {
        return Ok(());
    }

    // Check heights are consistent
    let reference_height = network.node(0).get_tip_height().await?;
    for i in 1..node_count {
        let height = network.node(i).get_tip_height().await?;
        if height != reference_height {
            anyhow::bail!(
                "Height mismatch: node 0 has {}, node {} has {}",
                reference_height,
                i,
                height
            );
        }
    }

    // Check tips are consistent
    let reference_tips = network.node(0).get_tips().await?;
    for i in 1..node_count {
        let tips = network.node(i).get_tips().await?;
        if tips != reference_tips {
            anyhow::bail!(
                "Tips mismatch: node 0 has {:?}, node {} has {:?}",
                reference_tips,
                i,
                tips
            );
        }
    }

    Ok(())
}

/// Run a sequence of transactions and verify consistent state across all nodes.
pub async fn run_transaction_sequence(
    network: &LocalTosNetwork,
    miner_node: usize,
    transactions: Vec<(Hash, Hash, u64)>,
    timeout: Duration,
) -> Result<()> {
    for (i, (from, to, amount)) in transactions.iter().enumerate() {
        let nonce = network.node(miner_node).get_nonce(from).await?;
        let tx = create_transfer_tx(
            from,
            to,
            *amount,
            1,
            nonce.saturating_add(1).saturating_add(i as u64),
        );
        network.submit_and_propagate(miner_node, tx).await?;
    }

    // Mine a block with all transactions
    network.mine_and_propagate(miner_node).await?;

    // Wait for propagation
    network.wait_for_height_convergence(timeout).await?;

    // Verify consistency
    verify_cluster_consistency(network).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_transfer_tx() {
        let sender = Hash::new([1u8; 32]);
        let recipient = Hash::new([2u8; 32]);
        let tx = create_transfer_tx(&sender, &recipient, 100, 1, 1);

        assert_eq!(tx.sender, sender);
        assert_eq!(tx.recipient, recipient);
        assert_eq!(tx.amount, 100);
        assert_eq!(tx.fee, 1);
        assert_eq!(tx.nonce, 1);
    }

    #[test]
    fn test_create_transfer_tx_unique_hashes() {
        let sender = Hash::new([1u8; 32]);
        let recipient = Hash::new([2u8; 32]);
        let tx1 = create_transfer_tx(&sender, &recipient, 100, 1, 1);
        let tx2 = create_transfer_tx(&sender, &recipient, 100, 1, 2);

        // Different nonces should produce different hashes
        assert_ne!(tx1.hash, tx2.hash);
    }
}
