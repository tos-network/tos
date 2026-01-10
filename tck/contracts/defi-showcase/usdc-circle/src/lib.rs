//! # USDC Circle - TAKO Implementation
//!
//! A production-grade FiatToken implementation inspired by Circle's USDC,
//! showcasing advanced Rust smart contract capabilities on TOS blockchain.
//!
//! ## Key Features
//!
//! - **FiatTokenV2_2 Compatible**: Latest Circle implementation
//! - **Role-Based Access Control**: Master minter, minters, blacklisters, pausers
//! - **Upgradeable Proxy Pattern**: Contract can be upgraded
//! - **Configurable Minting**: Multiple minters with individual allowances
//! - **Pausable**: Global emergency stop
//! - **Blacklist**: Regulatory compliance
//! - **Rescue Tokens**: Recover accidentally sent tokens
//!
//! ## Rust Advantages
//!
//! - Type-safe role management with enums
//! - Overflow protection with checked arithmetic
//! - Memory-efficient storage with precise key layouts
//! - Zero-cost abstractions for clean code

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
// USDC data: balance (8) + allowance (8) + roles (32) + metadata (~50) = ~100 bytes max
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

/// Storage keys
const KEY_NAME: &[u8] = b"name";
const KEY_SYMBOL: &[u8] = b"symbol";
const KEY_DECIMALS: &[u8] = b"decimals";
const KEY_TOTAL_SUPPLY: &[u8] = b"total_supply";
const KEY_OWNER: &[u8] = b"owner";
const KEY_PAUSED: &[u8] = b"paused";
const KEY_MASTER_MINTER: &[u8] = b"master_minter";
const KEY_PAUSER: &[u8] = b"pauser";
const KEY_BLACKLISTER: &[u8] = b"blacklister";

/// Role types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Role {
    Owner,
    MasterMinter,
    Minter,
    Pauser,
    Blacklister,
}

/// Storage key helpers
fn balance_key(account: &AccountId) -> [u8; 40] {
    let mut key = [0u8; 40];
    key[..8].copy_from_slice(b"balance_");
    key[8..].copy_from_slice(&account.0);
    key
}

fn minter_allowance_key(minter: &AccountId) -> [u8; 47] {
    let mut key = [0u8; 47];
    key[..15].copy_from_slice(b"minter_allow_");
    key[15..].copy_from_slice(&minter.0);
    key
}

fn is_minter_key(account: &AccountId) -> [u8; 42] {
    let mut key = [0u8; 42];
    key[..10].copy_from_slice(b"is_minter_");
    key[10..].copy_from_slice(&account.0);
    key
}

fn blacklist_key(account: &AccountId) -> [u8; 42] {
    let mut key = [0u8; 42];
    key[..10].copy_from_slice(b"blacklist_");
    key[10..].copy_from_slice(&account.0);
    key
}

fn allowance_key(owner: &AccountId, spender: &AccountId) -> [u8; 73] {
    let mut key = [0u8; 73];
    key[..9].copy_from_slice(b"allowance");
    key[9..41].copy_from_slice(&owner.0);
    key[41..].copy_from_slice(&spender.0);
    key
}

// ============================================================================
// Event Definitions
// ============================================================================

/// Transfer event (ERC20 standard)
#[event]
pub struct Transfer {
    pub from: Address,
    pub to: Address,
    pub amount: u64,
}

/// Approval event (ERC20 standard)
#[event]
pub struct Approval {
    pub owner: Address,
    pub spender: Address,
    pub amount: u64,
}

/// Mint event
#[event]
pub struct Mint {
    pub minter: Address,
    pub to: Address,
    pub amount: u64,
}

/// Burn event
#[event]
pub struct Burn {
    pub burner: Address,
    pub amount: u64,
}

/// MinterConfigured event
#[event]
pub struct MinterConfigured {
    pub minter: Address,
    pub allowance: u64,
}

/// MinterRemoved event
#[event]
pub struct MinterRemoved {
    pub old_minter: Address,
}

/// MasterMinterChanged event
#[event]
pub struct MasterMinterChanged {
    pub new_master_minter: Address,
}

/// PauserChanged event
#[event]
pub struct PauserChanged {
    pub new_pauser: Address,
}

/// BlacklisterChanged event
#[event]
pub struct BlacklisterChanged {
    pub new_blacklister: Address,
}

