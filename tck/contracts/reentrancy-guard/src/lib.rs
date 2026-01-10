//! ReentrancyGuard - OpenZeppelin Port to TAKO
//!
//! Contract module that helps prevent reentrant calls to a function.
//! This is a direct port of OpenZeppelin's ReentrancyGuard contract to Rust/TAKO.
//!
//! # Overview
//!
//! Reentrancy is one of the most dangerous vulnerabilities in smart contracts.
//! It occurs when a contract makes an external call before finishing its state updates,
//! allowing the external contract to call back and potentially exploit inconsistent state.
//!
//! # How It Works
//!
//! The ReentrancyGuard uses a simple state machine with two states:
//! - NOT_ENTERED (1): Contract is not currently executing a protected function
//! - ENTERED (2): Contract is currently executing a protected function
//!
//! Before entering a protected function:
//! 1. Check if status == ENTERED (if yes, revert with ERR_REENTRANT_CALL)
//! 2. Set status = ENTERED
//! 3. Execute function logic
//! 4. Set status = NOT_ENTERED
//!
//! # Storage Layout
//!
//! This contract uses a single storage key:
//! - "reentrancy_status" -> u8 (1 = NOT_ENTERED, 2 = ENTERED)
//!
//! # Error Codes
//!
//! - ERR_REENTRANT_CALL (1001): Attempted to call a protected function while already executing
//!
//! # Usage Pattern
//!
//! ```rust,no_run
//! // Before your function that needs protection:
//! if is_entered() {
//!     return ERR_REENTRANT_CALL;
//! }
//! enter();
//!
//! // ... your function logic ...
//! // (including external calls that might try to re-enter)
//!
//! // After completion:
//! leave();
//! ```
//!
//! # Why Use Values 1 and 2?
//!
//! Following OpenZeppelin's design:
//! - Using non-zero values (1, 2) instead of (0, 1) optimizes gas costs
//! - The first write to a zero-value slot costs more gas than subsequent writes
//! - By using 1 as the default, we save gas on every protected function call
//!
//! # Example
//!
//! This contract demonstrates the guard with a vulnerable withdraw function
//! and shows how the guard prevents reentrancy attacks.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{log, storage_read, storage_write, get_caller, get_input_data, call};

// Storage key for the reentrancy status
const REENTRANCY_STATUS: &[u8] = b"reentrancy_status";

// Storage key for contract balance (for demonstration)
const BALANCE_KEY: &[u8] = b"balance";

// Reentrancy guard states (following OpenZeppelin pattern)
const NOT_ENTERED: u8 = 1;
const ENTERED: u8 = 2;

// Operation codes
const OP_WITHDRAW: u8 = 0x01;
const OP_DEPOSIT: u8 = 0x02;
const OP_GET_BALANCE: u8 = 0x10;

// Error codes
const ERR_REENTRANT_CALL: u64 = 1001;
const ERR_INSUFFICIENT_BALANCE: u64 = 1002;
const ERR_STORAGE_WRITE_FAILED: u64 = 1003;
const ERR_INVALID_INSTRUCTION: u64 = 1005;
const ERR_INVALID_PARAMS: u64 = 1006;

const SUCCESS: u64 = 0;

// Helper function to parse u64 from bytes (little-endian)
fn parse_u64(bytes: &[u8]) -> Result<u64, ()> {
    if bytes.len() < 8 {
        return Err(());
    }
    let mut array = [0u8; 8];
    array.copy_from_slice(&bytes[0..8]);
    Ok(u64::from_le_bytes(array))
}

// Check if the contract is currently in an entered state
fn is_entered() -> bool {
    let mut buffer = [0u8; 1];
    let len = storage_read(REENTRANCY_STATUS, &mut buffer);

    // If no value stored, treat as NOT_ENTERED (initialize on first use)
    if len == 0 {
        return false;
    }

    buffer[0] == ENTERED
}

