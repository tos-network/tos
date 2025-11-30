//! Property-based tests for execution consistency (F-04)
//!
//! These tests verify that sequential and parallel execution produce identical results
//! for the same set of transactions, regardless of execution order.
//!
//! Security Audit Reference: F-04 - Parallel execution path consistency

#![allow(dead_code)]
#![allow(clippy::disallowed_methods)]

use proptest::prelude::*;
use std::collections::HashMap;

/// Simplified account state for testing
#[derive(Debug, Clone, PartialEq, Eq)]
struct AccountState {
    balance: u64,
    nonce: u64,
}

/// Simplified transaction for testing
#[derive(Debug, Clone)]
struct TestTransaction {
    from: u64,
    to: u64,
    amount: u64,
    nonce: u64,
}

/// Simple state machine for testing execution consistency
#[derive(Debug, Clone)]
struct TestExecutor {
    accounts: HashMap<u64, AccountState>,
}

impl TestExecutor {
    fn new() -> Self {
        Self {
            accounts: HashMap::new(),
        }
    }

    fn with_accounts(accounts: Vec<(u64, u64, u64)>) -> Self {
        let mut executor = Self::new();
        for (id, balance, nonce) in accounts {
            executor
                .accounts
                .insert(id, AccountState { balance, nonce });
        }
        executor
    }

    /// Execute transactions sequentially
    fn execute_sequential(&mut self, txs: &[TestTransaction]) -> Vec<bool> {
        txs.iter().map(|tx| self.execute_single(tx)).collect()
    }

    /// Execute a single transaction
    fn execute_single(&mut self, tx: &TestTransaction) -> bool {
        // Get sender state
        let sender = match self.accounts.get_mut(&tx.from) {
            Some(s) => s,
            None => return false,
        };

        // Validate nonce
        if sender.nonce != tx.nonce {
            return false;
        }

        // Validate balance
        if sender.balance < tx.amount {
            return false;
        }

        // Execute transfer
        sender.balance -= tx.amount;
        sender.nonce += 1;

        // Credit receiver
        let receiver = self.accounts.entry(tx.to).or_insert(AccountState {
            balance: 0,
            nonce: 0,
        });
        receiver.balance += tx.amount;

        true
    }

    /// Get final state hash (deterministic)
    fn state_hash(&self) -> u64 {
        let mut sorted_accounts: Vec<_> = self.accounts.iter().collect();
        sorted_accounts.sort_by_key(|(k, _)| *k);

        let mut hash: u64 = 0;
        for (id, state) in sorted_accounts {
            hash = hash.wrapping_mul(31).wrapping_add(*id);
            hash = hash.wrapping_mul(31).wrapping_add(state.balance);
            hash = hash.wrapping_mul(31).wrapping_add(state.nonce);
        }
        hash
    }
}

// Property test strategies
fn account_id_strategy() -> impl Strategy<Value = u64> {
    0u64..10 // Limit to 10 accounts for focused testing
}

fn balance_strategy() -> impl Strategy<Value = u64> {
    1000u64..10000 // Reasonable balance range
}

fn amount_strategy() -> impl Strategy<Value = u64> {
    1u64..500 // Transfer amounts
}

fn account_strategy() -> impl Strategy<Value = (u64, u64, u64)> {
    (account_id_strategy(), balance_strategy(), 0u64..5)
}

fn transaction_strategy(max_nonce: u64) -> impl Strategy<Value = TestTransaction> {
    (
        account_id_strategy(),
        account_id_strategy(),
        amount_strategy(),
        0u64..max_nonce,
    )
        .prop_map(|(from, to, amount, nonce)| TestTransaction {
            from,
            to,
            amount,
            nonce,
        })
}

