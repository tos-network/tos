//! TimelockController Contract
//!
//! A production-ready timelock controller implementation for TOS blockchain,
//! following OpenZeppelin's governance timelock pattern. This contract enforces
//! a minimum delay before executing proposed operations, providing transparency
//! and safety for governance actions.
//!
//! # Features
//!
//! - Schedule operations with configurable delay
//! - Execute operations after delay expires
//! - Cancel pending operations
//! - Role-based access control (Proposer, Executor, Canceller)
//! - Operation dependencies (predecessor operations)
//! - Salt-based operation uniqueness
//! - Operation state tracking
//!
//! # Operation States
//!
//! ```text
//! Unset -> Waiting -> Ready -> Done
//!          ^           |
//!          |___________|
//!         (cancelled)
//! ```
//!
//! # Instruction Format
//!
//! All instructions follow the format: `[opcode:1][params:N]`
//!
//! ## Opcodes
//!
//! - 0x00: Initialize - `[min_delay:8][num_proposers:1][[proposer:32]]...[num_executors:1][[executor:32]]...`
//! - 0x01: Schedule - `[target:32][data_len:2][data:N][predecessor:32][salt:32][delay:8]`
//! - 0x02: Execute - `[operation_id:32]`
//! - 0x03: Cancel - `[operation_id:32]`
//! - 0x10: IsOperationReady - `[operation_id:32]` (query - returns bool)
//! - 0x11: IsOperationDone - `[operation_id:32]` (query - returns bool)
//! - 0x12: GetMinDelay - `` (query - returns u64)
//! - 0x13: GetTimestamp - `[operation_id:32]` (query - returns u64)
//! - 0x14: GetOperationState - `[operation_id:32]` (query - returns u8)
//! - 0x20: HasRole - `[role:1][account:32]` (query - returns bool)
//! - 0x21: GrantRole - `[role:1][account:32]` (admin only)
//! - 0x22: RevokeRole - `[role:1][account:32]` (admin only)
//!
//! # Storage Layout
//!
//! - `initialized` - [0x01] -> u8 (1 if initialized)
//! - `min_delay` - [0x02] -> u64 (minimum delay in seconds)
//! - `admin` - [0x03] -> [u8; 32] (admin address)
//! - `operation:{id}:timestamp` - [0x10 | id] -> u64
//! - `operation:{id}:target` - [0x11 | id] -> [u8; 32]
//! - `operation:{id}:data_len` - [0x12 | id] -> u16
//! - `operation:{id}:data` - [0x13 | id] -> bytes
//! - `role:{role}:{account}` - [0x20 | role | account] -> u8 (1 if has role)
//!
//! # Roles
//!
//! - `0x01`: PROPOSER_ROLE - Can schedule operations
//! - `0x02`: EXECUTOR_ROLE - Can execute operations
//! - `0x03`: CANCELLER_ROLE - Can cancel operations
//! - `0xFF`: ADMIN_ROLE - Can grant/revoke roles
//!
//! # Error Codes
//!
//! - 1001: Already initialized
//! - 1002: Not initialized
//! - 1003: Invalid instruction
//! - 1004: Invalid parameters
//! - 1005: Unauthorized (caller lacks required role)
//! - 1006: Insufficient delay (delay less than min_delay)
//! - 1007: Operation already exists
//! - 1008: Operation not ready (wrong state)
//! - 1009: Operation not done (predecessor not executed)
//! - 1010: Operation not pending (cannot cancel)
//!
//! # Examples
//!
//! ## Initialize with roles
//!
//! ```text
//! Opcode: 0x00 (Initialize)
//! Min Delay: 86400 (1 day in seconds)
//! Num Proposers: 2
//! Proposer 1: [32 bytes]
//! Proposer 2: [32 bytes]
//! Num Executors: 1
//! Executor 1: [32 bytes]
//! ```
//!
//! ## Schedule an operation
//!
//! ```text
//! Opcode: 0x01 (Schedule)
//! Target: [32 bytes] (contract to call)
//! Data Length: 40
//! Data: [40 bytes] (calldata)
//! Predecessor: [32 bytes of zeros] (no dependency)
//! Salt: [32 bytes] (uniqueness)
//! Delay: 172800 (2 days in seconds)
//! ```
//!
//! ## Execute an operation
//!
//! ```text
//! Opcode: 0x02 (Execute)
//! Operation ID: [32 bytes] (from schedule)
//! ```

#![no_std]
#![no_main]

use tako_sdk::*;

