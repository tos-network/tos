//! ERC-721 Non-Fungible Token (NFT) Implementation
//!
//! This contract implements the ERC-721 standard for NFTs on TOS Kernel(TAKO).

#![no_std]
#![no_main]

use tako_sdk::*;

type Address32 = [u8; 32];
type TokenId = u128;

// Storage key prefixes
const KEY_TOTAL_SUPPLY: &[u8] = b"supply";
const KEY_OWNER_PREFIX: &[u8] = b"own:"; // own:{token_id}
const KEY_BALANCE_PREFIX: &[u8] = b"bal:"; // bal:{address}
const KEY_APPROVAL_PREFIX: &[u8] = b"apr:"; // apr:{token_id}
const KEY_OPERATOR_PREFIX: &[u8] = b"opr:"; // opr:{owner}{operator}

// Error codes
define_errors! {
    ZeroAddress = 1201,
    TokenNotExists = 1202,
    TokenAlreadyMinted = 1203,
    TransferFromIncorrectOwner = 1204,
    SameAddress = 1205,
    StorageError = 1206,
    InvalidInput = 1207,
    InsufficientBalance = 1208,
}

entrypoint!(process_instruction);

fn process_instruction(input: &[u8]) -> entrypoint::Result<()> {
    if input.is_empty() {
        return Ok(());
    }

    match input[0] {
        // Balance of: [1, owner[32]] -> returns count
        1 => {
            if input.len() < 33 {
                return Err(InvalidInput);
            }
            let mut owner = [0u8; 32];
            owner.copy_from_slice(&input[1..33]);
            let balance = balance_of(&owner);
            set_return_data(&balance.to_le_bytes());
            Ok(())
        }
        // Owner of: [2, token_id[16]] -> returns owner address
        2 => {
            if input.len() < 17 {
                return Err(InvalidInput);
            }
            let token_id = u128::from_le_bytes(input[1..17].try_into().unwrap());
            let owner = owner_of(token_id)?;
            set_return_data(&owner);
            Ok(())
        }
        // Approve: [3, to[32], token_id[16]]
        3 => {
            if input.len() < 49 {
                return Err(InvalidInput);
            }
            let mut to = [0u8; 32];
            to.copy_from_slice(&input[1..33]);
            let token_id = u128::from_le_bytes(input[33..49].try_into().unwrap());
            approve(&to, token_id)
        }
        // Set approval for all: [4, owner[32], operator[32], approved[1]]
        4 => {
            if input.len() < 66 {
                return Err(InvalidInput);
            }
            let mut owner = [0u8; 32];
            let mut operator = [0u8; 32];
            owner.copy_from_slice(&input[1..33]);
            operator.copy_from_slice(&input[33..65]);
            let approved = input[65] != 0;
            set_approval_for_all(&owner, &operator, approved)
        }
        // Transfer from: [5, from[32], to[32], token_id[16]]
        5 => {
            if input.len() < 81 {
                return Err(InvalidInput);
            }
            let mut from = [0u8; 32];
            let mut to = [0u8; 32];
            from.copy_from_slice(&input[1..33]);
            to.copy_from_slice(&input[33..65]);
            let token_id = u128::from_le_bytes(input[65..81].try_into().unwrap());
            transfer_from(&from, &to, token_id)
        }
        // Mint: [6, to[32], token_id[16]]
        6 => {
            if input.len() < 49 {
                return Err(InvalidInput);
            }
            let mut to = [0u8; 32];
            to.copy_from_slice(&input[1..33]);
            let token_id = u128::from_le_bytes(input[33..49].try_into().unwrap());
            mint(&to, token_id)
        }
        // Burn: [7, token_id[16]]
        7 => {
            if input.len() < 17 {
                return Err(InvalidInput);
            }
            let token_id = u128::from_le_bytes(input[1..17].try_into().unwrap());
            burn(token_id)
        }
        _ => Ok(()),
    }
}

/// Get balance of owner
fn balance_of(owner: &Address32) -> u128 {
    let key = make_balance_key(owner);
    read_u128(&key)
}

/// Get owner of token
fn owner_of(token_id: TokenId) -> entrypoint::Result<Address32> {
    let key = make_owner_key(token_id);
    let mut buffer = [0u8; 32];
    let len = storage_read(&key, &mut buffer);
    if len != 32 {
        return Err(TokenNotExists);
    }
    Ok(buffer)
}

/// Check if token exists
fn exists(token_id: TokenId) -> bool {
    let key = make_owner_key(token_id);
    let mut buffer = [0u8; 32];
    let len = storage_read(&key, &mut buffer);
    len == 32
}

/// Approve address to transfer token
fn approve(to: &Address32, token_id: TokenId) -> entrypoint::Result<()> {
    let owner = owner_of(token_id)?;
    if *to == owner {
        return Err(SameAddress);
    }

    let key = make_approval_key(token_id);
    storage_write(&key, to).map_err(|_| StorageError)?;

    log("Token approved");
    Ok(())
}

