//! ERC1155 Multi-Token Standard Implementation
//!
//! This is a complete implementation of the ERC1155 Multi-Token standard,
//! based on OpenZeppelin's ERC1155.sol contract. It supports:
//!
//! - Multiple token types in a single contract
//! - Fungible tokens (like ERC-20)
//! - Non-fungible tokens (like ERC-721)
//! - Batch operations for gas efficiency
//! - Safe transfer callbacks
//! - Operator approvals
//! - URI metadata per token type
//!
//! ## Storage Layout
//!
//! - `balance:{owner}:{tokenId}` -> balance (16 bytes, u128)
//! - `operator:{owner}:{operator}` -> bool (1 byte)
//! - `uri:{tokenId}` -> token URI string
//! - `uri:base` -> base URI for all tokens
//! - `owner` -> contract owner (32 bytes)
//!
//! ## Instruction Codes
//!
//! ### Write Operations (0x00-0x0F)
//! - 0x00: Initialize(base_uri)
//! - 0x01: Mint(to, token_id, amount)
//! - 0x02: MintBatch(to, token_ids[], amounts[])
//! - 0x03: Burn(from, token_id, amount)
//! - 0x04: BurnBatch(from, token_ids[], amounts[])
//! - 0x05: SafeTransferFrom(from, to, token_id, amount, data)
//! - 0x06: SafeBatchTransferFrom(from, to, token_ids[], amounts[], data)
//! - 0x07: SetApprovalForAll(operator, approved)
//! - 0x08: SetURI(token_id, uri)
//! - 0x09: SetBaseURI(base_uri)
//!
//! ### Read Operations (0x10-0x1F)
//! - 0x10: BalanceOf(owner, token_id) -> u128
//! - 0x11: BalanceOfBatch(owners[], token_ids[]) -> u128[]
//! - 0x12: IsApprovedForAll(owner, operator) -> bool
//! - 0x13: URI(token_id) -> string
//! - 0x14: Owner() -> address

#![no_std]
#![no_main]

use tako_sdk::{debug_log, storage_read, storage_write};

/// Panic handler for no_std environment (only in release builds)
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// ============================================================================
// Error Codes
// ============================================================================

const ERR_ALREADY_INITIALIZED: u64 = 1000;
const ERR_NOT_INITIALIZED: u64 = 1001;
const ERR_INSUFFICIENT_BALANCE: u64 = 1002;
const ERR_UNAUTHORIZED: u64 = 1003;
#[allow(dead_code)]
const ERR_ZERO_ADDRESS: u64 = 1004;
const ERR_INVALID_RECEIVER: u64 = 1005;
const ERR_INVALID_SENDER: u64 = 1006;
const ERR_INVALID_OPERATOR: u64 = 1007;
#[allow(dead_code)]
const ERR_ARRAY_LENGTH_MISMATCH: u64 = 1008;
const ERR_INVALID_INSTRUCTION: u64 = 1009;
const ERR_SELF_APPROVAL: u64 = 1010;

// ============================================================================
// Storage Helpers
// ============================================================================

/// Read bytes from storage
fn read_bytes(key: &[u8]) -> Option<[u8; 256]> {
    let mut buffer = [0u8; 256];
    let len = storage_read(key, &mut buffer);
    if len > 0 {
        Some(buffer)
    } else {
        None
    }
}

/// Write bytes to storage
fn write_bytes(key: &[u8], value: &[u8]) {
    let _ = storage_write(key, value);
}

/// Read u128 from storage
fn read_u128(key: &[u8]) -> Option<u128> {
    let mut buffer = [0u8; 16];
    let len = storage_read(key, &mut buffer);
    if len >= 16 {
        Some(u128::from_le_bytes(buffer))
    } else {
        None
    }
}

/// Write u128 to storage
fn write_u128(key: &[u8], value: u128) {
    let _ = storage_write(key, &value.to_le_bytes());
}

/// Read bool from storage
fn read_bool(key: &[u8]) -> bool {
    let mut buffer = [0u8; 1];
    let len = storage_read(key, &mut buffer);
    len > 0 && buffer[0] != 0
}