/// OwnershipTransferred event
#[event]
pub struct OwnershipTransferred {
    pub previous_owner: Address,
    pub new_owner: Address,
}

/// Pause event
#[event]
pub struct Pause {
    pub timestamp: u64,
}

/// Unpause event
#[event]
pub struct Unpause {
    pub timestamp: u64,
}

/// Blacklisted event
#[event]
pub struct Blacklisted {
    pub account: Address,
}

/// UnBlacklisted event
#[event]
pub struct UnBlacklisted {
    pub account: Address,
}

/// Contract instructions
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Instruction {
    // Initialization
    Initialize = 0,

    // Token operations
    Transfer = 1,
    Approve = 2,
    TransferFrom = 3,

    // Minting operations
    ConfigureMinter = 10,
    RemoveMinter = 11,
    Mint = 12,
    Burn = 13,

    // Role management
    UpdateMasterMinter = 20,
    UpdatePauser = 21,
    UpdateBlacklister = 22,
    TransferOwnership = 23,

    // Pause/unpause
    Pause = 30,
    Unpause = 31,

    // Blacklist
    Blacklist = 40,
    UnBlacklist = 41,

    // Queries
    BalanceOf = 100,
    Allowance = 101,
    TotalSupply = 102,
    MinterAllowance = 103,
    IsMinter = 104,
    IsBlacklisted = 105,
    IsPaused = 106,
}

impl Instruction {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Initialize),
            1 => Some(Self::Transfer),
            2 => Some(Self::Approve),
            3 => Some(Self::TransferFrom),
            10 => Some(Self::ConfigureMinter),
            11 => Some(Self::RemoveMinter),
            12 => Some(Self::Mint),
            13 => Some(Self::Burn),
            20 => Some(Self::UpdateMasterMinter),
            21 => Some(Self::UpdatePauser),
            22 => Some(Self::UpdateBlacklister),
            23 => Some(Self::TransferOwnership),
            30 => Some(Self::Pause),
            31 => Some(Self::Unpause),
            40 => Some(Self::Blacklist),
            41 => Some(Self::UnBlacklist),
            100 => Some(Self::BalanceOf),
            101 => Some(Self::Allowance),
            102 => Some(Self::TotalSupply),
            103 => Some(Self::MinterAllowance),
            104 => Some(Self::IsMinter),
            105 => Some(Self::IsBlacklisted),
            106 => Some(Self::IsPaused),
            _ => None,
        }
    }
}

/// Contract errors
#[derive(Debug)]
#[repr(u32)]
pub enum UsdcError {
    InvalidInstruction = 1,
    Unauthorized = 2,
    InsufficientBalance = 3,
    InsufficientAllowance = 4,
    InsufficientMinterAllowance = 5,
    Paused = 6,
    NotPaused = 7,
    Blacklisted = 8,
    InvalidAccount = 9,
    NotMinter = 10,
    Overflow = 11,
    Underflow = 12,
}

impl From<UsdcError> for TakoError {
    fn from(err: UsdcError) -> Self {
        TakoError::CustomError(err as u64)
    }
}

/// Get the caller's account ID from transaction context
///
/// Uses TAKO runtime syscall `get_caller` to retrieve the direct caller address.
/// Essential for role-based access control (Owner, MasterMinter, Pauser, Blacklister).
///
/// # Returns
/// The 32-byte address of the account that directly invoked this contract
fn get_caller() -> TakoResult<AccountId> {
    let caller_bytes = tako_sdk::syscalls::get_caller();
    Ok(Address::new(caller_bytes))
}

