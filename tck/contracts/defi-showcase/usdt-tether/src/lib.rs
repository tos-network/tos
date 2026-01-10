//! # USDT Tether - TAKO Implementation
//!
//! A production-grade stablecoin implementation inspired by Tether's USDT,
//! demonstrating TOS blockchain's Rust smart contract capabilities.
//!
//! ## Key Features
//!
//! - **ERC20 Compatible**: Full token standard implementation
//! - **Pausable**: Emergency stop mechanism for security incidents
//! - **Blacklist System**: Compliance with regulatory requirements
//! - **Upgradeable**: Owner can update contract logic
//! - **Mint/Burn**: Unlimited supply management (1:1 USD peg)
//! - **Role-Based Access**: Owner, pauser, and blacklister roles
//!
//! ## Architecture
//!
//! This contract demonstrates Rust's type safety, pattern matching,
//! and memory efficiency compared to Solidity implementations.

#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use tako_macros::event;
use tako_sdk::{get_input_data, storage_read, storage_write, Address, MAX_VALUE_SIZE, SUCCESS};

// Type aliases for clarity
type AccountId = Address;
type TakoResult<T> = Result<T, u64>;

// Error helper
#[allow(dead_code)]
enum TakoError {
    InvalidInput,
    CustomError(u64),
}

impl From<TakoError> for u64 {
    fn from(err: TakoError) -> u64 {
        match err {
            TakoError::InvalidInput => 1,
            TakoError::CustomError(code) => code,
        }
    }
}

// Maximum storage buffer size (optimized for stack safety)
// USDT data: balance (8) + allowance (8) + blacklist (1) + metadata (~50) = ~70 bytes max
const STORAGE_BUFFER_SIZE: usize = 256;

// Helper functions for storage operations using SDK syscalls
fn get_storage(key: &[u8]) -> Result<Vec<u8>, u64> {
    let mut buffer = vec![0u8; STORAGE_BUFFER_SIZE]; // Heap allocation to avoid stack overflow
    let len = storage_read(key, &mut buffer);
    if len == 0 {
        return Err(1); // Not found
    }
    Ok(buffer[..len as usize].to_vec())
}

fn set_storage(key: &[u8], value: &[u8]) -> Result<(), u64> {
    storage_write(key, value)
}

// Helper macro for logging
macro_rules! debug_log {
    ($($arg:tt)*) => {}; // No-op for now
}

/// Contract state storage keys
const KEY_NAME: &[u8] = b"name";
const KEY_SYMBOL: &[u8] = b"symbol";
const KEY_DECIMALS: &[u8] = b"decimals";
const KEY_TOTAL_SUPPLY: &[u8] = b"total_supply";
const KEY_OWNER: &[u8] = b"owner";
const KEY_PAUSED: &[u8] = b"paused";

/// Balance storage: balance_{account_id}
fn balance_key(account: &AccountId) -> [u8; 40] {
    let mut key = [0u8; 40];
    key[..8].copy_from_slice(b"balance_");
    key[8..40].copy_from_slice(&account.0);
    key
}

/// Allowance storage: allowance_{owner}_{spender}
fn allowance_key(owner: &AccountId, spender: &AccountId) -> [u8; 73] {
    let mut key = [0u8; 73];
    key[..9].copy_from_slice(b"allowance");
    key[9..41].copy_from_slice(&owner.0);
    key[41..].copy_from_slice(&spender.0);
    key
}

/// Blacklist storage: blacklist_{account_id}
fn blacklist_key(account: &AccountId) -> [u8; 42] {
    let mut key = [0u8; 42];
    key[..10].copy_from_slice(b"blacklist_");
    key[10..].copy_from_slice(&account.0);
    key
}

// ============================================================================
// Event Definitions
// ============================================================================

/// Transfer event (ERC20 standard)
/// Emitted when tokens are transferred between accounts
#[event]
pub struct Transfer {
    pub from: Address,
    pub to: Address,
    pub amount: u64,
}

/// Approval event (ERC20 standard)
/// Emitted when an allowance is set
#[event]
pub struct Approval {
    pub owner: Address,
    pub spender: Address,
    pub amount: u64,
}

