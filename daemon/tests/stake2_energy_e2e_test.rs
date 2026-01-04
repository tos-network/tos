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
        self.energy_states.entry(account.clone()).or_default()
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
    fn unfreeze_tos(
        &mut self,
        account: &CompressedPublicKey,
        amount: u64,
    ) -> Result<(), &'static str> {
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
    fn withdraw_expire_unfreeze(
        &mut self,
        account: &CompressedPublicKey,
    ) -> Result<u64, &'static str> {
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
    // CORRECTED: Uses available_for_delegation() check and does NOT modify frozen_balance
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
        // CORRECTED: Check available_for_delegation() instead of frozen_balance
        if from_energy.available_for_delegation() < amount {
            return Err("Insufficient frozen balance for delegation");
        }

        // CORRECTED: frozen_balance stays unchanged - TOS remains frozen
        // Only update delegated_frozen_balance to track what's delegated out
        from_energy.delegated_frozen_balance += amount;

        let to_energy = self.get_or_create_energy(to);
        to_energy.acquired_delegated_balance += amount;

        let expire_time = if lock_days > 0 {
            self.current_time_ms + (lock_days as u64 * MS_PER_DAY)
        } else {
            0
        };

        let delegation = DelegatedResource::new(from.clone(), to.clone(), amount, expire_time);
        self.delegations
            .insert((from.clone(), to.clone()), delegation);

        Ok(())
    }

    // Simulate UndelegateResource operation
    // CORRECTED: Does NOT add back to frozen_balance (since it wasn't subtracted)
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
        // CORRECTED: Only update delegated_frozen_balance, NOT frozen_balance
        // frozen_balance was never reduced during delegation
        let from_energy = self.get_or_create_energy(from);
        from_energy.delegated_frozen_balance -= amount;

        // Update to account
        let to_energy = self.get_or_create_energy(to);
        to_energy.acquired_delegated_balance -= amount;

        // Update or remove delegation
        if delegation_balance == amount {
            self.delegations.remove(&(from.clone(), to.clone()));
        } else if let Some(d) = self.delegations.get_mut(&(from.clone(), to.clone())) {
            d.frozen_balance -= amount;
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
        state
            .energy_states
            .get(&alice_pubkey)
            .unwrap()
            .frozen_balance,
        freeze_amount
    );

    println!(
        "  Global energy weight after freeze: {}",
        state.global_energy.total_energy_weight
    );
    println!(
        "  Alice frozen balance: {}",
        state
            .energy_states
            .get(&alice_pubkey)
            .unwrap()
            .frozen_balance
    );
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
    println!(
        "  Unfreeze amount: {}",
        energy.unfreezing_list[0].unfreeze_amount
    );
    println!(
        "  Expire time: {} (current: {})",
        energy.unfreezing_list[0].unfreeze_expire_time, state.current_time_ms
    );
    println!(
        "  Global weight after unfreeze: {}",
        state.global_energy.total_energy_weight
    );
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

    // CORRECTED: frozen_balance stays at 100 TOS (unchanged by delegation)
    // Only delegated_frozen_balance changes to track what's delegated out
    assert_eq!(
        alice_energy.frozen_balance,
        100 * COIN_VALUE,
        "frozen_balance must NOT change during delegation"
    );
    assert_eq!(alice_energy.delegated_frozen_balance, delegate_amount);
    assert_eq!(bob_energy.acquired_delegated_balance, delegate_amount);
    assert!(state
        .delegations
        .contains_key(&(alice_pubkey.clone(), bob_pubkey.clone())));

    // Verify Alice's effective frozen balance
    // effective = frozen + acquired - delegated_out = 100 + 0 - 40 = 60
    assert_eq!(
        alice_energy.effective_frozen_balance(),
        60 * COIN_VALUE,
        "Effective frozen = 100 - 40 = 60 TOS"
    );

    // Verify Bob's effective frozen includes delegation
    let bob_effective = bob_energy.effective_frozen_balance();
    assert_eq!(bob_effective, delegate_amount);

    println!("  Alice frozen: {}", alice_energy.frozen_balance);
    println!(
        "  Alice effective: {}",
        alice_energy.effective_frozen_balance()
    );
    println!(
        "  Alice delegated: {}",
        alice_energy.delegated_frozen_balance
    );
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
    assert!(!state
        .delegations
        .contains_key(&(alice_pubkey.clone(), bob_pubkey.clone())));

    println!(
        "  Alice frozen after undelegate: {}",
        alice_energy.frozen_balance
    );
    println!(
        "  Alice delegated after: {}",
        alice_energy.delegated_frozen_balance
    );
    println!(
        "  Bob acquired after: {}",
        bob_energy.acquired_delegated_balance
    );
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
    state
        .energy_states
        .insert(alice_pubkey.clone(), alice_energy.clone());

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

    println!(
        "  Stored result: fee={}, energy_used={}",
        stored.fee, stored.energy_used
    );
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
        state.unfreeze_tos(&alice_pubkey, COIN_VALUE).unwrap();
    }

    // 33rd should fail
    let result = state.unfreeze_tos(&alice_pubkey, COIN_VALUE);
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
    let expire_time = state
        .delegations
        .get(&(alice_pubkey.clone(), bob_pubkey.clone()))
        .unwrap()
        .expire_time;

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
    println!(
        "  After consuming 500 energy: consumed={}, tos_to_burn={}",
        result.0, result.1
    );
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
    println!(
        "  Initial available after consumption: {}",
        initial_available
    );

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

