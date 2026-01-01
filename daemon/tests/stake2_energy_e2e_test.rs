#![allow(clippy::disallowed_methods)]

//! Stake 2.0 Energy E2E Tests
//!
//! Comprehensive tests for the Stake 2.0 energy model including:
//! - FreezeTos: Freeze TOS to get proportional energy
//! - UnfreezeTos: Initiate unfreeze (14-day queue)
//! - WithdrawExpireUnfreeze: Withdraw expired unfreezes
//! - CancelAllUnfreeze: Cancel all pending unfreezes
//! - DelegateResource: Delegate energy to another account
//! - UndelegateResource: Undelegate energy
//! - GlobalEnergyState: Network-wide energy tracking
//! - TransactionResult: Fee/energy consumption tracking

use std::collections::HashMap;

use tos_common::{
    account::{AccountEnergy, DelegatedResource, GlobalEnergyState, UnfreezingRecord},
    block::TopoHeight,
    config::{COIN_VALUE, TOTAL_ENERGY_LIMIT, UNFREEZE_DELAY_DAYS},
    crypto::{elgamal::CompressedPublicKey, Hash, KeyPair},
    transaction::TransactionResult,
};

// Convert days to milliseconds
const MS_PER_DAY: u64 = 24 * 60 * 60 * 1000;
const UNFREEZE_DELAY_MS: u64 = UNFREEZE_DELAY_DAYS as u64 * MS_PER_DAY;

// ============================================================================
// Test Chain State (simplified for energy testing)
// ============================================================================

struct EnergyTestState {
    // Account energy states (Stake 2.0)
    energy_states: HashMap<CompressedPublicKey, AccountEnergy>,
    // Delegations: (from, to) -> DelegatedResource
    delegations: HashMap<(CompressedPublicKey, CompressedPublicKey), DelegatedResource>,
    // Global energy state
    global_energy: GlobalEnergyState,
    // Transaction results
    tx_results: HashMap<Hash, TransactionResult>,
    // Current timestamp (milliseconds)
    current_time_ms: u64,
    // Current topoheight
    topoheight: TopoHeight,
}

impl EnergyTestState {
    fn new() -> Self {
        Self {
            energy_states: HashMap::new(),
            delegations: HashMap::new(),
            global_energy: GlobalEnergyState::new(),
            tx_results: HashMap::new(),
            current_time_ms: 1_700_000_000_000, // Some base timestamp
            topoheight: 1,
        }
    }

    fn get_or_create_energy(&mut self, account: &CompressedPublicKey) -> &mut AccountEnergy {
        self.energy_states
            .entry(account.clone())
            .or_insert_with(AccountEnergy::new)
    }

    fn advance_time(&mut self, ms: u64) {
        self.current_time_ms += ms;
    }

    // Simulate FreezeTos operation
    fn freeze_tos(&mut self, account: &CompressedPublicKey, amount: u64) {
        let energy = self.get_or_create_energy(account);
        energy.frozen_balance += amount;
        self.global_energy.add_weight(amount, self.topoheight);
    }

    // Simulate UnfreezeTos operation (adds to queue)
    fn unfreeze_tos(&mut self, account: &CompressedPublicKey, amount: u64) -> Result<(), &'static str> {
        // Capture values before mutable borrow
        let now = self.current_time_ms;
        let topoheight = self.topoheight;

        let energy = self.get_or_create_energy(account);

        if energy.frozen_balance < amount {
            return Err("Insufficient frozen balance");
        }
        if energy.unfreezing_list.len() >= 32 {
            return Err("Unfreeze queue full (max 32)");
        }