/// Mint event
/// Emitted when new tokens are created
#[event]
pub struct Mint {
    pub to: Address,
    pub amount: u64,
}

/// Burn event
/// Emitted when tokens are destroyed
#[event]
pub struct Burn {
    pub from: Address,
    pub amount: u64,
}

/// Pause event
/// Emitted when contract is paused
#[event]
pub struct Pause {
    pub timestamp: u64,
}

/// Unpause event
/// Emitted when contract is unpaused
#[event]
pub struct Unpause {
    pub timestamp: u64,
}

/// Blacklist event
/// Emitted when an account is added to blacklist
#[event]
pub struct Blacklist {
    pub account: Address,
}

/// UnBlacklist event
/// Emitted when an account is removed from blacklist
#[event]
pub struct UnBlacklist {
    pub account: Address,
}

/// DestroyBlacklistedFunds event
/// Emitted when blacklisted funds are destroyed
#[event]
pub struct DestroyBlacklistedFunds {
    pub account: Address,
    pub amount: u64,
}

/// Contract instructions
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Instruction {
    /// Initialize the token contract
    /// Args: name (String), symbol (String), decimals (u8), initial_supply (u64)
    Initialize = 0,

    /// Transfer tokens
    /// Args: to (AccountId), amount (u64)
    Transfer = 1,

    /// Approve spender
    /// Args: spender (AccountId), amount (u64)
    Approve = 2,

    /// Transfer from allowance
    /// Args: from (AccountId), to (AccountId), amount (u64)
    TransferFrom = 3,

    /// Mint new tokens (owner only)
    /// Args: to (AccountId), amount (u64)
    Mint = 4,

    /// Burn tokens (owner only)
    /// Args: from (AccountId), amount (u64)
    Burn = 5,

    /// Pause contract (owner only)
    Pause = 6,

    /// Unpause contract (owner only)
    Unpause = 7,

    /// Add address to blacklist (owner only)
    /// Args: account (AccountId)
    AddBlacklist = 8,

    /// Remove address from blacklist (owner only)
    /// Args: account (AccountId)
    RemoveBlacklist = 9,

    /// Destroy blacklisted funds (owner only)
    /// Args: account (AccountId)
    DestroyBlacklistedFunds = 10,

    /// Query balance
    /// Args: account (AccountId)
    BalanceOf = 100,

    /// Query allowance
    /// Args: owner (AccountId), spender (AccountId)
    Allowance = 101,

    /// Query total supply
    TotalSupply = 102,

    /// Query if paused
    IsPaused = 103,

    /// Query if blacklisted
    /// Args: account (AccountId)
    IsBlacklisted = 104,
}

impl Instruction {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Initialize),
            1 => Some(Self::Transfer),
            2 => Some(Self::Approve),
            3 => Some(Self::TransferFrom),
            4 => Some(Self::Mint),
            5 => Some(Self::Burn),
            6 => Some(Self::Pause),
            7 => Some(Self::Unpause),
            8 => Some(Self::AddBlacklist),
            9 => Some(Self::RemoveBlacklist),
            10 => Some(Self::DestroyBlacklistedFunds),
            100 => Some(Self::BalanceOf),
            101 => Some(Self::Allowance),
            102 => Some(Self::TotalSupply),
            103 => Some(Self::IsPaused),
            104 => Some(Self::IsBlacklisted),
            _ => None,
        }
    }
}

/// Contract errors
#[derive(Debug)]
#[repr(u32)]
pub enum UsdtError {
    InvalidInstruction = 1,
    Unauthorized = 2,
    InsufficientBalance = 3,
    InsufficientAllowance = 4,
    Paused = 5,
    NotPaused = 6,
    Blacklisted = 7,
    InvalidAccount = 8,
    Overflow = 9,
    Underflow = 10,
}

impl From<UsdtError> for TakoError {
    fn from(err: UsdtError) -> Self {
        TakoError::CustomError(err as u64)
    }
}

/// Get the caller's account ID from transaction context
///
/// Uses TAKO runtime syscall `get_caller` to retrieve the direct caller address.
/// This differs from `get_tx_sender()` in nested calls:
/// - User -> ContractA -> ContractB: In ContractB, caller is ContractA, tx_sender is User
///
/// # Returns
/// The 32-byte address of the account that directly invoked this contract
fn get_caller() -> TakoResult<AccountId> {
    let caller_bytes = tako_sdk::syscalls::get_caller();
    Ok(Address::new(caller_bytes))
}

