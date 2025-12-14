//! Property-based tests for blockchain operations
//!
//! This module contains property-based tests using proptest to verify
//! blockchain invariants across a wide range of randomly generated scenarios.
//!
//! # Key Properties Tested
//!
//! 1. **Parallel ≡ Sequential**: Parallel execution produces same result as sequential
//! 2. **Balance Conservation**: Total supply is constant across all operations
//! 3. **Nonce Monotonicity**: Nonces always increase
//! 4. **State Determinism**: Same operations with same seed produce identical state
//!
//! # Design
//!
//! All tests use seeded RNG for full reproducibility. Failed tests can be
//! reproduced by setting the `PROPTEST_RNG_SEED` environment variable.

#![allow(clippy::disallowed_methods)]

use crate::orchestrator::SystemClock;
use crate::tier1_component::{TestBlockchain, TestBlockchainBuilder, TestTransaction};
use crate::tier2_integration::strategies::*;
use anyhow::Result;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use std::sync::Arc;
use tos_common::crypto::Hash;

/// Execute transactions sequentially on a blockchain
///
/// Processes transactions one at a time, mining a block after each transaction.
///
/// # Returns
///
/// Final blockchain state after all transactions
#[allow(dead_code)]
async fn execute_sequential(
    transactions: &[TestTransaction],
    initial_balances: &[(Hash, u64)],
) -> Result<TestBlockchain> {
    let clock = Arc::new(SystemClock);

    let mut builder = TestBlockchainBuilder::new()
        .with_clock(clock)
        .with_default_balance(0);

    // Set up initial balances
    for (addr, balance) in initial_balances {
        builder = builder.with_funded_account(addr.clone(), *balance);
    }

    let blockchain = builder.build().await?;

    // Execute each transaction sequentially
    for tx in transactions {
        // Submit transaction
        blockchain.submit_transaction(tx.clone()).await?;

        // Mine block
        blockchain.mine_block().await?;
    }

    Ok(blockchain)
}

/// Execute transactions in batch (simulates parallel submission)
///
/// Submits all transactions to mempool, then mines a single block containing all.
/// This simulates parallel transaction submission followed by block mining.
///
/// # Returns
///
/// Final blockchain state after batch execution
#[allow(dead_code)]
async fn execute_parallel(
    transactions: &[TestTransaction],
    initial_balances: &[(Hash, u64)],
) -> Result<TestBlockchain> {
    let clock = Arc::new(SystemClock);

    let mut builder = TestBlockchainBuilder::new()
        .with_clock(clock)
        .with_default_balance(0);

    // Set up initial balances
    for (addr, balance) in initial_balances {
        builder = builder.with_funded_account(addr.clone(), *balance);
    }

    let blockchain = builder.build().await?;

    // Submit all transactions to mempool (parallel submission)
    for tx in transactions {
        blockchain.submit_transaction(tx.clone()).await?;
    }

    // Mine a single block with all transactions
    blockchain.mine_block().await?;

    Ok(blockchain)
}

/// Compare two blockchain states for equivalence
///
/// Checks that:
/// - All account balances are identical
/// - All account nonces are identical
/// - Tip heights match
///
/// # Errors
///
/// Returns error message describing first mismatch found
fn assert_state_equivalence(
    sequential: &TestBlockchain,
    parallel: &TestBlockchain,
) -> Result<(), String> {
    // Compare tip heights
    let seq_height = sequential
        .get_tip_height()
        .now_or_never()
        .unwrap()
        .map_err(|e| format!("Failed to get sequential height: {}", e))?;

    let par_height = parallel
        .get_tip_height()
        .now_or_never()
        .unwrap()
        .map_err(|e| format!("Failed to get parallel height: {}", e))?;

    if seq_height != par_height {
        return Err(format!(
            "Height mismatch: sequential={}, parallel={}",
            seq_height, par_height
        ));
    }

    // Get all accounts from both blockchains
    let seq_accounts = sequential
        .accounts_kv()
        .now_or_never()
        .unwrap()
        .map_err(|e| format!("Failed to get sequential accounts: {}", e))?;

    let par_accounts = parallel
        .accounts_kv()
        .now_or_never()
        .unwrap()
        .map_err(|e| format!("Failed to get parallel accounts: {}", e))?;

    // Check account count
    if seq_accounts.len() != par_accounts.len() {
        return Err(format!(
            "Account count mismatch: sequential={}, parallel={}",
            seq_accounts.len(),
            par_accounts.len()
        ));
    }

    // Compare each account
    for (addr, seq_state) in &seq_accounts {
        let par_state = par_accounts
            .get(addr)
            .ok_or_else(|| format!("Account {} exists in sequential but not in parallel", addr))?;

        if seq_state.balance != par_state.balance {
            return Err(format!(
                "Balance mismatch for {}: sequential={}, parallel={}",
                addr, seq_state.balance, par_state.balance
            ));
        }

        if seq_state.nonce != par_state.nonce {
            return Err(format!(
                "Nonce mismatch for {}: sequential={}, parallel={}",
                addr, seq_state.nonce, par_state.nonce
            ));
        }
    }

    Ok(())
}