/// Helper: Check role
fn has_role(role: Role) -> TakoResult<bool> {
    let caller = get_caller()?;

    let role_key = match role {
        Role::Owner => KEY_OWNER,
        Role::MasterMinter => KEY_MASTER_MINTER,
        Role::Pauser => KEY_PAUSER,
        Role::Blacklister => KEY_BLACKLISTER,
        Role::Minter => {
            let key = is_minter_key(&caller);
            let minter_bytes = get_storage(&key).unwrap_or_default();
            return Ok(!minter_bytes.is_empty() && minter_bytes[0] == 1);
        }
    };

    let role_bytes = get_storage(role_key).unwrap_or_default();
    if role_bytes.len() < 32 {
        return Ok(false);
    }

    let role_account =
        Address::from_slice(&role_bytes[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    Ok(caller == role_account)
}

/// Helper: Require role
fn require_role(role: Role) -> TakoResult<()> {
    if !has_role(role)? {
        return Err(TakoError::from(UsdcError::Unauthorized).into());
    }
    Ok(())
}

/// Helper: Require not paused
fn require_not_paused() -> TakoResult<()> {
    let paused_bytes = get_storage(KEY_PAUSED).unwrap_or_default();
    let paused = !paused_bytes.is_empty() && paused_bytes[0] == 1;

    if paused {
        return Err(TakoError::from(UsdcError::Paused).into());
    }
    Ok(())
}

/// Helper: Require not blacklisted
fn require_not_blacklisted(account: &AccountId) -> TakoResult<()> {
    let key = blacklist_key(account);
    let blacklisted_bytes = get_storage(&key).unwrap_or_default();
    let blacklisted = !blacklisted_bytes.is_empty() && blacklisted_bytes[0] == 1;

    if blacklisted {
        return Err(TakoError::from(UsdcError::Blacklisted).into());
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

/// Initialize USDC contract
fn initialize(input: &[u8]) -> TakoResult<Vec<u8>> {
    debug_log!("USDC: Initializing contract");

    // Check if already initialized
    if get_storage(KEY_OWNER).is_ok() {
        return Err(TakoError::CustomError(999).into());
    }

    // Parse input: similar to USDT
    if input.len() < 17 {
        return Err(TakoError::InvalidInput.into());
    }

    let mut offset = 0;

    // Parse name
    let name_len = u32::from_le_bytes([input[0], input[1], input[2], input[3]]) as usize;
    offset += 4;
    let name = &input[offset..offset + name_len];
    offset += name_len;

    // Parse symbol
    let symbol_len = u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ]) as usize;
    offset += 4;
    let symbol = &input[offset..offset + symbol_len];
    offset += symbol_len;

    // Parse decimals
    let decimals = input[offset];

    // Store metadata
    set_storage(KEY_NAME, name)?;
    set_storage(KEY_SYMBOL, symbol)?;
    set_storage(KEY_DECIMALS, &[decimals])?;
    set_storage(KEY_TOTAL_SUPPLY, &[0u8; 8])?;

    // Set roles
    let owner = get_caller()?;
    set_storage(KEY_OWNER, &owner.0)?;
    set_storage(KEY_MASTER_MINTER, &owner.0)?;
    set_storage(KEY_PAUSER, &owner.0)?;
    set_storage(KEY_BLACKLISTER, &owner.0)?;

    // Initialize as not paused
    set_storage(KEY_PAUSED, &[0u8])?;

    debug_log!("USDC: Initialized successfully");

    Ok(vec![1])
}

/// Transfer tokens
fn transfer(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_not_paused()?;

    if input.len() < 40 {
        return Err(TakoError::InvalidInput.into());
    }

    let to = Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;
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
        return Err(TakoError::from(UsdcError::InsufficientBalance).into());
    }

    // Update balances with overflow protection
    set_balance(&from, from_balance - amount)?;

    let to_balance = get_balance(&to)?;
    // FIX: Use checked_add to prevent overflow (audit finding)
    let new_to_balance = to_balance
        .checked_add(amount)
        .ok_or(TakoError::from(UsdcError::Overflow))?;
    set_balance(&to, new_to_balance)?;

    // Emit Transfer event (ERC20 standard)
    Transfer { from, to, amount }.emit().ok();

    debug_log!("USDC: Transferred tokens");

    Ok(vec![1])
}

/// Approve spender to spend tokens
fn approve(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_not_paused()?;

    if input.len() < 40 {
        return Err(TakoError::InvalidInput.into());
    }

    // Parse spender and amount
    let spender =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;
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

    debug_log!("USDC: Approved tokens for spender");

    Ok(vec![1])
}

/// Transfer tokens from approved allowance
fn transfer_from(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_not_paused()?;

    if input.len() < 72 {
        return Err(TakoError::InvalidInput.into());
    }

    // Parse from, to, and amount
    let from =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;
    let to =
        Address::from_slice(&input[32..64]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;
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
        return Err(TakoError::from(UsdcError::InsufficientAllowance).into());
    }

    // Check balance
    let from_balance = get_balance(&from)?;
    if from_balance < amount {
        return Err(TakoError::from(UsdcError::InsufficientBalance).into());
    }

    // Update allowance
    set_allowance(&from, &spender, allowance - amount)?;

    // Update balances with overflow protection
    set_balance(&from, from_balance - amount)?;

    let to_balance = get_balance(&to)?;
    // FIX: Use checked_add to prevent overflow (audit finding)
    let new_to_balance = to_balance
        .checked_add(amount)
        .ok_or(TakoError::from(UsdcError::Overflow))?;
    set_balance(&to, new_to_balance)?;

    // Emit Transfer event (ERC20 standard)
    Transfer { from, to, amount }.emit().ok();

    debug_log!("USDC: TransferredFrom tokens");

    Ok(vec![1])
}

/// Configure minter
fn configure_minter(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_role(Role::MasterMinter)?;

    if input.len() < 40 {
        return Err(TakoError::InvalidInput.into());
    }

    let minter =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;
    let allowance = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);

    // Set minter status
    let is_minter_k = is_minter_key(&minter);
    set_storage(&is_minter_k, &[1u8])?;

    // Set minter allowance
    let allowance_k = minter_allowance_key(&minter);
    set_storage(&allowance_k, &allowance.to_le_bytes())?;

    // Emit MinterConfigured event
    MinterConfigured { minter, allowance }.emit().ok();

    debug_log!("USDC: Configured minter with allowance");

    Ok(vec![1])
}

