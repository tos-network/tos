//! AccessControl ERC1155 - Multi-token contract with role-based access control
//!
//! This contract demonstrates the composition of three security patterns:
//! - ERC1155: Multi-token standard for fungible and non-fungible tokens
//! - AccessControl: Role-based permission management
//! - ReentrancyGuard: Protection against reentrancy attacks
//!
//! Use cases:
//! - Gaming platforms with multiple item types and administrative roles
//! - NFT marketplaces with curators and artists
//! - DAO treasury management with different token types

#![no_std]

extern crate alloc;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};

use tako_sdk::{debug_log, get_tx_sender, storage_read, storage_write};

// Simple bump allocator for no_std environment
struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        const HEAP_SIZE: usize = 32 * 1024; // 32KB heap
        static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
        static mut HEAP_POS: usize = 0;

        let size = layout.size();
        let align = layout.align();
        let pos = (HEAP_POS + align - 1) / align * align;

        if pos + size > HEAP_SIZE {
            core::ptr::null_mut()
        } else {
            HEAP_POS = pos + size;
            HEAP.as_mut_ptr().add(pos)
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // No-op for bump allocator
    }
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// Role definitions
const DEFAULT_ADMIN_ROLE: [u8; 32] = [0u8; 32];
const MINTER_ROLE: [u8; 32] = [
    0x9f, 0x2d, 0xf0, 0xfe, 0xd2, 0xc7, 0x7a, 0x88, 0x84, 0x66, 0xf1, 0x70, 0xb1, 0xbb, 0x5d, 0xd8,
    0xed, 0x6d, 0x52, 0xa4, 0xae, 0x9d, 0xcc, 0xad, 0x04, 0xda, 0xa9, 0xab, 0x96, 0xc1, 0x5c, 0xfe,
]; // keccak256("MINTER_ROLE")
const BURNER_ROLE: [u8; 32] = [
    0x3c, 0x11, 0xd1, 0x6c, 0xba, 0xa5, 0x26, 0x65, 0xd1, 0x94, 0x9c, 0x9a, 0xf6, 0x3b, 0xe5, 0x44,
    0xf5, 0x5c, 0x78, 0x89, 0x6c, 0xf5, 0x50, 0xde, 0x40, 0xd6, 0x41, 0x8b, 0x8b, 0x2e, 0xd8, 0xb8,
]; // keccak256("BURNER_ROLE")
const URI_SETTER_ROLE: [u8; 32] = [
    0x7d, 0xf1, 0xa5, 0xf6, 0x0b, 0x8a, 0x5e, 0x5d, 0x8a, 0x8e, 0xb9, 0x8f, 0x71, 0x0d, 0x87, 0x35,
    0xa7, 0x9c, 0x3b, 0x4e, 0x9a, 0x84, 0x9d, 0x3c, 0x09, 0x32, 0x6f, 0x41, 0x5e, 0x6e, 0x0b, 0x65,
]; // keccak256("URI_SETTER_ROLE")

// Instruction opcodes
const OP_INITIALIZE: u8 = 0x00;
const OP_MINT: u8 = 0x01;
const OP_MINT_BATCH: u8 = 0x02;
const OP_BURN: u8 = 0x03;
const OP_BURN_BATCH: u8 = 0x04;
const OP_SAFE_TRANSFER_FROM: u8 = 0x05;
const OP_SAFE_BATCH_TRANSFER_FROM: u8 = 0x06;
const OP_SET_APPROVAL_FOR_ALL: u8 = 0x07;
const OP_SET_URI: u8 = 0x08;
const OP_BALANCE_OF: u8 = 0x10;
const OP_BALANCE_OF_BATCH: u8 = 0x11;
const OP_IS_APPROVED_FOR_ALL: u8 = 0x12;
const OP_URI: u8 = 0x13;
const OP_TOTAL_MINTED: u8 = 0x14;
const OP_TOTAL_BURNED: u8 = 0x15;
const OP_GRANT_ROLE: u8 = 0x20;
const OP_REVOKE_ROLE: u8 = 0x21;
const OP_RENOUNCE_ROLE: u8 = 0x22;
const OP_HAS_ROLE: u8 = 0x23;
const OP_GET_ROLE_ADMIN: u8 = 0x24;
const OP_SET_ROLE_ADMIN: u8 = 0x25;

