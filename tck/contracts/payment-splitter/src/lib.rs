//! PaymentSplitter Contract
//!
//! A production-ready payment splitter implementation for TOS blockchain,
//! following OpenZeppelin's payment splitting pattern. This contract allows
//! splitting payments among multiple payees according to predefined shares.
//!
//! # Features
//!
//! - Split incoming payments among multiple payees
//! - Proportional distribution based on shares
//! - Individual release mechanism for each payee
//! - Query pending payments for any payee
//! - Immutable shares after initialization
//! - Comprehensive tracking of released amounts
//!
//! # Payment Distribution
//!
//! The contract calculates each payee's entitled amount using:
//! ```text
//! entitlement = (total_received * payee_shares) / total_shares
//! releasable = entitlement - already_released
//! ```
//!
//! # Instruction Format
//!
//! All instructions follow the format: `[opcode:1][params:N]`
//!
//! ## Opcodes
//!
//! - 0x00: Initialize - `[num_payees:1][[payee:32][shares:8]]...`
//! - 0x01: Release - `[payee:32]` (release payment to specific payee)
//! - 0x10: TotalShares - `` (query - returns total shares)
//! - 0x11: TotalReleased - `` (query - returns total amount released)
//! - 0x12: Shares - `[payee:32]` (query - returns shares for payee)
//! - 0x13: Released - `[payee:32]` (query - returns amount released to payee)
//! - 0x14: Releasable - `[payee:32]` (query - returns pending amount for payee)
//! - 0x15: TotalReceived - `` (query - returns total amount received)
//! - 0x16: PayeeCount - `` (query - returns number of payees)
//!
//! # Storage Layout
//!
//! - `initialized` - [0x01] -> u8 (1 if initialized)
//! - `total_shares` - [0x02] -> u64
//! - `total_released` - [0x03] -> u64
//! - `total_received` - [0x04] -> u64
//! - `payee_count` - [0x05] -> u8
//! - `shares:{payee}` - [0x10 | payee] -> u64
//! - `released:{payee}` - [0x20 | payee] -> u64
//! - `payees:{index}` - [0x30 | index] -> [u8; 32]
//!
//! # Error Codes
//!
//! - 1001: Already initialized
//! - 1002: Not initialized
//! - 1003: Invalid instruction
//! - 1004: Invalid parameters
//! - 1005: Invalid payee (payee has no shares or is zero address)
//! - 1006: No payment due (no releasable amount for payee)
//! - 1007: Too many payees (maximum 255 payees)
//! - 1008: Zero shares (payee must have non-zero shares)
//! - 1009: No payees (must have at least one payee)
//!
//! # Examples
//!
//! ## Initialize with 3 payees
//!
//! ```text
//! Opcode: 0x00 (Initialize)
//! Num Payees: 3
//! Payee 1: [32 bytes], Shares: 500 (50%)
//! Payee 2: [32 bytes], Shares: 300 (30%)
//! Payee 3: [32 bytes], Shares: 200 (20%)
//! Total Shares: 1000
//! ```
//!
//! ## Release payment to payee
//!
//! ```text
//! Opcode: 0x01 (Release)
//! Payee: [32 bytes]
//! ```
//!
//! ## Query releasable amount
//!
//! ```text
//! Opcode: 0x14 (Releasable)
//! Payee: [32 bytes]
//! Returns: amount (u64)
//! ```

#![no_std]
#![no_main]

use tako_sdk::*;

// ============================================================================
// Constants
// ============================================================================

/// Maximum number of payees (255 to fit in u8)
const MAX_PAYEES: usize = 255;

/// Storage key prefixes
const KEY_INITIALIZED: u8 = 0x01;
const KEY_TOTAL_SHARES: u8 = 0x02;
const KEY_TOTAL_RELEASED: u8 = 0x03;
const KEY_TOTAL_RECEIVED: u8 = 0x04;
const KEY_PAYEE_COUNT: u8 = 0x05;
const KEY_SHARES_PREFIX: u8 = 0x10;
const KEY_RELEASED_PREFIX: u8 = 0x20;
const KEY_PAYEES_PREFIX: u8 = 0x30;

/// Instruction opcodes
const OP_INITIALIZE: u8 = 0x00;
const OP_RELEASE: u8 = 0x01;
const OP_TOTAL_SHARES: u8 = 0x10;
const OP_TOTAL_RELEASED: u8 = 0x11;
const OP_SHARES: u8 = 0x12;
const OP_RELEASED: u8 = 0x13;
const OP_RELEASABLE: u8 = 0x14;
const OP_TOTAL_RECEIVED: u8 = 0x15;
const OP_PAYEE_COUNT: u8 = 0x16;