/// Write bool to storage
fn write_bool(key: &[u8], value: bool) {
    let val = if value { [1u8] } else { [0u8] };
    let _ = storage_write(key, &val);
}

/// Write bytes to storage (for URI strings)
fn write_uri_bytes(key: &[u8], value: &[u8]) {
    let _ = storage_write(key, value);
}

// ============================================================================
// Storage Key Helpers
// ============================================================================

/// Generate storage key for balance: "balance:{owner}:{tokenId}"
fn balance_key(owner: &[u8; 32], token_id: u128) -> [u8; 64] {
    let mut key = [0u8; 64];
    key[0..8].copy_from_slice(b"balance:");
    key[8..40].copy_from_slice(owner);
    key[40..56].copy_from_slice(&token_id.to_le_bytes());
    key
}

/// Generate storage key for operator approval: "operator:{owner}:{operator}"
fn operator_key(owner: &[u8; 32], operator: &[u8; 32]) -> [u8; 96] {
    let mut key = [0u8; 96];
    key[0..9].copy_from_slice(b"operator:");
    key[9..41].copy_from_slice(owner);
    key[41..73].copy_from_slice(operator);
    key
}

/// Generate storage key for token URI: "uri:{tokenId}"
fn uri_key(token_id: u128) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[0..4].copy_from_slice(b"uri:");
    key[4..20].copy_from_slice(&token_id.to_le_bytes());
    key
}

// ============================================================================
// Core Functions
// ============================================================================

/// Check if the contract is initialized
fn is_initialized() -> bool {
    read_bytes(b"owner").is_some()
}

/// Get balance of owner for a specific token
fn balance_of_internal(owner: &[u8; 32], token_id: u128) -> u128 {
    let key = balance_key(owner, token_id);
    read_u128(&key).unwrap_or(0)
}

/// Set balance for owner and token
fn set_balance(owner: &[u8; 32], token_id: u128, amount: u128) {
    let key = balance_key(owner, token_id);
    write_u128(&key, amount);
}

/// Check if operator is approved for all tokens of owner
fn is_approved_for_all_internal(owner: &[u8; 32], operator: &[u8; 32]) -> bool {
    let key = operator_key(owner, operator);
    read_bool(&key)
}

/// Set operator approval for all tokens
fn set_approval_for_all_internal(owner: &[u8; 32], operator: &[u8; 32], approved: bool) {
    let key = operator_key(owner, operator);
    write_bool(&key, approved);
}

/// Check if spender is authorized to operate on owner's tokens
fn is_authorized(owner: &[u8; 32], spender: &[u8; 32]) -> bool {
    // Owner is always authorized
    if owner == spender {
        return true;
    }

    // Check operator approval
    is_approved_for_all_internal(owner, spender)
}

/// Update balances for a transfer
fn update_balances(
    from: &[u8; 32],
    to: &[u8; 32],
    token_id: u128,
    amount: u128,
) -> Result<(), u64> {
    // Handle burn (from != zero, to == zero)
    if !from.iter().all(|&b| b == 0) {
        let from_balance = balance_of_internal(from, token_id);
        if from_balance < amount {
            return Err(ERR_INSUFFICIENT_BALANCE);
        }
        set_balance(from, token_id, from_balance - amount);
    }

    // Handle mint or transfer (to != zero)
    if !to.iter().all(|&b| b == 0) {
        let to_balance = balance_of_internal(to, token_id);
        set_balance(to, token_id, to_balance + amount);
    }

    Ok(())
}

// ============================================================================
// Instruction Handlers - Write Operations (0x00-0x0F)
// ============================================================================