// Error codes
const ERR_INSUFFICIENT_BALANCE: u64 = 1001;
const ERR_NOT_OWNER_OR_APPROVED: u64 = 1002;
const ERR_ZERO_ADDRESS: u64 = 1003;
const ERR_ARRAY_LENGTH_MISMATCH: u64 = 1004;
const ERR_MISSING_ROLE: u64 = 1005;
const ERR_INVALID_ROLE: u64 = 1006;
const ERR_REENTRANT_CALL: u64 = 1007;
const ERR_ALREADY_INITIALIZED: u64 = 1008;
const ERR_NOT_INITIALIZED: u64 = 1009;
const ERR_INVALID_INSTRUCTION: u64 = 1010;
const ERR_INVALID_PARAMS: u64 = 1011;
const ERR_UNAUTHORIZED: u64 = 1012;

// ReentrancyGuard states
const NOT_ENTERED: u8 = 1;
const ENTERED: u8 = 2;

// Storage key prefixes
const PREFIX_BALANCE: &[u8] = b"balance:";
const PREFIX_OPERATOR: &[u8] = b"operator:";
const PREFIX_URI: &[u8] = b"uri:";
const PREFIX_TOTAL_MINTED: &[u8] = b"total_minted:";
const PREFIX_TOTAL_BURNED: &[u8] = b"total_burned:";
const PREFIX_ROLE: &[u8] = b"role:";
const PREFIX_ROLE_ADMIN: &[u8] = b"role_admin:";
const KEY_INITIALIZED: &[u8] = b"initialized";
const KEY_OWNER: &[u8] = b"owner";
const KEY_REENTRANCY_STATUS: &[u8] = b"reentrancy_status";
const KEY_BASE_URI: &[u8] = b"uri:base";

// Storage helper functions

fn read_bytes(key: &[u8]) -> Option<Vec<u8>> {
    let mut buffer = [0u8; 256];
    let len = storage_read(key, &mut buffer);
    if len > 0 {
        Some(buffer[..len as usize].to_vec())
    } else {
        None
    }
}

fn write_bytes(key: &[u8], value: &[u8]) {
    let _ = storage_write(key, value);
}

fn read_u128(key: &[u8]) -> Option<u128> {
    let mut buffer = [0u8; 16];
    let len = storage_read(key, &mut buffer);
    if len >= 16 {
        Some(u128::from_le_bytes(buffer))
    } else {
        None
    }
}

fn write_u128(key: &[u8], value: u128) {
    let _ = storage_write(key, &value.to_le_bytes());
}

fn read_bool(key: &[u8]) -> bool {
    let mut buffer = [0u8; 1];
    let len = storage_read(key, &mut buffer);
    len > 0 && buffer[0] != 0
}

fn write_bool(key: &[u8], value: bool) {
    let val = if value { [1u8] } else { [0u8] };
    let _ = storage_write(key, &val);
}

fn read_address(key: &[u8]) -> Option<[u8; 32]> {
    let mut buffer = [0u8; 32];
    let len = storage_read(key, &mut buffer);
    if len >= 32 {
        Some(buffer)
    } else {
        None
    }
}

fn write_address(key: &[u8], addr: &[u8; 32]) {
    let _ = storage_write(key, addr);
}

// Helper functions - Storage access

fn is_initialized() -> bool {
    read_bool(KEY_INITIALIZED)
}

fn set_initialized() {
    write_bool(KEY_INITIALIZED, true);
}

fn get_owner() -> [u8; 32] {
    read_address(KEY_OWNER).unwrap_or([0u8; 32])
}

fn set_owner(owner: &[u8; 32]) {
    write_address(KEY_OWNER, owner);
}

fn get_balance(owner: &[u8; 32], token_id: u128) -> u128 {
    let mut key = Vec::from(PREFIX_BALANCE);
    key.extend_from_slice(owner);
    key.push(b':');
    key.extend_from_slice(&token_id.to_le_bytes());

    read_u128(&key).unwrap_or(0)
}