        energy.frozen_balance -= amount;
        energy.unfreezing_list.push(UnfreezingRecord {
            unfreeze_amount: amount,
            unfreeze_expire_time: now + UNFREEZE_DELAY_MS,
        });
        self.global_energy.remove_weight(amount, topoheight);
        Ok(())
    }

    // Simulate WithdrawExpireUnfreeze operation
    fn withdraw_expire_unfreeze(&mut self, account: &CompressedPublicKey) -> Result<u64, &'static str> {
        // Capture time before mutable borrow
        let now = self.current_time_ms;

        let energy = self.get_or_create_energy(account);

        let mut withdrawn = 0u64;
        energy.unfreezing_list.retain(|record| {
            if record.unfreeze_expire_time <= now {
                withdrawn += record.unfreeze_amount;
                false
            } else {
                true
            }
        });

        if withdrawn == 0 {
            return Err("No expired unfreeze to withdraw");
        }
        Ok(withdrawn)
    }

    // Simulate CancelAllUnfreeze operation
    fn cancel_all_unfreeze(&mut self, account: &CompressedPublicKey) -> (u64, u64) {
        // Capture values before mutable borrow
        let now = self.current_time_ms;
        let topoheight = self.topoheight;

        let energy = self.get_or_create_energy(account);

        let mut withdrawn = 0u64; // Expired -> returned to balance
        let mut cancelled = 0u64; // Not expired -> returned to frozen

        for record in &energy.unfreezing_list {
            if record.unfreeze_expire_time <= now {
                withdrawn += record.unfreeze_amount;
            } else {
                cancelled += record.unfreeze_amount;
            }
        }

        energy.unfreezing_list.clear();
        energy.frozen_balance += cancelled;
        self.global_energy.add_weight(cancelled, topoheight);

        (withdrawn, cancelled)
    }

    // Simulate DelegateResource operation
    fn delegate_resource(
        &mut self,
        from: &CompressedPublicKey,
        to: &CompressedPublicKey,
        amount: u64,
        lock_days: u32,
    ) -> Result<(), &'static str> {
        if from == to {
            return Err("Cannot delegate to self");
        }

        let from_energy = self.get_or_create_energy(from);
        if from_energy.frozen_balance < amount {
            return Err("Insufficient frozen balance");
        }

        from_energy.frozen_balance -= amount;
        from_energy.delegated_frozen_balance += amount;

        let to_energy = self.get_or_create_energy(to);
        to_energy.acquired_delegated_balance += amount;

        let expire_time = if lock_days > 0 {
            self.current_time_ms + (lock_days as u64 * MS_PER_DAY)
        } else {
            0
        };

        let delegation = DelegatedResource::new(from.clone(), to.clone(), amount, expire_time);
        self.delegations.insert((from.clone(), to.clone()), delegation);

        Ok(())
    }

    // Simulate UndelegateResource operation
    fn undelegate_resource(
        &mut self,
        from: &CompressedPublicKey,
        to: &CompressedPublicKey,
        amount: u64,
    ) -> Result<(), &'static str> {
        let delegation = self
            .delegations
            .get(&(from.clone(), to.clone()))
            .ok_or("Delegation not found")?;

        if delegation.expire_time > self.current_time_ms && delegation.expire_time != 0 {
            return Err("Delegation is still locked");
        }
        if delegation.frozen_balance < amount {
            return Err("Insufficient delegated balance");
        }

        let delegation_balance = delegation.frozen_balance;

        // Update from account
        let from_energy = self.get_or_create_energy(from);
        from_energy.delegated_frozen_balance -= amount;
        from_energy.frozen_balance += amount;

        // Update to account
        let to_energy = self.get_or_create_energy(to);
        to_energy.acquired_delegated_balance -= amount;

        // Update or remove delegation
        if delegation_balance == amount {
            self.delegations.remove(&(from.clone(), to.clone()));
        } else {
            if let Some(d) = self.delegations.get_mut(&(from.clone(), to.clone())) {
                d.frozen_balance -= amount;
            }
        }

        Ok(())
    }
}

// ============================================================================
// Test Cases
// ============================================================================