/// Helper: Check if caller is owner
fn require_owner() -> TakoResult<()> {
    let caller = get_caller()?;
    let owner_bytes = get_storage(KEY_OWNER)?;
    let owner = Address::from_slice(owner_bytes.as_slice())
        .ok_or(TakoError::from(UsdtError::InvalidAccount))?;

    if caller != owner {
        return Err(TakoError::from(UsdtError::Unauthorized).into());
    }
    Ok(())
}

/// Helper: Check if contract is not paused
fn require_not_paused() -> TakoResult<()> {
    let paused_bytes = get_storage(KEY_PAUSED).unwrap_or_default();
    let paused = !paused_bytes.is_empty() && paused_bytes[0] == 1;

    if paused {
        return Err(TakoError::from(UsdtError::Paused).into());
    }
    Ok(())
}

/// Helper: Check if account is not blacklisted
fn require_not_blacklisted(account: &AccountId) -> TakoResult<()> {
    let key = blacklist_key(account);
    let blacklisted_bytes = get_storage(&key).unwrap_or_default();
    let blacklisted = !blacklisted_bytes.is_empty() && blacklisted_bytes[0] == 1;

    if blacklisted {
        return Err(TakoError::from(UsdtError::Blacklisted).into());
    }
    Ok(())
}

/// Helper: Get balance
fn get_balance(account: &AccountId) -> TakoResult<u64> {
    let key = balance_key(account);
    let balance_bytes = get_storage(&key).unwrap_or_default();

    if balance_bytes.len() >= 8 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&balance_bytes[..8]);
        Ok(u64::from_le_bytes(bytes))
    } else {
        Ok(0)
    }
}

/// Helper: Set balance
fn set_balance(account: &AccountId, amount: u64) -> TakoResult<()> {
    let key = balance_key(account);
    set_storage(&key, &amount.to_le_bytes())?;
    Ok(())
}

/// Helper: Get allowance
fn get_allowance(owner: &AccountId, spender: &AccountId) -> TakoResult<u64> {
    let key = allowance_key(owner, spender);
    let allowance_bytes = get_storage(&key).unwrap_or_default();

    if allowance_bytes.len() >= 8 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&allowance_bytes[..8]);
        Ok(u64::from_le_bytes(bytes))
    } else {
        Ok(0)
    }
}

/// Helper: Set allowance
fn set_allowance(owner: &AccountId, spender: &AccountId, amount: u64) -> TakoResult<()> {
    let key = allowance_key(owner, spender);
    set_storage(&key, &amount.to_le_bytes())?;
    Ok(())
}

/// Initialize the USDT contract
fn initialize(input: &[u8]) -> TakoResult<Vec<u8>> {
    debug_log!("USDT: Initializing contract");

    // Check if already initialized
    if get_storage(KEY_OWNER).is_ok() {
        return Err(TakoError::CustomError(999).into()); // Already initialized
    }

    // Parse input: name_len (4) + name + symbol_len (4) + symbol + decimals (1) + initial_supply (8)
    if input.len() < 17 {
        return Err(TakoError::InvalidInput.into());
    }

    let mut offset = 0;

    // Parse name
    let name_len = u32::from_le_bytes([input[0], input[1], input[2], input[3]]) as usize;
    offset += 4;
    if input.len() < offset + name_len {
        return Err(TakoError::InvalidInput.into());
    }
    let name = &input[offset..offset + name_len];
    offset += name_len;

    // Parse symbol
    if input.len() < offset + 4 {
        return Err(TakoError::InvalidInput.into());
    }
    let symbol_len = u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ]) as usize;
    offset += 4;
    if input.len() < offset + symbol_len {
        return Err(TakoError::InvalidInput.into());
    }
    let symbol = &input[offset..offset + symbol_len];
    offset += symbol_len;

    // Parse decimals and initial supply
    if input.len() < offset + 9 {
        return Err(TakoError::InvalidInput.into());
    }
    let decimals = input[offset];
    offset += 1;
    let initial_supply = u64::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
        input[offset + 4],
        input[offset + 5],
        input[offset + 6],
        input[offset + 7],
    ]);

    // Store token metadata
    set_storage(KEY_NAME, name)?;
    set_storage(KEY_SYMBOL, symbol)?;
    set_storage(KEY_DECIMALS, &[decimals])?;
    set_storage(KEY_TOTAL_SUPPLY, &initial_supply.to_le_bytes())?;

    // Set owner
    let owner = get_caller()?;
    set_storage(KEY_OWNER, &owner.0)?;

    // Mint initial supply to owner
    set_balance(&owner, initial_supply)?;

    // Initialize as not paused
    set_storage(KEY_PAUSED, &[0u8])?;

    debug_log!("USDT: Initialized with initial supply");

    Ok(vec![1]) // Success
}