fn set_balance(owner: &[u8; 32], token_id: u128, amount: u128) {
    let mut key = Vec::from(PREFIX_BALANCE);
    key.extend_from_slice(owner);
    key.push(b':');
    key.extend_from_slice(&token_id.to_le_bytes());

    write_u128(&key, amount);
}

fn get_operator_approval(owner: &[u8; 32], operator: &[u8; 32]) -> bool {
    let mut key = Vec::from(PREFIX_OPERATOR);
    key.extend_from_slice(owner);
    key.push(b':');
    key.extend_from_slice(operator);

    read_bool(&key)
}

fn set_operator_approval(owner: &[u8; 32], operator: &[u8; 32], approved: bool) {
    let mut key = Vec::from(PREFIX_OPERATOR);
    key.extend_from_slice(owner);
    key.push(b':');
    key.extend_from_slice(operator);

    write_bool(&key, approved);
}

fn get_token_uri(token_id: u128) -> Option<Vec<u8>> {
    let mut key = Vec::from(PREFIX_URI);
    key.extend_from_slice(&token_id.to_le_bytes());
    read_bytes(&key)
}

fn set_token_uri(token_id: u128, uri: &[u8]) {
    let mut key = Vec::from(PREFIX_URI);
    key.extend_from_slice(&token_id.to_le_bytes());
    write_bytes(&key, uri);
}

fn get_base_uri() -> Option<Vec<u8>> {
    read_bytes(KEY_BASE_URI)
}

fn set_base_uri(uri: &[u8]) {
    write_bytes(KEY_BASE_URI, uri);
}

fn get_total_minted(token_id: u128) -> u128 {
    let mut key = Vec::from(PREFIX_TOTAL_MINTED);
    key.extend_from_slice(&token_id.to_le_bytes());
    read_u128(&key).unwrap_or(0)
}

fn set_total_minted(token_id: u128, amount: u128) {
    let mut key = Vec::from(PREFIX_TOTAL_MINTED);
    key.extend_from_slice(&token_id.to_le_bytes());
    write_u128(&key, amount);
}

fn get_total_burned(token_id: u128) -> u128 {
    let mut key = Vec::from(PREFIX_TOTAL_BURNED);
    key.extend_from_slice(&token_id.to_le_bytes());
    read_u128(&key).unwrap_or(0)
}

fn set_total_burned(token_id: u128, amount: u128) {
    let mut key = Vec::from(PREFIX_TOTAL_BURNED);
    key.extend_from_slice(&token_id.to_le_bytes());
    write_u128(&key, amount);
}

// Helper functions - AccessControl

fn has_role(role: &[u8; 32], account: &[u8; 32]) -> bool {
    let mut key = Vec::from(PREFIX_ROLE);
    key.extend_from_slice(role);
    key.push(b':');
    key.extend_from_slice(account);

    read_bool(&key)
}

fn grant_role_internal(role: &[u8; 32], account: &[u8; 32]) {
    let mut key = Vec::from(PREFIX_ROLE);
    key.extend_from_slice(role);
    key.push(b':');
    key.extend_from_slice(account);

    write_bool(&key, true);
}

fn revoke_role_internal(role: &[u8; 32], account: &[u8; 32]) {
    let mut key = Vec::from(PREFIX_ROLE);
    key.extend_from_slice(role);
    key.push(b':');
    key.extend_from_slice(account);

    write_bool(&key, false);
}

fn get_role_admin(role: &[u8; 32]) -> [u8; 32] {
    let mut key = Vec::from(PREFIX_ROLE_ADMIN);
    key.extend_from_slice(role);

    read_address(&key).unwrap_or(DEFAULT_ADMIN_ROLE)
}

fn set_role_admin_internal(role: &[u8; 32], admin_role: &[u8; 32]) {
    let mut key = Vec::from(PREFIX_ROLE_ADMIN);
    key.extend_from_slice(role);
    write_address(&key, admin_role);
}

// Helper functions - ReentrancyGuard