#[test]
fn test_freeze_tos_updates_global_energy() {
    println!("Testing FreezeTos updates GlobalEnergyState...");

    let mut state = EnergyTestState::new();
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    // Verify initial global state
    assert_eq!(state.global_energy.total_energy_weight, 0);
    assert_eq!(state.global_energy.total_energy_limit, TOTAL_ENERGY_LIMIT);

    // Freeze 100 TOS
    let freeze_amount = 100 * COIN_VALUE;
    state.freeze_tos(&alice_pubkey, freeze_amount);

    // Verify results
    assert_eq!(state.global_energy.total_energy_weight, freeze_amount);
    assert_eq!(
        state.energy_states.get(&alice_pubkey).unwrap().frozen_balance,
        freeze_amount
    );

    println!("  Global energy weight after freeze: {}", state.global_energy.total_energy_weight);
    println!("  Alice frozen balance: {}", state.energy_states.get(&alice_pubkey).unwrap().frozen_balance);
    println!("FreezeTos GlobalEnergyState test passed!");
}

#[test]
fn test_unfreeze_tos_adds_to_queue() {
    println!("Testing UnfreezeTos adds to 14-day queue...");

    let mut state = EnergyTestState::new();
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    // Setup: Freeze 100 TOS first
    state.freeze_tos(&alice_pubkey, 100 * COIN_VALUE);

    // Unfreeze 50 TOS
    let unfreeze_amount = 50 * COIN_VALUE;
    state.unfreeze_tos(&alice_pubkey, unfreeze_amount).unwrap();

    let energy = state.energy_states.get(&alice_pubkey).unwrap();

    // Verify results
    assert_eq!(energy.frozen_balance, 50 * COIN_VALUE);
    assert_eq!(energy.unfreezing_list.len(), 1);
    assert_eq!(energy.unfreezing_list[0].unfreeze_amount, unfreeze_amount);
    assert_eq!(state.global_energy.total_energy_weight, 50 * COIN_VALUE);

    println!("  Unfreezing queue size: {}", energy.unfreezing_list.len());
    println!("  Unfreeze amount: {}", energy.unfreezing_list[0].unfreeze_amount);
    println!("  Expire time: {} (current: {})", energy.unfreezing_list[0].unfreeze_expire_time, state.current_time_ms);
    println!("  Global weight after unfreeze: {}", state.global_energy.total_energy_weight);
    println!("UnfreezeTos queue test passed!");
}

#[test]
fn test_withdraw_expire_unfreeze() {
    println!("Testing WithdrawExpireUnfreeze...");

    let mut state = EnergyTestState::new();
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    // Setup: Freeze and unfreeze
    state.freeze_tos(&alice_pubkey, 100 * COIN_VALUE);
    state.unfreeze_tos(&alice_pubkey, 50 * COIN_VALUE).unwrap();

    // Try to withdraw before expiry (should fail)
    let result = state.withdraw_expire_unfreeze(&alice_pubkey);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "No expired unfreeze to withdraw");

    // Advance time past 14 days
    state.advance_time(UNFREEZE_DELAY_MS + 1);

    // Now withdraw should succeed
    let withdrawn = state.withdraw_expire_unfreeze(&alice_pubkey).unwrap();
    assert_eq!(withdrawn, 50 * COIN_VALUE);

    let energy = state.energy_states.get(&alice_pubkey).unwrap();
    assert_eq!(energy.unfreezing_list.len(), 0);

    println!("  Withdrawn amount: {}", withdrawn);
    println!("  Remaining queue size: {}", energy.unfreezing_list.len());
    println!("WithdrawExpireUnfreeze test passed!");
}