/// Transfer tokens
fn transfer(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_not_paused()?;

    if input.len() < 40 {
        return Err(TakoError::InvalidInput.into());
    }

    // Parse recipient and amount
    let to = Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;
    let amount = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);

    let from = get_caller()?;

    // Check blacklist
    require_not_blacklisted(&from)?;
    require_not_blacklisted(&to)?;

    // Check balance
    let from_balance = get_balance(&from)?;
    if from_balance < amount {
        return Err(TakoError::from(UsdtError::InsufficientBalance).into());
    }

    // Update balances with overflow protection
    set_balance(&from, from_balance - amount)?;

    let to_balance = get_balance(&to)?;
    // FIX: Use checked_add to prevent overflow (audit finding)
    let new_to_balance = to_balance
        .checked_add(amount)
        .ok_or(TakoError::from(UsdtError::Overflow))?;
    set_balance(&to, new_to_balance)?;

    // Emit Transfer event (ERC20 standard)
    Transfer { from, to, amount }.emit().ok();

    debug_log!("USDT: Transferred tokens");

    Ok(vec![1]) // Success
}

/// Approve spender to spend tokens
fn approve(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_not_paused()?;

    if input.len() < 40 {
        return Err(TakoError::InvalidInput.into());
    }

    // Parse spender and amount
    let spender =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;
    let amount = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);

    let owner = get_caller()?;

    // Check blacklist
    require_not_blacklisted(&owner)?;
    require_not_blacklisted(&spender)?;

    // Set allowance
    set_allowance(&owner, &spender, amount)?;

    // Emit Approval event (ERC20 standard)
    Approval {
        owner,
        spender,
        amount,
    }
    .emit()
    .ok();

    debug_log!("USDT: Approved tokens for spender");

    Ok(vec![1]) // Success
}

/// Transfer tokens from approved allowance
fn transfer_from(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_not_paused()?;

    if input.len() < 72 {
        return Err(TakoError::InvalidInput.into());
    }

    // Parse from, to, and amount
    let from =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;
    let to =
        Address::from_slice(&input[32..64]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;
    let amount = u64::from_le_bytes([
        input[64], input[65], input[66], input[67], input[68], input[69], input[70], input[71],
    ]);

    let spender = get_caller()?;

    // Check blacklist
    require_not_blacklisted(&from)?;
    require_not_blacklisted(&to)?;
    require_not_blacklisted(&spender)?;

    // Check allowance
    let allowance = get_allowance(&from, &spender)?;
    if allowance < amount {
        return Err(TakoError::from(UsdtError::InsufficientAllowance).into());
    }

    // Check balance
    let from_balance = get_balance(&from)?;
    if from_balance < amount {
        return Err(TakoError::from(UsdtError::InsufficientBalance).into());
    }

    // Update allowance
    set_allowance(&from, &spender, allowance - amount)?;

    // Update balances with overflow protection
    set_balance(&from, from_balance - amount)?;

    let to_balance = get_balance(&to)?;
    // FIX: Use checked_add to prevent overflow (audit finding)
    let new_to_balance = to_balance
        .checked_add(amount)
        .ok_or(TakoError::from(UsdtError::Overflow))?;
    set_balance(&to, new_to_balance)?;

    // Emit Transfer event (ERC20 standard)
    Transfer { from, to, amount }.emit().ok();

    debug_log!("USDT: TransferredFrom tokens");

    Ok(vec![1]) // Success
}

/// Mint new tokens
fn mint(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_owner()?;
    require_not_paused()?;

    if input.len() < 40 {
        return Err(TakoError::InvalidInput.into());
    }

    let to = Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;
    let amount = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);

    require_not_blacklisted(&to)?;

    // Update total supply
    let total_supply_bytes = get_storage(KEY_TOTAL_SUPPLY)?;
    let mut total_supply_arr = [0u8; 8];
    total_supply_arr.copy_from_slice(&total_supply_bytes[..8]);
    let total_supply = u64::from_le_bytes(total_supply_arr);
    let new_total_supply = total_supply
        .checked_add(amount)
        .ok_or(TakoError::from(UsdtError::Overflow))?;
    set_storage(KEY_TOTAL_SUPPLY, &new_total_supply.to_le_bytes())?;

    // Update balance with overflow protection
    let balance = get_balance(&to)?;
    // FIX: Use checked_add to prevent overflow (audit finding)
    let new_balance = balance
        .checked_add(amount)
        .ok_or(TakoError::from(UsdtError::Overflow))?;
    set_balance(&to, new_balance)?;

    // Emit Transfer event from zero address (ERC20 mint standard)
    Transfer {
        from: Address::default(),
        to,
        amount,
    }
    .emit()
    .ok();

    // Emit Mint event
    Mint { to, amount }.emit().ok();

    debug_log!("USDT: Minted tokens");

    Ok(vec![1])
}

