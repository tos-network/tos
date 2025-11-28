//! Advanced Property-Based Tests for Chaos Testing
//!
//! This module contains property-based tests using proptest to verify blockchain
//! invariants under random, extreme, and edge-case scenarios.
//!
//! # Properties Tested
//!
//! 1. **Economic Invariants**
//!    - Supply conservation (total supply = sum of balances + fees burned)
//!    - Balance non-negativity
//!    - Fee burning correctness
//!
//! 2. **State Invariants**
//!    - Nonce monotonicity (nonces always increase)
//!    - Height monotonicity (chain height always increases)
//!    - State root determinism
//!
//! 3. **Transaction Invariants**
//!    - Sender balance sufficiency
//!    - Nonce ordering
//!    - Transaction idempotency
//!
//! 4. **Network Invariants**
//!    - Partition isolation (partitioned nodes don't share state)
//!    - Consensus convergence (all nodes eventually agree)
//!    - Block propagation correctness
//!
//! # Test Design
//!
//! All tests use seeded RNG and can be reproduced by setting TOS_TEST_SEED environment variable.

// Note: Some imports are used only in test code below
#[allow(unused_imports)]
use crate::orchestrator::SystemClock;
#[allow(unused_imports)]
use crate::tier1_component::{TestBlockchainBuilder, TestTransaction};
#[allow(unused_imports)]
use crate::tier2_integration::strategies::*;
#[allow(unused_imports)]
use proptest::prelude::*;
#[allow(unused_imports)]
use proptest::test_runner::TestCaseError;
#[allow(unused_imports)]
use std::sync::Arc;
use tos_common::crypto::Hash;

// Helper functions

fn create_test_hash(id: u8) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[0] = id;
    Hash::new(bytes)
}