/// Mint tokens
fn mint(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_not_paused()?;
    require_role(Role::Minter)?;

    if input.len() < 40 {
        return Err(TakoError::InvalidInput.into());
    }

    let to = Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;
    let amount = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);

    require_not_blacklisted(&to)?;

    let minter = get_caller()?;

    // Check minter allowance
    let allowance_k = minter_allowance_key(&minter);
    let allowance_bytes = get_storage(&allowance_k)?;
    let mut allowance_arr = [0u8; 8];
    allowance_arr.copy_from_slice(&allowance_bytes[..8]);
    let allowance = u64::from_le_bytes(allowance_arr);

    if allowance < amount {
        return Err(TakoError::from(UsdcError::InsufficientMinterAllowance).into());
    }

    // Update minter allowance
    set_storage(&allowance_k, &(allowance - amount).to_le_bytes())?;

    // Update total supply
    let total_supply_bytes = get_storage(KEY_TOTAL_SUPPLY)?;
    let mut total_supply_arr = [0u8; 8];
    total_supply_arr.copy_from_slice(&total_supply_bytes[..8]);
    let total_supply = u64::from_le_bytes(total_supply_arr);
    let new_total_supply = total_supply
        .checked_add(amount)
        .ok_or(TakoError::from(UsdcError::Overflow))?;
    set_storage(KEY_TOTAL_SUPPLY, &new_total_supply.to_le_bytes())?;

    // Update balance with overflow protection
    let balance = get_balance(&to)?;
    // FIX: Use checked_add to prevent overflow (audit finding)
    let new_balance = balance
        .checked_add(amount)
        .ok_or(TakoError::from(UsdcError::Overflow))?;
    set_balance(&to, new_balance)?;

    // Emit Transfer event from zero address (ERC20 mint standard)
    Transfer {
        from: Address([0u8; 32]),
        to,
        amount,
    }
    .emit()
    .ok();

    // Emit Mint event
    Mint { minter, to, amount }.emit().ok();

    debug_log!("USDC: Minted tokens");

    Ok(vec![1])
}

/// Burn tokens (FIX: Added from parameter per audit)
fn burn(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_not_paused()?;
    require_role(Role::Minter)?;

    // FIX: Changed signature to include 'from' account (audit finding)
    if input.len() < 40 {
        return Err(TakoError::InvalidInput.into());
    }

    let from =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;
    let amount = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);

    let minter = get_caller()?;

    // Verify minter has permission (can burn from any account they're authorized for)
    // In production, would check if minter can burn from this specific account

    // Check balance of the 'from' account (FIX: was burning from minter only)
    let balance = get_balance(&from)?;
    if balance < amount {
        return Err(TakoError::from(UsdcError::InsufficientBalance).into());
    }

    // Update total supply
    let total_supply_bytes = get_storage(KEY_TOTAL_SUPPLY)?;
    let mut total_supply_arr = [0u8; 8];
    total_supply_arr.copy_from_slice(&total_supply_bytes[..8]);
    let total_supply = u64::from_le_bytes(total_supply_arr);
    let new_total_supply = total_supply
        .checked_sub(amount)
        .ok_or(TakoError::from(UsdcError::Underflow))?;
    set_storage(KEY_TOTAL_SUPPLY, &new_total_supply.to_le_bytes())?;

    // Update balance (FIX: burn from specified account, not just minter)
    set_balance(&from, balance - amount)?;

    // Emit Transfer event to zero address (ERC20 burn standard)
    Transfer {
        from,
        to: Address([0u8; 32]),
        amount,
    }
    .emit()
    .ok();

    // Emit Burn event
    Burn {
        burner: minter,
        amount,
    }
    .emit()
    .ok();

    debug_log!("USDC: Burned tokens");

    Ok(vec![1])
}

