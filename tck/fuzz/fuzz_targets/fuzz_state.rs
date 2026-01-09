//! Fuzz target for state transition operations
//!
//! Tests that arbitrary state transitions do not cause panics
//! and maintain state consistency.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use std::collections::HashMap;

/// State transition input
#[derive(Debug, Arbitrary)]
struct StateInput {
    /// Initial balances
    initial_balances: Vec<(u64, u64)>, // (account_id, balance)
    /// State transitions to apply
    transitions: Vec<StateTransition>,
}

/// A state transition operation
#[derive(Debug, Arbitrary)]
enum StateTransition {
    /// Transfer value between accounts
    Transfer {
        from: u64,
        to: u64,
        amount: u64,
    },
    /// Create new account
    CreateAccount {
        id: u64,
        initial_balance: u64,
    },
    /// Update storage
    SetStorage {
        account: u64,
        key: [u8; 32],
        value: [u8; 32],
    },
    /// Delete storage
    DeleteStorage {
        account: u64,
        key: [u8; 32],
    },
    /// Increment nonce
    IncrementNonce {
        account: u64,
    },
}

/// Simple state representation
struct State {
    balances: HashMap<u64, u64>,
    nonces: HashMap<u64, u64>,
    storage: HashMap<(u64, [u8; 32]), [u8; 32]>,
}

impl State {
    fn new() -> Self {
        Self {
            balances: HashMap::new(),
            nonces: HashMap::new(),
            storage: HashMap::new(),
        }
    }

    fn get_balance(&self, account: u64) -> u64 {
        *self.balances.get(&account).unwrap_or(&0)
    }

    fn set_balance(&mut self, account: u64, balance: u64) {
        self.balances.insert(account, balance);
    }

    fn get_nonce(&self, account: u64) -> u64 {
        *self.nonces.get(&account).unwrap_or(&0)
    }

    fn increment_nonce(&mut self, account: u64) {
        let nonce = self.get_nonce(account);
        if let Some(new_nonce) = nonce.checked_add(1) {
            self.nonces.insert(account, new_nonce);
        }
    }

    fn set_storage(&mut self, account: u64, key: [u8; 32], value: [u8; 32]) {
        self.storage.insert((account, key), value);
    }

    fn delete_storage(&mut self, account: u64, key: [u8; 32]) {
        self.storage.remove(&(account, key));
    }

    fn total_balance(&self) -> u128 {
        self.balances.values().map(|&b| b as u128).sum()
    }
}

fuzz_target!(|input: StateInput| {
    // Limit input size
    if input.initial_balances.len() > 100 || input.transitions.len() > 100 {
        return;
    }

    // Initialize state
    let mut state = State::new();
    for (account, balance) in &input.initial_balances {
        state.set_balance(*account, *balance);
    }

    // Record initial total balance
    let initial_total = state.total_balance();

    // Apply transitions
    for transition in &input.transitions {
        apply_transition(&mut state, transition);
    }

    // Verify invariant: total balance conserved (excluding creates)
    // Note: CreateAccount adds new balance, so we track that separately
    let mut added_balance: u128 = 0;
    for transition in &input.transitions {
        if let StateTransition::CreateAccount {
            initial_balance, ..
        } = transition
        {
            added_balance = added_balance.saturating_add(*initial_balance as u128);
        }
    }

    let final_total = state.total_balance();
    assert!(
        final_total <= initial_total.saturating_add(added_balance),
        "Balance increased unexpectedly"
    );
});

/// Apply a single state transition
fn apply_transition(state: &mut State, transition: &StateTransition) {
    match transition {
        StateTransition::Transfer { from, to, amount } => {
            let from_balance = state.get_balance(*from);
            let to_balance = state.get_balance(*to);

            // Use checked arithmetic to prevent overflow/underflow
            if let Some(new_from) = from_balance.checked_sub(*amount) {
                if let Some(new_to) = to_balance.checked_add(*amount) {
                    state.set_balance(*from, new_from);
                    state.set_balance(*to, new_to);
                }
            }
        }
        StateTransition::CreateAccount {
            id,
            initial_balance,
        } => {
            // Only create if doesn't exist
            if state.get_balance(*id) == 0 {
                state.set_balance(*id, *initial_balance);
            }
        }
        StateTransition::SetStorage {
            account,
            key,
            value,
        } => {
            state.set_storage(*account, *key, *value);
        }
        StateTransition::DeleteStorage { account, key } => {
            state.delete_storage(*account, *key);
        }
        StateTransition::IncrementNonce { account } => {
            state.increment_nonce(*account);
        }
    }
}