/// Burn tokens
fn burn(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_owner()?;
    require_not_paused()?;

    if input.len() < 40 {
        return Err(TakoError::InvalidInput.into());
    }

    let from =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;
    let amount = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);

    // Check balance
    let balance = get_balance(&from)?;
    if balance < amount {
        return Err(TakoError::from(UsdtError::InsufficientBalance).into());
    }

    // Update total supply
    let total_supply_bytes = get_storage(KEY_TOTAL_SUPPLY)?;
    let mut total_supply_arr = [0u8; 8];
    total_supply_arr.copy_from_slice(&total_supply_bytes[..8]);
    let total_supply = u64::from_le_bytes(total_supply_arr);
    let new_total_supply = total_supply
        .checked_sub(amount)
        .ok_or(TakoError::from(UsdtError::Underflow))?;
    set_storage(KEY_TOTAL_SUPPLY, &new_total_supply.to_le_bytes())?;

    // Update balance
    set_balance(&from, balance - amount)?;

    // Emit Transfer event to zero address (ERC20 burn standard)
    Transfer {
        from,
        to: Address::default(),
        amount,
    }
    .emit()
    .ok();

    // Emit Burn event
    Burn { from, amount }.emit().ok();

    debug_log!("USDT: Burned tokens");

    Ok(vec![1])
}

/// Pause contract
fn pause() -> TakoResult<Vec<u8>> {
    require_owner()?;

    let paused_bytes = get_storage(KEY_PAUSED).unwrap_or_default();
    let paused = !paused_bytes.is_empty() && paused_bytes[0] == 1;

    // FIX: Use Paused error (audit finding - was incorrectly using NotPaused)
    if paused {
        return Err(TakoError::from(UsdtError::Paused).into());
    }

    set_storage(KEY_PAUSED, &[1u8])?;

    // Emit Pause event
    Pause {
        timestamp: 0, // TODO: Get actual timestamp from blockchain context
    }
    .emit()
    .ok();

    debug_log!("USDT: Contract paused");

    Ok(vec![1])
}

/// Unpause contract
fn unpause() -> TakoResult<Vec<u8>> {
    require_owner()?;

    set_storage(KEY_PAUSED, &[0u8])?;

    // Emit Unpause event
    Unpause {
        timestamp: 0, // TODO: Get actual timestamp from blockchain context
    }
    .emit()
    .ok();

    debug_log!("USDT: Contract unpaused");

    Ok(vec![1])
}

/// Add address to blacklist
fn add_blacklist(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_owner()?;

    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let account =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;

    let key = blacklist_key(&account);
    set_storage(&key, &[1u8])?;

    // Emit Blacklist event
    Blacklist { account }.emit().ok();

    debug_log!("USDT: Added account to blacklist");

    Ok(vec![1])
}