/// Set or unset operator approval
fn set_approval_for_all(
    owner: &Address32,
    operator: &Address32,
    approved: bool,
) -> entrypoint::Result<()> {
    if owner == operator {
        return Err(SameAddress);
    }

    let key = make_operator_key(owner, operator);
    let value: u8 = if approved { 1 } else { 0 };
    storage_write(&key, &[value]).map_err(|_| StorageError)?;

    log("Operator approval updated");
    Ok(())
}

/// Check if operator is approved for all
fn is_approved_for_all(owner: &Address32, operator: &Address32) -> bool {
    let key = make_operator_key(owner, operator);
    let mut buffer = [0u8; 1];
    let len = storage_read(&key, &mut buffer);
    len == 1 && buffer[0] != 0
}

/// Transfer token from one address to another
fn transfer_from(from: &Address32, to: &Address32, token_id: TokenId) -> entrypoint::Result<()> {
    if *to == [0u8; 32] {
        return Err(ZeroAddress);
    }

    let owner = owner_of(token_id)?;
    if *from != owner {
        return Err(TransferFromIncorrectOwner);
    }

    // Clear approvals
    let approval_key = make_approval_key(token_id);
    storage_delete(&approval_key);

    // Update balances
    let from_balance = balance_of(from);
    if from_balance == 0 {
        return Err(InsufficientBalance);
    }
    let from_key = make_balance_key(from);
    storage_write(&from_key, &(from_balance - 1).to_le_bytes()).map_err(|_| StorageError)?;

    let to_balance = balance_of(to);
    let to_key = make_balance_key(to);
    storage_write(&to_key, &(to_balance + 1).to_le_bytes()).map_err(|_| StorageError)?;

    // Transfer ownership
    let owner_key = make_owner_key(token_id);
    storage_write(&owner_key, to).map_err(|_| StorageError)?;

    log("Token transferred");
    Ok(())
}

/// Mint new token
fn mint(to: &Address32, token_id: TokenId) -> entrypoint::Result<()> {
    if *to == [0u8; 32] {
        return Err(ZeroAddress);
    }

    if exists(token_id) {
        return Err(TokenAlreadyMinted);
    }

    // Update balance
    let balance = balance_of(to);
    let balance_key = make_balance_key(to);
    storage_write(&balance_key, &(balance + 1).to_le_bytes()).map_err(|_| StorageError)?;

    // Set owner
    let owner_key = make_owner_key(token_id);
    storage_write(&owner_key, to).map_err(|_| StorageError)?;

    // Update total supply
    let supply = read_u128(KEY_TOTAL_SUPPLY);
    storage_write(KEY_TOTAL_SUPPLY, &(supply + 1).to_le_bytes()).map_err(|_| StorageError)?;

    log("Token minted");
    Ok(())
}

/// Burn token
fn burn(token_id: TokenId) -> entrypoint::Result<()> {
    let owner = owner_of(token_id)?;

    // Clear approvals
    let approval_key = make_approval_key(token_id);
    storage_delete(&approval_key);

    // Update balance
    let balance = balance_of(&owner);
    if balance == 0 {
        return Err(InsufficientBalance);
    }
    let balance_key = make_balance_key(&owner);
    storage_write(&balance_key, &(balance - 1).to_le_bytes()).map_err(|_| StorageError)?;

    // Remove owner
    let owner_key = make_owner_key(token_id);
    storage_delete(&owner_key);

    // Update total supply
    let supply = read_u128(KEY_TOTAL_SUPPLY);
    storage_write(KEY_TOTAL_SUPPLY, &supply.saturating_sub(1).to_le_bytes())
        .map_err(|_| StorageError)?;

    log("Token burned");
    Ok(())
}

// Helper functions

fn read_u128(key: &[u8]) -> u128 {
    let mut buffer = [0u8; 16];
    let len = storage_read(key, &mut buffer);
    if len == 16 {
        u128::from_le_bytes(buffer)
    } else {
        0
    }
}

fn make_owner_key(token_id: TokenId) -> [u8; 20] {
    // own: (4) + token_id (16) = 20
    let mut key = [0u8; 20];
    key[0..4].copy_from_slice(KEY_OWNER_PREFIX);
    key[4..20].copy_from_slice(&token_id.to_le_bytes());
    key
}

fn make_balance_key(address: &Address32) -> [u8; 36] {
    // bal: (4) + address (32) = 36
    let mut key = [0u8; 36];
    key[0..4].copy_from_slice(KEY_BALANCE_PREFIX);
    key[4..36].copy_from_slice(address);
    key
}

fn make_approval_key(token_id: TokenId) -> [u8; 20] {
    // apr: (4) + token_id (16) = 20
    let mut key = [0u8; 20];
    key[0..4].copy_from_slice(KEY_APPROVAL_PREFIX);
    key[4..20].copy_from_slice(&token_id.to_le_bytes());
    key
}

fn make_operator_key(owner: &Address32, operator: &Address32) -> [u8; 68] {
    // opr: (4) + owner (32) + operator (32) = 68
    let mut key = [0u8; 68];
    key[0..4].copy_from_slice(KEY_OPERATOR_PREFIX);
    key[4..36].copy_from_slice(owner);
    key[36..68].copy_from_slice(operator);
    key
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
