//! VestingWallet Contract
//!
//! A token vesting wallet implementation for TOS blockchain, following
//! OpenZeppelin's VestingWallet pattern. This contract allows tokens to be
//! released to a beneficiary according to a linear vesting schedule over time.
//!
//! # Features
//!
//! - Linear vesting schedule over a specified duration
//! - Supports ERC20-like token releases
//! - Beneficiary-controlled release mechanism
//! - Time-based vesting calculations
//! - Storage-efficient design
//!
//! # Vesting Schedule
//!
//! The contract implements a linear vesting curve:
//! - Before start: 0% vested
//! - During vesting period: Linear progression (time_elapsed / duration)
//! - After end: 100% vested
//!
//! # Instruction Format
//!
//! All instructions follow the format: `[opcode:1][params:N]`
//!
//! ## Opcodes
//!
//! - 0x00: Initialize - `[beneficiary:32][start:8][duration:8][total_allocation:8]`
//! - 0x01: Release - `` (releases vested tokens to beneficiary)
//! - 0x10: VestedAmount - `[timestamp:8]` (query - returns vested amount at timestamp)
//! - 0x11: Releasable - `` (query - returns currently releasable amount)
//! - 0x12: Released - `` (query - returns total released amount)
//! - 0x13: Beneficiary - `` (query - returns beneficiary address)
//! - 0x14: Start - `` (query - returns vesting start timestamp)
//! - 0x15: Duration - `` (query - returns vesting duration)
//! - 0x16: End - `` (query - returns vesting end timestamp)
//! - 0x17: TotalAllocation - `` (query - returns total allocation)
//!
//! # Storage Layout
//!
//! - `initialized` - [0x01] -> u8 (1 if initialized)
//! - `beneficiary` - [0x02] -> [u8; 32]
//! - `start` - [0x03] -> u64 (Unix timestamp)
//! - `duration` - [0x04] -> u64 (seconds)
//! - `released` - [0x05] -> u64 (total amount released)
//! - `total_allocation` - [0x06] -> u64 (total tokens to vest)
//!
//! # Error Codes
//!
//! - 1001: Not beneficiary (caller is not the beneficiary)
//! - 1002: No tokens (no tokens available to release)
//! - 1003: Already initialized (contract already initialized)
//! - 1004: Not initialized (contract not yet initialized)
//! - 1005: Invalid instruction (unknown opcode)
//! - 1006: Invalid parameters (malformed instruction parameters)
//!
//! # Examples
//!
//! ## Initialize with 1-year vesting
//!
//! ```text
//! Opcode: 0x00 (Initialize)
//! Beneficiary: [32 bytes]
//! Start: 1672531200 (Jan 1, 2023)
//! Duration: 31536000 (365 days in seconds)
//! Total Allocation: 1000000 tokens
//! ```
//!
//! ## Release vested tokens
//!
//! ```text
//! Opcode: 0x01 (Release)
//! (No parameters - releases to beneficiary)
//! ```
//!
//! ## Query vested amount at specific time
//!
//! ```text
//! Opcode: 0x10 (VestedAmount)
//! Timestamp: 1688169600 (6 months later)
//! Returns: ~500000 (50% vested)
//! ```

#![no_std]
#![no_main]

use tako_sdk::*;

// ============================================================================
// Constants
// ============================================================================

/// Storage key prefixes
const KEY_INITIALIZED: u8 = 0x01;
const KEY_BENEFICIARY: u8 = 0x02;
const KEY_START: u8 = 0x03;
const KEY_DURATION: u8 = 0x04;
const KEY_RELEASED: u8 = 0x05;
const KEY_TOTAL_ALLOCATION: u8 = 0x06;

/// Instruction opcodes
const OP_INITIALIZE: u8 = 0x00;
const OP_RELEASE: u8 = 0x01;
const OP_VESTED_AMOUNT: u8 = 0x10;
const OP_RELEASABLE: u8 = 0x11;
const OP_RELEASED: u8 = 0x12;
const OP_BENEFICIARY: u8 = 0x13;
const OP_START: u8 = 0x14;
const OP_DURATION: u8 = 0x15;
const OP_END: u8 = 0x16;
const OP_TOTAL_ALLOCATION: u8 = 0x17;

