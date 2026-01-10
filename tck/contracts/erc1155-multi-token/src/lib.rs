//! ERC-1155 Multi-Token Standard Implementation
//!
//! This contract implements the ERC-1155 multi-token standard, which supports:
//! - Multiple token types in a single contract
//! - Fungible tokens (like ERC-20)
//! - Non-fungible tokens (like ERC-721)
//! - Safe transfer operations

#![no_std]
#![no_main]

use tako_sdk::*;

/// Address type (32 bytes)
pub type Address = [u8; 32];

/// Token ID type
pub type TokenId = u128;

// Storage key prefixes
const KEY_OWNER: &[u8] = b"owner";
const KEY_BALANCE_PREFIX: &[u8] = b"bal:"; // bal:{addr}{token_id}
const KEY_APPROVAL_PREFIX: &[u8] = b"appr:"; // appr:{owner}{operator}
const KEY_SUPPLY_PREFIX: &[u8] = b"sup:"; // sup:{token_id}

// Error codes
define_errors! {
    NotAuthorized = 1101,
    ZeroAddress = 1102,
    InsufficientBalance = 1103,
    SameAddress = 1104,
    OnlyOwner = 1105,
    StorageError = 1106,
    InvalidInput = 1107,
}

entrypoint!(process_instruction);