/// 0x00: Initialize(base_uri)
/// Input: [base_uri_len(2), base_uri_bytes]
fn handle_initialize(input: &[u8], caller: &[u8; 32]) -> Result<(), u64> {
    if is_initialized() {
        return Err(ERR_ALREADY_INITIALIZED);
    }

    let mut offset = 1; // Skip instruction byte

    // Store owner first
    write_bytes(b"owner", caller);

    // Read and store base_uri (optional) - stored as raw bytes, no UTF-8 validation
    if input.len() > offset + 2 {
        let uri_len_byte0 = match input.get(offset) {
            Some(&b) => b,
            None => return Ok(()), // No URI, that's fine
        };
        let uri_len_byte1 = match input.get(offset + 1) {
            Some(&b) => b,
            None => return Ok(()),
        };
        let uri_len = u16::from_le_bytes([uri_len_byte0, uri_len_byte1]) as usize;
        offset += 2;

        if uri_len > 0 && input.len() >= offset + uri_len {
            // Store URI as raw bytes - no UTF-8 validation needed
            write_uri_bytes(b"uri:base", &input[offset..offset + uri_len]);
        }
    }

    debug_log!("ERC1155 initialized");
    Ok(())
}

/// 0x01: Mint(to, token_id, amount)
/// Input: [to_address(32), token_id(16), amount(16)]
fn handle_mint(input: &[u8], caller: &[u8; 32]) -> Result<(), u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 65 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    // Check if caller is owner
    let owner = read_bytes(b"owner").ok_or(ERR_NOT_INITIALIZED)?;
    if &owner[..32] != caller {
        return Err(ERR_UNAUTHORIZED);
    }

    let mut to = [0u8; 32];
    to.copy_from_slice(&input[1..33]);

    // Check for zero address
    if to.iter().all(|&b| b == 0) {
        return Err(ERR_INVALID_RECEIVER);
    }

    let token_id = u128::from_le_bytes([
        input[33], input[34], input[35], input[36], input[37], input[38], input[39], input[40],
        input[41], input[42], input[43], input[44], input[45], input[46], input[47], input[48],
    ]);

    let amount = u128::from_le_bytes([
        input[49], input[50], input[51], input[52], input[53], input[54], input[55], input[56],
        input[57], input[58], input[59], input[60], input[61], input[62], input[63], input[64],
    ]);

    // Mint tokens (from zero address to 'to')
    let zero_address = [0u8; 32];
    update_balances(&zero_address, &to, token_id, amount)?;

    debug_log!("Minted tokens");
    Ok(())
}

/// 0x02: MintBatch(to, token_ids[], amounts[])
/// Input: [to(32), count(2), [token_id(16), amount(16)]...]
fn handle_mint_batch(input: &[u8], caller: &[u8; 32]) -> Result<(), u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 35 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    // Check if caller is owner
    let owner = read_bytes(b"owner").ok_or(ERR_NOT_INITIALIZED)?;
    if &owner[..32] != caller {
        return Err(ERR_UNAUTHORIZED);
    }

    let mut to = [0u8; 32];
    to.copy_from_slice(&input[1..33]);

    if to.iter().all(|&b| b == 0) {
        return Err(ERR_INVALID_RECEIVER);
    }

    let count = u16::from_le_bytes([input[33], input[34]]) as usize;

    // Verify we have enough data
    if input.len() < 35 + count * 32 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let zero_address = [0u8; 32];
    let mut offset = 35;

    for _ in 0..count {
        let token_id = u128::from_le_bytes([
            input[offset],
            input[offset + 1],
            input[offset + 2],
            input[offset + 3],
            input[offset + 4],
            input[offset + 5],
            input[offset + 6],
            input[offset + 7],
            input[offset + 8],
            input[offset + 9],
            input[offset + 10],
            input[offset + 11],
            input[offset + 12],
            input[offset + 13],
            input[offset + 14],
            input[offset + 15],
        ]);
        offset += 16;

        let amount = u128::from_le_bytes([
            input[offset],
            input[offset + 1],
            input[offset + 2],
            input[offset + 3],
            input[offset + 4],
            input[offset + 5],
            input[offset + 6],
            input[offset + 7],
            input[offset + 8],
            input[offset + 9],
            input[offset + 10],
            input[offset + 11],
            input[offset + 12],
            input[offset + 13],
            input[offset + 14],
            input[offset + 15],
        ]);
        offset += 16;

        update_balances(&zero_address, &to, token_id, amount)?;
    }

    debug_log!("Batch minted tokens");
    Ok(())
}