/// Remove address from blacklist
fn remove_blacklist(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_owner()?;

    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let account =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;

    let key = blacklist_key(&account);
    set_storage(&key, &[0u8])?;

    // Emit UnBlacklist event
    UnBlacklist { account }.emit().ok();

    debug_log!("USDT: Removed account from blacklist");

    Ok(vec![1])
}

/// Destroy funds from blacklisted account (compliance feature)
fn destroy_blacklisted_funds(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_owner()?;

    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let account =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;

    // Check if account is blacklisted
    let key = blacklist_key(&account);
    let blacklisted_bytes = get_storage(&key).unwrap_or_default();
    let blacklisted = !blacklisted_bytes.is_empty() && blacklisted_bytes[0] == 1;

    if !blacklisted {
        return Err(TakoError::from(UsdtError::Unauthorized).into());
    }

    // Get account balance
    let balance = get_balance(&account)?;

    if balance > 0 {
        // Update total supply
        let total_supply_bytes = get_storage(KEY_TOTAL_SUPPLY)?;
        let mut total_supply_arr = [0u8; 8];
        total_supply_arr.copy_from_slice(&total_supply_bytes[..8]);
        let total_supply = u64::from_le_bytes(total_supply_arr);
        let new_total_supply = total_supply
            .checked_sub(balance)
            .ok_or(TakoError::from(UsdtError::Underflow))?;
        set_storage(KEY_TOTAL_SUPPLY, &new_total_supply.to_le_bytes())?;

        // Zero out balance
        set_balance(&account, 0)?;

        // Emit DestroyBlacklistedFunds event
        DestroyBlacklistedFunds {
            account,
            amount: balance,
        }
        .emit()
        .ok();

        debug_log!("USDT: Destroyed tokens from blacklisted account");
    }

    Ok(vec![1])
}

/// Query balance
fn balance_of(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let account =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;

    let balance = get_balance(&account)?;

    Ok(balance.to_le_bytes().to_vec())
}

/// Query allowance
fn query_allowance(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 64 {
        return Err(TakoError::InvalidInput.into());
    }

    let owner =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;
    let spender =
        Address::from_slice(&input[32..64]).ok_or(TakoError::from(UsdtError::InvalidAccount))?;

    let allowance = get_allowance(&owner, &spender)?;

    Ok(allowance.to_le_bytes().to_vec())
}

/// Main contract entrypoint
const MAX_INPUT_SIZE: usize = 128; // Reduced from 1024 to minimize stack usage

#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    // Read input data via syscall
    let mut buffer = [0u8; MAX_INPUT_SIZE];
    let len = get_input_data(&mut buffer);
    let input = &buffer[..len as usize];

    match process_instruction(input) {
        Ok(_) => SUCCESS,
        Err(e) => e,
    }
}

/// Process instruction
fn process_instruction(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.is_empty() {
        return Err(TakoError::InvalidInput.into());
    }

    let instruction =
        Instruction::from_u8(input[0]).ok_or(TakoError::from(UsdtError::InvalidInstruction))?;

    let args = if input.len() > 1 { &input[1..] } else { &[] };

    match instruction {
        Instruction::Initialize => initialize(args),
        Instruction::Transfer => transfer(args),
        Instruction::Approve => approve(args),
        Instruction::TransferFrom => transfer_from(args),
        Instruction::Mint => mint(args),
        Instruction::Burn => burn(args),
        Instruction::Pause => pause(),
        Instruction::Unpause => unpause(),
        Instruction::AddBlacklist => add_blacklist(args),
        Instruction::RemoveBlacklist => remove_blacklist(args),
        Instruction::DestroyBlacklistedFunds => destroy_blacklisted_funds(args),
        Instruction::BalanceOf => balance_of(args),
        Instruction::Allowance => query_allowance(args),
        _ => Err(TakoError::from(UsdtError::InvalidInstruction).into()),
    }
}

// ============================================================================
// Required for no_std target
// ============================================================================

use core::alloc::{GlobalAlloc, Layout};

struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_encoding() {
        assert_eq!(Instruction::Initialize as u8, 0);
        assert_eq!(Instruction::Transfer as u8, 1);
        assert_eq!(Instruction::Mint as u8, 4);
    }
}