fn get_reentrancy_status() -> u8 {
    let mut buffer = [0u8; 1];
    let len = storage_read(KEY_REENTRANCY_STATUS, &mut buffer);
    if len > 0 {
        buffer[0]
    } else {
        NOT_ENTERED
    }
}

fn set_reentrancy_status(status: u8) {
    let _ = storage_write(KEY_REENTRANCY_STATUS, &[status]);
}

fn is_entered() -> bool {
    get_reentrancy_status() == ENTERED
}

fn enter() -> bool {
    if is_entered() {
        return false;
    }
    set_reentrancy_status(ENTERED);
    true
}

fn leave() -> bool {
    if !is_entered() {
        return false;
    }
    set_reentrancy_status(NOT_ENTERED);
    true
}

// Helper functions - Validation

fn is_zero_address(addr: &[u8; 32]) -> bool {
    addr.iter().all(|&b| b == 0)
}

fn parse_address(data: &[u8], offset: usize) -> Result<[u8; 32], u64> {
    if data.len() < offset + 32 {
        return Err(ERR_INVALID_PARAMS);
    }
    let mut addr = [0u8; 32];
    addr.copy_from_slice(&data[offset..offset + 32]);
    Ok(addr)
}

fn parse_u128(data: &[u8], offset: usize) -> Result<u128, u64> {
    if data.len() < offset + 16 {
        return Err(ERR_INVALID_PARAMS);
    }
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&data[offset..offset + 16]);
    Ok(u128::from_le_bytes(bytes))
}

fn parse_u32(data: &[u8], offset: usize) -> Result<u32, u64> {
    if data.len() < offset + 4 {
        return Err(ERR_INVALID_PARAMS);
    }
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&data[offset..offset + 4]);
    Ok(u32::from_le_bytes(bytes))
}

// ERC1155 Operations

fn op_initialize(params: &[u8]) -> u64 {
    if is_initialized() {
        return ERR_ALREADY_INITIALIZED;
    }

    let sender = get_tx_sender();

    // Parse base URI length and data
    let uri_len = if params.len() >= 4 {
        match parse_u32(params, 0) {
            Ok(len) => len as usize,
            Err(e) => return e,
        }
    } else {
        0
    };

    if params.len() < 4 + uri_len {
        return ERR_INVALID_PARAMS;
    }

    // Set base URI if provided
    if uri_len > 0 {
        set_base_uri(&params[4..4 + uri_len]);
    }

    // Grant admin role to deployer
    grant_role_internal(&DEFAULT_ADMIN_ROLE, &sender);

    // Set role admins
    set_role_admin_internal(&MINTER_ROLE, &DEFAULT_ADMIN_ROLE);
    set_role_admin_internal(&BURNER_ROLE, &DEFAULT_ADMIN_ROLE);
    set_role_admin_internal(&URI_SETTER_ROLE, &DEFAULT_ADMIN_ROLE);

    set_owner(&sender);
    set_initialized();
    set_reentrancy_status(NOT_ENTERED);

    debug_log!("Initialized AccessControl ERC1155");
    0
}

fn op_mint(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    let sender = get_tx_sender();
    if !has_role(&MINTER_ROLE, &sender) {
        return ERR_MISSING_ROLE;
    }

    if params.len() < 32 + 16 + 16 {
        return ERR_INVALID_PARAMS;
    }

    let to = match parse_address(params, 0) {
        Ok(addr) => addr,
        Err(e) => return e,
    };
    if is_zero_address(&to) {
        return ERR_ZERO_ADDRESS;
    }

    let token_id = match parse_u128(params, 32) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let amount = match parse_u128(params, 48) {
        Ok(amt) => amt,
        Err(e) => return e,
    };

    // Update balances
    let current_balance = get_balance(&to, token_id);
    let new_balance = match current_balance.checked_add(amount) {
        Some(b) => b,
        None => return ERR_INVALID_PARAMS,
    };
    set_balance(&to, token_id, new_balance);

    // Update total minted
    let total = get_total_minted(token_id);
    let new_total = match total.checked_add(amount) {
        Some(t) => t,
        None => return ERR_INVALID_PARAMS,
    };
    set_total_minted(token_id, new_total);

    debug_log!("Minted tokens");
    0
}