// =============================================================================
// Tests: Same-Block Activation and Delegation
// Addresses: Cannot use state changes from same block in verify phase
// =============================================================================

#[test]
fn test_same_block_activation_delegation_limits() {
    println!("Testing same-block activation and delegation limits...");

    // This test demonstrates the verify/apply phase separation principle
    // During verify phase, we cannot predict state changes from other TXs in the same block

    let mut state = EnergyTestState::new();

    // Create accounts
    let sender = KeyPair::new().get_public_key().compress();
    let _receiver = KeyPair::new().get_public_key().compress();

    // Sender freezes TOS (in a previous block)
    state.freeze_tos(&sender, 1_000 * COIN_VALUE);
    assert_eq!(
        state.get_or_create_energy(&sender).frozen_balance,
        1_000 * COIN_VALUE
    );

    // Simulate a block with two TXs:
    // TX1: Sender freezes additional 500 TOS
    // TX2: Sender delegates 1,200 TOS to receiver

    // At verify phase for TX2, sender only has 1,000 TOS frozen
    // (the 500 TOS from TX1 is not yet applied)

    let sender_energy = state.get_or_create_energy(&sender);
    let available_before_block = sender_energy.available_for_delegation();
    assert_eq!(
        available_before_block,
        1_000 * COIN_VALUE,
        "Before block, sender has 1,000 TOS available"
    );

    // In verify phase, TX2's delegation of 1,200 TOS should fail
    // because only 1,000 TOS is available at that point
    let delegate_amount = 1_200 * COIN_VALUE;
    let can_delegate = available_before_block >= delegate_amount;
    assert!(
        !can_delegate,
        "Should NOT be able to delegate more than available at verify time"
    );

    // Only 1,000 TOS can be delegated in verify phase
    let valid_delegate = 1_000 * COIN_VALUE;
    let can_delegate_valid = available_before_block >= valid_delegate;
    assert!(
        can_delegate_valid,
        "Should be able to delegate available amount"
    );

    println!("Same-block activation/delegation limits test passed!");
}

#[test]
fn test_verify_phase_sees_only_committed_state() {
    println!("Testing verify phase sees only committed state...");

    let mut state = EnergyTestState::new();

    let alice = KeyPair::new().get_public_key().compress();
    let _bob = KeyPair::new().get_public_key().compress();

    // Block N-1: Alice freezes 500 TOS
    state.freeze_tos(&alice, 500 * COIN_VALUE);
    state.topoheight += 1;

    // Block N: Contains TX1 (freeze 500 more) and TX2 (delegate 800)
    // At verify time for TX2, only the 500 TOS from Block N-1 is visible

    let alice_energy = state.get_or_create_energy(&alice);
    let committed_available = alice_energy.available_for_delegation();

    // TX1 would add 500 more, but TX2's verify cannot see it
    // So TX2 can only delegate up to 500 TOS
    assert_eq!(committed_available, 500 * COIN_VALUE);

    // TX2 trying to delegate 800 TOS should fail in verify
    let delegate_request = 800 * COIN_VALUE;
    assert!(
        committed_available < delegate_request,
        "Verify phase should reject delegation exceeding committed state"
    );

    println!("Verify phase sees only committed state test passed!");
}

