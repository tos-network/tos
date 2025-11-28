//! RPC assertion helpers for integration testing
//!
//! This module provides assertion utilities for verifying node state via RPC interface.
//! These helpers make tests more readable and provide better error messages.

use crate::tier2_integration::NodeRpc;
use anyhow::{Context, Result};
use tos_common::crypto::Hash;

/// Assert that node is at expected tip height
///
/// # Arguments
///
/// * `node` - Node implementing NodeRpc
/// * `expected_height` - Expected tip height
///
/// # Errors
///
/// Returns an error if:
/// - RPC call fails
/// - Actual height doesn't match expected
///
/// # Example
///
/// ```rust,ignore
/// assert_tip_height(&daemon, 10).await?;
/// ```
pub async fn assert_tip_height<N: NodeRpc>(node: &N, expected_height: u64) -> Result<()> {
    let actual = node
        .get_tip_height()
        .await
        .context("Failed to get tip height")?;

    if actual != expected_height {
        anyhow::bail!(
            "Tip height mismatch: expected {}, got {}",
            expected_height,
            actual
        );
    }

    Ok(())
}

/// Assert that account has expected balance
///
/// # Arguments
///
/// * `node` - Node implementing NodeRpc
/// * `address` - Account address to check
/// * `expected_balance` - Expected balance in nanoTOS
///
/// # Errors
///
/// Returns an error if:
/// - RPC call fails
/// - Actual balance doesn't match expected
///
/// # Example
///
/// ```rust,ignore
/// let alice = create_test_address(1);
/// assert_balance(&daemon, &alice, 1_000_000).await?;
/// ```
pub async fn assert_balance<N: NodeRpc>(
    node: &N,
    address: &Hash,
    expected_balance: u64,
) -> Result<()> {
    let actual = node
        .get_balance(address)
        .await
        .with_context(|| format!("Failed to get balance for address {}", address))?;

    if actual != expected_balance {
        anyhow::bail!(
            "Balance mismatch for address {}: expected {}, got {}",
            address,
            expected_balance,
            actual
        );
    }

    Ok(())
}

/// Assert that account balance is within tolerance
///
/// Useful for testing scenarios where exact balance may vary slightly
/// (e.g., due to fees, rounding, or non-deterministic execution order).
///
/// # Arguments
///
/// * `node` - Node implementing NodeRpc
/// * `address` - Account address to check
/// * `expected_balance` - Expected balance in nanoTOS
/// * `tolerance` - Acceptable difference (±)
///
/// # Errors
///
/// Returns an error if:
/// - RPC call fails
/// - Actual balance is outside tolerance range
///
/// # Example
///
/// ```rust,ignore
/// // Allow ±100 nanoTOS difference
/// assert_balance_within(&daemon, &alice, 1_000_000, 100).await?;
/// ```
pub async fn assert_balance_within<N: NodeRpc>(
    node: &N,
    address: &Hash,
    expected_balance: u64,
    tolerance: u64,
) -> Result<()> {
    let actual = node
        .get_balance(address)
        .await
        .with_context(|| format!("Failed to get balance for address {}", address))?;

    let min = expected_balance.saturating_sub(tolerance);
    let max = expected_balance.saturating_add(tolerance);

    if actual < min || actual > max {
        anyhow::bail!(
            "Balance for address {} outside tolerance: expected {} ± {}, got {} (range: {}-{})",
            address,
            expected_balance,
            tolerance,
            actual,
            min,
            max
        );
    }

    Ok(())
}

/// Assert that account balance is greater than or equal to threshold
///
/// # Arguments
///
/// * `node` - Node implementing NodeRpc
/// * `address` - Account address to check
/// * `min_balance` - Minimum expected balance
///
/// # Errors
///
/// Returns an error if balance is less than threshold
///
/// # Example
///
/// ```rust,ignore
/// assert_balance_gte(&daemon, &alice, 1_000_000).await?;
/// ```
pub async fn assert_balance_gte<N: NodeRpc>(
    node: &N,
    address: &Hash,
    min_balance: u64,
) -> Result<()> {
    let actual = node
        .get_balance(address)
        .await
        .with_context(|| format!("Failed to get balance for address {}", address))?;

    if actual < min_balance {
        anyhow::bail!(
            "Balance for address {} too low: expected >={}, got {}",
            address,
            min_balance,
            actual
        );
    }

    Ok(())
}