fn op_mint_batch(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    let sender = get_tx_sender();
    if !has_role(&MINTER_ROLE, &sender) {
        return ERR_MISSING_ROLE;
    }

    if params.len() < 32 + 4 {
        return ERR_INVALID_PARAMS;
    }

    let to = match parse_address(params, 0) {
        Ok(addr) => addr,
        Err(e) => return e,
    };
    if is_zero_address(&to) {
        return ERR_ZERO_ADDRESS;
    }

    let count = match parse_u32(params, 32) {
        Ok(c) => c as usize,
        Err(e) => return e,
    };

    if params.len() < 36 + count * 32 {
        return ERR_INVALID_PARAMS;
    }

    let mut offset = 36;
    for _ in 0..count {
        let token_id = match parse_u128(params, offset) {
            Ok(id) => id,
            Err(e) => return e,
        };
        let amount = match parse_u128(params, offset + 16) {
            Ok(amt) => amt,
            Err(e) => return e,
        };

        // Update balance
        let current_balance = get_balance(&to, token_id);
        let new_balance = match current_balance.checked_add(amount) {
            Some(b) => b,
            None => return ERR_INVALID_PARAMS,
        };
        set_balance(&to, token_id, new_balance);

        // Update total minted
        let total = get_total_minted(token_id);
        let new_total = match total.checked_add(amount) {
            Some(t) => t,
            None => return ERR_INVALID_PARAMS,
        };
        set_total_minted(token_id, new_total);

        offset += 32;
    }

    debug_log!("Batch minted tokens");
    0
}

fn op_burn(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    let sender = get_tx_sender();
    if !has_role(&BURNER_ROLE, &sender) {
        return ERR_MISSING_ROLE;
    }

    if params.len() < 32 + 16 + 16 {
        return ERR_INVALID_PARAMS;
    }

    let from = match parse_address(params, 0) {
        Ok(addr) => addr,
        Err(e) => return e,
    };
    if is_zero_address(&from) {
        return ERR_ZERO_ADDRESS;
    }

    let token_id = match parse_u128(params, 32) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let amount = match parse_u128(params, 48) {
        Ok(amt) => amt,
        Err(e) => return e,
    };

    // Check balance
    let current_balance = get_balance(&from, token_id);
    if current_balance < amount {
        return ERR_INSUFFICIENT_BALANCE;
    }

    // Update balance
    let new_balance = current_balance - amount;
    set_balance(&from, token_id, new_balance);

    // Update total burned
    let total = get_total_burned(token_id);
    let new_total = match total.checked_add(amount) {
        Some(t) => t,
        None => return ERR_INVALID_PARAMS,
    };
    set_total_burned(token_id, new_total);

    debug_log!("Burned tokens");
    0
}

fn op_burn_batch(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    let sender = get_tx_sender();
    if !has_role(&BURNER_ROLE, &sender) {
        return ERR_MISSING_ROLE;
    }

    if params.len() < 32 + 4 {
        return ERR_INVALID_PARAMS;
    }

    let from = match parse_address(params, 0) {
        Ok(addr) => addr,
        Err(e) => return e,
    };
    if is_zero_address(&from) {
        return ERR_ZERO_ADDRESS;
    }

    let count = match parse_u32(params, 32) {
        Ok(c) => c as usize,
        Err(e) => return e,
    };

    if params.len() < 36 + count * 32 {
        return ERR_INVALID_PARAMS;
    }

    let mut offset = 36;
    for _ in 0..count {
        let token_id = match parse_u128(params, offset) {
            Ok(id) => id,
            Err(e) => return e,
        };
        let amount = match parse_u128(params, offset + 16) {
            Ok(amt) => amt,
            Err(e) => return e,
        };

        // Check balance
        let current_balance = get_balance(&from, token_id);
        if current_balance < amount {
            return ERR_INSUFFICIENT_BALANCE;
        }

        // Update balance
        let new_balance = current_balance - amount;
        set_balance(&from, token_id, new_balance);

        // Update total burned
        let total = get_total_burned(token_id);
        let new_total = match total.checked_add(amount) {
            Some(t) => t,
            None => return ERR_INVALID_PARAMS,
        };
        set_total_burned(token_id, new_total);

        offset += 32;
    }

    debug_log!("Batch burned tokens");
    0
}

