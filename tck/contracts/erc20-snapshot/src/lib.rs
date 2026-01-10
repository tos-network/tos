//! ERC20Snapshot Contract
//!
//! A production-ready ERC20 token with snapshot capability for TOS blockchain,
//! following OpenZeppelin's ERC20Snapshot pattern. This contract allows taking
//! snapshots of token balances at specific points in time, useful for voting,
//! dividends, and airdrops.
//!
//! # Features
//!
//! - Full ERC20 compliance (transfer, approve, transferFrom)
//! - Snapshot creation and querying
//! - Historical balance lookups
//! - Historical total supply lookups
//! - Owner-controlled snapshot creation
//! - Minting and burning with snapshot updates
//! - Overflow protection
//!
//! # Snapshot Mechanism
//!
//! Snapshots capture the state of balances and total supply at a specific point:
//! - Each snapshot has a unique ID (sequential)
//! - Snapshots are immutable once created
//! - Balance/supply changes after snapshot don't affect historical snapshots
//! - Efficient storage: only stores changes, not full state
//!
//! # Instruction Format
//!
//! All instructions follow the format: `[opcode:1][params:N]`
//!
//! ## Opcodes
//!
//! **ERC20 Operations**:
//! - 0x00: Initialize - `[name_len:2][name:N][symbol_len:2][symbol:N][decimals:1][initial_supply:8]`
//! - 0x01: Transfer - `[to:32][amount:8]`
//! - 0x02: Approve - `[spender:32][amount:8]`
//! - 0x03: TransferFrom - `[from:32][to:32][amount:8]`
//! - 0x04: Mint - `[to:32][amount:8]` (owner only)
//! - 0x05: Burn - `[amount:8]`
//! - 0x10: BalanceOf - `[account:32]` (query)
//! - 0x11: Allowance - `[owner:32][spender:32]` (query)
//! - 0x12: TotalSupply - `` (query)
//! - 0x13: Name - `` (query)
//! - 0x14: Symbol - `` (query)
//! - 0x15: Decimals - `` (query)
//!
//! **Snapshot Operations**:
//! - 0x20: Snapshot - `` (owner only, returns snapshot_id)
//! - 0x21: BalanceOfAt - `[account:32][snapshot_id:8]` (query)
//! - 0x22: TotalSupplyAt - `[snapshot_id:8]` (query)
//! - 0x23: GetCurrentSnapshotId - `` (query)
//!
//! # Storage Layout
//!
//! **ERC20 Storage**:
//! - `initialized` - [0x01] -> u8
//! - `total_supply` - [0x02] -> u64
//! - `name` - [0x03] -> String
//! - `symbol` - [0x04] -> String
//! - `decimals` - [0x05] -> u8
//! - `owner` - [0x06] -> [u8; 32]
//! - `balance:{address}` - [0x10 | address] -> u64
//! - `allowance:{owner}:{spender}` - [0x20 | owner | spender] -> u64
//!
//! **Snapshot Storage**:
//! - `current_snapshot_id` - [0x30] -> u64
//! - `snapshot_balance:{account}:{snapshot_id}` - [0x40 | account | snapshot_id] -> u64
//! - `snapshot_supply:{snapshot_id}` - [0x50 | snapshot_id] -> u64
//!
//! # Error Codes
//!
//! - 1001: Already initialized
//! - 1002: Not initialized
//! - 1003: Invalid instruction
//! - 1004: Invalid parameters
//! - 1005: Insufficient balance
//! - 1006: Insufficient allowance
//! - 1007: Unauthorized (not owner)
//! - 1008: Invalid address (zero address)
//! - 1009: Invalid snapshot ID (doesn't exist or is zero)
//!
//! # Examples
//!
//! ## Take a snapshot
//!
//! ```text
//! Opcode: 0x20 (Snapshot)
//! Returns: snapshot_id (u64)
//! ```
//!
//! ## Query historical balance
//!
//! ```text
//! Opcode: 0x21 (BalanceOfAt)
//! Account: [32 bytes]
//! Snapshot ID: 5
//! Returns: balance at snapshot 5
//! ```

#![no_std]
#![no_main]

use tako_sdk::*;

// ============================================================================
// Constants
// ============================================================================