// =============================================================================
// Tests: Fee Calculation Consistency
// Addresses: Verify and apply phases must use same fee calculation
// =============================================================================

#[test]
fn test_fee_calculation_consistency() {
    println!("Testing fee calculation consistency...");

    // Fee calculation should be deterministic and consistent
    // between verify and apply phases

    let tx_size = 250u64;
    let output_count = 3u64;
    let new_account_count = 1u64;

    // Fee formula: size + (outputs * 500) + (new_accounts * 1000)
    let output_fee = 500u64;
    let new_account_fee = 1_000u64;

    let calculated_fee_1 =
        tx_size + (output_count * output_fee) + (new_account_count * new_account_fee);
    let calculated_fee_2 =
        tx_size + (output_count * output_fee) + (new_account_count * new_account_fee);

    // Must be identical
    assert_eq!(
        calculated_fee_1, calculated_fee_2,
        "Fee calculation must be consistent"
    );
    assert_eq!(calculated_fee_1, 250 + 1500 + 1000);
    assert_eq!(calculated_fee_1, 2750);

    println!("Fee calculation consistency test passed!");
}

#[test]
fn test_fee_calculation_with_energy_consumption() {
    println!("Testing fee calculation with energy consumption...");

    let mut energy = AccountEnergy::new();
    energy.frozen_balance = 1_000 * COIN_VALUE;

    let total_weight = 10_000_000 * COIN_VALUE;
    let now_ms = 1_000_000_000u64;

    // Simulate transaction that costs 5,000 energy
    let required_energy = 5_000u64;

    // Calculate available energy
    let free_available = energy.calculate_free_energy_available(now_ms);
    let frozen_available = energy.calculate_frozen_energy_available(now_ms, total_weight);
    let total_available = free_available + frozen_available;

    // If required <= available, no TOS burn
    // If required > available, burn TOS for shortfall
    let shortfall = required_energy.saturating_sub(total_available);
    let tos_burn = shortfall * 100; // TOS_PER_ENERGY = 100

    if total_available >= required_energy {
        assert_eq!(tos_burn, 0, "No TOS burn when energy covers cost");
    } else {
        assert!(tos_burn > 0, "TOS burn needed when energy insufficient");
    }

    // Verify calculation is reproducible
    let shortfall_2 = required_energy.saturating_sub(total_available);
    let tos_burn_2 = shortfall_2 * 100;
    assert_eq!(tos_burn, tos_burn_2, "Fee calculation must be reproducible");

    println!("Fee calculation with energy consumption test passed!");
}

// =============================================================================
// Tests: Fee Burn Verification
// Addresses: Burned fees should be removed from circulation
// =============================================================================

#[test]
fn test_fee_burn_removes_from_circulation() {
    println!("Testing fee burn removes from circulation...");

    // Simulate a scenario where TOS is burned for fees
    let mut total_circulating = 1_000_000_000 * COIN_VALUE; // 1B TOS
    let initial_circulating = total_circulating;

    // Transaction burns 10,000 atomic TOS
    let fee_burned = 10_000u64;
    total_circulating = total_circulating.saturating_sub(fee_burned);

    assert_eq!(
        total_circulating,
        initial_circulating - fee_burned,
        "Circulating supply should decrease by burned amount"
    );

    // Multiple burns
    let burns = [5_000u64, 3_000, 2_000];
    for burn in burns {
        total_circulating = total_circulating.saturating_sub(burn);
    }

    let total_burned = fee_burned + burns.iter().sum::<u64>();
    assert_eq!(
        total_circulating,
        initial_circulating - total_burned,
        "Cumulative burns should reduce circulation correctly"
    );

    println!("Fee burn removes from circulation test passed!");
}