/// Assert that account balance is less than or equal to threshold
///
/// # Arguments
///
/// * `node` - Node implementing NodeRpc
/// * `address` - Account address to check
/// * `max_balance` - Maximum expected balance
///
/// # Errors
///
/// Returns an error if balance exceeds threshold
///
/// # Example
///
/// ```rust,ignore
/// assert_balance_lte(&daemon, &alice, 1_000_000).await?;
/// ```
pub async fn assert_balance_lte<N: NodeRpc>(
    node: &N,
    address: &Hash,
    max_balance: u64,
) -> Result<()> {
    let actual = node
        .get_balance(address)
        .await
        .with_context(|| format!("Failed to get balance for address {}", address))?;

    if actual > max_balance {
        anyhow::bail!(
            "Balance for address {} too high: expected <={}, got {}",
            address,
            max_balance,
            actual
        );
    }

    Ok(())
}

/// Assert that account has expected nonce
///
/// # Arguments
///
/// * `node` - Node implementing NodeRpc
/// * `address` - Account address to check
/// * `expected_nonce` - Expected nonce value
///
/// # Errors
///
/// Returns an error if:
/// - RPC call fails
/// - Actual nonce doesn't match expected
///
/// # Example
///
/// ```rust,ignore
/// let alice = create_test_address(1);
/// assert_nonce(&daemon, &alice, 5).await?;
/// ```
pub async fn assert_nonce<N: NodeRpc>(node: &N, address: &Hash, expected_nonce: u64) -> Result<()> {
    let actual = node
        .get_nonce(address)
        .await
        .with_context(|| format!("Failed to get nonce for address {}", address))?;

    if actual != expected_nonce {
        anyhow::bail!(
            "Nonce mismatch for address {}: expected {}, got {}",
            address,
            expected_nonce,
            actual
        );
    }

    Ok(())
}

/// Assert that DAG has expected number of tips
///
/// # Arguments
///
/// * `node` - Node implementing NodeRpc
/// * `expected_count` - Expected number of tips
///
/// # Errors
///
/// Returns an error if:
/// - RPC call fails
/// - Actual tip count doesn't match expected
///
/// # Example
///
/// ```rust,ignore
/// // Genesis should have 1 tip
/// assert_tip_count(&daemon, 1).await?;
///
/// // After parallel mining might have multiple tips
/// assert_tip_count(&daemon, 3).await?;
/// ```
pub async fn assert_tip_count<N: NodeRpc>(node: &N, expected_count: usize) -> Result<()> {
    let tips = node.get_tips().await.context("Failed to get tips")?;

    if tips.len() != expected_count {
        anyhow::bail!(
            "Tip count mismatch: expected {}, got {}",
            expected_count,
            tips.len()
        );
    }

    Ok(())
}

/// Assert that specific block hash is in the current tips
///
/// # Arguments
///
/// * `node` - Node implementing NodeRpc
/// * `expected_tip` - Block hash that should be a tip
///
/// # Errors
///
/// Returns an error if:
/// - RPC call fails
/// - Expected hash is not in current tips
///
/// # Example
///
/// ```rust,ignore
/// let block_hash = daemon.mine_block().await?;
/// assert_is_tip(&daemon, &block_hash).await?;
/// ```
pub async fn assert_is_tip<N: NodeRpc>(node: &N, expected_tip: &Hash) -> Result<()> {
    let tips = node.get_tips().await.context("Failed to get tips")?;

    if !tips.contains(expected_tip) {
        anyhow::bail!(
            "Block {} is not in current tips. Current tips: {:?}",
            expected_tip,
            tips
        );
    }

    Ok(())
}

