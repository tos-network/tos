//! AccessControl - Role-Based Access Control (RBAC) Contract
//!
//! A production-ready RBAC implementation for TOS blockchain,
//! following OpenZeppelin's AccessControl pattern and best practices.
//!
//! # Features
//!
//! - Role-based access control with hierarchical admin roles
//! - Grant, revoke, and renounce role operations
//! - DEFAULT_ADMIN_ROLE (0x00) controls all other roles
//! - Secure role admin management
//! - Comprehensive error handling
//! - Detailed logging for all operations
//! - Storage-efficient design using role:{roleId}:{account} pattern
//!
//! # Role System
//!
//! Roles are 32-byte identifiers (like keccak256 hashes). Each role has:
//! - **Members**: Accounts that have been granted the role
//! - **Admin Role**: The role that controls granting/revoking this role
//!
//! By default, all roles have DEFAULT_ADMIN_ROLE (0x00) as their admin.
//!
//! # Instruction Format
//!
//! All instructions follow the format: `[opcode:1][params:N]`
//!
//! ## Opcodes
//!
//! - 0x00: Initialize - `[]` (sets deployer as DEFAULT_ADMIN_ROLE)
//! - 0x01: GrantRole - `[role:32][account:32]`
//! - 0x02: RevokeRole - `[role:32][account:32]`
//! - 0x03: RenounceRole - `[role:32]`
//! - 0x04: SetRoleAdmin - `[role:32][adminRole:32]`
//! - 0x10: HasRole - `[role:32][account:32]` (query)
//! - 0x11: GetRoleAdmin - `[role:32]` (query)
//!
//! # Storage Layout
//!
//! - `initialized` - [0x01] -> u8 (1 if initialized)
//! - `role:{roleId}:{account}` - [0x10 | roleId(32) | account(32)] -> u8 (1 if has role)
//! - `role_admin:{roleId}` - [0x20 | roleId(32)] -> [u8; 32] (admin role ID)
//!
//! # Error Codes
//!
//! - 1001: Missing role (ERR_MISSING_ROLE)
//! - 1002: Invalid role (ERR_INVALID_ROLE)
//! - 1003: Already initialized
//! - 1004: Not initialized
//! - 1005: Invalid instruction
//! - 1006: Invalid parameters
//! - 1007: Unauthorized (caller doesn't have required role)
//! - 1008: Bad confirmation (renounce role safety check)
//!
//! # Example Usage
//!
//! ```text
//! // Define a custom role
//! const MINTER_ROLE: [u8; 32] = keccak256("MINTER_ROLE");
//!
//! // 1. Initialize contract (deployer becomes DEFAULT_ADMIN)
//! op: 0x00, params: []
//!
//! // 2. Grant MINTER_ROLE to account (requires DEFAULT_ADMIN)
//! op: 0x01, params: [MINTER_ROLE, account_address]
//!
//! // 3. Check if account has MINTER_ROLE
//! op: 0x10, params: [MINTER_ROLE, account_address]
//! // Returns: [1] if has role, [0] if not
//!
//! // 4. Revoke MINTER_ROLE from account
//! op: 0x02, params: [MINTER_ROLE, account_address]
//!
//! // 5. Account renounces MINTER_ROLE
//! op: 0x03, params: [MINTER_ROLE]
//! ```

#![no_std]
#![no_main]

use tako_sdk::*;

// ============================================================================
// Constants
// ============================================================================

/// Storage key prefixes
const KEY_INITIALIZED: u8 = 0x01;
const KEY_ROLE_MEMBER_PREFIX: u8 = 0x10;
const KEY_ROLE_ADMIN_PREFIX: u8 = 0x20;

/// Instruction opcodes
const OP_INITIALIZE: u8 = 0x00;
const OP_GRANT_ROLE: u8 = 0x01;
const OP_REVOKE_ROLE: u8 = 0x02;
const OP_RENOUNCE_ROLE: u8 = 0x03;
const OP_SET_ROLE_ADMIN: u8 = 0x04;
const OP_HAS_ROLE: u8 = 0x10;
const OP_GET_ROLE_ADMIN: u8 = 0x11;

/// Error codes
const ERR_MISSING_ROLE: u64 = 1001;
#[allow(dead_code)]
const ERR_INVALID_ROLE: u64 = 1002;
const ERR_ALREADY_INITIALIZED: u64 = 1003;
const ERR_NOT_INITIALIZED: u64 = 1004;
const ERR_INVALID_INSTRUCTION: u64 = 1005;
const ERR_INVALID_PARAMS: u64 = 1006;
#[allow(dead_code)]
const ERR_UNAUTHORIZED: u64 = 1007;
#[allow(dead_code)]
const ERR_BAD_CONFIRMATION: u64 = 1008;

/// DEFAULT_ADMIN_ROLE - The default admin role (0x00...00)
/// This role has permission to grant and revoke all other roles by default
const DEFAULT_ADMIN_ROLE: [u8; 32] = [0u8; 32];

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