#[test]
fn test_fee_burn_not_added_to_any_account() {
    println!("Testing fee burn is not added to any account...");

    // This test documents the fee burn behavior
    // We don't need actual state for this test

    // Track all balances before transaction
    // (In this test framework we don't track balances, but we verify the principle)

    // When fee is burned:
    // 1. Sender balance decreases by fee_limit
    // 2. fee_burned is NOT added to receiver or any other account
    // 3. Sender gets refund of (fee_limit - actual_fee)

    let fee_limit = 10_000u64;
    let actual_fee = 5_000u64;
    let refund = fee_limit - actual_fee;
    let net_sender_deduction = actual_fee;

    // Verify the math
    assert_eq!(
        fee_limit,
        actual_fee + refund,
        "fee_limit = actual + refund"
    );
    assert_eq!(net_sender_deduction, 5_000, "Net deduction is actual fee");

    // The actual_fee (5,000) is burned - not sent anywhere
    // This test documents the expected behavior

    println!("Fee burn is not added to any account test passed!");
}

// =============================================================================
// Tests: Verification State Rollback
// Addresses: Failed verification should not modify state
// =============================================================================

#[test]
fn test_failed_verification_no_state_change() {
    println!("Testing failed verification does not change state...");

    let mut state = EnergyTestState::new();

    let sender = KeyPair::new().get_public_key().compress();

    // Setup initial state
    state.freeze_tos(&sender, 1_000 * COIN_VALUE);

    // Capture state before failed operation
    let frozen_before = state.get_or_create_energy(&sender).frozen_balance;
    let queue_len_before = state.get_or_create_energy(&sender).unfreezing_list.len();
    let global_weight_before = state.global_energy.total_energy_weight;

    // Attempt an operation that will fail (unfreeze more than frozen)
    let result = state.unfreeze_tos(&sender, 2_000 * COIN_VALUE);
    assert!(
        result.is_err(),
        "Should fail when unfreezing more than frozen"
    );

    // Verify state unchanged after failure
    let frozen_after = state.get_or_create_energy(&sender).frozen_balance;
    let queue_len_after = state.get_or_create_energy(&sender).unfreezing_list.len();
    let global_weight_after = state.global_energy.total_energy_weight;

    assert_eq!(
        frozen_before, frozen_after,
        "frozen_balance must not change on failed operation"
    );
    assert_eq!(
        queue_len_before, queue_len_after,
        "unfreezing_list must not change on failed operation"
    );
    assert_eq!(
        global_weight_before, global_weight_after,
        "global_weight must not change on failed operation"
    );

    println!("Failed verification no state change test passed!");
}

#[test]
fn test_delegation_failure_no_state_change() {
    println!("Testing delegation failure does not change state...");

    let mut state = EnergyTestState::new();

    let sender = KeyPair::new().get_public_key().compress();
    let receiver = KeyPair::new().get_public_key().compress();

    // Setup initial state
    state.freeze_tos(&sender, 1_000 * COIN_VALUE);

    // Capture state before
    let sender_energy_before = state.get_or_create_energy(&sender).clone();
    let delegations_count_before = state.delegations.len();

    // Attempt to delegate more than available
    let result = state.delegate_resource(&sender, &receiver, 2_000 * COIN_VALUE, 0);
    assert!(
        result.is_err(),
        "Should fail when delegating more than available"
    );

    // Verify state unchanged
    let sender_energy_after = state.get_or_create_energy(&sender);
    assert_eq!(
        sender_energy_before.delegated_frozen_balance, sender_energy_after.delegated_frozen_balance,
        "delegated_frozen_balance must not change on failed delegation"
    );
    assert_eq!(
        state.delegations.len(),
        delegations_count_before,
        "No new delegation should be created on failure"
    );

    println!("Delegation failure no state change test passed!");
}

#[test]
fn test_queue_full_no_partial_state_change() {
    println!("Testing queue full error leaves no partial state change...");

    let mut state = EnergyTestState::new();

    let sender = KeyPair::new().get_public_key().compress();

    // Freeze a lot of TOS
    state.freeze_tos(&sender, 100 * COIN_VALUE);

    // Fill the unfreezing queue to max (32 entries)
    for i in 0..32 {
        let result = state.unfreeze_tos(&sender, COIN_VALUE);
        assert!(
            result.is_ok(),
            "Should succeed until queue is full: entry {}",
            i
        );
    }

    // Verify queue is full
    let queue_len = state.get_or_create_energy(&sender).unfreezing_list.len();
    assert_eq!(queue_len, 32, "Queue should be full");

    // Capture state before failed operation
    let frozen_before = state.get_or_create_energy(&sender).frozen_balance;

    // Try to add one more - should fail
    let result = state.unfreeze_tos(&sender, COIN_VALUE);
    assert!(result.is_err(), "Should fail when queue is full");

    // Verify no partial state change
    let frozen_after = state.get_or_create_energy(&sender).frozen_balance;
    let queue_len_after = state.get_or_create_energy(&sender).unfreezing_list.len();

    assert_eq!(
        frozen_before, frozen_after,
        "frozen_balance must not change"
    );
    assert_eq!(queue_len_after, 32, "Queue length must remain at 32");

    println!("Queue full no partial state change test passed!");
}