#[test]
fn test_cancel_all_unfreeze() {
    println!("Testing CancelAllUnfreeze...");

    let mut state = EnergyTestState::new();
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    // Setup: Freeze 100 TOS, unfreeze in two batches
    state.freeze_tos(&alice_pubkey, 100 * COIN_VALUE);
    state.unfreeze_tos(&alice_pubkey, 30 * COIN_VALUE).unwrap();

    // Advance time past expiry for first unfreeze
    state.advance_time(UNFREEZE_DELAY_MS + 1);

    // Second unfreeze (not yet expired)
    state.unfreeze_tos(&alice_pubkey, 20 * COIN_VALUE).unwrap();

    // Cancel all
    let (withdrawn, cancelled) = state.cancel_all_unfreeze(&alice_pubkey);

    let energy = state.energy_states.get(&alice_pubkey).unwrap();

    // Verify results
    assert_eq!(withdrawn, 30 * COIN_VALUE); // First unfreeze was expired
    assert_eq!(cancelled, 20 * COIN_VALUE); // Second unfreeze was not expired
    assert_eq!(energy.frozen_balance, 50 * COIN_VALUE + cancelled); // Original 50 + cancelled 20
    assert_eq!(energy.unfreezing_list.len(), 0);

    println!("  Withdrawn (expired): {}", withdrawn);
    println!("  Cancelled (back to frozen): {}", cancelled);
    println!("  Final frozen balance: {}", energy.frozen_balance);
    println!("CancelAllUnfreeze test passed!");
}

#[test]
fn test_delegate_resource() {
    println!("Testing DelegateResource...");

    let mut state = EnergyTestState::new();
    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();
    let bob_pubkey = bob.get_public_key().compress();

    // Setup: Freeze 100 TOS
    state.freeze_tos(&alice_pubkey, 100 * COIN_VALUE);

    // Delegate 40 TOS to Bob with 3-day lock
    let delegate_amount = 40 * COIN_VALUE;
    state
        .delegate_resource(&alice_pubkey, &bob_pubkey, delegate_amount, 3)
        .unwrap();

    let alice_energy = state.energy_states.get(&alice_pubkey).unwrap();
    let bob_energy = state.energy_states.get(&bob_pubkey).unwrap();

    // Verify results
    assert_eq!(alice_energy.frozen_balance, 60 * COIN_VALUE);
    assert_eq!(alice_energy.delegated_frozen_balance, delegate_amount);
    assert_eq!(bob_energy.acquired_delegated_balance, delegate_amount);
    assert!(state.delegations.contains_key(&(alice_pubkey.clone(), bob_pubkey.clone())));

    // Verify Bob's effective frozen includes delegation
    let bob_effective = bob_energy.frozen_balance + bob_energy.acquired_delegated_balance;
    assert_eq!(bob_effective, delegate_amount);

    println!("  Alice frozen: {}", alice_energy.frozen_balance);
    println!("  Alice delegated: {}", alice_energy.delegated_frozen_balance);
    println!("  Bob acquired: {}", bob_energy.acquired_delegated_balance);
    println!("DelegateResource test passed!");
}

#[test]
fn test_undelegate_resource() {
    println!("Testing UndelegateResource...");

    let mut state = EnergyTestState::new();
    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();
    let bob_pubkey = bob.get_public_key().compress();

    // Setup: Freeze and delegate with 3-day lock
    state.freeze_tos(&alice_pubkey, 100 * COIN_VALUE);
    state
        .delegate_resource(&alice_pubkey, &bob_pubkey, 40 * COIN_VALUE, 3)
        .unwrap();

    // Try to undelegate before lock expires (should fail)
    let result = state.undelegate_resource(&alice_pubkey, &bob_pubkey, 40 * COIN_VALUE);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Delegation is still locked");

    // Advance time past lock period
    state.advance_time(3 * MS_PER_DAY + 1);

    // Now undelegate should succeed
    state
        .undelegate_resource(&alice_pubkey, &bob_pubkey, 40 * COIN_VALUE)
        .unwrap();

    let alice_energy = state.energy_states.get(&alice_pubkey).unwrap();
    let bob_energy = state.energy_states.get(&bob_pubkey).unwrap();

    // Verify results
    assert_eq!(alice_energy.frozen_balance, 100 * COIN_VALUE);
    assert_eq!(alice_energy.delegated_frozen_balance, 0);
    assert_eq!(bob_energy.acquired_delegated_balance, 0);
    assert!(!state.delegations.contains_key(&(alice_pubkey.clone(), bob_pubkey.clone())));

    println!("  Alice frozen after undelegate: {}", alice_energy.frozen_balance);
    println!("  Alice delegated after: {}", alice_energy.delegated_frozen_balance);
    println!("  Bob acquired after: {}", bob_energy.acquired_delegated_balance);
    println!("UndelegateResource test passed!");
}