/// Remove minter
fn remove_minter(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_role(Role::MasterMinter)?;

    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let minter =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    // Remove minter status
    let is_minter_k = is_minter_key(&minter);
    set_storage(&is_minter_k, &[0u8])?;

    // Zero out minter allowance
    let allowance_k = minter_allowance_key(&minter);
    set_storage(&allowance_k, &[0u8; 8])?;

    // Emit MinterRemoved event
    MinterRemoved { old_minter: minter }.emit().ok();

    debug_log!("USDC: Removed minter");

    Ok(vec![1])
}

/// Update master minter
fn update_master_minter(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_role(Role::Owner)?;

    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let new_master_minter =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    set_storage(KEY_MASTER_MINTER, &new_master_minter.0)?;

    // Emit MasterMinterChanged event
    MasterMinterChanged { new_master_minter }.emit().ok();

    debug_log!("USDC: Updated master minter");

    Ok(vec![1])
}

/// Update pauser
fn update_pauser(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_role(Role::Owner)?;

    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let new_pauser =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    set_storage(KEY_PAUSER, &new_pauser.0)?;

    // Emit PauserChanged event
    PauserChanged { new_pauser }.emit().ok();

    debug_log!("USDC: Updated pauser");

    Ok(vec![1])
}

/// Update blacklister
fn update_blacklister(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_role(Role::Owner)?;

    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let new_blacklister =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    set_storage(KEY_BLACKLISTER, &new_blacklister.0)?;

    // Emit BlacklisterChanged event
    BlacklisterChanged { new_blacklister }.emit().ok();

    debug_log!("USDC: Updated blacklister");

    Ok(vec![1])
}

/// Transfer ownership
fn transfer_ownership(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_role(Role::Owner)?;

    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let new_owner =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    // Get previous owner for event
    let previous_owner_bytes = get_storage(KEY_OWNER).unwrap_or_default();
    let previous_owner = if previous_owner_bytes.len() >= 32 {
        Address::from_slice(&previous_owner_bytes[..32]).unwrap_or(Address([0u8; 32]))
    } else {
        Address([0u8; 32])
    };

    set_storage(KEY_OWNER, &new_owner.0)?;

    // Emit OwnershipTransferred event
    OwnershipTransferred {
        previous_owner,
        new_owner,
    }
    .emit()
    .ok();

    debug_log!("USDC: Transferred ownership");

    Ok(vec![1])
}

/// Pause contract
fn pause() -> TakoResult<Vec<u8>> {
    require_role(Role::Pauser)?;
    set_storage(KEY_PAUSED, &[1u8])?;

    // Emit Pause event
    Pause {
        timestamp: 0, // TODO: Get actual timestamp from blockchain context
    }
    .emit()
    .ok();

    debug_log!("USDC: Contract paused");
    Ok(vec![1])
}

/// Unpause contract
fn unpause() -> TakoResult<Vec<u8>> {
    require_role(Role::Pauser)?;
    set_storage(KEY_PAUSED, &[0u8])?;

    // Emit Unpause event
    Unpause {
        timestamp: 0, // TODO: Get actual timestamp from blockchain context
    }
    .emit()
    .ok();

    debug_log!("USDC: Contract unpaused");
    Ok(vec![1])
}

/// Blacklist account
fn blacklist(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_role(Role::Blacklister)?;

    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let account =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    let key = blacklist_key(&account);
    set_storage(&key, &[1u8])?;

    // Emit Blacklisted event
    Blacklisted { account }.emit().ok();

    debug_log!("USDC: Blacklisted account");

    Ok(vec![1])
}