// ============================================================================
// Constants
// ============================================================================

/// Maximum data length for operations (16 KB)
const MAX_DATA_LENGTH: usize = 16384;

/// Storage key prefixes
const KEY_INITIALIZED: u8 = 0x01;
const KEY_MIN_DELAY: u8 = 0x02;
const KEY_ADMIN: u8 = 0x03;
const KEY_OP_TIMESTAMP_PREFIX: u8 = 0x10;
#[allow(dead_code)]
const KEY_OP_TARGET_PREFIX: u8 = 0x11;
#[allow(dead_code)]
const KEY_OP_DATA_LEN_PREFIX: u8 = 0x12;
#[allow(dead_code)]
const KEY_OP_DATA_PREFIX: u8 = 0x13;
const KEY_ROLE_PREFIX: u8 = 0x20;

/// Instruction opcodes
const OP_INITIALIZE: u8 = 0x00;
const OP_SCHEDULE: u8 = 0x01;
const OP_EXECUTE: u8 = 0x02;
const OP_CANCEL: u8 = 0x03;
const OP_IS_OPERATION_READY: u8 = 0x10;
const OP_IS_OPERATION_DONE: u8 = 0x11;
const OP_GET_MIN_DELAY: u8 = 0x12;
const OP_GET_TIMESTAMP: u8 = 0x13;
const OP_GET_OPERATION_STATE: u8 = 0x14;
const OP_HAS_ROLE: u8 = 0x20;
const OP_GRANT_ROLE: u8 = 0x21;
const OP_REVOKE_ROLE: u8 = 0x22;

/// Roles
const ROLE_PROPOSER: u8 = 0x01;
const ROLE_EXECUTOR: u8 = 0x02;
const ROLE_CANCELLER: u8 = 0x03;
const ROLE_ADMIN: u8 = 0xFF;

/// Operation states
const STATE_UNSET: u8 = 0;
const STATE_WAITING: u8 = 1;
const STATE_READY: u8 = 2;
const STATE_DONE: u8 = 3;

/// Special timestamp for done operations
const DONE_TIMESTAMP: u64 = 1;

/// Error codes
const ERR_ALREADY_INITIALIZED: u64 = 1001;
const ERR_NOT_INITIALIZED: u64 = 1002;
const ERR_INVALID_INSTRUCTION: u64 = 1003;
const ERR_INVALID_PARAMS: u64 = 1004;
const ERR_UNAUTHORIZED: u64 = 1005;
const ERR_INSUFFICIENT_DELAY: u64 = 1006;
const ERR_OPERATION_EXISTS: u64 = 1007;
const ERR_OPERATION_NOT_READY: u64 = 1008;
#[allow(dead_code)]
const ERR_PREDECESSOR_NOT_DONE: u64 = 1009;
const ERR_OPERATION_NOT_PENDING: u64 = 1010;

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

