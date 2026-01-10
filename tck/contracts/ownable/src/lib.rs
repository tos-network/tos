//! Ownable Contract - OpenZeppelin-style Access Control
//!
//! A production-ready ownership access control implementation for TOS blockchain,
//! following OpenZeppelin's security patterns and best practices.
//!
//! # Features
//!
//! - Single owner access control
//! - Ownership transfer capability
//! - Ownership renunciation support
//! - Comprehensive error handling
//! - Detailed logging for all operations
//! - Storage-efficient design
//!
//! # Instruction Format
//!
//! All instructions follow the format: `[opcode:1][params:N]`
//!
//! ## Opcodes
//!
//! - 0x00: TransferOwnership - `[new_owner:32]` (owner-only)
//! - 0x01: RenounceOwnership - `` (owner-only)
//! - 0x10: Owner - `` (query)
//!
//! # Storage Layout
//!
//! - `owner` - [0x01] -> [u8; 32] (32-byte address)
//!
//! # Error Codes
//!
//! - 1001: Unauthorized (caller is not the owner)
//! - 1002: Invalid address (zero address not allowed for transfer)
//! - 1003: Invalid instruction
//! - 1004: Invalid parameters

#![no_std]
#![no_main]

use tako_sdk::*;

// ============================================================================
// Constants
// ============================================================================

/// Storage key for owner address
const KEY_OWNER: u8 = 0x01;

/// Instruction opcodes
const OP_TRANSFER_OWNERSHIP: u8 = 0x00;
const OP_RENOUNCE_OWNERSHIP: u8 = 0x01;
const OP_OWNER: u8 = 0x10;

/// Error codes
const ERR_UNAUTHORIZED: u64 = 1001;
const ERR_ZERO_ADDRESS: u64 = 1002;
const ERR_INVALID_INSTRUCTION: u64 = 1003;
const ERR_INVALID_PARAMS: u64 = 1004;

// ============================================================================
// Helper Functions
// ============================================================================

/// Get the current owner address
///
/// Returns the zero address if no owner is set
fn get_owner() -> [u8; 32] {
    let mut owner = [0u8; 32];
    let len = storage_read(&[KEY_OWNER], &mut owner);
    if len == 32 {
        owner
    } else {
        [0u8; 32]
    }
}

/// Set the owner address
fn set_owner(owner: &[u8; 32]) {
    let _ = storage_write(&[KEY_OWNER], owner);
}

/// Check if an address is the zero address
fn is_zero_address(address: &[u8; 32]) -> bool {
    address.iter().all(|&b| b == 0)
}

/// Check if the caller is the owner
///
/// Returns true if the caller is the owner, false otherwise
fn is_owner() -> bool {
    let caller = get_tx_sender();
    let owner = get_owner();
    caller == owner
}

// ============================================================================
// Core Operations
// ============================================================================

/// Transfer ownership to a new owner
///
/// Format: [new_owner:32]
///
/// Requirements:
/// - Caller must be the current owner
/// - New owner cannot be the zero address
fn op_transfer_ownership(params: &[u8]) -> u64 {
    log("Ownable: TransferOwnership");

    // Verify caller is the owner
    if !is_owner() {
        log("Ownable: Unauthorized - caller is not owner");
        return ERR_UNAUTHORIZED;
    }

    // Validate parameters
    if params.len() < 32 {
        log("Ownable: Invalid parameters - missing new owner");
        return ERR_INVALID_PARAMS;
    }

    // Parse new owner address
    let mut new_owner = [0u8; 32];
    new_owner.copy_from_slice(&params[0..32]);

    // Validate new owner is not zero address
    if is_zero_address(&new_owner) {
        log("Ownable: Invalid address - new owner cannot be zero address");
        return ERR_ZERO_ADDRESS;
    }

    // Get previous owner for logging
    let previous_owner = get_owner();

    // Transfer ownership
    set_owner(&new_owner);

    log("Ownable: Ownership transferred successfully");
    log("Ownable: Previous owner:");
    log_pubkey(&previous_owner);
    log("Ownable: New owner:");
    log_pubkey(&new_owner);

    SUCCESS
}

/// Renounce ownership (set owner to zero address)
///
/// Format: (no parameters)
///
/// Requirements:
/// - Caller must be the current owner
///
/// Warning: This will leave the contract without an owner,
/// disabling any functionality that requires owner access.
fn op_renounce_ownership() -> u64 {
    log("Ownable: RenounceOwnership");

    // Verify caller is the owner
    if !is_owner() {
        log("Ownable: Unauthorized - caller is not owner");
        return ERR_UNAUTHORIZED;
    }

    // Get previous owner for logging
    let previous_owner = get_owner();

    // Set owner to zero address
    let zero_address = [0u8; 32];
    set_owner(&zero_address);

    log("Ownable: Ownership renounced successfully");
    log("Ownable: Previous owner:");
    log_pubkey(&previous_owner);

    SUCCESS
}

// ============================================================================
// Query Operations
// ============================================================================

/// Get the current owner address
///
/// Format: (no parameters)
/// Returns: [owner:32]
fn op_owner() -> u64 {
    log("Ownable: Owner query");

    let owner = get_owner();

    // Return owner as return data
    match set_return_data(&owner) {
        Ok(_) => {
            log("Ownable: Owner query successful");
            log("Ownable: Owner:");
            log_pubkey(&owner);
            SUCCESS
        }
        Err(e) => {
            log("Ownable: Failed to set return data");
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
    log("Ownable: Contract invoked");

    // Get input data
    let mut input = [0u8; 1024];
    let len = get_input_data(&mut input);

    // If no input data, initialize with caller as owner
    if len == 0 {
        log("Ownable: Initializing with caller as owner");
        let caller = get_tx_sender();
        set_owner(&caller);
        log("Ownable: Initial owner:");
        log_pubkey(&caller);
        return SUCCESS;
    }

    // Extract opcode
    let opcode = input[0];
    let params = &input[1..len as usize];

    // Dispatch based on opcode
    match opcode {
        OP_TRANSFER_OWNERSHIP => op_transfer_ownership(params),
        OP_RENOUNCE_OWNERSHIP => op_renounce_ownership(),
        OP_OWNER => op_owner(),
        _ => {
            log("Ownable: Unknown opcode");
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