fn process_instruction(input: &[u8]) -> entrypoint::Result<()> {
    if input.is_empty() {
        return Ok(());
    }

    match input[0] {
        // Initialize: [0, owner[32]]
        0 => {
            if input.len() < 33 {
                return Err(InvalidInput);
            }
            let mut owner = [0u8; 32];
            owner.copy_from_slice(&input[1..33]);
            init(&owner)
        }
        // Balance of: [1, account[32], token_id[16]] -> returns balance via return data
        1 => {
            if input.len() < 49 {
                return Err(InvalidInput);
            }
            let mut account = [0u8; 32];
            account.copy_from_slice(&input[1..33]);
            let token_id = u128::from_le_bytes(input[33..49].try_into().unwrap());
            let balance = balance_of(&account, token_id);
            // Return balance via set_return_data
            set_return_data(&balance.to_le_bytes());
            Ok(())
        }
        // Set approval for all: [2, owner[32], operator[32], approved[1]]
        2 => {
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
        // Safe transfer: [3, caller[32], from[32], to[32], token_id[16], amount[16]]
        3 => {
            if input.len() < 129 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            let mut from = [0u8; 32];
            let mut to = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            from.copy_from_slice(&input[33..65]);
            to.copy_from_slice(&input[65..97]);
            let token_id = u128::from_le_bytes(input[97..113].try_into().unwrap());
            let amount = u128::from_le_bytes(input[113..129].try_into().unwrap());
            safe_transfer_from(&caller, &from, &to, token_id, amount)
        }
        // Mint: [4, caller[32], to[32], token_id[16], amount[16]]
        4 => {
            if input.len() < 97 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            let mut to = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            to.copy_from_slice(&input[33..65]);
            let token_id = u128::from_le_bytes(input[65..81].try_into().unwrap());
            let amount = u128::from_le_bytes(input[81..97].try_into().unwrap());
            mint(&caller, &to, token_id, amount)
        }
        // Burn: [5, caller[32], from[32], token_id[16], amount[16]]
        5 => {
            if input.len() < 97 {
                return Err(InvalidInput);
            }
            let mut caller = [0u8; 32];
            let mut from = [0u8; 32];
            caller.copy_from_slice(&input[1..33]);
            from.copy_from_slice(&input[33..65]);
            let token_id = u128::from_le_bytes(input[65..81].try_into().unwrap());
            let amount = u128::from_le_bytes(input[81..97].try_into().unwrap());
            burn(&caller, &from, token_id, amount)
        }
        _ => Ok(()),
    }
}

/// Initialize contract with owner
fn init(owner: &Address) -> entrypoint::Result<()> {
    storage_write(KEY_OWNER, owner).map_err(|_| StorageError)?;
    log("ERC1155 initialized");
    Ok(())
}

/// Get balance of account for a specific token
fn balance_of(account: &Address, token_id: TokenId) -> u128 {
    let key = make_balance_key(account, token_id);
    read_u128(&key)
}

/// Set or unset approval for operator
fn set_approval_for_all(
    owner: &Address,
    operator: &Address,
    approved: bool,
) -> entrypoint::Result<()> {
    if owner == operator {
        return Err(SameAddress);
    }

    let key = make_approval_key(owner, operator);
    let value: u8 = if approved { 1 } else { 0 };
    storage_write(&key, &[value]).map_err(|_| StorageError)?;

    log("Approval updated");
    Ok(())
}

/// Check if operator is approved for all tokens of owner
fn is_approved_for_all(owner: &Address, operator: &Address) -> bool {
    let key = make_approval_key(owner, operator);
    let mut buffer = [0u8; 1];
    let len = storage_read(&key, &mut buffer);
    len == 1 && buffer[0] != 0
}

/// Transfer tokens from one account to another
fn safe_transfer_from(
    caller: &Address,
    from: &Address,
    to: &Address,
    token_id: TokenId,
    amount: u128,
) -> entrypoint::Result<()> {
    // Check authorization
    if caller != from && !is_approved_for_all(from, caller) {
        return Err(NotAuthorized);
    }

    // Check zero address
    if *to == [0u8; 32] {
        return Err(ZeroAddress);
    }

    // Check balance
    let from_balance = balance_of(from, token_id);
    if from_balance < amount {
        return Err(InsufficientBalance);
    }

    // Update balances
    let from_key = make_balance_key(from, token_id);
    storage_write(&from_key, &(from_balance - amount).to_le_bytes()).map_err(|_| StorageError)?;

    let to_balance = balance_of(to, token_id);
    let to_key = make_balance_key(to, token_id);
    storage_write(&to_key, &(to_balance + amount).to_le_bytes()).map_err(|_| StorageError)?;

    log("Transfer completed");
    Ok(())
}

/// Mint new tokens (only owner)
fn mint(caller: &Address, to: &Address, token_id: TokenId, amount: u128) -> entrypoint::Result<()> {
    // Only owner can mint
    let owner = read_owner()?;
    if *caller != owner {
        return Err(OnlyOwner);
    }

    if *to == [0u8; 32] {
        return Err(ZeroAddress);
    }

    // Update balance
    let balance = balance_of(to, token_id);
    let balance_key = make_balance_key(to, token_id);
    storage_write(&balance_key, &(balance + amount).to_le_bytes()).map_err(|_| StorageError)?;

    // Update total supply
    let supply = read_supply(token_id);
    let supply_key = make_supply_key(token_id);
    storage_write(&supply_key, &(supply + amount).to_le_bytes()).map_err(|_| StorageError)?;

    log("Tokens minted");
    Ok(())
}

/// Burn tokens
fn burn(
    caller: &Address,
    from: &Address,
    token_id: TokenId,
    amount: u128,
) -> entrypoint::Result<()> {
    // Check authorization
    if caller != from && !is_approved_for_all(from, caller) {
        return Err(NotAuthorized);
    }

    // Check balance
    let balance = balance_of(from, token_id);
    if balance < amount {
        return Err(InsufficientBalance);
    }

    // Update balance
    let balance_key = make_balance_key(from, token_id);
    storage_write(&balance_key, &(balance - amount).to_le_bytes()).map_err(|_| StorageError)?;

    // Update total supply
    let supply = read_supply(token_id);
    let supply_key = make_supply_key(token_id);
    storage_write(&supply_key, &supply.saturating_sub(amount).to_le_bytes())
        .map_err(|_| StorageError)?;

    log("Tokens burned");
    Ok(())
}

// Helper functions

fn read_owner() -> entrypoint::Result<Address> {
    let mut buffer = [0u8; 32];
    let len = storage_read(KEY_OWNER, &mut buffer);
    if len != 32 {
        return Err(StorageError);
    }
    Ok(buffer)
}

fn read_u128(key: &[u8]) -> u128 {
    let mut buffer = [0u8; 16];
    let len = storage_read(key, &mut buffer);
    if len == 16 {
        u128::from_le_bytes(buffer)
    } else {
        0
    }
}

fn read_supply(token_id: TokenId) -> u128 {
    let key = make_supply_key(token_id);
    read_u128(&key)
}

fn make_balance_key(account: &Address, token_id: TokenId) -> [u8; 52] {
    // bal: (4) + address (32) + token_id (16) = 52
    let mut key = [0u8; 52];
    key[0..4].copy_from_slice(KEY_BALANCE_PREFIX);
    key[4..36].copy_from_slice(account);
    key[36..52].copy_from_slice(&token_id.to_le_bytes());
    key
}

fn make_approval_key(owner: &Address, operator: &Address) -> [u8; 69] {
    // appr: (5) + owner (32) + operator (32) = 69
    let mut key = [0u8; 69];
    key[0..5].copy_from_slice(KEY_APPROVAL_PREFIX);
    key[5..37].copy_from_slice(owner);
    key[37..69].copy_from_slice(operator);
    key
}

fn make_supply_key(token_id: TokenId) -> [u8; 20] {
    // sup: (4) + token_id (16) = 20
    let mut key = [0u8; 20];
    key[0..4].copy_from_slice(KEY_SUPPLY_PREFIX);
    key[4..20].copy_from_slice(&token_id.to_le_bytes());
    key
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