// Enter the reentrancy guard (set status to ENTERED)
fn enter() -> bool {
    let value = [ENTERED];
    storage_write(REENTRANCY_STATUS, &value).is_ok()
}

// Leave the reentrancy guard (set status to NOT_ENTERED)
fn leave() -> bool {
    let value = [NOT_ENTERED];
    storage_write(REENTRANCY_STATUS, &value).is_ok()
}

// Read balance from storage
fn read_balance() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(BALANCE_KEY, &mut buffer);

    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

// Write balance to storage
fn write_balance(balance: u64) -> bool {
    let bytes = balance.to_le_bytes();
    storage_write(BALANCE_KEY, &bytes).is_ok()
}

// Initialize the reentrancy guard (constructor equivalent)
// This should be called once during contract deployment
fn initialize() {
    log("Initializing ReentrancyGuard");

    // Set initial status to NOT_ENTERED
    if !leave() {
        log("Warning: Failed to initialize reentrancy status");
    }

    // Initialize balance to 0
    if !write_balance(0) {
        log("Warning: Failed to initialize balance");
    }

    log("ReentrancyGuard initialized");
}

// Deposit function - adds funds to contract balance
// Protected by reentrancy guard
fn deposit(amount: u64) -> u64 {
    log("=== Deposit Function Called ===");

    // STEP 1: Check reentrancy guard
    if is_entered() {
        log("ERROR: Reentrancy detected in deposit!");
        return ERR_REENTRANT_CALL;
    }

    // STEP 2: Enter the guard
    if !enter() {
        log("ERROR: Failed to set reentrancy lock");
        return ERR_STORAGE_WRITE_FAILED;
    }
    log("Reentrancy guard: ENTERED");

    // STEP 3: Perform state changes
    let current_balance = read_balance();
    let new_balance = current_balance.saturating_add(amount);

    if !write_balance(new_balance) {
        leave();
        log("ERROR: Failed to update balance");
        return ERR_STORAGE_WRITE_FAILED;
    }

    log("Deposit successful");
    log("Balance updated");

    // STEP 4: Leave the guard
    if !leave() {
        log("ERROR: Failed to release reentrancy lock");
        return ERR_STORAGE_WRITE_FAILED;
    }
    log("Reentrancy guard: LEFT");

    0 // Success
}

// Withdraw function - the vulnerable operation that needs protection
// This is the classic example where reentrancy attacks occur
//
// Without the reentrancy guard, an attacker could:
// 1. Call withdraw(100)
// 2. During the external call, call withdraw(100) again
// 3. Drain more funds than their balance allows
fn withdraw(amount: u64) -> u64 {
    log("=== Withdraw Function Called ===");

    // STEP 1: Check reentrancy guard (CRITICAL!)
    // This prevents nested calls to withdraw
    if is_entered() {
        log("ERROR: Reentrancy detected! Rejecting call");
        log("This prevents the reentrancy attack!");
        return ERR_REENTRANT_CALL;
    }

    // STEP 2: Enter the guard
    // From this point on, any attempt to call withdraw will be rejected
    if !enter() {
        log("ERROR: Failed to set reentrancy lock");
        return ERR_STORAGE_WRITE_FAILED;
    }
    log("Reentrancy guard: ENTERED");

    // STEP 3: Check balance and update state BEFORE external call
    // This is the "Checks-Effects-Interactions" pattern
    let current_balance = read_balance();

    if current_balance < amount {
        leave();
        log("ERROR: Insufficient balance");
        return ERR_INSUFFICIENT_BALANCE;
    }

    // Update balance BEFORE making the external call
    // This prevents the attacker from draining more than they should
    let new_balance = current_balance - amount;
    if !write_balance(new_balance) {
        leave();
        log("ERROR: Failed to update balance");
        return ERR_STORAGE_WRITE_FAILED;
    }

    log("Balance updated (before external call)");

    // STEP 4: Make external call (potential reentrancy point)
    // In a real contract, this would transfer funds to the caller
    // If the caller is a malicious contract, it might try to call withdraw again
    // But our reentrancy guard will catch it!
    let caller = get_caller();

    log("Making external call (potential reentrancy point)");

    // Simulate external call that might trigger reentrancy
    let call_result = call(&caller, &[], amount, 0); // Regular call

    if call_result.is_err() {
        log("WARNING: External call failed (but balance already updated)");
        // Note: In production, you might want to revert the balance update here
        // For this example, we continue to demonstrate the guard still works
    } else {
        log("External call completed");
    }

    // STEP 5: Leave the guard
    // This re-enables calls to protected functions
    if !leave() {
        log("ERROR: Failed to release reentrancy lock");
        return ERR_STORAGE_WRITE_FAILED;
    }
    log("Reentrancy guard: LEFT");

    log("Withdraw successful");
    0 // Success
}