/// Assert that specific block hash is NOT in the current tips
///
/// Useful for testing DAG reorganization scenarios.
///
/// # Arguments
///
/// * `node` - Node implementing NodeRpc
/// * `block_hash` - Block hash that should not be a tip
///
/// # Errors
///
/// Returns an error if:
/// - RPC call fails
/// - Hash is found in current tips
///
/// # Example
///
/// ```rust,ignore
/// let old_tip = daemon.get_tips().await?[0];
/// daemon.mine_block().await?;
/// // Old tip should no longer be a tip (unless parallel mining)
/// assert_not_tip(&daemon, &old_tip).await?;
/// ```
pub async fn assert_not_tip<N: NodeRpc>(node: &N, block_hash: &Hash) -> Result<()> {
    let tips = node.get_tips().await.context("Failed to get tips")?;

    if tips.contains(block_hash) {
        anyhow::bail!("Block {} is unexpectedly in current tips", block_hash);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_methods)]

    use super::*;
    use crate::orchestrator::SystemClock;
    use crate::tier1_component::TestBlockchainBuilder;
    use crate::tier2_integration::TestDaemon;
    use std::sync::Arc;

    fn create_test_address(id: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        Hash::new(bytes)
    }

    async fn create_test_daemon() -> TestDaemon {
        let clock = Arc::new(SystemClock);
        let alice = create_test_address(1);

        let blockchain = TestBlockchainBuilder::new()
            .with_clock(clock.clone())
            .with_funded_account(alice, 1_000_000)
            .build()
            .await
            .unwrap();

        TestDaemon::new(blockchain, clock)
    }

    #[tokio::test]
    async fn test_assert_tip_height_success() {
        let daemon = create_test_daemon().await;

        // Genesis should be at height 0
        assert_tip_height(&daemon, 0).await.unwrap();
    }

    #[tokio::test]
    async fn test_assert_tip_height_failure() {
        let daemon = create_test_daemon().await;

        // Should fail - genesis is at height 0, not 1
        let result = assert_tip_height(&daemon, 1).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Tip height mismatch"));
    }

    #[tokio::test]
    async fn test_assert_balance_success() {
        let daemon = create_test_daemon().await;
        let alice = create_test_address(1);

        assert_balance(&daemon, &alice, 1_000_000).await.unwrap();
    }

    #[tokio::test]
    async fn test_assert_balance_failure() {
        let daemon = create_test_daemon().await;
        let alice = create_test_address(1);

        let result = assert_balance(&daemon, &alice, 999_999).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Balance mismatch"));
    }

    #[tokio::test]
    async fn test_assert_balance_within_success() {
        let daemon = create_test_daemon().await;
        let alice = create_test_address(1);

        // Exact match
        assert_balance_within(&daemon, &alice, 1_000_000, 0)
            .await
            .unwrap();

        // Within tolerance
        assert_balance_within(&daemon, &alice, 1_000_100, 200)
            .await
            .unwrap();
        assert_balance_within(&daemon, &alice, 999_900, 200)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_assert_balance_within_failure() {
        let daemon = create_test_daemon().await;
        let alice = create_test_address(1);

        // Outside tolerance
        let result = assert_balance_within(&daemon, &alice, 1_000_000, 0).await;
        assert!(result.is_ok());

        let result = assert_balance_within(&daemon, &alice, 1_001_000, 100).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("outside tolerance"));
    }

    #[tokio::test]
    async fn test_assert_nonce_success() {
        let daemon = create_test_daemon().await;
        let alice = create_test_address(1);

        // New account should have nonce 0
        assert_nonce(&daemon, &alice, 0).await.unwrap();
    }

    #[tokio::test]
    async fn test_assert_nonce_failure() {
        let daemon = create_test_daemon().await;
        let alice = create_test_address(1);

        let result = assert_nonce(&daemon, &alice, 1).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Nonce mismatch"));
    }

    #[tokio::test]
    async fn test_assert_tip_count() {
        let daemon = create_test_daemon().await;

        // Genesis should have 1 tip
        assert_tip_count(&daemon, 1).await.unwrap();

        // Wrong count should fail
        let result = assert_tip_count(&daemon, 2).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_assert_is_tip() {
        let daemon = create_test_daemon().await;

        let tips = daemon.get_tips().await.unwrap();
        let genesis = tips[0].clone();

        // Genesis should be a tip
        assert_is_tip(&daemon, &genesis).await.unwrap();

        // Random hash should not be a tip
        let random = create_test_address(0xFF);
        let result = assert_is_tip(&daemon, &random).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_assert_not_tip() {
        let daemon = create_test_daemon().await;

        // Random hash should not be a tip
        let random = create_test_address(0xFF);
        assert_not_tip(&daemon, &random).await.unwrap();

        // Genesis IS a tip, so this should fail
        let tips = daemon.get_tips().await.unwrap();
        let genesis = tips[0].clone();
        let result = assert_not_tip(&daemon, &genesis).await;
        assert!(result.is_err());
    }
}