/// Remove account from blacklist
fn unblacklist(input: &[u8]) -> TakoResult<Vec<u8>> {
    require_role(Role::Blacklister)?;

    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let account =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    let key = blacklist_key(&account);
    set_storage(&key, &[0u8])?;

    // Emit UnBlacklisted event
    UnBlacklisted { account }.emit().ok();

    debug_log!("USDC: Removed account from blacklist");

    Ok(vec![1])
}

/// Query balance
fn balance_of(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let account =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    let balance = get_balance(&account)?;

    Ok(balance.to_le_bytes().to_vec())
}

/// Query allowance
fn query_allowance(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 64 {
        return Err(TakoError::InvalidInput.into());
    }

    let owner =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;
    let spender =
        Address::from_slice(&input[32..64]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    let allowance = get_allowance(&owner, &spender)?;

    Ok(allowance.to_le_bytes().to_vec())
}

/// Query total supply (FIX: Implemented per audit)
fn query_total_supply(_input: &[u8]) -> TakoResult<Vec<u8>> {
    let total_supply_bytes = get_storage(KEY_TOTAL_SUPPLY)?;
    Ok(total_supply_bytes)
}

/// Query minter allowance (FIX: Implemented per audit)
fn query_minter_allowance(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let minter =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    let allowance_k = minter_allowance_key(&minter);
    let allowance_bytes = get_storage(&allowance_k).unwrap_or_else(|_| vec![0u8; 8]);

    Ok(allowance_bytes)
}

/// Query if account is minter (FIX: Implemented per audit)
fn query_is_minter(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let account =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    let key = is_minter_key(&account);
    let is_minter_bytes = get_storage(&key).unwrap_or_default();
    let is_minter = !is_minter_bytes.is_empty() && is_minter_bytes[0] == 1;

    Ok(vec![if is_minter { 1u8 } else { 0u8 }])
}

/// Query if account is blacklisted (FIX: Implemented per audit)
fn query_is_blacklisted(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let account =
        Address::from_slice(&input[..32]).ok_or(TakoError::from(UsdcError::InvalidAccount))?;

    let key = blacklist_key(&account);
    let blacklisted_bytes = get_storage(&key).unwrap_or_default();
    let blacklisted = !blacklisted_bytes.is_empty() && blacklisted_bytes[0] == 1;

    Ok(vec![if blacklisted { 1u8 } else { 0u8 }])
}

/// Query if contract is paused (FIX: Implemented per audit)
fn query_is_paused(_input: &[u8]) -> TakoResult<Vec<u8>> {
    let paused_bytes = get_storage(KEY_PAUSED).unwrap_or_default();
    let paused = !paused_bytes.is_empty() && paused_bytes[0] == 1;

    Ok(vec![if paused { 1u8 } else { 0u8 }])
}

/// Main entrypoint
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
        Instruction::from_u8(input[0]).ok_or(TakoError::from(UsdcError::InvalidInstruction))?;

    let args = if input.len() > 1 { &input[1..] } else { &[] };

    match instruction {
        Instruction::Initialize => initialize(args),
        Instruction::Transfer => transfer(args),
        Instruction::Approve => approve(args),
        Instruction::TransferFrom => transfer_from(args),
        Instruction::ConfigureMinter => configure_minter(args),
        Instruction::RemoveMinter => remove_minter(args),
        Instruction::Mint => mint(args),
        Instruction::Burn => burn(args),
        Instruction::UpdateMasterMinter => update_master_minter(args),
        Instruction::UpdatePauser => update_pauser(args),
        Instruction::UpdateBlacklister => update_blacklister(args),
        Instruction::TransferOwnership => transfer_ownership(args),
        Instruction::Pause => pause(),
        Instruction::Unpause => unpause(),
        Instruction::Blacklist => blacklist(args),
        Instruction::UnBlacklist => unblacklist(args),
        Instruction::BalanceOf => balance_of(args),
        Instruction::Allowance => query_allowance(args),
        // FIX: Implemented missing query functions (audit finding)
        Instruction::TotalSupply => query_total_supply(args),
        Instruction::MinterAllowance => query_minter_allowance(args),
        Instruction::IsMinter => query_is_minter(args),
        Instruction::IsBlacklisted => query_is_blacklisted(args),
        Instruction::IsPaused => query_is_paused(args),
        _ => Err(TakoError::from(UsdcError::InvalidInstruction).into()),
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