/// Check if an account has a specific role
///
/// Storage key: [0x10 | role(32) | account(32)]
fn has_role(role: &[u8; 32], account: &[u8; 32]) -> bool {
    let mut key = [0u8; 65];
    key[0] = KEY_ROLE_MEMBER_PREFIX;
    key[1..33].copy_from_slice(role);
    key[33..65].copy_from_slice(account);

    let mut buffer = [0u8; 1];
    let len = storage_read(&key, &mut buffer);
    len > 0 && buffer[0] == 1
}

/// Grant a role to an account
///
/// Storage key: [0x10 | role(32) | account(32)]
/// Returns: true if role was newly granted, false if already had role
fn grant_role_internal(role: &[u8; 32], account: &[u8; 32]) -> bool {
    if has_role(role, account) {
        return false; // Already has role
    }

    let mut key = [0u8; 65];
    key[0] = KEY_ROLE_MEMBER_PREFIX;
    key[1..33].copy_from_slice(role);
    key[33..65].copy_from_slice(account);

    let _ = storage_write(&key, &[1u8]);
    true
}

/// Revoke a role from an account
///
/// Storage key: [0x10 | role(32) | account(32)]
/// Returns: true if role was revoked, false if didn't have role
fn revoke_role_internal(role: &[u8; 32], account: &[u8; 32]) -> bool {
    if !has_role(role, account) {
        return false; // Doesn't have role
    }

    let mut key = [0u8; 65];
    key[0] = KEY_ROLE_MEMBER_PREFIX;
    key[1..33].copy_from_slice(role);
    key[33..65].copy_from_slice(account);

    let _ = storage_write(&key, &[0u8]);
    true
}

/// Get the admin role for a given role
///
/// Storage key: [0x20 | role(32)]
/// Returns: Admin role ID (defaults to DEFAULT_ADMIN_ROLE if not set)
fn get_role_admin(role: &[u8; 32]) -> [u8; 32] {
    let mut key = [0u8; 33];
    key[0] = KEY_ROLE_ADMIN_PREFIX;
    key[1..33].copy_from_slice(role);

    let mut admin_role = [0u8; 32];
    let len = storage_read(&key, &mut admin_role);

    if len == 32 {
        admin_role
    } else {
        // Default to DEFAULT_ADMIN_ROLE if not set
        DEFAULT_ADMIN_ROLE
    }
}

/// Set the admin role for a given role
///
/// Storage key: [0x20 | role(32)]
fn set_role_admin_internal(role: &[u8; 32], admin_role: &[u8; 32]) {
    let mut key = [0u8; 33];
    key[0] = KEY_ROLE_ADMIN_PREFIX;
    key[1..33].copy_from_slice(role);

    let _ = storage_write(&key, admin_role);
}

/// Check if caller has a specific role, revert if not
fn check_role(role: &[u8; 32]) -> Result<(), u64> {
    let caller = get_tx_sender();
    if !has_role(role, &caller) {
        log("AccessControl: Missing required role");
        return Err(ERR_MISSING_ROLE);
    }
    Ok(())
}

// ============================================================================
// Core Operations
// ============================================================================

/// Initialize the AccessControl contract
///
/// Format: []
/// Sets the deployer as DEFAULT_ADMIN_ROLE
fn op_initialize(params: &[u8]) -> u64 {
    log("AccessControl: Initialize");

    // Check if already initialized
    if is_initialized() {
        log("AccessControl: Already initialized");
        return ERR_ALREADY_INITIALIZED;
    }

    // Validate parameters (should be empty)
    if !params.is_empty() {
        log("AccessControl: Initialize expects no parameters");
        return ERR_INVALID_PARAMS;
    }

    // Get deployer address
    let deployer = get_tx_sender();

    // Grant DEFAULT_ADMIN_ROLE to deployer
    let granted = grant_role_internal(&DEFAULT_ADMIN_ROLE, &deployer);

    // Mark as initialized
    set_initialized();

    log("AccessControl: Initialized successfully");
    if granted {
        log("AccessControl: Deployer granted DEFAULT_ADMIN_ROLE");
    }

    SUCCESS
}