fn op_safe_transfer_from(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if is_entered() {
        return ERR_REENTRANT_CALL;
    }
    if !enter() {
        return ERR_REENTRANT_CALL;
    }

    let result = safe_transfer_from_internal(params);

    if !leave() {
        return ERR_REENTRANT_CALL;
    }

    result
}

fn safe_transfer_from_internal(params: &[u8]) -> u64 {
    if params.len() < 32 + 32 + 16 + 16 {
        return ERR_INVALID_PARAMS;
    }

    let from = match parse_address(params, 0) {
        Ok(addr) => addr,
        Err(e) => return e,
    };
    let to = match parse_address(params, 32) {
        Ok(addr) => addr,
        Err(e) => return e,
    };
    if is_zero_address(&to) {
        return ERR_ZERO_ADDRESS;
    }

    let token_id = match parse_u128(params, 64) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let amount = match parse_u128(params, 80) {
        Ok(amt) => amt,
        Err(e) => return e,
    };

    let sender = get_tx_sender();

    // Check authorization
    if sender != from && !get_operator_approval(&from, &sender) {
        return ERR_NOT_OWNER_OR_APPROVED;
    }

    // Check balance
    let from_balance = get_balance(&from, token_id);
    if from_balance < amount {
        return ERR_INSUFFICIENT_BALANCE;
    }

    // Update balances
    set_balance(&from, token_id, from_balance - amount);
    let to_balance = get_balance(&to, token_id);
    let new_to_balance = match to_balance.checked_add(amount) {
        Some(b) => b,
        None => return ERR_INVALID_PARAMS,
    };
    set_balance(&to, token_id, new_to_balance);

    debug_log!("Transferred tokens");
    0
}

fn op_safe_batch_transfer_from(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if is_entered() {
        return ERR_REENTRANT_CALL;
    }
    if !enter() {
        return ERR_REENTRANT_CALL;
    }

    let result = safe_batch_transfer_from_internal(params);

    if !leave() {
        return ERR_REENTRANT_CALL;
    }

    result
}

fn safe_batch_transfer_from_internal(params: &[u8]) -> u64 {
    if params.len() < 32 + 32 + 4 {
        return ERR_INVALID_PARAMS;
    }

    let from = match parse_address(params, 0) {
        Ok(addr) => addr,
        Err(e) => return e,
    };
    let to = match parse_address(params, 32) {
        Ok(addr) => addr,
        Err(e) => return e,
    };
    if is_zero_address(&to) {
        return ERR_ZERO_ADDRESS;
    }

    let count = match parse_u32(params, 64) {
        Ok(c) => c as usize,
        Err(e) => return e,
    };

    if params.len() < 68 + count * 32 {
        return ERR_INVALID_PARAMS;
    }

    let sender = get_tx_sender();

    // Check authorization
    if sender != from && !get_operator_approval(&from, &sender) {
        return ERR_NOT_OWNER_OR_APPROVED;
    }

    let mut offset = 68;
    for _ in 0..count {
        let token_id = match parse_u128(params, offset) {
            Ok(id) => id,
            Err(e) => return e,
        };
        let amount = match parse_u128(params, offset + 16) {
            Ok(amt) => amt,
            Err(e) => return e,
        };

        // Check balance
        let from_balance = get_balance(&from, token_id);
        if from_balance < amount {
            return ERR_INSUFFICIENT_BALANCE;
        }

        // Update balances
        set_balance(&from, token_id, from_balance - amount);
        let to_balance = get_balance(&to, token_id);
        let new_to_balance = match to_balance.checked_add(amount) {
            Some(b) => b,
            None => return ERR_INVALID_PARAMS,
        };
        set_balance(&to, token_id, new_to_balance);

        offset += 32;
    }

    debug_log!("Batch transferred tokens");
    0
}