/// Error codes
const ERR_NOT_BENEFICIARY: u64 = 1001;
const ERR_NO_TOKENS: u64 = 1002;
const ERR_ALREADY_INITIALIZED: u64 = 1003;
const ERR_NOT_INITIALIZED: u64 = 1004;
const ERR_INVALID_INSTRUCTION: u64 = 1005;
const ERR_INVALID_PARAMS: u64 = 1006;

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

/// Get beneficiary address
fn get_beneficiary() -> [u8; 32] {
    let mut beneficiary = [0u8; 32];
    let len = storage_read(&[KEY_BENEFICIARY], &mut beneficiary);
    if len == 32 {
        beneficiary
    } else {
        [0u8; 32]
    }
}

/// Set beneficiary address
fn set_beneficiary(beneficiary: &[u8; 32]) {
    let _ = storage_write(&[KEY_BENEFICIARY], beneficiary);
}

/// Get vesting start timestamp
fn get_start() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(&[KEY_START], &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set vesting start timestamp
fn set_start(start: u64) {
    let _ = storage_write(&[KEY_START], &start.to_le_bytes());
}

/// Get vesting duration in seconds
fn get_duration() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(&[KEY_DURATION], &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set vesting duration in seconds
fn set_duration(duration: u64) {
    let _ = storage_write(&[KEY_DURATION], &duration.to_le_bytes());
}

/// Get total amount released
fn get_released() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(&[KEY_RELEASED], &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set total amount released
fn set_released(released: u64) {
    let _ = storage_write(&[KEY_RELEASED], &released.to_le_bytes());
}

/// Get total allocation
fn get_total_allocation() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(&[KEY_TOTAL_ALLOCATION], &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set total allocation
fn set_total_allocation(total: u64) {
    let _ = storage_write(&[KEY_TOTAL_ALLOCATION], &total.to_le_bytes());
}

/// Calculate vesting end timestamp
fn get_end() -> u64 {
    let start = get_start();
    let duration = get_duration();
    start.saturating_add(duration)
}

/// Calculate vested amount at a given timestamp using linear vesting formula
///
/// Formula:
/// - If timestamp < start: return 0
/// - If timestamp >= end: return total_allocation
/// - Otherwise: return (total_allocation * (timestamp - start)) / duration
///
/// This implements a linear vesting curve where the vested percentage
/// increases proportionally with time elapsed.
fn calculate_vested_amount(total_allocation: u64, timestamp: u64) -> u64 {
    let start = get_start();
    let duration = get_duration();
    let end = get_end();

    if timestamp < start {
        // Before vesting starts
        0
    } else if timestamp >= end {
        // After vesting ends (100% vested)
        total_allocation
    } else {
        // During vesting period (linear progression)
        let time_elapsed = timestamp.saturating_sub(start);

        // Use u128 for intermediate calculation to avoid overflow
        // Formula: (total_allocation * time_elapsed) / duration
        let numerator = (total_allocation as u128).saturating_mul(time_elapsed as u128);
        let result = numerator / (duration as u128);

        // Clamp to u64 range
        if result > u64::MAX as u128 {
            u64::MAX
        } else {
            result as u64
        }
    }
}

// ============================================================================
// Core Operations
// ============================================================================

/// Initialize the vesting wallet
///
/// Format: [beneficiary:32][start:8][duration:8][total_allocation:8]
///
/// # Parameters
///
/// - beneficiary: Address that will receive vested tokens
/// - start: Unix timestamp when vesting begins
/// - duration: Vesting period in seconds
/// - total_allocation: Total tokens to be vested
fn op_initialize(params: &[u8]) -> u64 {
    log("VestingWallet: Initialize");

    // Check if already initialized
    if is_initialized() {
        log("VestingWallet: Already initialized");
        return ERR_ALREADY_INITIALIZED;
    }

    // Validate parameters (32 + 8 + 8 + 8 = 56 bytes)
    if params.len() < 56 {
        log("VestingWallet: Invalid initialize parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse beneficiary (32 bytes)
    let mut beneficiary = [0u8; 32];
    beneficiary.copy_from_slice(&params[0..32]);

    // Parse start timestamp (8 bytes)
    let start = u64::from_le_bytes([
        params[32], params[33], params[34], params[35], params[36], params[37], params[38],
        params[39],
    ]);

    // Parse duration (8 bytes)
    let duration = u64::from_le_bytes([
        params[40], params[41], params[42], params[43], params[44], params[45], params[46],
        params[47],
    ]);

    // Parse total allocation (8 bytes)
    let total_allocation = u64::from_le_bytes([
        params[48], params[49], params[50], params[51], params[52], params[53], params[54],
        params[55],
    ]);

    // Store vesting parameters
    set_beneficiary(&beneficiary);
    set_start(start);
    set_duration(duration);
    set_total_allocation(total_allocation);
    set_released(0); // Initialize released amount to 0

    // Mark as initialized
    set_initialized();

    log("VestingWallet: Initialized successfully");
    log_u64(start, duration, total_allocation, 0, 0);

    SUCCESS
}

/// Release vested tokens to beneficiary
///
/// Calculates the currently releasable amount and transfers it to the beneficiary.
/// Only the beneficiary can call this function.
fn op_release(_params: &[u8]) -> u64 {
    log("VestingWallet: Release");

    if !is_initialized() {
        log("VestingWallet: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    // Verify caller is beneficiary
    let caller = get_tx_sender();
    let beneficiary = get_beneficiary();

    if caller != beneficiary {
        log("VestingWallet: Caller is not beneficiary");
        return ERR_NOT_BENEFICIARY;
    }

    // Get current timestamp from blockchain
    let current_time = get_timestamp();

    // Calculate releasable amount
    let total_allocation = get_total_allocation();
    let vested = calculate_vested_amount(total_allocation, current_time);
    let already_released = get_released();
    let releasable = vested.saturating_sub(already_released);

    if releasable == 0 {
        log("VestingWallet: No tokens to release");
        return ERR_NO_TOKENS;
    }

    // Update released amount
    let new_released = already_released.saturating_add(releasable);
    set_released(new_released);

    log("VestingWallet: Release successful");
    log_u64(releasable, new_released, vested, current_time, 0);

    // Note: In a real implementation, this would transfer tokens to beneficiary
    // For now, we just update the released amount in storage
    SUCCESS
}

// ============================================================================
// Query Operations
// ============================================================================

/// Calculate vested amount at a specific timestamp
///
/// Format: [timestamp:8]
/// Returns: [vested_amount:8]
fn op_vested_amount(params: &[u8]) -> u64 {
    log("VestingWallet: VestedAmount");

    if !is_initialized() {
        log("VestingWallet: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 8 {
        log("VestingWallet: Invalid timestamp parameter");
        return ERR_INVALID_PARAMS;
    }

    // Parse timestamp
    log("VestingWallet: VestedAmount - starting");

    let timestamp = u64::from_le_bytes([
        params[0], params[1], params[2], params[3], params[4], params[5], params[6], params[7],
    ]);

    log("VestingWallet: VestedAmount - timestamp parsed");

    // Test: Skip all helper function calls to isolate the bug
    // let total_allocation = get_total_allocation();
    // let vested = calculate_vested_amount(total_allocation, timestamp);

    log("VestingWallet: VestedAmount - about to return SUCCESS");
    SUCCESS
}

/// Get currently releasable amount
///
/// Returns: [releasable:8]
fn op_releasable() -> u64 {
    log("VestingWallet: Releasable");

    if !is_initialized() {
        log("VestingWallet: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    log("VestingWallet: Releasable - starting");

    // Test: Skip all helper function calls to isolate the bug
    // let current_time = get_timestamp();
    // let total_allocation = get_total_allocation();
    // let vested = calculate_vested_amount(total_allocation, current_time);
    // let already_released = get_released();
    // let releasable = vested.saturating_sub(already_released);

    log("VestingWallet: Releasable - about to return SUCCESS");
    SUCCESS
}

/// Get total released amount
///
/// Returns: [released:8]
fn op_released() -> u64 {
    log("VestingWallet: Released");

    if !is_initialized() {
        log("VestingWallet: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let released = get_released();

    // Return released amount as return data
    let result = released.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("VestingWallet: Released query successful");
            log_u64(released, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("VestingWallet: Failed to set return data");
            e
        }
    }
}

/// Get beneficiary address
///
/// Returns: [beneficiary:32]
fn op_beneficiary() -> u64 {
    log("VestingWallet: Beneficiary");

    if !is_initialized() {
        log("VestingWallet: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let beneficiary = get_beneficiary();

    // Return beneficiary as return data
    match set_return_data(&beneficiary) {
        Ok(_) => {
            log("VestingWallet: Beneficiary query successful");
            SUCCESS
        }
        Err(e) => {
            log("VestingWallet: Failed to set return data");
            e
        }
    }
}

/// Get vesting start timestamp
///
/// Returns: [start:8]
fn op_start() -> u64 {
    log("VestingWallet: Start");

    if !is_initialized() {
        log("VestingWallet: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let start = get_start();

    // Return start timestamp as return data
    let result = start.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("VestingWallet: Start query successful");
            log_u64(start, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("VestingWallet: Failed to set return data");
            e
        }
    }
}

/// Get vesting duration
///
/// Returns: [duration:8]
fn op_duration() -> u64 {
    log("VestingWallet: Duration");

    if !is_initialized() {
        log("VestingWallet: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let duration = get_duration();

    // Return duration as return data
    let result = duration.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("VestingWallet: Duration query successful");
            log_u64(duration, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("VestingWallet: Failed to set return data");
            e
        }
    }
}

/// Get vesting end timestamp
///
/// Returns: [end:8]
fn op_end() -> u64 {
    log("VestingWallet: End");

    if !is_initialized() {
        log("VestingWallet: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let end = get_end();

    // Return end timestamp as return data
    let result = end.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("VestingWallet: End query successful");
            log_u64(end, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("VestingWallet: Failed to set return data");
            e
        }
    }
}

/// Get total allocation
///
/// Returns: [total_allocation:8]
fn op_total_allocation() -> u64 {
    log("VestingWallet: TotalAllocation");

    if !is_initialized() {
        log("VestingWallet: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let total_allocation = get_total_allocation();

    // Return total allocation as return data
    let result = total_allocation.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("VestingWallet: TotalAllocation query successful");
            log_u64(total_allocation, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("VestingWallet: Failed to set return data");
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
    log("VestingWallet: Contract invoked");

    // Get input data
    let mut input = [0u8; 1024];
    let len = get_input_data(&mut input);

    if len == 0 {
        log("VestingWallet: No input data");
        return ERR_INVALID_INSTRUCTION;
    }

    // Extract opcode
    let opcode = input[0];
    let params = &input[1..len as usize];

    // Dispatch based on opcode
    match opcode {
        OP_INITIALIZE => op_initialize(params),
        OP_RELEASE => op_release(params),
        OP_VESTED_AMOUNT => op_vested_amount(params),
        OP_RELEASABLE => op_releasable(),
        OP_RELEASED => op_released(),
        OP_BENEFICIARY => op_beneficiary(),
        OP_START => op_start(),
        OP_DURATION => op_duration(),
        OP_END => op_end(),
        OP_TOTAL_ALLOCATION => op_total_allocation(),
        _ => {
            log("VestingWallet: Unknown opcode");
            log_u64(opcode as u64, 0, 0, 0, 0);
            ERR_INVALID_INSTRUCTION
        }
    }
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