/// Grant a role to an account
///
/// Format: [role:32][account:32]
/// Requirements: Caller must have the role's admin role
fn op_grant_role(params: &[u8]) -> u64 {
    log("AccessControl: GrantRole");

    if !is_initialized() {
        log("AccessControl: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 64 {
        log("AccessControl: Invalid grant_role parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse parameters
    let mut role = [0u8; 32];
    role.copy_from_slice(&params[0..32]);

    let mut account = [0u8; 32];
    account.copy_from_slice(&params[32..64]);

    // Get the admin role for this role
    let admin_role = get_role_admin(&role);

    // Check if caller has the admin role
    if let Err(e) = check_role(&admin_role) {
        log("AccessControl: Caller is not admin for this role");
        return e;
    }

    // Grant the role
    let granted = grant_role_internal(&role, &account);

    if granted {
        log("AccessControl: Role granted successfully");
    } else {
        log("AccessControl: Account already has role");
    }

    SUCCESS
}

/// Revoke a role from an account
///
/// Format: [role:32][account:32]
/// Requirements: Caller must have the role's admin role
fn op_revoke_role(params: &[u8]) -> u64 {
    log("AccessControl: RevokeRole");

    if !is_initialized() {
        log("AccessControl: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 64 {
        log("AccessControl: Invalid revoke_role parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse parameters
    let mut role = [0u8; 32];
    role.copy_from_slice(&params[0..32]);

    let mut account = [0u8; 32];
    account.copy_from_slice(&params[32..64]);

    // Get the admin role for this role
    let admin_role = get_role_admin(&role);

    // Check if caller has the admin role
    if let Err(e) = check_role(&admin_role) {
        log("AccessControl: Caller is not admin for this role");
        return e;
    }

    // Revoke the role
    let revoked = revoke_role_internal(&role, &account);

    if revoked {
        log("AccessControl: Role revoked successfully");
    } else {
        log("AccessControl: Account doesn't have role");
    }

    SUCCESS
}

/// Renounce a role from the calling account
///
/// Format: [role:32]
/// Requirements: None (self-renunciation)
/// Note: This is a safety mechanism for compromised accounts
fn op_renounce_role(params: &[u8]) -> u64 {
    log("AccessControl: RenounceRole");

    if !is_initialized() {
        log("AccessControl: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        log("AccessControl: Invalid renounce_role parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse role
    let mut role = [0u8; 32];
    role.copy_from_slice(&params[0..32]);

    // Get caller
    let caller = get_tx_sender();

    // Revoke the role from caller
    let revoked = revoke_role_internal(&role, &caller);

    if revoked {
        log("AccessControl: Role renounced successfully");
    } else {
        log("AccessControl: Caller doesn't have role to renounce");
    }

    SUCCESS
}

/// Set the admin role for a given role
///
/// Format: [role:32][adminRole:32]
/// Requirements: Caller must have the current admin role for this role
/// Note: This is an internal operation, typically called during initialization
fn op_set_role_admin(params: &[u8]) -> u64 {
    log("AccessControl: SetRoleAdmin");

    if !is_initialized() {
        log("AccessControl: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 64 {
        log("AccessControl: Invalid set_role_admin parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse parameters
    let mut role = [0u8; 32];
    role.copy_from_slice(&params[0..32]);

    let mut new_admin_role = [0u8; 32];
    new_admin_role.copy_from_slice(&params[32..64]);

    // Get current admin role
    let current_admin = get_role_admin(&role);

    // Check if caller has the current admin role
    if let Err(e) = check_role(&current_admin) {
        log("AccessControl: Caller is not current admin for this role");
        return e;
    }

    // Set new admin role
    set_role_admin_internal(&role, &new_admin_role);

    log("AccessControl: Role admin changed successfully");

    SUCCESS
}

// ============================================================================
// Query Operations
// ============================================================================

/// Check if an account has a role
///
/// Format: [role:32][account:32]
/// Returns: [1] if has role, [0] if not
fn op_has_role(params: &[u8]) -> u64 {
    log("AccessControl: HasRole");

    if !is_initialized() {
        log("AccessControl: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 64 {
        log("AccessControl: Invalid has_role parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse parameters
    let mut role = [0u8; 32];
    role.copy_from_slice(&params[0..32]);

    let mut account = [0u8; 32];
    account.copy_from_slice(&params[32..64]);

    // Check role
    let has = has_role(&role, &account);

    // Return result
    let result = [if has { 1u8 } else { 0u8 }];
    match set_return_data(&result) {
        Ok(_) => {
            log("AccessControl: HasRole query successful");
            log_u64(if has { 1 } else { 0 }, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("AccessControl: Failed to set return data");
            e
        }
    }
}

/// Get the admin role for a given role
///
/// Format: [role:32]
/// Returns: [adminRole:32]
fn op_get_role_admin(params: &[u8]) -> u64 {
    log("AccessControl: GetRoleAdmin");

    if !is_initialized() {
        log("AccessControl: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        log("AccessControl: Invalid get_role_admin parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse role
    let mut role = [0u8; 32];
    role.copy_from_slice(&params[0..32]);

    // Get admin role
    let admin_role = get_role_admin(&role);

    // Return result
    match set_return_data(&admin_role) {
        Ok(_) => {
            log("AccessControl: GetRoleAdmin query successful");
            SUCCESS
        }
        Err(e) => {
            log("AccessControl: Failed to set return data");
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
    log("AccessControl: Contract invoked");

    // Get input data
    let mut input = [0u8; 1024];
    let len = get_input_data(&mut input);

    if len == 0 {
        log("AccessControl: No input data");
        return ERR_INVALID_INSTRUCTION;
    }

    // Extract opcode
    let opcode = input[0];
    let params = &input[1..len as usize];

    // Dispatch based on opcode
    match opcode {
        OP_INITIALIZE => op_initialize(params),
        OP_GRANT_ROLE => op_grant_role(params),
        OP_REVOKE_ROLE => op_revoke_role(params),
        OP_RENOUNCE_ROLE => op_renounce_role(params),
        OP_SET_ROLE_ADMIN => op_set_role_admin(params),
        OP_HAS_ROLE => op_has_role(params),
        OP_GET_ROLE_ADMIN => op_get_role_admin(params),
        _ => {
            log("AccessControl: Unknown opcode");
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
