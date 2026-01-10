//! Pausable Contract
//!
//! A contract module that implements emergency stop mechanism,
//! following OpenZeppelin's Pausable pattern.
//!
//! This contract allows authorized accounts to pause and unpause operations,
//! enabling emergency shutdown of critical functions.
//!
//! # Features
//!
//! - Pause/unpause functionality
//! - Owner-only access control
//! - State query capability
//! - Comprehensive error handling
//! - Detailed logging for all operations
//!
//! # Instruction Format
//!
//! All instructions follow the format: `[opcode:1][params:N]`
//!
//! ## Opcodes
//!
//! - 0x00: Initialize - `[owner:32]` - Initialize contract with owner
//! - 0x01: Pause - `[]` - Pause the contract (owner-only)
//! - 0x02: Unpause - `[]` - Unpause the contract (owner-only)
//! - 0x10: Paused - `[]` - Query if contract is paused (returns bool)
//! - 0x11: Owner - `[]` - Query contract owner (returns address)
//!
//! # Storage Layout
//!
//! - `initialized` - [0x01] -> u8 (1 if initialized)
//! - `paused` - [0x02] -> u8 (1 if paused, 0 if not paused)
//! - `owner` - [0x03] -> [u8; 32] (owner address)
//!
//! # Error Codes
//!
//! - 1001: ERR_PAUSED - Operation attempted while contract is paused
//! - 1002: ERR_NOT_PAUSED - Unpause attempted while contract is not paused
//! - 1003: ERR_UNAUTHORIZED - Operation attempted by non-owner
//! - 1004: ERR_ALREADY_INITIALIZED - Initialize called on already initialized contract
//! - 1005: ERR_NOT_INITIALIZED - Operation attempted on uninitialized contract
//! - 1006: ERR_INVALID_INSTRUCTION - Unknown opcode
//! - 1007: ERR_INVALID_PARAMS - Invalid instruction parameters

#![no_std]
#![no_main]

use tako_sdk::*;

// ============================================================================
// Constants
// ============================================================================

/// Storage key prefixes
const KEY_INITIALIZED: u8 = 0x01;
const KEY_PAUSED: u8 = 0x02;
const KEY_OWNER: u8 = 0x03;

/// Instruction opcodes
const OP_INITIALIZE: u8 = 0x00;
const OP_PAUSE: u8 = 0x01;
const OP_UNPAUSE: u8 = 0x02;
const OP_PAUSED: u8 = 0x10;
const OP_OWNER: u8 = 0x11;

/// Error codes
const ERR_PAUSED: u64 = 1001;
const ERR_NOT_PAUSED: u64 = 1002;
const ERR_UNAUTHORIZED: u64 = 1003;
const ERR_ALREADY_INITIALIZED: u64 = 1004;
const ERR_NOT_INITIALIZED: u64 = 1005;
const ERR_INVALID_INSTRUCTION: u64 = 1006;
const ERR_INVALID_PARAMS: u64 = 1007;

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

/// Check if contract is paused
fn is_paused() -> bool {
    let mut buffer = [0u8; 1];
    let len = storage_read(&[KEY_PAUSED], &mut buffer);
    len > 0 && buffer[0] == 1
}

/// Set paused state
fn set_paused(paused: bool) {
    let value = if paused { 1u8 } else { 0u8 };
    let _ = storage_write(&[KEY_PAUSED], &[value]);
}

/// Get owner address
fn get_owner() -> [u8; 32] {
    let mut owner = [0u8; 32];
    let len = storage_read(&[KEY_OWNER], &mut owner);
    if len == 32 {
        owner
    } else {
        [0u8; 32]
    }
}

/// Set owner address
fn set_owner(owner: &[u8; 32]) {
    let _ = storage_write(&[KEY_OWNER], owner);
}

/// Check if sender is the owner
fn require_owner() -> Result<(), u64> {
    let sender = get_tx_sender();
    let owner = get_owner();

    if sender != owner {
        log("Pausable: Unauthorized - caller is not the owner");
        return Err(ERR_UNAUTHORIZED);
    }

    Ok(())
}