fn op_set_approval_for_all(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 + 1 {
        return ERR_INVALID_PARAMS;
    }

    let operator = match parse_address(params, 0) {
        Ok(addr) => addr,
        Err(e) => return e,
    };
    let approved = params[32] != 0;

    let sender = get_tx_sender();
    set_operator_approval(&sender, &operator, approved);

    debug_log!("Approval for all set");
    0
}

fn op_set_uri(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    let sender = get_tx_sender();
    if !has_role(&URI_SETTER_ROLE, &sender) {
        return ERR_MISSING_ROLE;
    }

    if params.len() < 16 + 4 {
        return ERR_INVALID_PARAMS;
    }

    let token_id = match parse_u128(params, 0) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let uri_len = match parse_u32(params, 16) {
        Ok(len) => len as usize,
        Err(e) => return e,
    };

    if params.len() < 20 + uri_len {
        return ERR_INVALID_PARAMS;
    }

    set_token_uri(token_id, &params[20..20 + uri_len]);

    debug_log!("Token URI set");
    0
}

fn op_balance_of(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 + 16 {
        return ERR_INVALID_PARAMS;
    }

    let owner = match parse_address(params, 0) {
        Ok(addr) => addr,
        Err(e) => return e,
    };
    let token_id = match parse_u128(params, 32) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let balance = get_balance(&owner, token_id);
    let bytes = balance.to_le_bytes();
    let _ = storage_write(b"return_data", &bytes);

    0
}

fn op_balance_of_batch(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 4 {
        return ERR_INVALID_PARAMS;
    }

    let count = match parse_u32(params, 0) {
        Ok(c) => c as usize,
        Err(e) => return e,
    };

    if params.len() < 4 + count * 48 {
        return ERR_INVALID_PARAMS;
    }

    let mut result = Vec::new();
    let mut offset = 4;

    for _ in 0..count {
        let owner = match parse_address(params, offset) {
            Ok(addr) => addr,
            Err(e) => return e,
        };
        let token_id = match parse_u128(params, offset + 32) {
            Ok(id) => id,
            Err(e) => return e,
        };

        let balance = get_balance(&owner, token_id);
        result.extend_from_slice(&balance.to_le_bytes());

        offset += 48;
    }

    let _ = storage_write(b"return_data", &result);
    0
}

fn op_is_approved_for_all(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 + 32 {
        return ERR_INVALID_PARAMS;
    }

    let owner = match parse_address(params, 0) {
        Ok(addr) => addr,
        Err(e) => return e,
    };
    let operator = match parse_address(params, 32) {
        Ok(addr) => addr,
        Err(e) => return e,
    };

    let approved = get_operator_approval(&owner, &operator);
    let _ = storage_write(b"return_data", &[if approved { 1 } else { 0 }]);

    0
}

fn op_uri(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 16 {
        return ERR_INVALID_PARAMS;
    }

    let token_id = match parse_u128(params, 0) {
        Ok(id) => id,
        Err(e) => return e,
    };

    // Check for token-specific URI first
    if let Some(uri) = get_token_uri(token_id) {
        let _ = storage_write(b"return_data", &uri);
        return 0;
    }

    // Fall back to base URI
    if let Some(base_uri) = get_base_uri() {
        let _ = storage_write(b"return_data", &base_uri);
        return 0;
    }

    let _ = storage_write(b"return_data", &[]);
    0
}

fn op_total_minted(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 16 {
        return ERR_INVALID_PARAMS;
    }

    let token_id = match parse_u128(params, 0) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let total = get_total_minted(token_id);
    let _ = storage_write(b"return_data", &total.to_le_bytes());

    0
}

fn op_total_burned(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 16 {
        return ERR_INVALID_PARAMS;
    }

    let token_id = match parse_u128(params, 0) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let total = get_total_burned(token_id);
    let _ = storage_write(b"return_data", &total.to_le_bytes());

    0
}

// AccessControl Operations

fn op_grant_role(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 + 32 {
        return ERR_INVALID_PARAMS;
    }

    let role = match parse_address(params, 0) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let account = match parse_address(params, 32) {
        Ok(a) => a,
        Err(e) => return e,
    };

    let sender = get_tx_sender();
    let admin_role = get_role_admin(&role);

    if !has_role(&admin_role, &sender) {
        return ERR_MISSING_ROLE;
    }

    grant_role_internal(&role, &account);

    debug_log!("Role granted");
    0
}