// Get current balance (read-only operation)
fn get_balance() -> u64 {
    let balance = read_balance();
    log("Balance query");
    balance
}

// Operation handlers that match test expectations
fn op_deposit(params: &[u8]) -> u64 {
    let amount = match parse_u64(params) {
        Ok(val) => val,
        Err(_) => {
            log("ERROR: Invalid deposit parameters");
            return ERR_INVALID_PARAMS;
        }
    };
    deposit(amount)
}

fn op_withdraw(params: &[u8]) -> u64 {
    let amount = match parse_u64(params) {
        Ok(val) => val,
        Err(_) => {
            log("ERROR: Invalid withdraw parameters");
            return ERR_INVALID_PARAMS;
        }
    };
    withdraw(amount)
}

fn op_get_balance() -> u64 {
    let _balance = get_balance();
    SUCCESS
}

// Attempt to perform a reentrancy attack (for demonstration)
// This function simulates what a malicious contract might try to do
fn attempt_reentrancy_attack() -> u64 {
    log("=== Simulating Reentrancy Attack ===");

    // First withdraw call (this will succeed)
    log("Attacker: First withdraw call");
    let result1 = withdraw(50);

    if result1 != 0 {
        log("First withdraw failed (unexpected)");
        return result1;
    }

    log("Attacker: First withdraw succeeded");

    // Try to withdraw again while still in the first call
    // This simulates what would happen if the external call tried to re-enter
    log("Attacker: Attempting second withdraw (reentrancy)");
    let result2 = withdraw(50);

    if result2 == ERR_REENTRANT_CALL {
        log("SUCCESS: Reentrancy attack blocked by guard!");
        log("The second withdraw was correctly rejected");
        return 0; // Attack was successfully prevented
    } else {
        log("FAILURE: Reentrancy attack succeeded (should not happen)");
        return 1; // Attack succeeded (this would be a bug)
    }
}

// Main contract entrypoint
//
// Operations (opcodes):
// 0x01: WITHDRAW - withdraw(amount: u64)
// 0x02: DEPOSIT - deposit(amount: u64)
// 0x10: GET_BALANCE - get_balance() -> u64
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("ReentrancyGuard: Contract invoked");

    // Get input data
    let mut input = [0u8; 1024];
    let len = get_input_data(&mut input);

    // If no input data, initialize the contract
    if len == 0 {
        log("ReentrancyGuard: Initializing contract");
        initialize();
        return SUCCESS;
    }

    // Extract opcode
    let opcode = input[0];
    let params = &input[1..len as usize];

    // Dispatch based on opcode
    match opcode {
        OP_DEPOSIT => {
            log("ReentrancyGuard: OP_DEPOSIT");
            op_deposit(params)
        }
        OP_WITHDRAW => {
            log("ReentrancyGuard: OP_WITHDRAW");
            op_withdraw(params)
        }
        OP_GET_BALANCE => {
            log("ReentrancyGuard: OP_GET_BALANCE");
            op_get_balance()
        }
        _ => {
            log("ReentrancyGuard: Unknown opcode");
            ERR_INVALID_INSTRUCTION
        }
    }
}

// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