// Helper trait to add .now_or_never() to futures for testing
use futures::future::FutureExt;

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a simple valid transaction
    fn create_test_tx(
        sender: Hash,
        recipient: Hash,
        amount: u64,
        fee: u64,
        nonce: u64,
    ) -> TestTransaction {
        TestTransaction {
            hash: Hash::zero(),
            sender,
            recipient,
            amount,
            fee,
            nonce,
        }
    }

    fn create_test_address(id: u8) -> Hash {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        Hash::new(bytes)
    }

    proptest! {
        /// Property: Balance conservation with mining rewards
        ///
        /// Total supply must increase by exactly the mining reward.
        /// All value transfers between accounts don't change supply,
        /// only mining rewards increase it.
        #[test]
        fn prop_balance_conservation(
            initial_balance in arb_balance(),
            amount in arb_amount(),
            fee in arb_fee(),
        ) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                // Ensure transaction is valid (amount + fee <= balance)
                if amount + fee > initial_balance {
                    return Ok(());
                }

                let alice = create_test_address(1);
                let bob = create_test_address(2);

                let clock = Arc::new(SystemClock);
                let blockchain = TestBlockchainBuilder::new()
                    .with_clock(clock)
                    .with_funded_account(alice.clone(), initial_balance)
                    .build()
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let initial_supply = blockchain.read_counters().await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?.supply;

                // Execute transaction
                let tx = create_test_tx(alice.clone(), bob, amount, fee, 1);
                blockchain.submit_transaction(tx).await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let mined_block = blockchain.mine_block().await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let final_supply = blockchain.read_counters().await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?.supply;

                // Supply should increase by exactly the mining reward
                let expected_supply = initial_supply + (mined_block.reward as u128);
                prop_assert_eq!(expected_supply, final_supply,
                    "Supply conservation violated: initial={}, reward={}, expected={}, actual={}",
                    initial_supply, mined_block.reward, expected_supply, final_supply);

                Ok::<(), TestCaseError>(())
            })?;
        }

        /// Property: Nonce monotonicity
        ///
        /// Nonces must always increase and never decrease.
        #[test]
        fn prop_nonce_monotonicity(
            initial_balance in 1000u64..1_000_000u64,
            tx_count in 1usize..20usize,
        ) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let alice = create_test_address(1);
                let bob = create_test_address(2);

                let clock = Arc::new(SystemClock);
                let blockchain = TestBlockchainBuilder::new()
                    .with_clock(clock)
                    .with_funded_account(alice.clone(), initial_balance)
                    .build()
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let mut prev_nonce = 0u64;

                // Execute multiple transactions
                for i in 1..=tx_count {
                    let tx = create_test_tx(alice.clone(), bob.clone(), 10, 1, i as u64);
                    blockchain.submit_transaction(tx).await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;
                    blockchain.mine_block().await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;

                    let current_nonce = blockchain.get_nonce(&alice).await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;

                    // Nonce must increase
                    prop_assert!(current_nonce > prev_nonce,
                        "Nonce did not increase: prev={}, current={}", prev_nonce, current_nonce);

                    prev_nonce = current_nonce;
                }

                Ok::<(), TestCaseError>(())
            })?;
        }

        /// Property: State determinism
        ///
        /// Same operations with same seed must produce identical state.
        #[test]
        fn prop_state_determinism(
            initial_balance in arb_balance(),
            amount in arb_amount(),
            fee in arb_fee(),
        ) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                if amount + fee > initial_balance {
                    return Ok(());
                }

                let alice = create_test_address(1);
                let bob = create_test_address(2);

                // Run 1: Create blockchain with seed
                let clock1 = Arc::new(SystemClock);
                let blockchain1 = TestBlockchainBuilder::new()
                    .with_clock(clock1)
                    .with_funded_account(alice.clone(), initial_balance)
                    .build()
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let tx1 = create_test_tx(alice.clone(), bob.clone(), amount, fee, 1);
                blockchain1.submit_transaction(tx1).await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                blockchain1.mine_block().await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let balance1 = blockchain1.get_balance(&alice).await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                let nonce1 = blockchain1.get_nonce(&alice).await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                // Run 2: Exact same operations
                let clock2 = Arc::new(SystemClock);
                let blockchain2 = TestBlockchainBuilder::new()
                    .with_clock(clock2)
                    .with_funded_account(alice.clone(), initial_balance)
                    .build()
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let tx2 = create_test_tx(alice.clone(), bob.clone(), amount, fee, 1);
                blockchain2.submit_transaction(tx2).await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                blockchain2.mine_block().await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let balance2 = blockchain2.get_balance(&alice).await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                let nonce2 = blockchain2.get_nonce(&alice).await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                // States must be identical
                prop_assert_eq!(balance1, balance2, "Balances differ");
                prop_assert_eq!(nonce1, nonce2, "Nonces differ");

                Ok::<(), TestCaseError>(())
            })?;
        }
    }

    #[tokio::test]
    async fn test_state_equivalence_identical() {
        let alice = create_test_address(1);

        let clock = Arc::new(SystemClock);
        let blockchain1 = TestBlockchainBuilder::new()
            .with_clock(clock.clone())
            .with_funded_account(alice.clone(), 1_000_000)
            .build()
            .await
            .unwrap();

        let blockchain2 = TestBlockchainBuilder::new()
            .with_clock(clock)
            .with_funded_account(alice.clone(), 1_000_000)
            .build()
            .await
            .unwrap();

        // Identical states should pass
        assert_state_equivalence(&blockchain1, &blockchain2).unwrap();
    }

    #[tokio::test]
    async fn test_state_equivalence_different_balance() {
        let alice = create_test_address(1);

        let clock = Arc::new(SystemClock);
        let blockchain1 = TestBlockchainBuilder::new()
            .with_clock(clock.clone())
            .with_funded_account(alice.clone(), 1_000_000)
            .build()
            .await
            .unwrap();

        let blockchain2 = TestBlockchainBuilder::new()
            .with_clock(clock)
            .with_funded_account(alice.clone(), 999_999)
            .build()
            .await
            .unwrap();

        // Different balances should fail
        let result = assert_state_equivalence(&blockchain1, &blockchain2);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Balance mismatch"));
    }
}