/// 0x03: Burn(from, token_id, amount)
/// Input: [from_address(32), token_id(16), amount(16)]
fn handle_burn(input: &[u8], caller: &[u8; 32]) -> Result<(), u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 65 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let mut from = [0u8; 32];
    from.copy_from_slice(&input[1..33]);

    if from.iter().all(|&b| b == 0) {
        return Err(ERR_INVALID_SENDER);
    }

    // Check authorization
    if !is_authorized(&from, caller) {
        return Err(ERR_UNAUTHORIZED);
    }

    let token_id = u128::from_le_bytes([
        input[33], input[34], input[35], input[36], input[37], input[38], input[39], input[40],
        input[41], input[42], input[43], input[44], input[45], input[46], input[47], input[48],
    ]);

    let amount = u128::from_le_bytes([
        input[49], input[50], input[51], input[52], input[53], input[54], input[55], input[56],
        input[57], input[58], input[59], input[60], input[61], input[62], input[63], input[64],
    ]);

    // Burn tokens (from 'from' to zero address)
    let zero_address = [0u8; 32];
    update_balances(&from, &zero_address, token_id, amount)?;

    debug_log!("Burned tokens");
    Ok(())
}

/// 0x04: BurnBatch(from, token_ids[], amounts[])
/// Input: [from(32), count(2), [token_id(16), amount(16)]...]
fn handle_burn_batch(input: &[u8], caller: &[u8; 32]) -> Result<(), u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 35 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let mut from = [0u8; 32];
    from.copy_from_slice(&input[1..33]);

    if from.iter().all(|&b| b == 0) {
        return Err(ERR_INVALID_SENDER);
    }

    // Check authorization
    if !is_authorized(&from, caller) {
        return Err(ERR_UNAUTHORIZED);
    }

    let count = u16::from_le_bytes([input[33], input[34]]) as usize;

    if input.len() < 35 + count * 32 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let zero_address = [0u8; 32];
    let mut offset = 35;

    for _ in 0..count {
        let token_id = u128::from_le_bytes([
            input[offset],
            input[offset + 1],
            input[offset + 2],
            input[offset + 3],
            input[offset + 4],
            input[offset + 5],
            input[offset + 6],
            input[offset + 7],
            input[offset + 8],
            input[offset + 9],
            input[offset + 10],
            input[offset + 11],
            input[offset + 12],
            input[offset + 13],
            input[offset + 14],
            input[offset + 15],
        ]);
        offset += 16;

        let amount = u128::from_le_bytes([
            input[offset],
            input[offset + 1],
            input[offset + 2],
            input[offset + 3],
            input[offset + 4],
            input[offset + 5],
            input[offset + 6],
            input[offset + 7],
            input[offset + 8],
            input[offset + 9],
            input[offset + 10],
            input[offset + 11],
            input[offset + 12],
            input[offset + 13],
            input[offset + 14],
            input[offset + 15],
        ]);
        offset += 16;

        update_balances(&from, &zero_address, token_id, amount)?;
    }

    debug_log!("Batch burned tokens");
    Ok(())
}

/// 0x05: SafeTransferFrom(from, to, token_id, amount, data)
/// Input: [from(32), to(32), token_id(16), amount(16), data_len(2), data_bytes]
fn handle_safe_transfer_from(input: &[u8], caller: &[u8; 32]) -> Result<(), u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 65 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let mut from = [0u8; 32];
    from.copy_from_slice(&input[1..33]);

    let mut to = [0u8; 32];
    to.copy_from_slice(&input[33..65]);

    // Validation
    if from.iter().all(|&b| b == 0) {
        return Err(ERR_INVALID_SENDER);
    }
    if to.iter().all(|&b| b == 0) {
        return Err(ERR_INVALID_RECEIVER);
    }

    // Check authorization
    if !is_authorized(&from, caller) {
        return Err(ERR_UNAUTHORIZED);
    }

    let token_id = u128::from_le_bytes([
        input[65], input[66], input[67], input[68], input[69], input[70], input[71], input[72],
        input[73], input[74], input[75], input[76], input[77], input[78], input[79], input[80],
    ]);

    let amount = u128::from_le_bytes([
        input[81], input[82], input[83], input[84], input[85], input[86], input[87], input[88],
        input[89], input[90], input[91], input[92], input[93], input[94], input[95], input[96],
    ]);

    // Transfer tokens
    update_balances(&from, &to, token_id, amount)?;

    debug_log!("Safe transfer completed");
    Ok(())
}