#[test]
fn test_energy_limit_calculation() {
    println!("Testing proportional energy limit calculation...");

    let mut state = EnergyTestState::new();

    // Setup: Total network frozen is 1,000,000 TOS
    let total_frozen = 1_000_000 * COIN_VALUE;
    state.global_energy.add_weight(total_frozen, 0);

    // Alice freezes 10,000 TOS (1% of network)
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();
    let alice_frozen = 10_000 * COIN_VALUE;

    let mut alice_energy = AccountEnergy::new();
    alice_energy.frozen_balance = alice_frozen;
    state.energy_states.insert(alice_pubkey.clone(), alice_energy.clone());

    // Calculate Alice's energy limit
    let alice_limit = alice_energy.calculate_energy_limit(state.global_energy.total_energy_weight);

    // Expected: 1% of 18.4 billion = 184,000,000
    let expected_limit = (alice_frozen as u128 * TOTAL_ENERGY_LIMIT as u128
        / state.global_energy.total_energy_weight as u128) as u64;

    assert_eq!(alice_limit, expected_limit);
    println!("  Total network frozen: {} TOS", total_frozen / COIN_VALUE);
    println!("  Alice frozen: {} TOS", alice_frozen / COIN_VALUE);
    println!("  Alice energy limit: {}", alice_limit);
    println!("  Expected (1% of 18.4B): {}", expected_limit);
    println!("Energy limit calculation test passed!");
}

#[test]
fn test_transaction_result_storage() {
    println!("Testing TransactionResult storage...");

    let mut state = EnergyTestState::new();
    let tx_hash = Hash::new([1u8; 32]);

    // Create a transaction result
    let result = TransactionResult {
        fee: 1000,
        energy_used: 500,
        free_energy_used: 100,
        frozen_energy_used: 400,
    };

    // Store the result
    state.tx_results.insert(tx_hash.clone(), result.clone());

    // Verify storage
    let stored = state.tx_results.get(&tx_hash).unwrap();
    assert_eq!(stored.fee, 1000);
    assert_eq!(stored.energy_used, 500);
    assert_eq!(stored.free_energy_used, 100);
    assert_eq!(stored.frozen_energy_used, 400);
    assert_eq!(stored.auto_burned_energy(), 0); // No auto-burn

    println!("  Stored result: fee={}, energy_used={}", stored.fee, stored.energy_used);
    println!("  Auto-burned energy: {}", stored.auto_burned_energy());
    println!("TransactionResult storage test passed!");
}

#[test]
fn test_unfreeze_queue_max_32() {
    println!("Testing unfreeze queue max 32 entries...");

    let mut state = EnergyTestState::new();
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    // Freeze enough TOS for 33 unfreezes
    state.freeze_tos(&alice_pubkey, 33 * COIN_VALUE);

    // Add 32 unfreeze entries (max allowed)
    for _ in 0..32 {
        state.unfreeze_tos(&alice_pubkey, 1 * COIN_VALUE).unwrap();
    }

    // 33rd should fail
    let result = state.unfreeze_tos(&alice_pubkey, 1 * COIN_VALUE);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Unfreeze queue full (max 32)");

    let energy = state.energy_states.get(&alice_pubkey).unwrap();
    assert_eq!(energy.unfreezing_list.len(), 32);

    println!("  Queue size: {}", energy.unfreezing_list.len());
    println!("  33rd unfreeze correctly rejected");
    println!("Unfreeze queue max test passed!");
}