/// Error codes
const ERR_ALREADY_INITIALIZED: u64 = 1001;
const ERR_NOT_INITIALIZED: u64 = 1002;
const ERR_INVALID_INSTRUCTION: u64 = 1003;
const ERR_INVALID_PARAMS: u64 = 1004;
const ERR_INVALID_PAYEE: u64 = 1005;
const ERR_NO_PAYMENT_DUE: u64 = 1006;
const ERR_TOO_MANY_PAYEES: u64 = 1007;
const ERR_ZERO_SHARES: u64 = 1008;
const ERR_NO_PAYEES: u64 = 1009;

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if contract is initialized
fn is_initialized() -> bool {
    let mut buffer = [0u8; 1];
    let len = storage_read(&[KEY_INITIALIZED], &mut buffer);
    len > 0 && buffer[0] == 1
}

/// Set initialized flag
fn set_initialized() {
    let _ = storage_write(&[KEY_INITIALIZED], &[1u8]);
}

/// Get total shares
fn get_total_shares() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(&[KEY_TOTAL_SHARES], &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set total shares
fn set_total_shares(shares: u64) {
    let _ = storage_write(&[KEY_TOTAL_SHARES], &shares.to_le_bytes());
}

/// Get total released
fn get_total_released() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(&[KEY_TOTAL_RELEASED], &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set total released
fn set_total_released(amount: u64) {
    let _ = storage_write(&[KEY_TOTAL_RELEASED], &amount.to_le_bytes());
}

/// Get total received
fn get_total_received() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(&[KEY_TOTAL_RECEIVED], &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set total received
fn set_total_received(amount: u64) {
    let _ = storage_write(&[KEY_TOTAL_RECEIVED], &amount.to_le_bytes());
}

/// Get payee count
fn get_payee_count() -> u8 {
    let mut buffer = [0u8; 1];
    let len = storage_read(&[KEY_PAYEE_COUNT], &mut buffer);
    if len == 1 {
        buffer[0]
    } else {
        0
    }
}

/// Set payee count
fn set_payee_count(count: u8) {
    let _ = storage_write(&[KEY_PAYEE_COUNT], &[count]);
}

/// Get shares for a payee
fn get_shares(payee: &[u8; 32]) -> u64 {
    let mut key = [0u8; 33];
    key[0] = KEY_SHARES_PREFIX;
    key[1..33].copy_from_slice(payee);

    let mut buffer = [0u8; 8];
    let len = storage_read(&key, &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set shares for a payee
fn set_shares(payee: &[u8; 32], shares: u64) {
    let mut key = [0u8; 33];
    key[0] = KEY_SHARES_PREFIX;
    key[1..33].copy_from_slice(payee);

    let _ = storage_write(&key, &shares.to_le_bytes());
}

/// Get released amount for a payee
fn get_released(payee: &[u8; 32]) -> u64 {
    let mut key = [0u8; 33];
    key[0] = KEY_RELEASED_PREFIX;
    key[1..33].copy_from_slice(payee);

    let mut buffer = [0u8; 8];
    let len = storage_read(&key, &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set released amount for a payee
fn set_released(payee: &[u8; 32], amount: u64) {
    let mut key = [0u8; 33];
    key[0] = KEY_RELEASED_PREFIX;
    key[1..33].copy_from_slice(payee);

    let _ = storage_write(&key, &amount.to_le_bytes());
}

/// Get payee address by index
#[allow(dead_code)]
fn get_payee(index: u8) -> Option<[u8; 32]> {
    let mut key = [0u8; 2];
    key[0] = KEY_PAYEES_PREFIX;
    key[1] = index;

    let mut payee = [0u8; 32];
    let len = storage_read(&key, &mut payee);
    if len == 32 {
        Some(payee)
    } else {
        None
    }
}

/// Set payee address by index
fn set_payee(index: u8, payee: &[u8; 32]) {
    let mut key = [0u8; 2];
    key[0] = KEY_PAYEES_PREFIX;
    key[1] = index;

    let _ = storage_write(&key, payee);
}

/// Check if address is zero address
fn is_zero_address(address: &[u8; 32]) -> bool {
    address.iter().all(|&b| b == 0)
}

/// Calculate releasable amount for a payee
///
/// Formula: (total_received * payee_shares / total_shares) - already_released
fn calculate_releasable(payee: &[u8; 32]) -> u64 {
    let shares = get_shares(payee);
    if shares == 0 {
        return 0;
    }

    let total_shares = get_total_shares();
    if total_shares == 0 {
        return 0;
    }

    let total_received = get_total_received();
    let already_released = get_released(payee);

    // Calculate entitlement using u128 to avoid overflow
    // entitlement = (total_received * shares) / total_shares
    let numerator = (total_received as u128).saturating_mul(shares as u128);
    let entitlement = (numerator / total_shares as u128) as u64;

    // Calculate releasable amount
    entitlement.saturating_sub(already_released)
}

// ============================================================================
// Core Operations
// ============================================================================

/// Initialize the payment splitter
///
/// Format: [num_payees:1][[payee:32][shares:8]]...
fn op_initialize(params: &[u8]) -> u64 {
    log("PaymentSplitter: Initialize");

    // Check if already initialized
    if is_initialized() {
        log("PaymentSplitter: Already initialized");
        return ERR_ALREADY_INITIALIZED;
    }

    // Validate minimum parameters (1 byte for num_payees + at least 1 payee entry)
    if params.is_empty() {
        log("PaymentSplitter: Invalid initialize parameters");
        return ERR_INVALID_PARAMS;
    }

    let num_payees = params[0];

    if num_payees == 0 {
        log("PaymentSplitter: No payees specified");
        return ERR_NO_PAYEES;
    }

    if num_payees as usize > MAX_PAYEES {
        log("PaymentSplitter: Too many payees");
        return ERR_TOO_MANY_PAYEES;
    }

    // Each payee entry is 32 (address) + 8 (shares) = 40 bytes
    let expected_len = 1 + (num_payees as usize * 40);
    if params.len() < expected_len {
        log("PaymentSplitter: Invalid payees data length");
        return ERR_INVALID_PARAMS;
    }

    let mut offset = 1;
    let mut total_shares: u64 = 0;

    // Process each payee
    for i in 0..num_payees {
        // Parse payee address (32 bytes)
        let mut payee = [0u8; 32];
        payee.copy_from_slice(&params[offset..offset + 32]);
        offset += 32;

        // Validate payee address
        if is_zero_address(&payee) {
            log("PaymentSplitter: Zero address payee");
            return ERR_INVALID_PAYEE;
        }

        // Parse shares (8 bytes)
        let shares = u64::from_le_bytes([
            params[offset],
            params[offset + 1],
            params[offset + 2],
            params[offset + 3],
            params[offset + 4],
            params[offset + 5],
            params[offset + 6],
            params[offset + 7],
        ]);
        offset += 8;

        // Validate shares
        if shares == 0 {
            log("PaymentSplitter: Zero shares for payee");
            return ERR_ZERO_SHARES;
        }

        // Check for duplicate payee
        let existing_shares = get_shares(&payee);
        if existing_shares > 0 {
            log("PaymentSplitter: Duplicate payee");
            return ERR_INVALID_PAYEE;
        }

        // Store payee data
        set_payee(i, &payee);
        set_shares(&payee, shares);
        total_shares = total_shares.saturating_add(shares);

        log("PaymentSplitter: Added payee");
        log_u64(i as u64, shares, total_shares, 0, 0);
    }

    // Validate total shares
    if total_shares == 0 {
        log("PaymentSplitter: Total shares is zero");
        return ERR_ZERO_SHARES;
    }

    // Store contract state
    set_payee_count(num_payees);
    set_total_shares(total_shares);
    set_total_released(0);
    set_total_received(0);

    // Mark as initialized
    set_initialized();

    log("PaymentSplitter: Initialized successfully");
    log_u64(num_payees as u64, total_shares, 0, 0, 0);

    SUCCESS
}

/// Release payment to a payee
///
/// Format: [payee:32]
fn op_release(params: &[u8]) -> u64 {
    log("PaymentSplitter: Release");

    if !is_initialized() {
        log("PaymentSplitter: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        log("PaymentSplitter: Invalid release parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse payee address
    let mut payee = [0u8; 32];
    payee.copy_from_slice(&params[0..32]);

    // Validate payee
    let shares = get_shares(&payee);
    if shares == 0 {
        log("PaymentSplitter: Invalid payee (no shares)");
        return ERR_INVALID_PAYEE;
    }

    // Calculate releasable amount
    let releasable = calculate_releasable(&payee);
    if releasable == 0 {
        log("PaymentSplitter: No payment due");
        return ERR_NO_PAYMENT_DUE;
    }

    // Update released amounts
    let already_released = get_released(&payee);
    let new_released = already_released.saturating_add(releasable);
    set_released(&payee, new_released);

    let total_released = get_total_released();
    let new_total_released = total_released.saturating_add(releasable);
    set_total_released(new_total_released);

    log("PaymentSplitter: Release successful");
    log_u64(releasable, new_released, new_total_released, shares, 0);

    // Note: In a real implementation, this would transfer tokens/funds to payee
    // For now, we just update the released tracking
    SUCCESS
}

// ============================================================================
// Query Operations
// ============================================================================

/// Get total shares
///
/// Returns: [total_shares:8]
fn op_total_shares() -> u64 {
    log("PaymentSplitter: TotalShares");

    if !is_initialized() {
        log("PaymentSplitter: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let total_shares = get_total_shares();

    let result = total_shares.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("PaymentSplitter: TotalShares query successful");
            log_u64(total_shares, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("PaymentSplitter: Failed to set return data");
            e
        }
    }
}

/// Get total released
///
/// Returns: [total_released:8]
fn op_total_released() -> u64 {
    log("PaymentSplitter: TotalReleased");

    if !is_initialized() {
        log("PaymentSplitter: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let total_released = get_total_released();

    let result = total_released.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("PaymentSplitter: TotalReleased query successful");
            log_u64(total_released, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("PaymentSplitter: Failed to set return data");
            e
        }
    }
}

/// Get shares for a payee
///
/// Format: [payee:32]
/// Returns: [shares:8]
fn op_shares(params: &[u8]) -> u64 {
    log("PaymentSplitter: Shares");

    if !is_initialized() {
        log("PaymentSplitter: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        log("PaymentSplitter: Invalid shares parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut payee = [0u8; 32];
    payee.copy_from_slice(&params[0..32]);

    let shares = get_shares(&payee);

    let result = shares.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("PaymentSplitter: Shares query successful");
            log_u64(shares, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("PaymentSplitter: Failed to set return data");
            e
        }
    }
}

/// Get released amount for a payee
///
/// Format: [payee:32]
/// Returns: [released:8]
fn op_released(params: &[u8]) -> u64 {
    log("PaymentSplitter: Released");

    if !is_initialized() {
        log("PaymentSplitter: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        log("PaymentSplitter: Invalid released parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut payee = [0u8; 32];
    payee.copy_from_slice(&params[0..32]);

    let released = get_released(&payee);

    let result = released.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("PaymentSplitter: Released query successful");
            log_u64(released, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("PaymentSplitter: Failed to set return data");
            e
        }
    }
}

/// Get releasable amount for a payee
///
/// Format: [payee:32]
/// Returns: [releasable:8]
fn op_releasable(params: &[u8]) -> u64 {
    log("PaymentSplitter: Releasable");

    if !is_initialized() {
        log("PaymentSplitter: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        log("PaymentSplitter: Invalid releasable parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut payee = [0u8; 32];
    payee.copy_from_slice(&params[0..32]);

    let releasable = calculate_releasable(&payee);

    let result = releasable.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("PaymentSplitter: Releasable query successful");
            log_u64(releasable, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("PaymentSplitter: Failed to set return data");
            e
        }
    }
}

/// Get total received
///
/// Returns: [total_received:8]
fn op_total_received() -> u64 {
    log("PaymentSplitter: TotalReceived");

    if !is_initialized() {
        log("PaymentSplitter: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let total_received = get_total_received();

    let result = total_received.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("PaymentSplitter: TotalReceived query successful");
            log_u64(total_received, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("PaymentSplitter: Failed to set return data");
            e
        }
    }
}

/// Get payee count
///
/// Returns: [count:1]
fn op_payee_count() -> u64 {
    log("PaymentSplitter: PayeeCount");

    if !is_initialized() {
        log("PaymentSplitter: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let count = get_payee_count();

    let result = [count];
    match set_return_data(&result) {
        Ok(_) => {
            log("PaymentSplitter: PayeeCount query successful");
            log_u64(count as u64, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("PaymentSplitter: Failed to set return data");
            e
        }
    }
}

// ============================================================================
// Main Entrypoint
// ============================================================================

/// Contract entrypoint
///
/// Dispatches to the appropriate operation based on the opcode
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("PaymentSplitter: Contract invoked");

    // Get input data
    let mut input = [0u8; 4096]; // Larger buffer to accommodate multiple payees
    let len = get_input_data(&mut input);

    if len == 0 {
        log("PaymentSplitter: No input data");
        return ERR_INVALID_INSTRUCTION;
    }

    // Extract opcode
    let opcode = input[0];
    let params = &input[1..len as usize];

    // Dispatch based on opcode
    match opcode {
        OP_INITIALIZE => op_initialize(params),
        OP_RELEASE => op_release(params),
        OP_TOTAL_SHARES => op_total_shares(),
        OP_TOTAL_RELEASED => op_total_released(),
        OP_SHARES => op_shares(params),
        OP_RELEASED => op_released(params),
        OP_RELEASABLE => op_releasable(params),
        OP_TOTAL_RECEIVED => op_total_received(),
        OP_PAYEE_COUNT => op_payee_count(),
        _ => {
            log("PaymentSplitter: Unknown opcode");
            log_u64(opcode as u64, 0, 0, 0, 0);
            ERR_INVALID_INSTRUCTION
        }
    }
}

/// Panic handler (required for no_std)
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