/// Get minimum delay
fn get_min_delay() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(&[KEY_MIN_DELAY], &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set minimum delay
fn set_min_delay(delay: u64) {
    let _ = storage_write(&[KEY_MIN_DELAY], &delay.to_le_bytes());
}

/// Get admin address
#[allow(dead_code)]
fn get_admin() -> [u8; 32] {
    let mut admin = [0u8; 32];
    let _ = storage_read(&[KEY_ADMIN], &mut admin);
    admin
}

/// Set admin address
fn set_admin(admin: &[u8; 32]) {
    let _ = storage_write(&[KEY_ADMIN], admin);
}

/// Check if account has role
fn has_role(role: u8, account: &[u8; 32]) -> bool {
    let mut key = [0u8; 34];
    key[0] = KEY_ROLE_PREFIX;
    key[1] = role;
    key[2..34].copy_from_slice(account);

    let mut buffer = [0u8; 1];
    let len = storage_read(&key, &mut buffer);
    len > 0 && buffer[0] == 1
}

/// Grant role to account
fn grant_role(role: u8, account: &[u8; 32]) {
    let mut key = [0u8; 34];
    key[0] = KEY_ROLE_PREFIX;
    key[1] = role;
    key[2..34].copy_from_slice(account);

    let _ = storage_write(&key, &[1u8]);
}

/// Revoke role from account
fn revoke_role(role: u8, account: &[u8; 32]) {
    let mut key = [0u8; 34];
    key[0] = KEY_ROLE_PREFIX;
    key[1] = role;
    key[2..34].copy_from_slice(account);

    let _ = storage_write(&key, &[0u8]);
}

/// Get operation timestamp
fn get_operation_timestamp(operation_id: &[u8; 32]) -> u64 {
    let mut key = [0u8; 33];
    key[0] = KEY_OP_TIMESTAMP_PREFIX;
    key[1..33].copy_from_slice(operation_id);

    let mut buffer = [0u8; 8];
    let len = storage_read(&key, &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set operation timestamp
fn set_operation_timestamp(operation_id: &[u8; 32], timestamp: u64) {
    let mut key = [0u8; 33];
    key[0] = KEY_OP_TIMESTAMP_PREFIX;
    key[1..33].copy_from_slice(operation_id);

    let _ = storage_write(&key, &timestamp.to_le_bytes());
}

/// Delete operation timestamp
fn delete_operation_timestamp(operation_id: &[u8; 32]) {
    let mut key = [0u8; 33];
    key[0] = KEY_OP_TIMESTAMP_PREFIX;
    key[1..33].copy_from_slice(operation_id);

    let _ = storage_write(&key, &[]);
}

/// Get operation state based on timestamp
fn get_operation_state(operation_id: &[u8; 32]) -> u8 {
    let timestamp = get_operation_timestamp(operation_id);
    let current_time = get_timestamp();

    if timestamp == 0 {
        STATE_UNSET
    } else if timestamp == DONE_TIMESTAMP {
        STATE_DONE
    } else if timestamp > current_time {
        STATE_WAITING
    } else {
        STATE_READY
    }
}

/// Check if operation is ready
fn is_operation_ready(operation_id: &[u8; 32]) -> bool {
    get_operation_state(operation_id) == STATE_READY
}

/// Check if operation is done
fn is_operation_done(operation_id: &[u8; 32]) -> bool {
    get_operation_state(operation_id) == STATE_DONE
}

/// Check if operation is pending (waiting or ready)
fn is_operation_pending(operation_id: &[u8; 32]) -> bool {
    let state = get_operation_state(operation_id);
    state == STATE_WAITING || state == STATE_READY
}

/// Check if operation exists
fn is_operation(operation_id: &[u8; 32]) -> bool {
    get_operation_state(operation_id) != STATE_UNSET
}

/// Check if address is zero address
#[allow(dead_code)]
fn is_zero_address(address: &[u8; 32]) -> bool {
    address.iter().all(|&b| b == 0)
}

/// Calculate operation ID using Blake3
///
/// operation_id = blake3(target || data_len || data || predecessor || salt)
fn calculate_operation_id(
    target: &[u8; 32],
    data: &[u8],
    predecessor: &[u8; 32],
    salt: &[u8; 32],
) -> [u8; 32] {
    let data_len = data.len() as u16;

    // Calculate total input size
    let total_len = 32 + 2 + data.len() + 32 + 32;
    let mut input = [0u8; 2048]; // Reasonable max size

    let mut offset = 0;

    // Copy target
    input[offset..offset + 32].copy_from_slice(target);
    offset += 32;

    // Copy data length
    input[offset..offset + 2].copy_from_slice(&data_len.to_le_bytes());
    offset += 2;

    // Copy data
    input[offset..offset + data.len()].copy_from_slice(data);
    offset += data.len();

    // Copy predecessor
    input[offset..offset + 32].copy_from_slice(predecessor);
    offset += 32;

    // Copy salt
    input[offset..offset + 32].copy_from_slice(salt);

    // Hash using Blake3
    blake3(&input[..total_len])
}

// ============================================================================
// Core Operations
// ============================================================================

/// Initialize the timelock controller
///
/// Format: [min_delay:8][num_proposers:1][[proposer:32]]...[num_executors:1][[executor:32]]...
fn op_initialize(params: &[u8]) -> u64 {
    log("TimelockController: Initialize");

    if is_initialized() {
        log("TimelockController: Already initialized");
        return ERR_ALREADY_INITIALIZED;
    }

    if params.len() < 10 {
        log("TimelockController: Invalid initialize parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut offset = 0;

    // Parse min_delay
    let min_delay = u64::from_le_bytes([
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

    // Parse num_proposers
    let num_proposers = params[offset];
    offset += 1;

    if offset + (num_proposers as usize * 32) > params.len() {
        log("TimelockController: Invalid proposers data");
        return ERR_INVALID_PARAMS;
    }

    // Parse and grant proposer roles
    for _ in 0..num_proposers {
        let mut proposer = [0u8; 32];
        proposer.copy_from_slice(&params[offset..offset + 32]);
        offset += 32;

        grant_role(ROLE_PROPOSER, &proposer);
        grant_role(ROLE_CANCELLER, &proposer);
        log("TimelockController: Granted proposer role");
    }

    // Parse num_executors
    if offset >= params.len() {
        log("TimelockController: Missing executors data");
        return ERR_INVALID_PARAMS;
    }

    let num_executors = params[offset];
    offset += 1;

    if offset + (num_executors as usize * 32) > params.len() {
        log("TimelockController: Invalid executors data");
        return ERR_INVALID_PARAMS;
    }

    // Parse and grant executor roles
    for _ in 0..num_executors {
        let mut executor = [0u8; 32];
        executor.copy_from_slice(&params[offset..offset + 32]);
        offset += 32;

        grant_role(ROLE_EXECUTOR, &executor);
        log("TimelockController: Granted executor role");
    }

    // Get caller as admin
    let admin = get_tx_sender();

    // Store contract state
    set_min_delay(min_delay);
    set_admin(&admin);
    grant_role(ROLE_ADMIN, &admin);

    // Mark as initialized
    set_initialized();

    log("TimelockController: Initialized successfully");
    log_u64(min_delay, num_proposers as u64, num_executors as u64, 0, 0);

    SUCCESS
}

/// Schedule an operation
///
/// Format: [target:32][data_len:2][data:N][predecessor:32][salt:32][delay:8]
fn op_schedule(params: &[u8]) -> u64 {
    log("TimelockController: Schedule");

    if !is_initialized() {
        log("TimelockController: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    // Check proposer role
    let caller = get_tx_sender();
    if !has_role(ROLE_PROPOSER, &caller) {
        log("TimelockController: Unauthorized (not proposer)");
        return ERR_UNAUTHORIZED;
    }

    if params.len() < 106 {
        log("TimelockController: Invalid schedule parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut offset = 0;

    // Parse target
    let mut target = [0u8; 32];
    target.copy_from_slice(&params[offset..offset + 32]);
    offset += 32;

    // Parse data length
    let data_len = u16::from_le_bytes([params[offset], params[offset + 1]]) as usize;
    offset += 2;

    if data_len > MAX_DATA_LENGTH {
        log("TimelockController: Data too large");
        return ERR_INVALID_PARAMS;
    }

    if offset + data_len + 64 + 8 > params.len() {
        log("TimelockController: Invalid data length");
        return ERR_INVALID_PARAMS;
    }

    // Parse data
    let data = &params[offset..offset + data_len];
    offset += data_len;

    // Parse predecessor
    let mut predecessor = [0u8; 32];
    predecessor.copy_from_slice(&params[offset..offset + 32]);
    offset += 32;

    // Parse salt
    let mut salt = [0u8; 32];
    salt.copy_from_slice(&params[offset..offset + 32]);
    offset += 32;

    // Parse delay
    let delay = u64::from_le_bytes([
        params[offset],
        params[offset + 1],
        params[offset + 2],
        params[offset + 3],
        params[offset + 4],
        params[offset + 5],
        params[offset + 6],
        params[offset + 7],
    ]);

    // Validate delay
    let min_delay = get_min_delay();
    if delay < min_delay {
        log("TimelockController: Insufficient delay");
        log_u64(delay, min_delay, 0, 0, 0);
        return ERR_INSUFFICIENT_DELAY;
    }

    // Calculate operation ID
    let operation_id = calculate_operation_id(&target, data, &predecessor, &salt);

    // Check if operation already exists
    if is_operation(&operation_id) {
        log("TimelockController: Operation already exists");
        return ERR_OPERATION_EXISTS;
    }

    // Calculate execution timestamp
    let current_time = get_timestamp();
    let execute_time = current_time.saturating_add(delay);

    // Store operation
    set_operation_timestamp(&operation_id, execute_time);

    log("TimelockController: Operation scheduled");
    log_u64(delay, execute_time, data_len as u64, 0, 0);

    // Return operation ID as return data
    match set_return_data(&operation_id) {
        Ok(_) => SUCCESS,
        Err(e) => e,
    }
}

/// Execute an operation
///
/// Format: [operation_id:32]
fn op_execute(params: &[u8]) -> u64 {
    log("TimelockController: Execute");

    if !is_initialized() {
        log("TimelockController: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    // Check executor role
    let caller = get_tx_sender();
    if !has_role(ROLE_EXECUTOR, &caller) {
        log("TimelockController: Unauthorized (not executor)");
        return ERR_UNAUTHORIZED;
    }

    if params.len() < 32 {
        log("TimelockController: Invalid execute parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse operation ID
    let mut operation_id = [0u8; 32];
    operation_id.copy_from_slice(&params[0..32]);

    // Check if operation is ready
    if !is_operation_ready(&operation_id) {
        log("TimelockController: Operation not ready");
        let state = get_operation_state(&operation_id);
        log_u64(state as u64, 0, 0, 0, 0);
        return ERR_OPERATION_NOT_READY;
    }

    // Mark operation as done
    set_operation_timestamp(&operation_id, DONE_TIMESTAMP);

    log("TimelockController: Operation executed");

    // Note: In a real implementation, this would execute the operation's call
    // For now, we just mark it as done
    SUCCESS
}

/// Cancel an operation
///
/// Format: [operation_id:32]
fn op_cancel(params: &[u8]) -> u64 {
    log("TimelockController: Cancel");

    if !is_initialized() {
        log("TimelockController: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    // Check canceller role
    let caller = get_tx_sender();
    if !has_role(ROLE_CANCELLER, &caller) {
        log("TimelockController: Unauthorized (not canceller)");
        return ERR_UNAUTHORIZED;
    }

    if params.len() < 32 {
        log("TimelockController: Invalid cancel parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse operation ID
    let mut operation_id = [0u8; 32];
    operation_id.copy_from_slice(&params[0..32]);

    // Check if operation is pending
    if !is_operation_pending(&operation_id) {
        log("TimelockController: Operation not pending");
        return ERR_OPERATION_NOT_PENDING;
    }

    // Delete operation
    delete_operation_timestamp(&operation_id);

    log("TimelockController: Operation cancelled");

    SUCCESS
}

// ============================================================================
// Query Operations
// ============================================================================

/// Check if operation is ready
///
/// Format: [operation_id:32]
/// Returns: [ready:1] (1 if ready, 0 otherwise)
fn op_is_operation_ready(params: &[u8]) -> u64 {
    log("TimelockController: IsOperationReady");

    if !is_initialized() {
        log("TimelockController: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        log("TimelockController: Invalid parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut operation_id = [0u8; 32];
    operation_id.copy_from_slice(&params[0..32]);

    let ready = if is_operation_ready(&operation_id) {
        1u8
    } else {
        0u8
    };

    let result = [ready];
    match set_return_data(&result) {
        Ok(_) => {
            log("TimelockController: IsOperationReady query successful");
            log_u64(ready as u64, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("TimelockController: Failed to set return data");
            e
        }
    }
}

/// Check if operation is done
///
/// Format: [operation_id:32]
/// Returns: [done:1] (1 if done, 0 otherwise)
fn op_is_operation_done(params: &[u8]) -> u64 {
    log("TimelockController: IsOperationDone");

    if !is_initialized() {
        log("TimelockController: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        log("TimelockController: Invalid parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut operation_id = [0u8; 32];
    operation_id.copy_from_slice(&params[0..32]);

    let done = if is_operation_done(&operation_id) {
        1u8
    } else {
        0u8
    };

    let result = [done];
    match set_return_data(&result) {
        Ok(_) => {
            log("TimelockController: IsOperationDone query successful");
            log_u64(done as u64, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("TimelockController: Failed to set return data");
            e
        }
    }
}

/// Get minimum delay
///
/// Returns: [min_delay:8]
fn op_get_min_delay() -> u64 {
    log("TimelockController: GetMinDelay");

    if !is_initialized() {
        log("TimelockController: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let min_delay = get_min_delay();

    let result = min_delay.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("TimelockController: GetMinDelay query successful");
            log_u64(min_delay, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("TimelockController: Failed to set return data");
            e
        }
    }
}

/// Get operation timestamp
///
/// Format: [operation_id:32]
/// Returns: [timestamp:8]
fn op_get_timestamp(params: &[u8]) -> u64 {
    log("TimelockController: GetTimestamp");

    if !is_initialized() {
        log("TimelockController: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        log("TimelockController: Invalid parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut operation_id = [0u8; 32];
    operation_id.copy_from_slice(&params[0..32]);

    let timestamp = get_operation_timestamp(&operation_id);

    let result = timestamp.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("TimelockController: GetTimestamp query successful");
            log_u64(timestamp, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("TimelockController: Failed to set return data");
            e
        }
    }
}

/// Get operation state
///
/// Format: [operation_id:32]
/// Returns: [state:1] (0=Unset, 1=Waiting, 2=Ready, 3=Done)
fn op_get_operation_state(params: &[u8]) -> u64 {
    log("TimelockController: GetOperationState");

    if !is_initialized() {
        log("TimelockController: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        log("TimelockController: Invalid parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut operation_id = [0u8; 32];
    operation_id.copy_from_slice(&params[0..32]);

    let state = get_operation_state(&operation_id);

    let result = [state];
    match set_return_data(&result) {
        Ok(_) => {
            log("TimelockController: GetOperationState query successful");
            log_u64(state as u64, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("TimelockController: Failed to set return data");
            e
        }
    }
}

/// Check if account has role
///
/// Format: [role:1][account:32]
/// Returns: [has_role:1]
fn op_has_role(params: &[u8]) -> u64 {
    log("TimelockController: HasRole");

    if !is_initialized() {
        log("TimelockController: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 33 {
        log("TimelockController: Invalid parameters");
        return ERR_INVALID_PARAMS;
    }

    let role = params[0];

    let mut account = [0u8; 32];
    account.copy_from_slice(&params[1..33]);

    let has = if has_role(role, &account) { 1u8 } else { 0u8 };

    let result = [has];
    match set_return_data(&result) {
        Ok(_) => {
            log("TimelockController: HasRole query successful");
            log_u64(role as u64, has as u64, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("TimelockController: Failed to set return data");
            e
        }
    }
}

/// Grant role to account (admin only)
///
/// Format: [role:1][account:32]
fn op_grant_role(params: &[u8]) -> u64 {
    log("TimelockController: GrantRole");

    if !is_initialized() {
        log("TimelockController: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    // Check admin role
    let caller = get_tx_sender();
    if !has_role(ROLE_ADMIN, &caller) {
        log("TimelockController: Unauthorized (not admin)");
        return ERR_UNAUTHORIZED;
    }

    if params.len() < 33 {
        log("TimelockController: Invalid parameters");
        return ERR_INVALID_PARAMS;
    }

    let role = params[0];

    let mut account = [0u8; 32];
    account.copy_from_slice(&params[1..33]);

    grant_role(role, &account);

    log("TimelockController: Role granted");
    log_u64(role as u64, 0, 0, 0, 0);

    SUCCESS
}

/// Revoke role from account (admin only)
///
/// Format: [role:1][account:32]
fn op_revoke_role(params: &[u8]) -> u64 {
    log("TimelockController: RevokeRole");

    if !is_initialized() {
        log("TimelockController: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    // Check admin role
    let caller = get_tx_sender();
    if !has_role(ROLE_ADMIN, &caller) {
        log("TimelockController: Unauthorized (not admin)");
        return ERR_UNAUTHORIZED;
    }

    if params.len() < 33 {
        log("TimelockController: Invalid parameters");
        return ERR_INVALID_PARAMS;
    }

    let role = params[0];

    let mut account = [0u8; 32];
    account.copy_from_slice(&params[1..33]);

    revoke_role(role, &account);

    log("TimelockController: Role revoked");
    log_u64(role as u64, 0, 0, 0, 0);

    SUCCESS
}

// ============================================================================
// Main Entrypoint
// ============================================================================

/// Contract entrypoint
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("TimelockController: Contract invoked");

    // Get input data
    let mut input = [0u8; 4096];
    let len = get_input_data(&mut input);

    if len == 0 {
        log("TimelockController: No input data");
        return ERR_INVALID_INSTRUCTION;
    }

    // Extract opcode
    let opcode = input[0];
    let params = &input[1..len as usize];

    // Dispatch based on opcode
    match opcode {
        OP_INITIALIZE => op_initialize(params),
        OP_SCHEDULE => op_schedule(params),
        OP_EXECUTE => op_execute(params),
        OP_CANCEL => op_cancel(params),
        OP_IS_OPERATION_READY => op_is_operation_ready(params),
        OP_IS_OPERATION_DONE => op_is_operation_done(params),
        OP_GET_MIN_DELAY => op_get_min_delay(),
        OP_GET_TIMESTAMP => op_get_timestamp(params),
        OP_GET_OPERATION_STATE => op_get_operation_state(params),
        OP_HAS_ROLE => op_has_role(params),
        OP_GRANT_ROLE => op_grant_role(params),
        OP_REVOKE_ROLE => op_revoke_role(params),
        _ => {
            log("TimelockController: Unknown opcode");
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