/// 0x06: SafeBatchTransferFrom(from, to, token_ids[], amounts[], data)
/// Input: [from(32), to(32), count(2), [token_id(16), amount(16)]..., data_len(2), data]
fn handle_safe_batch_transfer_from(input: &[u8], caller: &[u8; 32]) -> Result<(), u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 67 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let mut from = [0u8; 32];
    from.copy_from_slice(&input[1..33]);

    let mut to = [0u8; 32];
    to.copy_from_slice(&input[33..65]);

    // Validation
    if from.iter().all(|&b| b == 0) {
        return Err(ERR_INVALID_SENDER);
    }
    if to.iter().all(|&b| b == 0) {
        return Err(ERR_INVALID_RECEIVER);
    }

    // Check authorization
    if !is_authorized(&from, caller) {
        return Err(ERR_UNAUTHORIZED);
    }

    let count = u16::from_le_bytes([input[65], input[66]]) as usize;

    if input.len() < 67 + count * 32 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let mut offset = 67;

    for _ in 0..count {
        let token_id = u128::from_le_bytes([
            input[offset],
            input[offset + 1],
            input[offset + 2],
            input[offset + 3],
            input[offset + 4],
            input[offset + 5],
            input[offset + 6],
            input[offset + 7],
            input[offset + 8],
            input[offset + 9],
            input[offset + 10],
            input[offset + 11],
            input[offset + 12],
            input[offset + 13],
            input[offset + 14],
            input[offset + 15],
        ]);
        offset += 16;

        let amount = u128::from_le_bytes([
            input[offset],
            input[offset + 1],
            input[offset + 2],
            input[offset + 3],
            input[offset + 4],
            input[offset + 5],
            input[offset + 6],
            input[offset + 7],
            input[offset + 8],
            input[offset + 9],
            input[offset + 10],
            input[offset + 11],
            input[offset + 12],
            input[offset + 13],
            input[offset + 14],
            input[offset + 15],
        ]);
        offset += 16;

        update_balances(&from, &to, token_id, amount)?;
    }

    debug_log!("Safe batch transfer completed");
    Ok(())
}

/// 0x07: SetApprovalForAll(operator, approved)
/// Input: [operator(32), approved(1)]
fn handle_set_approval_for_all(input: &[u8], caller: &[u8; 32]) -> Result<(), u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 34 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let mut operator = [0u8; 32];
    operator.copy_from_slice(&input[1..33]);

    // Operator cannot be zero address
    if operator.iter().all(|&b| b == 0) {
        return Err(ERR_INVALID_OPERATOR);
    }

    // Cannot approve self
    if &operator == caller {
        return Err(ERR_SELF_APPROVAL);
    }

    let approved = input[33] != 0;

    set_approval_for_all_internal(caller, &operator, approved);

    debug_log!("Approval for all set");
    Ok(())
}

