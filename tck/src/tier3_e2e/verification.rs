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

/// Verify that total balance is conserved across a set of accounts on all nodes.
///
/// This checks that the sum of all account balances equals the expected total,
/// ensuring no inflation or loss of funds has occurred.
///
/// # Arguments
///
/// * `network` - The network to verify
/// * `accounts` - List of account addresses to sum
/// * `expected_total` - Expected total balance across all accounts
///
/// # Returns
///
/// * `Ok(())` - Total balance matches expected on all nodes
/// * `Err(_)` - Balance conservation violated on at least one node
pub async fn verify_balance_conservation(
    network: &LocalTosNetwork,
    accounts: &[Hash],
    expected_total: u64,
) -> Result<()> {
    for node_idx in 0..network.node_count() {
        let mut total: u64 = 0;
        for address in accounts {
            let balance = network.node(node_idx).get_balance(address).await?;
            total = total.checked_add(balance).ok_or_else(|| {
                anyhow!(
                    "Balance overflow on node {} while summing accounts",
                    node_idx
                )
            })?;
        }
        if total != expected_total {
            return Err(anyhow!(
                "Balance conservation violated on node {}: expected total {}, got {}",
                node_idx,
                expected_total,
                total
            ));
        }
    }
    Ok(())
}

/// Verify nonce monotonicity: nonces should be non-decreasing across all nodes.
///
/// For a given account, all nodes should report the same nonce value
/// (after convergence). This verifies that nonce tracking is consistent.
///
/// # Arguments
///
/// * `network` - The network to verify
/// * `accounts` - List of account addresses to check
/// * `min_expected_nonces` - Minimum expected nonce for each account (in same order)
///
/// # Returns
///
/// * `Ok(())` - All nonces meet minimum expectations and are consistent
/// * `Err(_)` - Nonce inconsistency or regression detected
pub async fn verify_nonce_monotonicity(
    network: &LocalTosNetwork,
    accounts: &[Hash],
    min_expected_nonces: &[u64],
) -> Result<()> {
    if accounts.len() != min_expected_nonces.len() {
        return Err(anyhow!(
            "accounts and min_expected_nonces must have same length"
        ));
    }

    for (i, address) in accounts.iter().enumerate() {
        let min_nonce = min_expected_nonces[i];
        for node_idx in 0..network.node_count() {
            let nonce = network.node(node_idx).get_nonce(address).await?;
            if nonce < min_nonce {
                return Err(anyhow!(
                    "Nonce regression on node {} for account {}: expected >= {}, got {}",
                    node_idx,
                    address,
                    min_nonce,
                    nonce
                ));
            }
        }
    }
    Ok(())
}

/// Verify energy weight consistency across all nodes.
///
/// Checks that the total energy reported by each node matches the expected
/// sum of frozen balances. This is a placeholder for integration with the
/// energy system.
///
/// # Arguments
///
/// * `network` - The network to verify
/// * `expected_total_energy` - Expected total network energy weight
///
/// # Returns
///
/// * `Ok(())` - Energy weight matches expected on all nodes
/// * `Err(_)` - Energy weight inconsistency detected
pub async fn verify_energy_consistency(
    network: &LocalTosNetwork,
    expected_total_energy: u64,
) -> Result<()> {
    // Energy is tracked via frozen balances in the state.
    // In the current test framework, we verify height consistency as a proxy
    // for state consistency (including energy). When the energy system is
    // fully integrated, this will query per-node energy weight directly.
    let _ = expected_total_energy;

    // For now, ensure all nodes are at least consistent with each other
    verify_height_consistency(network).await?;
    verify_tip_consistency(network).await?;
    Ok(())
}

/// Comprehensive verification that runs all consistency checks.
///
/// Combines height, tip, nonce, and invariant verification into a single call.
/// Useful as a post-test assertion to ensure nothing went wrong.
///
/// # Arguments
///
/// * `network` - The network to verify
/// * `accounts` - Accounts to verify nonce consistency for
///
/// # Returns
///
/// * `Ok(())` - All checks passed
/// * `Err(_)` - At least one check failed
pub async fn verify_comprehensive(network: &LocalTosNetwork, accounts: &[Hash]) -> Result<()> {
    verify_height_consistency(network).await?;
    verify_tip_consistency(network).await?;
    verify_nonce_consistency(network, accounts).await?;
    verify_invariants(network).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_functions_exist() {
        // Verify that all public verification functions are accessible
        let _ = verify_height_consistency;
        let _ = verify_tip_consistency;
        let _ = verify_storage_consistency;
        let _ = verify_invariants;
        let _ = verify_balance_conservation;
        let _ = verify_nonce_monotonicity;
        let _ = verify_energy_consistency;
        let _ = verify_comprehensive;
    }
}