/// Require contract to be paused
fn require_paused() -> Result<(), u64> {
    if !is_paused() {
        log("Pausable: Expected contract to be paused");
        return Err(ERR_NOT_PAUSED);
    }
    Ok(())
}

/// Require contract to not be paused
fn require_not_paused() -> Result<(), u64> {
    if is_paused() {
        log("Pausable: Contract is paused");
        return Err(ERR_PAUSED);
    }
    Ok(())
}

// ============================================================================
// Core Operations
// ============================================================================

/// Initialize the contract
///
/// Format: [owner:32]
fn op_initialize(params: &[u8]) -> u64 {
    log("Pausable: Initialize");

    // Check if already initialized
    if is_initialized() {
        log("Pausable: Already initialized");
        return ERR_ALREADY_INITIALIZED;
    }

    // Validate parameters
    if params.len() < 32 {
        log("Pausable: Invalid initialize parameters - expected owner address (32 bytes)");
        return ERR_INVALID_PARAMS;
    }

    // Parse owner address
    let mut owner = [0u8; 32];
    owner.copy_from_slice(&params[0..32]);

    // Store owner
    set_owner(&owner);

    // Initialize paused state to false
    set_paused(false);

    // Mark as initialized
    set_initialized();

    log("Pausable: Initialized successfully");
    log_pubkey(&owner);

    SUCCESS
}

/// Pause the contract (owner-only)
///
/// Format: []
fn op_pause() -> u64 {
    log("Pausable: Pause");

    // Check initialization
    if !is_initialized() {
        log("Pausable: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    // Require owner
    if let Err(e) = require_owner() {
        return e;
    }

    // Require not already paused
    if let Err(e) = require_not_paused() {
        return e;
    }

    // Set paused state
    set_paused(true);

    let sender = get_tx_sender();
    log("Pausable: Contract paused");
    log_pubkey(&sender);

    SUCCESS
}

/// Unpause the contract (owner-only)
///
/// Format: []
fn op_unpause() -> u64 {
    log("Pausable: Unpause");

    // Check initialization
    if !is_initialized() {
        log("Pausable: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    // Require owner
    if let Err(e) = require_owner() {
        return e;
    }

    // Require currently paused
    if let Err(e) = require_paused() {
        return e;
    }

    // Set unpaused state
    set_paused(false);

    let sender = get_tx_sender();
    log("Pausable: Contract unpaused");
    log_pubkey(&sender);

    SUCCESS
}

// ============================================================================
// Query Operations
// ============================================================================

/// Query if contract is paused
///
/// Format: []
/// Returns: [paused:1] (1 if paused, 0 if not)
fn op_query_paused() -> u64 {
    log("Pausable: Query paused state");

    // Check initialization
    if !is_initialized() {
        log("Pausable: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let paused = is_paused();
    let result = if paused { 1u8 } else { 0u8 };

    // Return paused state as return data
    match set_return_data(&[result]) {
        Ok(_) => {
            log("Pausable: Query successful");
            log_u64(result as u64, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("Pausable: Failed to set return data");
            e
        }
    }
}

/// Query contract owner
///
/// Format: []
/// Returns: [owner:32]
fn op_query_owner() -> u64 {
    log("Pausable: Query owner");

    // Check initialization
    if !is_initialized() {
        log("Pausable: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let owner = get_owner();

    // Return owner as return data
    match set_return_data(&owner) {
        Ok(_) => {
            log("Pausable: Query successful");
            log_pubkey(&owner);
            SUCCESS
        }
        Err(e) => {
            log("Pausable: Failed to set return data");
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
    log("Pausable: Contract invoked");

    // Get input data
    let mut input = [0u8; 256];
    let len = get_input_data(&mut input);

    if len == 0 {
        log("Pausable: No input data");
        return ERR_INVALID_INSTRUCTION;
    }

    // Extract opcode
    let opcode = input[0];
    let params = &input[1..len as usize];

    // Dispatch based on opcode
    match opcode {
        OP_INITIALIZE => op_initialize(params),
        OP_PAUSE => op_pause(),
        OP_UNPAUSE => op_unpause(),
        OP_PAUSED => op_query_paused(),
        OP_OWNER => op_query_owner(),
        _ => {
            log("Pausable: Unknown opcode");
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