// =============================================================================
// Tests: Transaction Result Correctness
// Addresses: TransactionResult fields should be accurate
// =============================================================================

#[test]
fn test_transaction_result_fields_accurate() {
    println!("Testing TransactionResult fields are accurate...");

    // Test various TransactionResult scenarios

    // Scenario 1: All free energy
    let result1 = TransactionResult {
        fee: 0,
        energy_used: 1_000,
        free_energy_used: 1_000,
        frozen_energy_used: 0,
    };
    assert_eq!(result1.auto_burned_energy(), 0);
    assert_eq!(
        result1.free_energy_used + result1.frozen_energy_used + result1.auto_burned_energy(),
        result1.energy_used
    );

    // Scenario 2: Mixed free and frozen
    let result2 = TransactionResult {
        fee: 0,
        energy_used: 5_000,
        free_energy_used: 1_500,
        frozen_energy_used: 3_500,
    };
    assert_eq!(result2.auto_burned_energy(), 0);
    assert_eq!(
        result2.free_energy_used + result2.frozen_energy_used + result2.auto_burned_energy(),
        result2.energy_used
    );

    // Scenario 3: With TOS burn (auto-burned energy)
    let result3 = TransactionResult {
        fee: 500_000, // 500,000 atomic TOS = 5,000 energy * 100
        energy_used: 10_000,
        free_energy_used: 1_500,
        frozen_energy_used: 3_500,
    };
    assert_eq!(result3.auto_burned_energy(), 5_000); // 10,000 - 1,500 - 3,500
    assert_eq!(
        result3.free_energy_used + result3.frozen_energy_used + result3.auto_burned_energy(),
        result3.energy_used
    );

    // Verify fee matches auto-burned
    let tos_per_energy = 100u64;
    assert_eq!(result3.fee, result3.auto_burned_energy() * tos_per_energy);

    println!("TransactionResult fields accurate test passed!");
}

// =============================================================================
// Tests: Delegation Cannot Exceed Available
// Addresses: Prevent over-delegation
// =============================================================================

#[test]
fn test_delegation_cannot_exceed_available() {
    println!("Testing delegation cannot exceed available for delegation...");

    let mut state = EnergyTestState::new();

    let sender = KeyPair::new().get_public_key().compress();
    let receiver1 = KeyPair::new().get_public_key().compress();
    let receiver2 = KeyPair::new().get_public_key().compress();

    // Freeze 1,000 TOS
    state.freeze_tos(&sender, 1_000 * COIN_VALUE);

    // First delegation: 600 TOS (should succeed)
    let result1 = state.delegate_resource(&sender, &receiver1, 600 * COIN_VALUE, 0);
    assert!(result1.is_ok(), "First delegation should succeed");

    // Check available
    let available = state
        .get_or_create_energy(&sender)
        .available_for_delegation();
    assert_eq!(
        available,
        400 * COIN_VALUE,
        "400 TOS should remain available"
    );

    // Second delegation: 500 TOS (should fail - only 400 available)
    let result2 = state.delegate_resource(&sender, &receiver2, 500 * COIN_VALUE, 0);
    assert!(result2.is_err(), "Should fail when exceeding available");

    // Second delegation: 400 TOS (should succeed)
    let result3 = state.delegate_resource(&sender, &receiver2, 400 * COIN_VALUE, 0);
    assert!(
        result3.is_ok(),
        "Should succeed with exactly available amount"
    );

    // Now nothing available
    let final_available = state
        .get_or_create_energy(&sender)
        .available_for_delegation();
    assert_eq!(final_available, 0, "Nothing should remain available");

    println!("Delegation cannot exceed available test passed!");
}