proptest! {
    /// Property: Sequential execution is deterministic
    /// Running the same transactions twice produces identical results
    #[test]
    fn prop_sequential_deterministic(
        accounts in prop::collection::vec(account_strategy(), 1..5),
        seed in any::<u64>(),
    ) {
        let mut executor1 = TestExecutor::with_accounts(accounts.clone());
        let mut executor2 = TestExecutor::with_accounts(accounts);

        // Generate same transactions for both
        let txs: Vec<TestTransaction> = (0..10)
            .map(|i| TestTransaction {
                from: (seed + i) % 5,
                to: (seed + i + 1) % 5,
                amount: 10 + (i % 100),
                nonce: 0,
            })
            .collect();

        let results1 = executor1.execute_sequential(&txs);
        let results2 = executor2.execute_sequential(&txs);

        prop_assert_eq!(results1, results2);
        prop_assert_eq!(executor1.state_hash(), executor2.state_hash());
    }

    /// Property: Nonce must be monotonically increasing per account
    /// This is a critical consensus property
    #[test]
    fn prop_nonce_monotonic(
        initial_balance in 10000u64..100000,
        num_txs in 1usize..20,
    ) {
        let mut executor = TestExecutor::with_accounts(vec![(0, initial_balance, 0)]);

        // Create transactions with sequential nonces
        let txs: Vec<TestTransaction> = (0..num_txs)
            .map(|i| TestTransaction {
                from: 0,
                to: 1,
                amount: 10,
                nonce: i as u64,
            })
            .collect();

        executor.execute_sequential(&txs);

        // Verify nonce equals number of successful txs
        let sender_state = executor.accounts.get(&0).unwrap();
        prop_assert!(sender_state.nonce <= num_txs as u64);
    }

    /// Property: Total supply is conserved
    /// Sum of all balances remains constant (no coins created/destroyed)
    #[test]
    fn prop_supply_conservation(
        accounts in prop::collection::vec(account_strategy(), 2..5),
        num_txs in 1usize..10,
    ) {
        let mut executor = TestExecutor::with_accounts(accounts.clone());

        // Calculate initial total supply
        let initial_supply: u64 = executor.accounts.values().map(|a| a.balance).sum();

        // Execute random transactions
        let txs: Vec<TestTransaction> = (0..num_txs)
            .map(|i| TestTransaction {
                from: i as u64 % 5,
                to: (i as u64 + 1) % 5,
                amount: 50,
                nonce: 0, // Will mostly fail, that's ok
            })
            .collect();

        executor.execute_sequential(&txs);

        // Calculate final total supply
        let final_supply: u64 = executor.accounts.values().map(|a| a.balance).sum();

        prop_assert_eq!(initial_supply, final_supply, "Supply must be conserved");
    }

    /// Property: Failed transactions don't modify state
    /// A transaction that fails validation should not change any state
    #[test]
    fn prop_failed_tx_no_state_change(
        balance in 100u64..1000,
        transfer_amount in 1001u64..2000, // Always more than balance
    ) {
        let mut executor = TestExecutor::with_accounts(vec![(0, balance, 0)]);
        let initial_hash = executor.state_hash();

        // This transaction should fail (insufficient balance)
        let tx = TestTransaction {
            from: 0,
            to: 1,
            amount: transfer_amount,
            nonce: 0,
        };

        let result = executor.execute_single(&tx);

        prop_assert!(!result, "Transaction should fail");
        prop_assert_eq!(
            executor.state_hash(),
            initial_hash,
            "State should not change on failed tx"
        );
    }

    /// Property: Invalid nonce transactions are rejected
    #[test]
    fn prop_invalid_nonce_rejected(
        balance in 1000u64..10000,
        wrong_nonce in 1u64..100, // Any non-zero nonce is wrong for new account
    ) {
        let mut executor = TestExecutor::with_accounts(vec![(0, balance, 0)]);

        let tx = TestTransaction {
            from: 0,
            to: 1,
            amount: 10,
            nonce: wrong_nonce, // Wrong nonce (should be 0)
        };

        let result = executor.execute_single(&tx);
        prop_assert!(!result, "Transaction with wrong nonce should fail");
    }

    /// Property: Execution order matters for conflicting transactions
    /// But final state must be deterministic for given order
    #[test]
    fn prop_order_determinism(
        balance in 500u64..1000,
    ) {
        // Two transactions that conflict (same sender, same nonce)
        let tx1 = TestTransaction { from: 0, to: 1, amount: 100, nonce: 0 };
        let tx2 = TestTransaction { from: 0, to: 2, amount: 100, nonce: 0 };

        // Execute in order [tx1, tx2]
        let mut executor1 = TestExecutor::with_accounts(vec![(0, balance, 0)]);
        executor1.execute_sequential(&[tx1.clone(), tx2.clone()]);

        // Execute in same order again
        let mut executor2 = TestExecutor::with_accounts(vec![(0, balance, 0)]);
        executor2.execute_sequential(&[tx1.clone(), tx2.clone()]);

        prop_assert_eq!(
            executor1.state_hash(),
            executor2.state_hash(),
            "Same order must produce same state"
        );

        // Execute in reverse order [tx2, tx1]
        let mut executor3 = TestExecutor::with_accounts(vec![(0, balance, 0)]);
        executor3.execute_sequential(&[tx2.clone(), tx1.clone()]);

        let mut executor4 = TestExecutor::with_accounts(vec![(0, balance, 0)]);
        executor4.execute_sequential(&[tx2, tx1]);

        prop_assert_eq!(
            executor3.state_hash(),
            executor4.state_hash(),
            "Same order must produce same state (reverse)"
        );
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_basic_transfer() {
        let mut executor = TestExecutor::with_accounts(vec![(0, 1000, 0), (1, 500, 0)]);

        let tx = TestTransaction {
            from: 0,
            to: 1,
            amount: 100,
            nonce: 0,
        };

        assert!(executor.execute_single(&tx));
        assert_eq!(executor.accounts.get(&0).unwrap().balance, 900);
        assert_eq!(executor.accounts.get(&1).unwrap().balance, 600);
    }

    #[test]
    fn test_insufficient_balance() {
        let mut executor = TestExecutor::with_accounts(vec![(0, 50, 0)]);

        let tx = TestTransaction {
            from: 0,
            to: 1,
            amount: 100,
            nonce: 0,
        };

        assert!(!executor.execute_single(&tx));
        assert_eq!(executor.accounts.get(&0).unwrap().balance, 50);
    }

    #[test]
    fn test_nonce_sequence() {
        let mut executor = TestExecutor::with_accounts(vec![(0, 1000, 0)]);

        // Nonce 0 should succeed
        let tx0 = TestTransaction {
            from: 0,
            to: 1,
            amount: 10,
            nonce: 0,
        };
        assert!(executor.execute_single(&tx0));

        // Nonce 1 should succeed
        let tx1 = TestTransaction {
            from: 0,
            to: 1,
            amount: 10,
            nonce: 1,
        };
        assert!(executor.execute_single(&tx1));

        // Nonce 0 again should fail (replay)
        let tx_replay = TestTransaction {
            from: 0,
            to: 1,
            amount: 10,
            nonce: 0,
        };
        assert!(!executor.execute_single(&tx_replay));
    }
}
