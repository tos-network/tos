//! Reentrancy Guard Example
//!
//! Demonstrates using transient storage (tstore/tload) for reentrancy protection.
//! Transient storage is cleared at the end of each transaction, making it perfect for
//! temporary locks that are cheaper than persistent storage.
//!
//! This example shows:
//! - How to implement a reentrancy guard using transient storage
//! - Cost comparison: TSTORE (100 CU) vs SSTORE (20000+ CU)
//! - Protection against reentrancy attacks
//!
//! # How It Works
//!
//! The reentrancy guard uses a single transient storage slot with key "LOCK".
//! - Before executing sensitive operations, we set LOCK = 1
//! - At the end of the operation, we set LOCK = 0
//! - If LOCK is already 1, we reject the call (reentrancy detected)
//! - Transient storage is automatically cleared at transaction end

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{call, get_caller, log, storage_read, storage_write, tload, tstore};

// Storage key for the reentrancy lock (transient)
const REENTRANCY_LOCK: &[u8] = b"LOCK";

// Storage key for the balance (persistent)
const BALANCE_KEY: &[u8] = b"balance";

/// Check if reentrancy lock is set
fn is_locked() -> bool {
    let mut buffer = [0u8; 1];
    let len = tload(REENTRANCY_LOCK, &mut buffer);

    // If we read any data and it's non-zero, the lock is set
    len > 0 && buffer[0] != 0
}

/// Set the reentrancy lock
fn set_lock() {
    let value = [1u8];
    let _ = tstore(REENTRANCY_LOCK, &value);
}

/// Clear the reentrancy lock
fn clear_lock() {
    let value = [0u8];
    let _ = tstore(REENTRANCY_LOCK, &value);
}

/// Read balance from persistent storage
fn read_balance() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(BALANCE_KEY, &mut buffer);

    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Write balance to persistent storage
fn write_balance(balance: u64) -> bool {
    let bytes = balance.to_le_bytes();
    storage_write(BALANCE_KEY, &bytes).is_ok()
}

/// Withdraw function - protected against reentrancy
///
/// This function demonstrates the reentrancy guard pattern:
/// 1. Check if already locked (reentrancy detected)
/// 2. Set lock
/// 3. Perform state changes
/// 4. Make external call (potential reentrancy point)
/// 5. Clear lock
fn withdraw(amount: u64) -> u64 {
    log("Withdraw called");

    // STEP 1: Check reentrancy lock (uses transient storage - cheap!)
    if is_locked() {
        log("REENTRANCY DETECTED! Rejecting call");
        return 1; // Error: reentrancy detected
    }

    // STEP 2: Set reentrancy lock
    set_lock();
    log("Lock acquired");

    // STEP 3: Check balance and update state BEFORE external call
    let balance = read_balance();
    if balance < amount {
        log("Insufficient balance");
        clear_lock();
        return 2; // Error: insufficient balance
    }

    // Update balance BEFORE external call (Checks-Effects-Interactions pattern)
    let new_balance = balance - amount;
    if !write_balance(new_balance) {
        log("Failed to update balance");
        clear_lock();
        return 3; // Error: storage write failed
    }

    log("Balance updated, making external call");

    // STEP 4: Make external call (potential reentrancy point)
    // In a real contract, this would transfer funds to the caller
    // If the recipient is a malicious contract, it might try to call us back
    let caller = get_caller();

    // Simulate external call (in real implementation, this would transfer tokens)
    // The malicious contract could try to re-enter here
    let call_result = call(&caller, &[], amount, 0);

    if call_result.is_err() {
        log("External call failed (but balance already updated)");
        // Note: In production, you might want to revert here
    } else {
        log("External call succeeded");
    }

    // STEP 5: Clear lock before returning
    clear_lock();
    log("Lock released");

    0 // Success
}

/// Deposit function - also protected (though less critical)
fn deposit(amount: u64) -> u64 {
    if is_locked() {
        log("REENTRANCY DETECTED in deposit!");
        return 1;
    }

    set_lock();

    let balance = read_balance();
    let new_balance = balance.saturating_add(amount);

    let success = write_balance(new_balance);

    clear_lock();

    if success {
        log("Deposit successful");
        0
    } else {
        log("Deposit failed");
        1
    }
}

/// Main contract entrypoint
///
/// Instruction format:
/// - First byte: operation code
///   - 0: deposit (next 8 bytes: amount)
///   - 1: withdraw (next 8 bytes: amount)
///   - 2: get balance
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("Reentrancy Guard Example - Using Transient Storage");

    // For this simple example, we'll use a hardcoded instruction
    // In a real contract, you'd read this from input data

    // Example: withdraw 100 tokens
    let amount = 100u64;

    // First, deposit some funds
    log("=== Initial Deposit ===");
    deposit(1000);

    // Now try to withdraw
    log("=== Withdraw (should succeed) ===");
    let result = withdraw(amount);

    if result == 0 {
        log("SUCCESS: Withdraw completed");
        log("The reentrancy guard protected us!");
        log("Transient storage cost: ~200 CU (TSTORE + TLOAD)");
        log("vs Persistent storage: ~40000+ CU (SSTORE x2)");
        log("Cost savings: ~200x cheaper!");
        0
    } else {
        log("ERROR: Withdraw failed");
        result
    }
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