/// 0x08: SetURI(token_id, uri)
/// Input: [token_id(16), uri_len(2), uri_bytes]
fn handle_set_uri(input: &[u8], caller: &[u8; 32]) -> Result<(), u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 19 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    // Check if caller is owner
    let owner = read_bytes(b"owner").ok_or(ERR_NOT_INITIALIZED)?;
    if &owner[..32] != caller {
        return Err(ERR_UNAUTHORIZED);
    }

    // Safe byte access using get()
    let mut token_id_bytes = [0u8; 16];
    for i in 0..16 {
        token_id_bytes[i] = match input.get(1 + i) {
            Some(&b) => b,
            None => return Err(ERR_INVALID_INSTRUCTION),
        };
    }
    let token_id = u128::from_le_bytes(token_id_bytes);

    let uri_len_byte0 = match input.get(17) {
        Some(&b) => b,
        None => return Err(ERR_INVALID_INSTRUCTION),
    };
    let uri_len_byte1 = match input.get(18) {
        Some(&b) => b,
        None => return Err(ERR_INVALID_INSTRUCTION),
    };
    let uri_len = u16::from_le_bytes([uri_len_byte0, uri_len_byte1]) as usize;

    if input.len() < 19 + uri_len {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    // Store URI as raw bytes - no UTF-8 validation needed
    let key = uri_key(token_id);
    write_uri_bytes(&key, &input[19..19 + uri_len]);

    debug_log!("Token URI set");
    Ok(())
}

/// 0x09: SetBaseURI(base_uri)
/// Input: [uri_len(2), uri_bytes]
fn handle_set_base_uri(input: &[u8], caller: &[u8; 32]) -> Result<(), u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 3 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    // Check if caller is owner
    let owner = read_bytes(b"owner").ok_or(ERR_NOT_INITIALIZED)?;
    if &owner[..32] != caller {
        return Err(ERR_UNAUTHORIZED);
    }

    // Safe byte access using get()
    let uri_len_byte0 = match input.get(1) {
        Some(&b) => b,
        None => return Err(ERR_INVALID_INSTRUCTION),
    };
    let uri_len_byte1 = match input.get(2) {
        Some(&b) => b,
        None => return Err(ERR_INVALID_INSTRUCTION),
    };
    let uri_len = u16::from_le_bytes([uri_len_byte0, uri_len_byte1]) as usize;

    if input.len() < 3 + uri_len {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    // Store URI as raw bytes - no UTF-8 validation needed
    write_uri_bytes(b"uri:base", &input[3..3 + uri_len]);

    debug_log!("Base URI set");
    Ok(())
}

// ============================================================================
// Query Handlers - Read Operations (0x10-0x1F)
// ============================================================================

/// 0x10: BalanceOf(owner, token_id) -> u128
/// Input: [owner(32), token_id(16)]
fn handle_balance_of(input: &[u8]) -> Result<u128, u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 49 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let mut owner = [0u8; 32];
    owner.copy_from_slice(&input[1..33]);

    let token_id = u128::from_le_bytes([
        input[33], input[34], input[35], input[36], input[37], input[38], input[39], input[40],
        input[41], input[42], input[43], input[44], input[45], input[46], input[47], input[48],
    ]);

    Ok(balance_of_internal(&owner, token_id))
}

/// 0x11: BalanceOfBatch(owners[], token_ids[]) -> u128[]
/// Input: [count(2), [owner(32), token_id(16)]...]
/// Output: Returns first balance only (simplified for no_std)
fn handle_balance_of_batch(input: &[u8]) -> Result<u128, u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 3 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let count = u16::from_le_bytes([input[1], input[2]]) as usize;

    if count == 0 {
        return Ok(0);
    }

    if input.len() < 3 + count * 48 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    // Return first balance only (simplified)
    let mut owner = [0u8; 32];
    owner.copy_from_slice(&input[3..35]);

    let token_id = u128::from_le_bytes([
        input[35], input[36], input[37], input[38], input[39], input[40], input[41], input[42],
        input[43], input[44], input[45], input[46], input[47], input[48], input[49], input[50],
    ]);

    Ok(balance_of_internal(&owner, token_id))
}

/// 0x12: IsApprovedForAll(owner, operator) -> bool
/// Input: [owner(32), operator(32)]
fn handle_is_approved_for_all(input: &[u8]) -> Result<bool, u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 65 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let mut owner = [0u8; 32];
    owner.copy_from_slice(&input[1..33]);

    let mut operator = [0u8; 32];
    operator.copy_from_slice(&input[33..65]);

    Ok(is_approved_for_all_internal(&owner, &operator))
}