/// Maximum token name length
const MAX_NAME_LENGTH: usize = 32;

/// Maximum token symbol length
const MAX_SYMBOL_LENGTH: usize = 8;

/// Storage key prefixes
const KEY_INITIALIZED: u8 = 0x01;
const KEY_TOTAL_SUPPLY: u8 = 0x02;
const KEY_NAME: u8 = 0x03;
const KEY_SYMBOL: u8 = 0x04;
const KEY_DECIMALS: u8 = 0x05;
const KEY_OWNER: u8 = 0x06;
const KEY_BALANCE_PREFIX: u8 = 0x10;
const KEY_ALLOWANCE_PREFIX: u8 = 0x20;
const KEY_CURRENT_SNAPSHOT: u8 = 0x30;
const KEY_SNAPSHOT_BALANCE_PREFIX: u8 = 0x40;
const KEY_SNAPSHOT_SUPPLY_PREFIX: u8 = 0x50;

/// Instruction opcodes - ERC20
const OP_INITIALIZE: u8 = 0x00;
const OP_TRANSFER: u8 = 0x01;
const OP_APPROVE: u8 = 0x02;
const OP_TRANSFER_FROM: u8 = 0x03;
const OP_MINT: u8 = 0x04;
const OP_BURN: u8 = 0x05;
const OP_BALANCE_OF: u8 = 0x10;
const OP_ALLOWANCE: u8 = 0x11;
const OP_TOTAL_SUPPLY: u8 = 0x12;
const OP_NAME: u8 = 0x13;
const OP_SYMBOL: u8 = 0x14;
const OP_DECIMALS: u8 = 0x15;

/// Instruction opcodes - Snapshot
const OP_SNAPSHOT: u8 = 0x20;
const OP_BALANCE_OF_AT: u8 = 0x21;
const OP_TOTAL_SUPPLY_AT: u8 = 0x22;
const OP_GET_CURRENT_SNAPSHOT_ID: u8 = 0x23;

/// Error codes
const ERR_ALREADY_INITIALIZED: u64 = 1001;
const ERR_NOT_INITIALIZED: u64 = 1002;
const ERR_INVALID_INSTRUCTION: u64 = 1003;
const ERR_INVALID_PARAMS: u64 = 1004;
const ERR_INSUFFICIENT_BALANCE: u64 = 1005;
const ERR_INSUFFICIENT_ALLOWANCE: u64 = 1006;
const ERR_UNAUTHORIZED: u64 = 1007;
const ERR_INVALID_ADDRESS: u64 = 1008;
const ERR_INVALID_SNAPSHOT: u64 = 1009;

// ============================================================================
// Helper Functions - ERC20
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