#[test]
fn test_delegation_with_lock() {
    println!("Testing delegation with lock period...");

    let mut state = EnergyTestState::new();
    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();
    let bob_pubkey = bob.get_public_key().compress();

    // Freeze and delegate with 30-day lock
    state.freeze_tos(&alice_pubkey, 100 * COIN_VALUE);
    state
        .delegate_resource(&alice_pubkey, &bob_pubkey, 50 * COIN_VALUE, 30)
        .unwrap();

    // Copy expire_time before any mutable borrows
    let expire_time = state.delegations.get(&(alice_pubkey.clone(), bob_pubkey.clone())).unwrap().expire_time;

    // Verify lock
    let is_locked = expire_time > state.current_time_ms;
    assert!(is_locked);

    // Advance time past lock (30 days)
    state.advance_time(30 * MS_PER_DAY + 1);

    let is_locked_after = expire_time > state.current_time_ms;
    assert!(!is_locked_after);

    println!("  Lock period: 30 days");
    println!("  Is locked initially: {}", is_locked);
    println!("  Is locked after advance: {}", is_locked_after);
    println!("Delegation lock test passed!");
}

#[test]
fn test_self_delegation_rejected() {
    println!("Testing self-delegation is rejected...");

    let mut state = EnergyTestState::new();
    let alice = KeyPair::new();
    let alice_pubkey = alice.get_public_key().compress();

    // Freeze TOS
    state.freeze_tos(&alice_pubkey, 100 * COIN_VALUE);

    // Try to self-delegate (should fail)
    let result = state.delegate_resource(&alice_pubkey, &alice_pubkey, 50 * COIN_VALUE, 0);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Cannot delegate to self");

    println!("  Self-delegation correctly rejected");
    println!("Self-delegation rejection test passed!");
}

#[test]
fn test_energy_consumption_priority() {
    println!("Testing energy consumption priority (free -> frozen -> TOS burn)...");

    let mut energy = AccountEnergy::new();
    energy.frozen_balance = 1_000_000 * COIN_VALUE; // 1M TOS frozen
    let total_weight = 100_000_000 * COIN_VALUE; // 100M total network

    // Account has some energy
    let limit = energy.calculate_energy_limit(total_weight);
    println!("  Energy limit: {}", limit);

    // Test free energy first
    let now_ms = 1_700_000_000_000u64;
    let free_available = energy.calculate_free_energy_available(now_ms);
    println!("  Free energy available: {}", free_available);

    // Consume less than free quota
    let result = tos_common::utils::energy_fee::EnergyResourceManager::consume_transaction_energy(
        &mut energy,
        500, // Less than free quota (1000)
        total_weight,
        now_ms,
    );
    println!("  After consuming 500 energy: consumed={}, tos_to_burn={}", result.0, result.1);
    assert_eq!(result.1, 0); // No TOS burn needed
    assert_eq!(energy.free_energy_usage, 500);

    println!("Energy consumption priority test passed!");
}

#[test]
fn test_24h_linear_recovery() {
    println!("Testing 24-hour linear energy recovery...");

    let mut energy = AccountEnergy::new();
    energy.frozen_balance = 1_000_000 * COIN_VALUE; // 1M TOS frozen
    let total_weight = 100_000_000 * COIN_VALUE; // 100M total network

    let now_ms = 1_700_000_000_000u64;

    // Consume all frozen energy
    energy.consume_frozen_energy(1_000_000, now_ms, total_weight);
    assert!(energy.energy_usage > 0);

    let initial_available = energy.calculate_frozen_energy_available(now_ms, total_weight);
    println!("  Initial available after consumption: {}", initial_available);

    // After 12 hours, should recover ~50%
    let after_12h = now_ms + 12 * 60 * 60 * 1000;
    let recovered_12h = energy.calculate_frozen_energy_available(after_12h, total_weight);
    println!("  Available after 12 hours: {}", recovered_12h);

    // After 24 hours, should recover ~100%
    let after_24h = now_ms + 24 * 60 * 60 * 1000;
    let recovered_24h = energy.calculate_frozen_energy_available(after_24h, total_weight);
    let limit = energy.calculate_energy_limit(total_weight);
    println!("  Available after 24 hours: {}", recovered_24h);
    println!("  Energy limit: {}", limit);

    // Should be close to full limit after 24h
    assert!(recovered_24h >= limit - 1); // Allow for rounding

    println!("24-hour linear recovery test passed!");
}