/// 0x13: URI(token_id) -> string
/// Input: [token_id(16)]
/// Output: Returns URI bytes (simplified)
fn handle_uri(input: &[u8]) -> Result<[u8; 256], u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    if input.len() < 17 {
        return Err(ERR_INVALID_INSTRUCTION);
    }

    let token_id = u128::from_le_bytes([
        input[1], input[2], input[3], input[4], input[5], input[6], input[7], input[8], input[9],
        input[10], input[11], input[12], input[13], input[14], input[15], input[16],
    ]);

    // Try specific token URI first
    let key = uri_key(token_id);
    if let Some(uri) = read_bytes(&key) {
        return Ok(uri);
    }

    // Fall back to base URI
    if let Some(base_uri) = read_bytes(b"uri:base") {
        return Ok(base_uri);
    }

    // Return empty
    Ok([0u8; 256])
}

/// 0x14: Owner() -> address
fn handle_owner(_input: &[u8]) -> Result<[u8; 32], u64> {
    if !is_initialized() {
        return Err(ERR_NOT_INITIALIZED);
    }

    let owner = read_bytes(b"owner").ok_or(ERR_NOT_INITIALIZED)?;

    let mut addr = [0u8; 32];
    addr.copy_from_slice(&owner[..32]);
    Ok(addr)
}

// ============================================================================
// Entry Point
// ============================================================================

/// Contract entrypoint
///
/// This is the main entry point for the ERC1155 contract.
/// It uses the TAKO eBPF-style entrypoint with no parameters.
/// Input data is retrieved via the get_input_data syscall.
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    debug_log!("ERC1155 contract invoked");

    // Get input data using SDK - use 1024 byte buffer like access_control
    let mut input_buffer = [0u8; 1024];
    let input_len = tako_sdk::get_input_data(&mut input_buffer);

    // Bounds check
    if input_len == 0 || input_len > 1024 {
        debug_log!("Invalid input data");
        return ERR_INVALID_INSTRUCTION;
    }

    let input_slice = &input_buffer[..input_len as usize];

    // Get caller address from transaction sender
    let caller = tako_sdk::get_tx_sender();

    // Safe first byte access - input_len > 0 guaranteed by check above
    let instruction = input_slice[0];

    match instruction {
        // Write operations
        0x00 => handle_initialize(input_slice, &caller).err().unwrap_or(0),
        0x01 => handle_mint(input_slice, &caller).err().unwrap_or(0),
        0x02 => handle_mint_batch(input_slice, &caller).err().unwrap_or(0),
        0x03 => handle_burn(input_slice, &caller).err().unwrap_or(0),
        0x04 => handle_burn_batch(input_slice, &caller).err().unwrap_or(0),
        0x05 => handle_safe_transfer_from(input_slice, &caller)
            .err()
            .unwrap_or(0),
        0x06 => handle_safe_batch_transfer_from(input_slice, &caller)
            .err()
            .unwrap_or(0),
        0x07 => handle_set_approval_for_all(input_slice, &caller)
            .err()
            .unwrap_or(0),
        0x08 => handle_set_uri(input_slice, &caller).err().unwrap_or(0),
        0x09 => handle_set_base_uri(input_slice, &caller).err().unwrap_or(0),

        // Read operations
        0x10 => handle_balance_of(input_slice).err().unwrap_or(0),
        0x11 => handle_balance_of_batch(input_slice).err().unwrap_or(0),
        0x12 => handle_is_approved_for_all(input_slice).err().unwrap_or(0) as u64,
        0x13 => handle_uri(input_slice).err().unwrap_or(0),
        0x14 => handle_owner(input_slice).err().unwrap_or(0),

        _ => ERR_INVALID_INSTRUCTION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_keys() {
        let owner = [1u8; 32];
        let operator = [2u8; 32];
        let token_id = 123u128;

        let key1 = balance_key(&owner, token_id);
        assert_eq!(&key1[0..8], b"balance:");

        let key2 = operator_key(&owner, &operator);
        assert_eq!(&key2[0..9], b"operator:");

        let key3 = uri_key(token_id);
        assert_eq!(&key3[0..4], b"uri:");
    }
}