fn op_revoke_role(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 + 32 {
        return ERR_INVALID_PARAMS;
    }

    let role = match parse_address(params, 0) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let account = match parse_address(params, 32) {
        Ok(a) => a,
        Err(e) => return e,
    };

    let sender = get_tx_sender();
    let admin_role = get_role_admin(&role);

    if !has_role(&admin_role, &sender) {
        return ERR_MISSING_ROLE;
    }

    revoke_role_internal(&role, &account);

    debug_log!("Role revoked");
    0
}

fn op_renounce_role(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        return ERR_INVALID_PARAMS;
    }

    let role = match parse_address(params, 0) {
        Ok(r) => r,
        Err(e) => return e,
    };

    let sender = get_tx_sender();
    revoke_role_internal(&role, &sender);

    debug_log!("Role renounced");
    0
}

fn op_has_role(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 + 32 {
        return ERR_INVALID_PARAMS;
    }

    let role = match parse_address(params, 0) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let account = match parse_address(params, 32) {
        Ok(a) => a,
        Err(e) => return e,
    };

    let has = has_role(&role, &account);
    let _ = storage_write(b"return_data", &[if has { 1 } else { 0 }]);

    0
}

fn op_get_role_admin(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 {
        return ERR_INVALID_PARAMS;
    }

    let role = match parse_address(params, 0) {
        Ok(r) => r,
        Err(e) => return e,
    };

    let admin = get_role_admin(&role);
    let _ = storage_write(b"return_data", &admin);

    0
}

fn op_set_role_admin(params: &[u8]) -> u64 {
    if !is_initialized() {
        return ERR_NOT_INITIALIZED;
    }

    if params.len() < 32 + 32 {
        return ERR_INVALID_PARAMS;
    }

    let role = match parse_address(params, 0) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let admin_role = match parse_address(params, 32) {
        Ok(a) => a,
        Err(e) => return e,
    };

    let sender = get_tx_sender();
    let current_admin = get_role_admin(&role);

    if !has_role(&current_admin, &sender) {
        return ERR_MISSING_ROLE;
    }

    set_role_admin_internal(&role, &admin_role);

    debug_log!("Role admin set");
    0
}

// Entry point

#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *const u8, input_len: usize) -> u64 {
    let instruction_data = core::slice::from_raw_parts(input, input_len);

    if instruction_data.is_empty() {
        return ERR_INVALID_INSTRUCTION;
    }

    let opcode = instruction_data[0];
    let params = if instruction_data.len() > 1 {
        &instruction_data[1..]
    } else {
        &[]
    };

    match opcode {
        OP_INITIALIZE => op_initialize(params),
        OP_MINT => op_mint(params),
        OP_MINT_BATCH => op_mint_batch(params),
        OP_BURN => op_burn(params),
        OP_BURN_BATCH => op_burn_batch(params),
        OP_SAFE_TRANSFER_FROM => op_safe_transfer_from(params),
        OP_SAFE_BATCH_TRANSFER_FROM => op_safe_batch_transfer_from(params),
        OP_SET_APPROVAL_FOR_ALL => op_set_approval_for_all(params),
        OP_SET_URI => op_set_uri(params),
        OP_BALANCE_OF => op_balance_of(params),
        OP_BALANCE_OF_BATCH => op_balance_of_batch(params),
        OP_IS_APPROVED_FOR_ALL => op_is_approved_for_all(params),
        OP_URI => op_uri(params),
        OP_TOTAL_MINTED => op_total_minted(params),
        OP_TOTAL_BURNED => op_total_burned(params),
        OP_GRANT_ROLE => op_grant_role(params),
        OP_REVOKE_ROLE => op_revoke_role(params),
        OP_RENOUNCE_ROLE => op_renounce_role(params),
        OP_HAS_ROLE => op_has_role(params),
        OP_GET_ROLE_ADMIN => op_get_role_admin(params),
        OP_SET_ROLE_ADMIN => op_set_role_admin(params),
        _ => ERR_INVALID_INSTRUCTION,
    }
}
