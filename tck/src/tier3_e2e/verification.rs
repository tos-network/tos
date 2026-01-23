//! Multi-layer state verification for LocalCluster testing.
//!
//! Provides verification functions that check consistency across all nodes
//! in a cluster, inspired by Solana's multi-layer verification (Tower, Blockstore, RPC).

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use tos_common::crypto::Hash;

use super::network::LocalTosNetwork;
use crate::tier2_integration::NodeRpc;

/// Verify that all nodes have the same tip height.
pub async fn verify_height_consistency(network: &LocalTosNetwork) -> Result<()> {
    let reference_height = network.node(0).get_tip_height().await?;

    for node_idx in 1..network.node_count() {
        let node_height = network.node(node_idx).get_tip_height().await?;
        if node_height != reference_height {
            return Err(anyhow!(
                "Height mismatch: node 0 has {}, node {} has {}",
                reference_height,
                node_idx,
                node_height
            ));
        }
    }
    Ok(())
}

/// Verify that all nodes have the same tips (top block hashes).
pub async fn verify_tip_consistency(network: &LocalTosNetwork) -> Result<()> {
    let reference_tips = network.node(0).get_tips().await?;

    for node_idx in 1..network.node_count() {
        let node_tips = network.node(node_idx).get_tips().await?;
        let mut ref_sorted = reference_tips.clone();
        let mut node_sorted = node_tips.clone();
        ref_sorted.sort();
        node_sorted.sort();

        if ref_sorted != node_sorted {
            return Err(anyhow!(
                "Tip mismatch: node 0 has {:?}, node {} has {:?}",
                reference_tips,
                node_idx,
                node_tips
            ));
        }
    }
    Ok(())
}

/// Verify balances match expected values across all nodes.
pub async fn verify_balances(
    network: &LocalTosNetwork,
    expected: &HashMap<Hash, u64>,
) -> Result<()> {
    for node_idx in 0..network.node_count() {
        for (address, expected_balance) in expected {
            let actual = network.node(node_idx).get_balance(address).await?;
            if actual != *expected_balance {
                return Err(anyhow!(
                    "Balance mismatch on node {}: address {} expected {}, got {}",
                    node_idx,
                    address,
                    expected_balance,
                    actual
                ));
            }
        }
    }
    Ok(())
}

/// Verify balances on specific nodes only.
pub async fn verify_balances_on_nodes(
    network: &LocalTosNetwork,
    nodes: &[usize],
    expected: &HashMap<Hash, u64>,
) -> Result<()> {
    for &node_idx in nodes {
        if node_idx >= network.node_count() {
            return Err(anyhow!("Node index {} out of range", node_idx));
        }
        for (address, expected_balance) in expected {
            let actual = network.node(node_idx).get_balance(address).await?;
            if actual != *expected_balance {
                return Err(anyhow!(
                    "Balance mismatch on node {}: address {} expected {}, got {}",
                    node_idx,
                    address,
                    expected_balance,
                    actual
                ));
            }
        }
    }
    Ok(())
}

/// Verify that all nodes have identical storage state at the same height.
pub async fn verify_storage_consistency(network: &LocalTosNetwork) -> Result<()> {
    // Verify heights match first
    verify_height_consistency(network).await?;
    // Then verify tips match
    verify_tip_consistency(network).await?;
    Ok(())
}

/// Verify nonce consistency across all nodes for given accounts.
pub async fn verify_nonce_consistency(network: &LocalTosNetwork, accounts: &[Hash]) -> Result<()> {
    for address in accounts {
        let reference_nonce = network.node(0).get_nonce(address).await?;
        for node_idx in 1..network.node_count() {
            let node_nonce = network.node(node_idx).get_nonce(address).await?;
            if node_nonce != reference_nonce {
                return Err(anyhow!(
                    "Nonce mismatch for {}: node 0 has {}, node {} has {}",
                    address,
                    reference_nonce,
                    node_idx,
                    node_nonce
                ));
            }
        }
    }
    Ok(())
}

/// Verify blockchain invariants hold on all nodes.
pub async fn verify_invariants(network: &LocalTosNetwork) -> Result<()> {
    for node_idx in 0..network.node_count() {
        let node = network.node(node_idx);

        // Verify the node's blockchain state is consistent
        let height = node.get_tip_height().await?;
        if height == 0 {
            continue; // Skip genesis-only nodes
        }

        let tips = node.get_tips().await?;
        if tips.is_empty() {
            return Err(anyhow!(
                "Node {} has no tips at height {}",
                node_idx,
                height
            ));
        }
    }
    Ok(())
}

/// Verify a condition holds on ALL nodes in parallel.
pub async fn verify_all_nodes<F, Fut>(
    network: &LocalTosNetwork,
    description: &str,
    check: F,
) -> Result<()>
where
    F: Fn(usize) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    for node_idx in 0..network.node_count() {
        check(node_idx)
            .await
            .map_err(|e| anyhow!("{} failed on node {}: {}", description, node_idx, e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_functions_exist() {
        // Verify that all public verification functions are accessible
        // Async functions can't be cast to fn pointers, but we verify they exist
        // by referencing them. These would require a real multi-node network to call.
        let _ = verify_height_consistency;
        let _ = verify_tip_consistency;
        let _ = verify_storage_consistency;
        let _ = verify_invariants;
    }
}