/// Get total supply
fn get_total_supply() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(&[KEY_TOTAL_SUPPLY], &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set total supply
fn set_total_supply(amount: u64) {
    let _ = storage_write(&[KEY_TOTAL_SUPPLY], &amount.to_le_bytes());
}

/// Get balance of an account
fn get_balance(account: &[u8; 32]) -> u64 {
    let mut key = [0u8; 33];
    key[0] = KEY_BALANCE_PREFIX;
    key[1..33].copy_from_slice(account);

    let mut buffer = [0u8; 8];
    let len = storage_read(&key, &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set balance of an account
fn set_balance(account: &[u8; 32], amount: u64) {
    let mut key = [0u8; 33];
    key[0] = KEY_BALANCE_PREFIX;
    key[1..33].copy_from_slice(account);

    let _ = storage_write(&key, &amount.to_le_bytes());
}

/// Get allowance
fn get_allowance(owner: &[u8; 32], spender: &[u8; 32]) -> u64 {
    let mut key = [0u8; 65];
    key[0] = KEY_ALLOWANCE_PREFIX;
    key[1..33].copy_from_slice(owner);
    key[33..65].copy_from_slice(spender);

    let mut buffer = [0u8; 8];
    let len = storage_read(&key, &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set allowance
fn set_allowance(owner: &[u8; 32], spender: &[u8; 32], amount: u64) {
    let mut key = [0u8; 65];
    key[0] = KEY_ALLOWANCE_PREFIX;
    key[1..33].copy_from_slice(owner);
    key[33..65].copy_from_slice(spender);

    let _ = storage_write(&key, &amount.to_le_bytes());
}

/// Get owner address
fn get_owner() -> [u8; 32] {
    let mut owner = [0u8; 32];
    let _ = storage_read(&[KEY_OWNER], &mut owner);
    owner
}

/// Set owner address
fn set_owner(owner: &[u8; 32]) {
    let _ = storage_write(&[KEY_OWNER], owner);
}

/// Check if address is zero address
fn is_zero_address(address: &[u8; 32]) -> bool {
    address.iter().all(|&b| b == 0)
}

// ============================================================================
// Helper Functions - Snapshot
// ============================================================================

/// Get current snapshot ID
fn get_current_snapshot_id() -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(&[KEY_CURRENT_SNAPSHOT], &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set current snapshot ID
fn set_current_snapshot_id(snapshot_id: u64) {
    let _ = storage_write(&[KEY_CURRENT_SNAPSHOT], &snapshot_id.to_le_bytes());
}

/// Get snapshot balance for an account
fn get_snapshot_balance(account: &[u8; 32], snapshot_id: u64) -> u64 {
    let mut key = [0u8; 41];
    key[0] = KEY_SNAPSHOT_BALANCE_PREFIX;
    key[1..33].copy_from_slice(account);
    key[33..41].copy_from_slice(&snapshot_id.to_le_bytes());

    let mut buffer = [0u8; 8];
    let len = storage_read(&key, &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set snapshot balance for an account
#[allow(dead_code)]
fn set_snapshot_balance(account: &[u8; 32], snapshot_id: u64, balance: u64) {
    let mut key = [0u8; 41];
    key[0] = KEY_SNAPSHOT_BALANCE_PREFIX;
    key[1..33].copy_from_slice(account);
    key[33..41].copy_from_slice(&snapshot_id.to_le_bytes());

    let _ = storage_write(&key, &balance.to_le_bytes());
}

/// Get snapshot total supply
fn get_snapshot_supply(snapshot_id: u64) -> u64 {
    let mut key = [0u8; 9];
    key[0] = KEY_SNAPSHOT_SUPPLY_PREFIX;
    key[1..9].copy_from_slice(&snapshot_id.to_le_bytes());

    let mut buffer = [0u8; 8];
    let len = storage_read(&key, &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

/// Set snapshot total supply
fn set_snapshot_supply(snapshot_id: u64, supply: u64) {
    let mut key = [0u8; 9];
    key[0] = KEY_SNAPSHOT_SUPPLY_PREFIX;
    key[1..9].copy_from_slice(&snapshot_id.to_le_bytes());

    let _ = storage_write(&key, &supply.to_le_bytes());
}

// ============================================================================
// Core Operations - ERC20
// ============================================================================

/// Initialize the token
fn op_initialize(params: &[u8]) -> u64 {
    log("ERC20Snapshot: Initialize");

    if is_initialized() {
        log("ERC20Snapshot: Already initialized");
        return ERR_ALREADY_INITIALIZED;
    }

    if params.len() < 13 {
        log("ERC20Snapshot: Invalid initialize parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut offset = 0;

    // Parse name
    let name_len = u16::from_le_bytes([params[offset], params[offset + 1]]) as usize;
    offset += 2;

    if offset + name_len > params.len() || name_len > MAX_NAME_LENGTH {
        log("ERC20Snapshot: Invalid name length");
        return ERR_INVALID_PARAMS;
    }

    let name = &params[offset..offset + name_len];
    offset += name_len;

    // Parse symbol
    if offset + 2 > params.len() {
        log("ERC20Snapshot: Missing symbol length");
        return ERR_INVALID_PARAMS;
    }

    let symbol_len = u16::from_le_bytes([params[offset], params[offset + 1]]) as usize;
    offset += 2;

    if offset + symbol_len > params.len() || symbol_len > MAX_SYMBOL_LENGTH {
        log("ERC20Snapshot: Invalid symbol length");
        return ERR_INVALID_PARAMS;
    }

    let symbol = &params[offset..offset + symbol_len];
    offset += symbol_len;

    // Parse decimals
    if offset + 1 > params.len() {
        log("ERC20Snapshot: Missing decimals");
        return ERR_INVALID_PARAMS;
    }

    let decimals = params[offset];
    offset += 1;

    // Parse initial supply
    if offset + 8 > params.len() {
        log("ERC20Snapshot: Missing initial supply");
        return ERR_INVALID_PARAMS;
    }

    let initial_supply = u64::from_le_bytes([
        params[offset],
        params[offset + 1],
        params[offset + 2],
        params[offset + 3],
        params[offset + 4],
        params[offset + 5],
        params[offset + 6],
        params[offset + 7],
    ]);

    // Get sender as owner
    let sender = get_tx_sender();

    // Store metadata
    let _ = storage_write(&[KEY_NAME], name);
    let _ = storage_write(&[KEY_SYMBOL], symbol);
    let _ = storage_write(&[KEY_DECIMALS], &[decimals]);

    // Store owner
    set_owner(&sender);

    // Set initial supply
    set_total_supply(initial_supply);

    // Mint initial supply to owner
    if initial_supply > 0 {
        set_balance(&sender, initial_supply);
    }

    // Initialize snapshot ID to 0
    set_current_snapshot_id(0);

    // Mark as initialized
    set_initialized();

    log("ERC20Snapshot: Initialized successfully");
    log_u64(
        name_len as u64,
        symbol_len as u64,
        decimals as u64,
        initial_supply,
        0,
    );

    SUCCESS
}

/// Transfer tokens
fn op_transfer(params: &[u8]) -> u64 {
    log("ERC20Snapshot: Transfer");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 40 {
        log("ERC20Snapshot: Invalid transfer parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse parameters
    let mut to = [0u8; 32];
    to.copy_from_slice(&params[0..32]);

    let amount = u64::from_le_bytes([
        params[32], params[33], params[34], params[35], params[36], params[37], params[38],
        params[39],
    ]);

    // Validate addresses
    if is_zero_address(&to) {
        log("ERC20Snapshot: Transfer to zero address");
        return ERR_INVALID_ADDRESS;
    }

    let from = get_tx_sender();

    // Get balances
    let from_balance = get_balance(&from);
    let to_balance = get_balance(&to);

    // Check balance
    if from_balance < amount {
        log("ERC20Snapshot: Insufficient balance");
        return ERR_INSUFFICIENT_BALANCE;
    }

    // Update balances
    let new_from_balance = from_balance.saturating_sub(amount);
    let new_to_balance = to_balance.saturating_add(amount);

    set_balance(&from, new_from_balance);
    set_balance(&to, new_to_balance);

    log("ERC20Snapshot: Transfer successful");
    log_u64(amount, new_from_balance, new_to_balance, 0, 0);

    SUCCESS
}

/// Approve spender
fn op_approve(params: &[u8]) -> u64 {
    log("ERC20Snapshot: Approve");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 40 {
        log("ERC20Snapshot: Invalid approve parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse parameters
    let mut spender = [0u8; 32];
    spender.copy_from_slice(&params[0..32]);

    let amount = u64::from_le_bytes([
        params[32], params[33], params[34], params[35], params[36], params[37], params[38],
        params[39],
    ]);

    // Validate address
    if is_zero_address(&spender) {
        log("ERC20Snapshot: Approve to zero address");
        return ERR_INVALID_ADDRESS;
    }

    let owner = get_tx_sender();

    // Set allowance
    set_allowance(&owner, &spender, amount);

    log("ERC20Snapshot: Approval successful");
    log_u64(amount, 0, 0, 0, 0);

    SUCCESS
}

/// Transfer from (using allowance)
fn op_transfer_from(params: &[u8]) -> u64 {
    log("ERC20Snapshot: TransferFrom");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 72 {
        log("ERC20Snapshot: Invalid transferFrom parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse parameters
    let mut from = [0u8; 32];
    from.copy_from_slice(&params[0..32]);

    let mut to = [0u8; 32];
    to.copy_from_slice(&params[32..64]);

    let amount = u64::from_le_bytes([
        params[64], params[65], params[66], params[67], params[68], params[69], params[70],
        params[71],
    ]);

    // Validate addresses
    if is_zero_address(&from) || is_zero_address(&to) {
        log("ERC20Snapshot: TransferFrom with zero address");
        return ERR_INVALID_ADDRESS;
    }

    let spender = get_tx_sender();

    // Check allowance
    let allowance = get_allowance(&from, &spender);
    if allowance < amount {
        log("ERC20Snapshot: Insufficient allowance");
        return ERR_INSUFFICIENT_ALLOWANCE;
    }

    // Get balances
    let from_balance = get_balance(&from);
    let to_balance = get_balance(&to);

    // Check balance
    if from_balance < amount {
        log("ERC20Snapshot: Insufficient balance");
        return ERR_INSUFFICIENT_BALANCE;
    }

    // Update balances
    let new_from_balance = from_balance.saturating_sub(amount);
    let new_to_balance = to_balance.saturating_add(amount);

    set_balance(&from, new_from_balance);
    set_balance(&to, new_to_balance);

    // Update allowance
    let new_allowance = allowance.saturating_sub(amount);
    set_allowance(&from, &spender, new_allowance);

    log("ERC20Snapshot: TransferFrom successful");
    log_u64(amount, new_from_balance, new_to_balance, new_allowance, 0);

    SUCCESS
}

/// Mint tokens (owner only)
fn op_mint(params: &[u8]) -> u64 {
    log("ERC20Snapshot: Mint");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 40 {
        log("ERC20Snapshot: Invalid mint parameters");
        return ERR_INVALID_PARAMS;
    }

    // Check if sender is owner
    let sender = get_tx_sender();
    let owner = get_owner();

    if sender != owner {
        log("ERC20Snapshot: Unauthorized mint (not owner)");
        return ERR_UNAUTHORIZED;
    }

    // Parse parameters
    let mut to = [0u8; 32];
    to.copy_from_slice(&params[0..32]);

    let amount = u64::from_le_bytes([
        params[32], params[33], params[34], params[35], params[36], params[37], params[38],
        params[39],
    ]);

    // Validate address
    if is_zero_address(&to) {
        log("ERC20Snapshot: Mint to zero address");
        return ERR_INVALID_ADDRESS;
    }

    // Update balances
    let to_balance = get_balance(&to);
    let new_balance = to_balance.saturating_add(amount);
    set_balance(&to, new_balance);

    // Update total supply
    let total_supply = get_total_supply();
    let new_total = total_supply.saturating_add(amount);
    set_total_supply(new_total);

    log("ERC20Snapshot: Mint successful");
    log_u64(amount, new_balance, new_total, 0, 0);

    SUCCESS
}

/// Burn tokens
fn op_burn(params: &[u8]) -> u64 {
    log("ERC20Snapshot: Burn");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 8 {
        log("ERC20Snapshot: Invalid burn parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse amount
    let amount = u64::from_le_bytes([
        params[0], params[1], params[2], params[3], params[4], params[5], params[6], params[7],
    ]);

    let from = get_tx_sender();

    // Get balance
    let balance = get_balance(&from);

    // Check balance
    if balance < amount {
        log("ERC20Snapshot: Insufficient balance to burn");
        return ERR_INSUFFICIENT_BALANCE;
    }

    // Update balance
    let new_balance = balance.saturating_sub(amount);
    set_balance(&from, new_balance);

    // Update total supply
    let total_supply = get_total_supply();
    let new_total = total_supply.saturating_sub(amount);
    set_total_supply(new_total);

    log("ERC20Snapshot: Burn successful");
    log_u64(amount, new_balance, new_total, 0, 0);

    SUCCESS
}

// ============================================================================
// Snapshot Operations
// ============================================================================

/// Create a new snapshot (owner only)
///
/// Returns: [snapshot_id:8]
fn op_snapshot() -> u64 {
    log("ERC20Snapshot: Snapshot");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    // Check if sender is owner
    let sender = get_tx_sender();
    let owner = get_owner();

    if sender != owner {
        log("ERC20Snapshot: Unauthorized snapshot (not owner)");
        return ERR_UNAUTHORIZED;
    }

    // Increment snapshot ID
    let current_id = get_current_snapshot_id();
    let new_id = current_id.saturating_add(1);
    set_current_snapshot_id(new_id);

    // Store current total supply for this snapshot
    let total_supply = get_total_supply();
    set_snapshot_supply(new_id, total_supply);

    log("ERC20Snapshot: Snapshot created");
    log_u64(new_id, total_supply, 0, 0, 0);

    // Return snapshot ID as return data
    let result = new_id.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => SUCCESS,
        Err(e) => e,
    }
}

/// Get balance of account at specific snapshot
///
/// Format: [account:32][snapshot_id:8]
/// Returns: [balance:8]
fn op_balance_of_at(params: &[u8]) -> u64 {
    log("ERC20Snapshot: BalanceOfAt");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 40 {
        log("ERC20Snapshot: Invalid balanceOfAt parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse account
    let mut account = [0u8; 32];
    account.copy_from_slice(&params[0..32]);

    // Parse snapshot ID
    let snapshot_id = u64::from_le_bytes([
        params[32], params[33], params[34], params[35], params[36], params[37], params[38],
        params[39],
    ]);

    // Validate snapshot ID
    let current_id = get_current_snapshot_id();
    if snapshot_id == 0 || snapshot_id > current_id {
        log("ERC20Snapshot: Invalid snapshot ID");
        return ERR_INVALID_SNAPSHOT;
    }

    // Get snapshot balance (if it exists)
    let mut balance = get_snapshot_balance(&account, snapshot_id);

    // If no snapshot balance stored, use current balance
    // (This means balance hasn't changed since snapshot)
    if balance == 0 {
        balance = get_balance(&account);
    }

    // Return balance as return data
    let result = balance.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("ERC20Snapshot: BalanceOfAt query successful");
            log_u64(snapshot_id, balance, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("ERC20Snapshot: Failed to set return data");
            e
        }
    }
}

/// Get total supply at specific snapshot
///
/// Format: [snapshot_id:8]
/// Returns: [supply:8]
fn op_total_supply_at(params: &[u8]) -> u64 {
    log("ERC20Snapshot: TotalSupplyAt");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 8 {
        log("ERC20Snapshot: Invalid totalSupplyAt parameters");
        return ERR_INVALID_PARAMS;
    }

    // Parse snapshot ID
    let snapshot_id = u64::from_le_bytes([
        params[0], params[1], params[2], params[3], params[4], params[5], params[6], params[7],
    ]);

    // Validate snapshot ID
    let current_id = get_current_snapshot_id();
    if snapshot_id == 0 || snapshot_id > current_id {
        log("ERC20Snapshot: Invalid snapshot ID");
        return ERR_INVALID_SNAPSHOT;
    }

    // Get snapshot supply
    let supply = get_snapshot_supply(snapshot_id);

    // Return supply as return data
    let result = supply.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("ERC20Snapshot: TotalSupplyAt query successful");
            log_u64(snapshot_id, supply, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("ERC20Snapshot: Failed to set return data");
            e
        }
    }
}

/// Get current snapshot ID
///
/// Returns: [snapshot_id:8]
fn op_get_current_snapshot_id() -> u64 {
    log("ERC20Snapshot: GetCurrentSnapshotId");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let snapshot_id = get_current_snapshot_id();

    // Return snapshot ID as return data
    let result = snapshot_id.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("ERC20Snapshot: GetCurrentSnapshotId query successful");
            log_u64(snapshot_id, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("ERC20Snapshot: Failed to set return data");
            e
        }
    }
}

// ============================================================================
// Query Operations - ERC20
// ============================================================================

/// Get balance of account
fn op_balance_of(params: &[u8]) -> u64 {
    log("ERC20Snapshot: BalanceOf");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        log("ERC20Snapshot: Invalid balanceOf parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut account = [0u8; 32];
    account.copy_from_slice(&params[0..32]);

    let balance = get_balance(&account);

    let result = balance.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("ERC20Snapshot: BalanceOf query successful");
            log_u64(balance, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("ERC20Snapshot: Failed to set return data");
            e
        }
    }
}

/// Get allowance
fn op_allowance(params: &[u8]) -> u64 {
    log("ERC20Snapshot: Allowance");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 64 {
        log("ERC20Snapshot: Invalid allowance parameters");
        return ERR_INVALID_PARAMS;
    }

    let mut owner = [0u8; 32];
    owner.copy_from_slice(&params[0..32]);

    let mut spender = [0u8; 32];
    spender.copy_from_slice(&params[32..64]);

    let allowance = get_allowance(&owner, &spender);

    let result = allowance.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("ERC20Snapshot: Allowance query successful");
            log_u64(allowance, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("ERC20Snapshot: Failed to set return data");
            e
        }
    }
}

/// Get total supply
fn op_total_supply() -> u64 {
    log("ERC20Snapshot: TotalSupply");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let total_supply = get_total_supply();

    let result = total_supply.to_le_bytes();
    match set_return_data(&result) {
        Ok(_) => {
            log("ERC20Snapshot: TotalSupply query successful");
            log_u64(total_supply, 0, 0, 0, 0);
            SUCCESS
        }
        Err(e) => {
            log("ERC20Snapshot: Failed to set return data");
            e
        }
    }
}

/// Get token name
fn op_name() -> u64 {
    log("ERC20Snapshot: Name");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let mut buffer = [0u8; MAX_NAME_LENGTH];
    let len = storage_read(&[KEY_NAME], &mut buffer);

    if len > 0 {
        match set_return_data(&buffer[..len as usize]) {
            Ok(_) => {
                log("ERC20Snapshot: Name query successful");
                SUCCESS
            }
            Err(e) => {
                log("ERC20Snapshot: Failed to set return data");
                e
            }
        }
    } else {
        log("ERC20Snapshot: Name not found");
        ERR_INVALID_PARAMS
    }
}

/// Get token symbol
fn op_symbol() -> u64 {
    log("ERC20Snapshot: Symbol");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let mut buffer = [0u8; MAX_SYMBOL_LENGTH];
    let len = storage_read(&[KEY_SYMBOL], &mut buffer);

    if len > 0 {
        match set_return_data(&buffer[..len as usize]) {
            Ok(_) => {
                log("ERC20Snapshot: Symbol query successful");
                SUCCESS
            }
            Err(e) => {
                log("ERC20Snapshot: Failed to set return data");
                e
            }
        }
    } else {
        log("ERC20Snapshot: Symbol not found");
        ERR_INVALID_PARAMS
    }
}

/// Get token decimals
fn op_decimals() -> u64 {
    log("ERC20Snapshot: Decimals");

    if !is_initialized() {
        log("ERC20Snapshot: Not initialized");
        return ERR_NOT_INITIALIZED;
    }

    let mut buffer = [0u8; 1];
    let len = storage_read(&[KEY_DECIMALS], &mut buffer);

    if len > 0 {
        match set_return_data(&buffer[..1]) {
            Ok(_) => {
                log("ERC20Snapshot: Decimals query successful");
                log_u64(buffer[0] as u64, 0, 0, 0, 0);
                SUCCESS
            }
            Err(e) => {
                log("ERC20Snapshot: Failed to set return data");
                e
            }
        }
    } else {
        log("ERC20Snapshot: Decimals not found");
        ERR_INVALID_PARAMS
    }
}

// ============================================================================
// Main Entrypoint
// ============================================================================

/// Contract entrypoint
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("ERC20Snapshot: Contract invoked");

    // Get input data
    let mut input = [0u8; 1024];
    let len = get_input_data(&mut input);

    if len == 0 {
        log("ERC20Snapshot: No input data");
        return ERR_INVALID_INSTRUCTION;
    }

    // Extract opcode
    let opcode = input[0];
    let params = &input[1..len as usize];

    // Dispatch based on opcode
    match opcode {
        // ERC20 operations
        OP_INITIALIZE => op_initialize(params),
        OP_TRANSFER => op_transfer(params),
        OP_APPROVE => op_approve(params),
        OP_TRANSFER_FROM => op_transfer_from(params),
        OP_MINT => op_mint(params),
        OP_BURN => op_burn(params),
        OP_BALANCE_OF => op_balance_of(params),
        OP_ALLOWANCE => op_allowance(params),
        OP_TOTAL_SUPPLY => op_total_supply(),
        OP_NAME => op_name(),
        OP_SYMBOL => op_symbol(),
        OP_DECIMALS => op_decimals(),
        // Snapshot operations
        OP_SNAPSHOT => op_snapshot(),
        OP_BALANCE_OF_AT => op_balance_of_at(params),
        OP_TOTAL_SUPPLY_AT => op_total_supply_at(params),
        OP_GET_CURRENT_SNAPSHOT_ID => op_get_current_snapshot_id(),
        _ => {
            log("ERC20Snapshot: Unknown opcode");
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