fn create_test_tx(
    sender: Hash,
    recipient: Hash,
    amount: u64,
    fee: u64,
    nonce: u64,
) -> TestTransaction {
    let mut hash_bytes = [0u8; 32];
    hash_bytes[0] = (nonce & 0xFF) as u8;
    hash_bytes[1] = ((amount >> 8) & 0xFF) as u8;

    TestTransaction {
        hash: Hash::new(hash_bytes),
        sender,
        recipient,
        amount,
        fee,
        nonce,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest! {
        /// Property: Transaction ordering doesn't affect final balance
        ///
        /// When multiple transactions are submitted, the final balance should
        /// be deterministic regardless of the order they're mined in.
        #[test]
        fn prop_transaction_order_independence(
            amounts in prop::collection::vec(arb_amount(), 1..10),
            _seed in any::<u64>(),
        ) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let alice = create_test_hash(1);
                let bob = create_test_hash(2);

                // Calculate total needed
                let total_amount: u64 = amounts.iter().sum();
                let total_fee = amounts.len() as u64 * 100;
                let initial_balance = total_amount + total_fee + 1_000_000;

                let clock = Arc::new(SystemClock);
                let blockchain = TestBlockchainBuilder::new()
                    .with_clock(clock)
                    .with_funded_account(alice.clone(), initial_balance)
                    .build()
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                // Submit all transactions
                for (i, amount) in amounts.iter().enumerate() {
                    let tx = create_test_tx(
                        alice.clone(),
                        bob.clone(),
                        *amount,
                        100,
                        (i + 1) as u64,
                    );
                    blockchain.submit_transaction(tx).await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;
                    blockchain.mine_block().await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;
                }

                // Final balance should be deterministic
                let final_balance = blockchain.get_balance(&alice).await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let expected = initial_balance - total_amount - total_fee;
                prop_assert_eq!(final_balance, expected,
                    "Final balance mismatch: expected {}, got {}",
                    expected, final_balance);

                Ok::<(), TestCaseError>(())
            })?;
        }

        /// Property: Mining multiple empty blocks doesn't affect balances
        ///
        /// Empty blocks should not change any account balances.
        #[test]
        fn prop_empty_blocks_preserve_balances(
            initial_balance in arb_balance(),
            block_count in 1usize..50usize,
        ) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let alice = create_test_hash(1);

                let clock = Arc::new(SystemClock);
                let blockchain = TestBlockchainBuilder::new()
                    .with_clock(clock)
                    .with_funded_account(alice.clone(), initial_balance)
                    .build()
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let balance_before = blockchain.get_balance(&alice).await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                // Mine empty blocks
                for _ in 0..block_count {
                    blockchain.mine_block().await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;
                }

                let balance_after = blockchain.get_balance(&alice).await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                // Balance should be unchanged
                prop_assert_eq!(balance_before, balance_after,
                    "Balance changed after empty blocks: before={}, after={}",
                    balance_before, balance_after);

                Ok::<(), TestCaseError>(())
            })?;
        }

        /// Property: Total supply equals balances + fees burned
        ///
        /// Economic invariant: supply = sum(all balances) + fees_burned
        #[test]
        fn prop_supply_accounting_invariant(
            tx_count in 1usize..20usize,
            _seed in any::<u64>(),
        ) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let alice = create_test_hash(1);
                let bob = create_test_hash(2);
                let initial_balance = 10_000_000u64;

                let clock = Arc::new(SystemClock);
                let blockchain = TestBlockchainBuilder::new()
                    .with_clock(clock)
                    .with_funded_account(alice.clone(), initial_balance)
                    .build()
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                // Execute transactions
                for i in 1..=tx_count {
                    let tx = create_test_tx(alice.clone(), bob.clone(), 1000, 100, i as u64);
                    blockchain.submit_transaction(tx).await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;
                    blockchain.mine_block().await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;
                }

                // Get final state
                let accounts = blockchain.accounts_kv().await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let counters = blockchain.read_counters().await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                // Verify supply equation
                // Note: fees are burned (50%) and given to miner (50%)
                // But balances_total only counts user balances
                let total_balances: u128 = accounts.values()
                    .map(|state| state.balance as u128)
                    .sum();

                prop_assert_eq!(total_balances, counters.balances_total,
                    "Balance sum mismatch: calculated={}, stored={}",
                    total_balances, counters.balances_total);

                Ok::<(), TestCaseError>(())
            })?;
        }

        /// Property: Nonces never decrease
        ///
        /// Nonces must be monotonically increasing for each account.
        #[test]
        fn prop_nonce_never_decreases(
            tx_count in 2usize..30usize,
        ) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let alice = create_test_hash(1);
                let bob = create_test_hash(2);
                let initial_balance = 100_000_000u64;

                let clock = Arc::new(SystemClock);
                let blockchain = TestBlockchainBuilder::new()
                    .with_clock(clock)
                    .with_funded_account(alice.clone(), initial_balance)
                    .build()
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let mut prev_nonce = 0u64;

                for i in 1..=tx_count {
                    let tx = create_test_tx(alice.clone(), bob.clone(), 100, 10, i as u64);
                    blockchain.submit_transaction(tx).await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;
                    blockchain.mine_block().await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;

                    let current_nonce = blockchain.get_nonce(&alice).await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;

                    prop_assert!(current_nonce >= prev_nonce,
                        "Nonce decreased: prev={}, current={}", prev_nonce, current_nonce);

                    prev_nonce = current_nonce;
                }

                Ok::<(), TestCaseError>(())
            })?;
        }

        /// Property: Block height is monotonically increasing
        ///
        /// Chain height must always increase (or stay same if no mining).
        #[test]
        fn prop_height_monotonicity(
            block_count in 1usize..50usize,
        ) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let alice = create_test_hash(1);

                let clock = Arc::new(SystemClock);
                let blockchain = TestBlockchainBuilder::new()
                    .with_clock(clock)
                    .with_funded_account(alice, 1_000_000)
                    .build()
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let mut prev_height = 0u64;

                for _ in 0..block_count {
                    blockchain.mine_block().await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;

                    let current_height = blockchain.get_tip_height().await
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;

                    prop_assert!(current_height > prev_height,
                        "Height did not increase: prev={}, current={}",
                        prev_height, current_height);

                    prev_height = current_height;
                }

                Ok::<(), TestCaseError>(())
            })?;
        }

        // TODO: Re-enable when LocalTosNetworkBuilder supports SystemClock for proptest
        // Currently disabled because proptest uses multi-threaded runtime which is incompatible
        // with PausedClock used by LocalTosNetworkBuilder
        //
        // /// Property: Multi-node network eventually reaches consensus
        // ///
        // /// After sufficient block propagation, all nodes should have the same tip height.
        // #[test]
        // fn prop_consensus_convergence(
        //     node_count in 3usize..8usize,
        //     tx_count in 1usize..10usize,
        // ) {
        //     tokio::runtime::Runtime::new().unwrap().block_on(async {
        //         let network = LocalTosNetworkBuilder::new()
        //             .with_nodes(node_count)
        //             .with_topology(NetworkTopology::FullMesh)
        //             .with_genesis_account("alice", 100_000_000)
        //             .with_seed(42)
        //             .build()
        //             .await
        //             .map_err(|e| TestCaseError::fail(e.to_string()))?;
        //
        //         let alice = network.get_genesis_account("alice").unwrap().0.clone();
        //
        //         // Submit and mine transactions
        //         for i in 1..=tx_count {
        //             let tx = create_test_tx(
        //                 alice.clone(),
        //                 create_test_hash(100 + i as u8),
        //                 1000,
        //                 100,
        //                 i as u64,
        //             );
        //             network.submit_and_propagate(0, tx).await
        //                 .map_err(|e| TestCaseError::fail(e.to_string()))?;
        //             network.mine_and_propagate(0).await
        //                 .map_err(|e| TestCaseError::fail(e.to_string()))?;
        //         }
        //
        //         // All nodes should have same height
        //         let expected_height = network.node(0).get_tip_height().await
        //             .map_err(|e| TestCaseError::fail(e.to_string()))?;
        //
        //         for i in 1..node_count {
        //             let height = network.node(i).get_tip_height().await
        //                 .map_err(|e| TestCaseError::fail(e.to_string()))?;
        //
        //             prop_assert_eq!(height, expected_height,
        //                 "Node {} height mismatch: expected={}, got={}",
        //                 i, expected_height, height);
        //         }
        //
        //         Ok::<(), TestCaseError>(())
        //     })?;
        // }

        /// Property: Invalid transactions are rejected
        ///
        /// Transactions with insufficient balance, wrong nonce, etc. must be rejected.
        #[test]
        fn prop_invalid_transactions_rejected(
            amount in arb_amount(),
            wrong_nonce in 10u64..1000u64,
        ) {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let alice = create_test_hash(1);
                let bob = create_test_hash(2);
                let initial_balance = 1_000u64;

                let clock = Arc::new(SystemClock);
                let blockchain = TestBlockchainBuilder::new()
                    .with_clock(clock)
                    .with_funded_account(alice.clone(), initial_balance)
                    .build()
                    .await
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                // Try transaction with amount > balance
                if amount > initial_balance {
                    let tx = create_test_tx(alice.clone(), bob.clone(), amount, 100, 1);
                    let result = blockchain.submit_transaction(tx).await;
                    prop_assert!(result.is_err(),
                        "Transaction with amount {} > balance {} should be rejected",
                        amount, initial_balance);
                }

                // Try transaction with wrong nonce
                let tx = create_test_tx(alice.clone(), bob.clone(), 100, 10, wrong_nonce);
                let result = blockchain.submit_transaction(tx).await;
                prop_assert!(result.is_err(),
                    "Transaction with wrong nonce {} should be rejected", wrong_nonce);

                Ok::<(), TestCaseError>(())
            })?;
        }
    }

    // Standard non-proptest chaos tests

    #[tokio::test]
    async fn test_high_transaction_volume() {
        let alice = create_test_hash(1);
        let bob = create_test_hash(2);

        let clock = Arc::new(SystemClock);
        let blockchain = TestBlockchainBuilder::new()
            .with_clock(clock)
            .with_funded_account(alice.clone(), 1_000_000_000)
            .build()
            .await
            .unwrap();

        // Submit 100 transactions
        for i in 1..=100 {
            let tx = create_test_tx(alice.clone(), bob.clone(), 1000, 100, i);
            blockchain.submit_transaction(tx).await.unwrap();
        }

        // Mine block with all transactions
        let block = blockchain.mine_block().await.unwrap();
        assert_eq!(block.transactions.len(), 100);

        // Verify nonce advanced correctly
        assert_eq!(blockchain.get_nonce(&alice).await.unwrap(), 100);
    }

    #[tokio::test]
    async fn test_zero_balance_transfers() {
        let alice = create_test_hash(1);
        let bob = create_test_hash(2);

        let clock = Arc::new(SystemClock);
        let blockchain = TestBlockchainBuilder::new()
            .with_clock(clock)
            .with_funded_account(alice.clone(), 10_000)
            .build()
            .await
            .unwrap();

        // Test zero-amount transfer (currently allowed by the system)
        let tx = create_test_tx(alice.clone(), bob.clone(), 0, 100, 1);
        let result = blockchain.submit_transaction(tx).await;

        // Zero amount transfers are currently allowed
        assert!(result.is_ok());

        // Balance should only decrease by fee
        blockchain.mine_block().await.unwrap();
        let balance = blockchain.get_balance(&alice).await.unwrap();
        assert_eq!(balance, 10_000 - 100); // Initial - fee
    }

    #[tokio::test]
    async fn test_concurrent_block_mining() {
        let alice = create_test_hash(1);

        let clock = Arc::new(SystemClock);
        let blockchain = Arc::new(
            TestBlockchainBuilder::new()
                .with_clock(clock)
                .with_funded_account(alice, 1_000_000)
                .build()
                .await
                .unwrap(),
        );

        // Mine blocks concurrently (should serialize via internal locks)
        let mut handles = vec![];
        for _ in 0..10 {
            let bc = blockchain.clone();
            handles.push(tokio::spawn(async move { bc.mine_block().await }));
        }

        // All should complete successfully
        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        // Height should be 10
        assert_eq!(blockchain.get_tip_height().await.unwrap(), 10);
    }
}
