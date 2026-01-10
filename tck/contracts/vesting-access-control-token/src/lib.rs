//! # VestingAccessControlToken
//!
//! Advanced 4-pattern composition demonstrating:
//! - ERC20 token functionality
//! - Time-based vesting schedules
//! - Role-based access control
//! - Emergency pause mechanism
//!
//! This contract showcases the most complex composition pattern in the TAKO SDK,
//! integrating four independent contract patterns into a cohesive system.

#![no_std]
#![no_main]

// ============================================================================
// Constants and Role Definitions
// ============================================================================

/// Default admin role (can grant/revoke all roles)
const DEFAULT_ADMIN_ROLE: [u8; 32] = [0u8; 32];

/// Role for managing vesting schedules
const VESTING_ADMIN_ROLE: [u8; 32] = [
    0x15, 0x74, 0x8c, 0x2a, 0xd9, 0x0e, 0x3e, 0xf9, 0x47, 0xb4, 0x57, 0x9e, 0xcd, 0x1d, 0x77, 0x82,
    0xd3, 0x1b, 0xc0, 0x85, 0x3f, 0x16, 0xae, 0x45, 0xf4, 0x60, 0xe4, 0xfb, 0x1b, 0x61, 0xa7, 0x0e,
]; // keccak256("VESTING_ADMIN_ROLE")

/// Role for releasing vested tokens (beyond beneficiary self-release)
const BENEFICIARY_ROLE: [u8; 32] = [
    0x6a, 0xe1, 0xf9, 0x80, 0xd1, 0x1d, 0xf2, 0xc3, 0xbe, 0x51, 0x7e, 0x3c, 0xa6, 0x03, 0x3c, 0xf1,
    0x81, 0x0a, 0x6f, 0xde, 0xe5, 0xd8, 0x38, 0x26, 0xf0, 0x96, 0xa2, 0xc3, 0xa7, 0x78, 0xd6, 0x1f,
]; // keccak256("BENEFICIARY_ROLE")

/// Role for pausing/unpausing the contract
const PAUSER_ROLE: [u8; 32] = [
    0x65, 0xd7, 0xa2, 0x8e, 0x3b, 0xff, 0x92, 0xa7, 0x3c, 0x5a, 0x61, 0x17, 0xd7, 0xb8, 0x9f, 0x02,
    0x77, 0xd3, 0x29, 0x06, 0x47, 0xf7, 0x6c, 0x95, 0xa1, 0x74, 0xde, 0xf8, 0x3e, 0x79, 0xab, 0x9d,
]; // keccak256("PAUSER_ROLE")

/// Role for minting new tokens (for vesting)
const MINTER_ROLE: [u8; 32] = [
    0x9f, 0x2d, 0xf0, 0xfe, 0xd2, 0xc7, 0x7a, 0x4c, 0x27, 0xae, 0x6b, 0xf5, 0xe9, 0x8c, 0x53, 0x70,
    0xaa, 0x4f, 0x41, 0x1b, 0x72, 0x80, 0x67, 0x73, 0xdf, 0x1c, 0xaf, 0xed, 0x22, 0x3d, 0x13, 0x59,
]; // keccak256("MINTER_ROLE")

// ============================================================================
// Instruction Opcodes
// ============================================================================

// ERC20 Operations (0x00-0x15)
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

// Vesting Operations (0x20-0x27)
const OP_CREATE_VESTING_SCHEDULE: u8 = 0x20;
const OP_RELEASE: u8 = 0x21;
const OP_REVOKE_VESTING_SCHEDULE: u8 = 0x22;
const OP_VESTED_AMOUNT: u8 = 0x23;
const OP_RELEASABLE: u8 = 0x24;
const OP_RELEASED: u8 = 0x25;
const OP_GET_VESTING_SCHEDULE: u8 = 0x26;
const OP_GET_ACTIVE_SCHEDULE_COUNT: u8 = 0x27;

// AccessControl Operations (0x30-0x35)
const OP_GRANT_ROLE: u8 = 0x30;
const OP_REVOKE_ROLE: u8 = 0x31;
const OP_RENOUNCE_ROLE: u8 = 0x32;
const OP_HAS_ROLE: u8 = 0x33;
const OP_GET_ROLE_ADMIN: u8 = 0x34;
const OP_SET_ROLE_ADMIN: u8 = 0x35;

// Pausable Operations (0x40-0x42)
const OP_PAUSE: u8 = 0x40;
const OP_UNPAUSE: u8 = 0x41;
const OP_PAUSED: u8 = 0x42;

// ============================================================================
// Error Codes
// ============================================================================

const ERR_INSUFFICIENT_BALANCE: u64 = 1001;
const ERR_ZERO_ADDRESS: u64 = 1002;
const ERR_INSUFFICIENT_ALLOWANCE: u64 = 1003;
const ERR_CONTRACT_PAUSED: u64 = 1004;
const ERR_MISSING_ROLE: u64 = 1005;
const ERR_NOT_BENEFICIARY: u64 = 1007;
const ERR_NO_VESTING_SCHEDULE: u64 = 1008;
const ERR_VESTING_ALREADY_EXISTS: u64 = 1009;
const ERR_NOTHING_TO_RELEASE: u64 = 1010;
const ERR_ZERO_DURATION: u64 = 1011;
const ERR_ZERO_ALLOCATION: u64 = 1012;
const ERR_ALREADY_INITIALIZED: u64 = 1013;
const ERR_NOT_INITIALIZED: u64 = 1014;
const ERR_INVALID_INSTRUCTION: u64 = 1015;
const ERR_INVALID_PARAMS: u64 = 1016;

// ============================================================================
// Storage Keys
// ============================================================================

const KEY_INITIALIZED: &[u8] = b"initialized";
const KEY_PAUSED: &[u8] = b"paused";
const KEY_TOTAL_SUPPLY: &[u8] = b"total_supply";
const KEY_NAME: &[u8] = b"name";
const KEY_SYMBOL: &[u8] = b"symbol";
const KEY_DECIMALS: &[u8] = b"decimals";
const KEY_VESTING_COUNT: &[u8] = b"vesting_count";

// ============================================================================
// External Functions (Syscalls)
// ============================================================================

extern "C" {
    fn storage_read(key_ptr: *const u8, key_len: usize) -> u64;
    fn storage_write(key_ptr: *const u8, key_len: usize, value_ptr: *const u8, value_len: usize);
    fn get_tx_sender(output_ptr: *mut u8);
    fn get_block_timestamp() -> u64;
    fn return_data(data_ptr: *const u8, data_len: usize);
    fn abort_with_code(code: u64) -> !;
}

// ============================================================================
// Helper Functions - Storage Operations
// ============================================================================

/// Read a u64 value from storage
fn storage_read_u64(key: &[u8]) -> u64 {
    unsafe { storage_read(key.as_ptr(), key.len()) }
}

/// Write a u64 value to storage
fn storage_write_u64(key: &[u8], value: u64) {
    let bytes = value.to_le_bytes();
    unsafe {
        storage_write(key.as_ptr(), key.len(), bytes.as_ptr(), bytes.len());
    }
}

/// Read a boolean value from storage
fn storage_read_bool(key: &[u8]) -> bool {
    storage_read_u64(key) != 0
}

/// Write a boolean value to storage
fn storage_write_bool(key: &[u8], value: bool) {
    storage_write_u64(key, if value { 1 } else { 0 });
}

/// Read bytes from storage (for strings)
fn storage_read_bytes(key: &[u8]) -> [u8; 64] {
    let buffer = [0u8; 64];
    unsafe {
        let len = storage_read(key.as_ptr(), key.len());
        if len > 0 && len <= 64 {
            // Storage read returns length, actual data is in a separate syscall
            // For simplicity, we store the length in the first 8 bytes
        }
    }
    buffer
}

/// Write bytes to storage (for strings)
fn storage_write_bytes(key: &[u8], value: &[u8]) {
    unsafe {
        storage_write(key.as_ptr(), key.len(), value.as_ptr(), value.len());
    }
}

/// Get the transaction sender address
fn get_sender() -> [u8; 32] {
    let mut sender = [0u8; 32];
    unsafe {
        get_tx_sender(sender.as_mut_ptr());
    }
    sender
}

/// Get current block timestamp
fn get_timestamp() -> u64 {
    unsafe { get_block_timestamp() }
}

/// Return data to caller
fn return_value(data: &[u8]) {
    unsafe {
        return_data(data.as_ptr(), data.len());
    }
}

/// Abort with error code
fn abort(code: u64) -> ! {
    unsafe { abort_with_code(code) }
}

// ============================================================================
// Helper Functions - Key Generation
// ============================================================================

/// Generate storage key for balance
fn balance_key(account: &[u8; 32]) -> [u8; 40] {
    let mut key = [0u8; 40];
    key[0..8].copy_from_slice(b"balance:");
    key[8..40].copy_from_slice(account);
    key
}

/// Generate storage key for allowance
fn allowance_key(owner: &[u8; 32], spender: &[u8; 32]) -> [u8; 74] {
    let mut key = [0u8; 74];
    key[0..10].copy_from_slice(b"allowance:");
    key[10..42].copy_from_slice(owner);
    key[42..43].copy_from_slice(b":");
    key[43..75].copy_from_slice(spender);
    key
}

/// Generate storage key for role membership
fn role_key(role: &[u8; 32], account: &[u8; 32]) -> [u8; 69] {
    let mut key = [0u8; 69];
    key[0..5].copy_from_slice(b"role:");
    key[5..37].copy_from_slice(role);
    key[37..38].copy_from_slice(b":");
    key[38..70].copy_from_slice(account);
    key
}

/// Generate storage key for role admin
fn role_admin_key(role: &[u8; 32]) -> [u8; 43] {
    let mut key = [0u8; 43];
    key[0..11].copy_from_slice(b"role_admin:");
    key[11..43].copy_from_slice(role);
    key
}

/// Generate storage key for vesting start time
fn vesting_start_key(beneficiary: &[u8; 32]) -> [u8; 46] {
    let mut key = [0u8; 46];
    key[0..14].copy_from_slice(b"vesting_start:");
    key[14..46].copy_from_slice(beneficiary);
    key
}

/// Generate storage key for vesting duration
fn vesting_duration_key(beneficiary: &[u8; 32]) -> [u8; 49] {
    let mut key = [0u8; 49];
    key[0..17].copy_from_slice(b"vesting_duration:");
    key[17..49].copy_from_slice(beneficiary);
    key
}

/// Generate storage key for total allocation
fn vesting_total_key(beneficiary: &[u8; 32]) -> [u8; 46] {
    let mut key = [0u8; 46];
    key[0..14].copy_from_slice(b"vesting_total:");
    key[14..46].copy_from_slice(beneficiary);
    key
}

/// Generate storage key for released amount
fn vesting_released_key(beneficiary: &[u8; 32]) -> [u8; 49] {
    let mut key = [0u8; 49];
    key[0..17].copy_from_slice(b"vesting_released:");
    key[17..49].copy_from_slice(beneficiary);
    key
}

// ============================================================================
// Helper Functions - Validation
// ============================================================================

/// Check if address is zero
fn is_zero_address(address: &[u8; 32]) -> bool {
    address.iter().all(|&b| b == 0)
}

/// Require that the address is not zero
fn require_non_zero_address(address: &[u8; 32]) -> Result<(), u64> {
    if is_zero_address(address) {
        Err(ERR_ZERO_ADDRESS)
    } else {
        Ok(())
    }
}

/// Require that the contract is not paused
fn require_not_paused() -> Result<(), u64> {
    if storage_read_bool(KEY_PAUSED) {
        Err(ERR_CONTRACT_PAUSED)
    } else {
        Ok(())
    }
}

/// Require that the contract is initialized
fn require_initialized() -> Result<(), u64> {
    if !storage_read_bool(KEY_INITIALIZED) {
        Err(ERR_NOT_INITIALIZED)
    } else {
        Ok(())
    }
}

/// Require that the contract is not initialized
fn require_not_initialized() -> Result<(), u64> {
    if storage_read_bool(KEY_INITIALIZED) {
        Err(ERR_ALREADY_INITIALIZED)
    } else {
        Ok(())
    }
}

// ============================================================================
// Helper Functions - Access Control
// ============================================================================

/// Check if an account has a specific role
fn has_role(role: &[u8; 32], account: &[u8; 32]) -> bool {
    let key = role_key(role, account);
    storage_read_bool(&key)
}

/// Require that an account has a specific role
fn require_role(role: &[u8; 32], account: &[u8; 32]) -> Result<(), u64> {
    if !has_role(role, account) {
        Err(ERR_MISSING_ROLE)
    } else {
        Ok(())
    }
}

/// Grant a role to an account (internal)
fn grant_role_internal(role: &[u8; 32], account: &[u8; 32]) {
    let key = role_key(role, account);
    storage_write_bool(&key, true);
}

/// Revoke a role from an account (internal)
fn revoke_role_internal(role: &[u8; 32], account: &[u8; 32]) {
    let key = role_key(role, account);
    storage_write_bool(&key, false);
}

/// Get the admin role for a given role
fn get_role_admin(role: &[u8; 32]) -> [u8; 32] {
    let key = role_admin_key(role);
    let admin = [0u8; 32];
    // Read from storage (simplified - in real implementation would read bytes)
    let value = storage_read_u64(&key);
    if value == 0 {
        DEFAULT_ADMIN_ROLE
    } else {
        admin
    }
}

/// Set the admin role for a given role (internal)
fn set_role_admin_internal(role: &[u8; 32], admin_role: &[u8; 32]) {
    let key = role_admin_key(role);
    storage_write_bytes(&key, admin_role);
}

// ============================================================================
// Helper Functions - ERC20
// ============================================================================

/// Get balance of an account
fn get_balance(account: &[u8; 32]) -> u64 {
    let key = balance_key(account);
    storage_read_u64(&key)
}

/// Set balance of an account
fn set_balance(account: &[u8; 32], amount: u64) {
    let key = balance_key(account);
    storage_write_u64(&key, amount);
}

/// Get allowance
fn get_allowance(owner: &[u8; 32], spender: &[u8; 32]) -> u64 {
    let key = allowance_key(owner, spender);
    storage_read_u64(&key)
}

/// Set allowance
fn set_allowance(owner: &[u8; 32], spender: &[u8; 32], amount: u64) {
    let key = allowance_key(owner, spender);
    storage_write_u64(&key, amount);
}

/// Transfer tokens (internal)
fn transfer_internal(from: &[u8; 32], to: &[u8; 32], amount: u64) -> Result<(), u64> {
    require_non_zero_address(to)?;

    let from_balance = get_balance(from);
    if from_balance < amount {
        return Err(ERR_INSUFFICIENT_BALANCE);
    }

    let to_balance = get_balance(to);

    set_balance(from, from_balance.saturating_sub(amount));
    set_balance(to, to_balance.saturating_add(amount));

    Ok(())
}

/// Mint tokens (internal)
fn mint_internal(to: &[u8; 32], amount: u64) -> Result<(), u64> {
    require_non_zero_address(to)?;

    let total_supply = storage_read_u64(KEY_TOTAL_SUPPLY);
    let new_supply = total_supply.saturating_add(amount);
    storage_write_u64(KEY_TOTAL_SUPPLY, new_supply);

    let balance = get_balance(to);
    set_balance(to, balance.saturating_add(amount));

    Ok(())
}

/// Burn tokens (internal)
fn burn_internal(from: &[u8; 32], amount: u64) -> Result<(), u64> {
    let balance = get_balance(from);
    if balance < amount {
        return Err(ERR_INSUFFICIENT_BALANCE);
    }

    let total_supply = storage_read_u64(KEY_TOTAL_SUPPLY);
    storage_write_u64(KEY_TOTAL_SUPPLY, total_supply.saturating_sub(amount));

    set_balance(from, balance.saturating_sub(amount));

    Ok(())
}

// ============================================================================
// Helper Functions - Vesting
// ============================================================================

/// Check if a vesting schedule exists
fn has_vesting_schedule(beneficiary: &[u8; 32]) -> bool {
    let key = vesting_start_key(beneficiary);
    storage_read_u64(&key) != 0
}

/// Get vesting start time
fn get_vesting_start(beneficiary: &[u8; 32]) -> u64 {
    let key = vesting_start_key(beneficiary);
    storage_read_u64(&key)
}

/// Set vesting start time
fn set_vesting_start(beneficiary: &[u8; 32], start: u64) {
    let key = vesting_start_key(beneficiary);
    storage_write_u64(&key, start);
}

/// Get vesting duration
fn get_vesting_duration(beneficiary: &[u8; 32]) -> u64 {
    let key = vesting_duration_key(beneficiary);
    storage_read_u64(&key)
}

/// Set vesting duration
fn set_vesting_duration(beneficiary: &[u8; 32], duration: u64) {
    let key = vesting_duration_key(beneficiary);
    storage_write_u64(&key, duration);
}

/// Get total allocation
fn get_total_allocation(beneficiary: &[u8; 32]) -> u64 {
    let key = vesting_total_key(beneficiary);
    storage_read_u64(&key)
}

/// Set total allocation
fn set_total_allocation(beneficiary: &[u8; 32], total: u64) {
    let key = vesting_total_key(beneficiary);
    storage_write_u64(&key, total);
}

/// Get released amount
fn get_released(beneficiary: &[u8; 32]) -> u64 {
    let key = vesting_released_key(beneficiary);
    storage_read_u64(&key)
}

/// Set released amount
fn set_released(beneficiary: &[u8; 32], released: u64) {
    let key = vesting_released_key(beneficiary);
    storage_write_u64(&key, released);
}

/// Calculate vested amount at a given timestamp
fn calculate_vested_amount(beneficiary: &[u8; 32], timestamp: u64) -> u64 {
    if !has_vesting_schedule(beneficiary) {
        return 0;
    }

    let start = get_vesting_start(beneficiary);
    let duration = get_vesting_duration(beneficiary);
    let total = get_total_allocation(beneficiary);

    if timestamp < start {
        return 0;
    }

    if timestamp >= start.saturating_add(duration) {
        return total;
    }

    let elapsed = timestamp.saturating_sub(start);
    ((total as u128)
        .saturating_mul(elapsed as u128)
        .checked_div(duration as u128)
        .unwrap_or(0)) as u64
}

/// Calculate releasable amount (vested - released)
fn calculate_releasable(beneficiary: &[u8; 32]) -> u64 {
    let timestamp = get_timestamp();
    let vested = calculate_vested_amount(beneficiary, timestamp);
    let released = get_released(beneficiary);
    vested.saturating_sub(released)
}

/// Delete vesting schedule (internal)
fn delete_vesting_schedule(beneficiary: &[u8; 32]) {
    set_vesting_start(beneficiary, 0);
    set_vesting_duration(beneficiary, 0);
    set_total_allocation(beneficiary, 0);
    set_released(beneficiary, 0);
}

// ============================================================================
// Helper Functions - Parsing
// ============================================================================

/// Parse an address from parameter bytes
fn parse_address(params: &[u8], offset: usize) -> Result<[u8; 32], u64> {
    if params.len() < offset.saturating_add(32) {
        return Err(ERR_INVALID_PARAMS);
    }

    let mut address = [0u8; 32];
    address.copy_from_slice(&params[offset..offset.saturating_add(32)]);
    Ok(address)
}

/// Parse a u64 from parameter bytes
fn parse_u64(params: &[u8], offset: usize) -> Result<u64, u64> {
    if params.len() < offset.saturating_add(8) {
        return Err(ERR_INVALID_PARAMS);
    }

    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&params[offset..offset.saturating_add(8)]);
    Ok(u64::from_le_bytes(bytes))
}

/// Parse a u8 from parameter bytes
fn parse_u8(params: &[u8], offset: usize) -> Result<u8, u64> {
    if params.len() < offset.saturating_add(1) {
        return Err(ERR_INVALID_PARAMS);
    }

    Ok(params[offset])
}

/// Parse a string from parameter bytes (length-prefixed)
fn parse_string(params: &[u8], offset: usize) -> Result<(&[u8], usize), u64> {
    if params.len() < offset.saturating_add(8) {
        return Err(ERR_INVALID_PARAMS);
    }

    let len = parse_u64(params, offset)? as usize;
    let str_offset = offset.saturating_add(8);

    if params.len() < str_offset.saturating_add(len) {
        return Err(ERR_INVALID_PARAMS);
    }

    Ok((
        &params[str_offset..str_offset.saturating_add(len)],
        str_offset.saturating_add(len),
    ))
}

// ============================================================================
// ERC20 Operations
// ============================================================================

/// Initialize the contract
fn op_initialize(params: &[u8]) -> Result<(), u64> {
    require_not_initialized()?;

    // Parse parameters: name, symbol, decimals, initial_supply
    let (name, offset) = parse_string(params, 0)?;
    let (symbol, offset) = parse_string(params, offset)?;
    let decimals = parse_u8(params, offset)?;
    let initial_supply = parse_u64(params, offset.saturating_add(1))?;

    // Store metadata
    storage_write_bytes(KEY_NAME, name);
    storage_write_bytes(KEY_SYMBOL, symbol);
    storage_write_u64(KEY_DECIMALS, decimals as u64);
    storage_write_u64(KEY_TOTAL_SUPPLY, initial_supply);

    // Grant initial roles to deployer
    let sender = get_sender();
    grant_role_internal(&DEFAULT_ADMIN_ROLE, &sender);
    grant_role_internal(&VESTING_ADMIN_ROLE, &sender);
    grant_role_internal(&PAUSER_ROLE, &sender);
    grant_role_internal(&MINTER_ROLE, &sender);

    // Mint initial supply to deployer
    if initial_supply > 0 {
        set_balance(&sender, initial_supply);
    }

    // Mark as initialized
    storage_write_bool(KEY_INITIALIZED, true);
    storage_write_u64(KEY_VESTING_COUNT, 0);

    Ok(())
}

/// Transfer tokens
fn op_transfer(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;
    require_not_paused()?;

    let to = parse_address(params, 0)?;
    let amount = parse_u64(params, 32)?;

    let from = get_sender();
    transfer_internal(&from, &to, amount)?;

    Ok(())
}

/// Approve spender
fn op_approve(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let spender = parse_address(params, 0)?;
    let amount = parse_u64(params, 32)?;

    let owner = get_sender();
    require_non_zero_address(&spender)?;

    set_allowance(&owner, &spender, amount);

    Ok(())
}

/// Transfer from
fn op_transfer_from(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;
    require_not_paused()?;

    let from = parse_address(params, 0)?;
    let to = parse_address(params, 32)?;
    let amount = parse_u64(params, 64)?;

    let spender = get_sender();
    let allowance = get_allowance(&from, &spender);

    if allowance < amount {
        return Err(ERR_INSUFFICIENT_ALLOWANCE);
    }

    set_allowance(&from, &spender, allowance.saturating_sub(amount));
    transfer_internal(&from, &to, amount)?;

    Ok(())
}

/// Mint tokens
fn op_mint(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let sender = get_sender();
    require_role(&MINTER_ROLE, &sender)?;

    let to = parse_address(params, 0)?;
    let amount = parse_u64(params, 32)?;

    mint_internal(&to, amount)?;

    Ok(())
}

/// Burn tokens
fn op_burn(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;
    require_not_paused()?;

    let amount = parse_u64(params, 0)?;
    let from = get_sender();

    burn_internal(&from, amount)?;

    Ok(())
}

/// Get balance of account
fn op_balance_of(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let account = parse_address(params, 0)?;
    let balance = get_balance(&account);

    let bytes = balance.to_le_bytes();
    return_value(&bytes);

    Ok(())
}

/// Get allowance
fn op_allowance(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let owner = parse_address(params, 0)?;
    let spender = parse_address(params, 32)?;
    let allowance = get_allowance(&owner, &spender);

    let bytes = allowance.to_le_bytes();
    return_value(&bytes);

    Ok(())
}

/// Get total supply
fn op_total_supply() -> Result<(), u64> {
    require_initialized()?;

    let supply = storage_read_u64(KEY_TOTAL_SUPPLY);
    let bytes = supply.to_le_bytes();
    return_value(&bytes);

    Ok(())
}

/// Get token name
fn op_name() -> Result<(), u64> {
    require_initialized()?;

    let name = storage_read_bytes(KEY_NAME);
    return_value(&name);

    Ok(())
}

/// Get token symbol
fn op_symbol() -> Result<(), u64> {
    require_initialized()?;

    let symbol = storage_read_bytes(KEY_SYMBOL);
    return_value(&symbol);

    Ok(())
}

/// Get decimals
fn op_decimals() -> Result<(), u64> {
    require_initialized()?;

    let decimals = storage_read_u64(KEY_DECIMALS) as u8;
    let bytes = [decimals];
    return_value(&bytes);

    Ok(())
}

// ============================================================================
// Vesting Operations
// ============================================================================

/// Create vesting schedule
fn op_create_vesting_schedule(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;
    require_not_paused()?;

    let sender = get_sender();
    require_role(&VESTING_ADMIN_ROLE, &sender)?;

    let beneficiary = parse_address(params, 0)?;
    let start = parse_u64(params, 32)?;
    let duration = parse_u64(params, 40)?;
    let total_allocation = parse_u64(params, 48)?;

    require_non_zero_address(&beneficiary)?;

    if duration == 0 {
        return Err(ERR_ZERO_DURATION);
    }

    if total_allocation == 0 {
        return Err(ERR_ZERO_ALLOCATION);
    }

    if has_vesting_schedule(&beneficiary) {
        return Err(ERR_VESTING_ALREADY_EXISTS);
    }

    // Create schedule
    set_vesting_start(&beneficiary, start);
    set_vesting_duration(&beneficiary, duration);
    set_total_allocation(&beneficiary, total_allocation);
    set_released(&beneficiary, 0);

    // Increment count
    let count = storage_read_u64(KEY_VESTING_COUNT);
    storage_write_u64(KEY_VESTING_COUNT, count.saturating_add(1));

    Ok(())
}

/// Release vested tokens
fn op_release(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;
    require_not_paused()?;

    let beneficiary = parse_address(params, 0)?;
    let sender = get_sender();

    // Check authorization: must be beneficiary or have BENEFICIARY_ROLE
    if sender != beneficiary && !has_role(&BENEFICIARY_ROLE, &sender) {
        return Err(ERR_NOT_BENEFICIARY);
    }

    if !has_vesting_schedule(&beneficiary) {
        return Err(ERR_NO_VESTING_SCHEDULE);
    }

    let releasable = calculate_releasable(&beneficiary);

    if releasable == 0 {
        return Err(ERR_NOTHING_TO_RELEASE);
    }

    // Update released amount
    let released = get_released(&beneficiary);
    set_released(&beneficiary, released.saturating_add(releasable));

    // Mint tokens to beneficiary
    mint_internal(&beneficiary, releasable)?;

    Ok(())
}

/// Revoke vesting schedule
fn op_revoke_vesting_schedule(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let sender = get_sender();
    require_role(&VESTING_ADMIN_ROLE, &sender)?;

    let beneficiary = parse_address(params, 0)?;

    if !has_vesting_schedule(&beneficiary) {
        return Err(ERR_NO_VESTING_SCHEDULE);
    }

    // Delete schedule
    delete_vesting_schedule(&beneficiary);

    // Decrement count
    let count = storage_read_u64(KEY_VESTING_COUNT);
    if count > 0 {
        storage_write_u64(KEY_VESTING_COUNT, count.saturating_sub(1));
    }

    Ok(())
}

/// Get vested amount at timestamp
fn op_vested_amount(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let beneficiary = parse_address(params, 0)?;
    let timestamp = parse_u64(params, 32)?;

    let vested = calculate_vested_amount(&beneficiary, timestamp);
    let bytes = vested.to_le_bytes();
    return_value(&bytes);

    Ok(())
}

/// Get releasable amount
fn op_releasable(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let beneficiary = parse_address(params, 0)?;
    let releasable = calculate_releasable(&beneficiary);

    let bytes = releasable.to_le_bytes();
    return_value(&bytes);

    Ok(())
}

/// Get released amount
fn op_released(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let beneficiary = parse_address(params, 0)?;
    let released = get_released(&beneficiary);

    let bytes = released.to_le_bytes();
    return_value(&bytes);

    Ok(())
}

/// Get vesting schedule
fn op_get_vesting_schedule(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let beneficiary = parse_address(params, 0)?;

    if !has_vesting_schedule(&beneficiary) {
        return Err(ERR_NO_VESTING_SCHEDULE);
    }

    let start = get_vesting_start(&beneficiary);
    let duration = get_vesting_duration(&beneficiary);
    let total = get_total_allocation(&beneficiary);
    let released = get_released(&beneficiary);

    // Return as 32 bytes: start(8) + duration(8) + total(8) + released(8)
    let mut result = [0u8; 32];
    result[0..8].copy_from_slice(&start.to_le_bytes());
    result[8..16].copy_from_slice(&duration.to_le_bytes());
    result[16..24].copy_from_slice(&total.to_le_bytes());
    result[24..32].copy_from_slice(&released.to_le_bytes());

    return_value(&result);

    Ok(())
}

/// Get active schedule count
fn op_get_active_schedule_count() -> Result<(), u64> {
    require_initialized()?;

    let count = storage_read_u64(KEY_VESTING_COUNT);
    let bytes = count.to_le_bytes();
    return_value(&bytes);

    Ok(())
}

// ============================================================================
// Access Control Operations
// ============================================================================

/// Grant role
fn op_grant_role(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let role = parse_address(params, 0)?;
    let account = parse_address(params, 32)?;

    let sender = get_sender();
    let admin_role = get_role_admin(&role);
    require_role(&admin_role, &sender)?;

    grant_role_internal(&role, &account);

    Ok(())
}

/// Revoke role
fn op_revoke_role(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let role = parse_address(params, 0)?;
    let account = parse_address(params, 32)?;

    let sender = get_sender();
    let admin_role = get_role_admin(&role);
    require_role(&admin_role, &sender)?;

    revoke_role_internal(&role, &account);

    Ok(())
}

/// Renounce role
fn op_renounce_role(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let role = parse_address(params, 0)?;
    let sender = get_sender();

    revoke_role_internal(&role, &sender);

    Ok(())
}

/// Check if account has role
fn op_has_role(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let role = parse_address(params, 0)?;
    let account = parse_address(params, 32)?;

    let result = if has_role(&role, &account) { 1u8 } else { 0u8 };
    return_value(&[result]);

    Ok(())
}

/// Get role admin
fn op_get_role_admin(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let role = parse_address(params, 0)?;
    let admin = get_role_admin(&role);

    return_value(&admin);

    Ok(())
}

/// Set role admin
fn op_set_role_admin(params: &[u8]) -> Result<(), u64> {
    require_initialized()?;

    let role = parse_address(params, 0)?;
    let admin_role = parse_address(params, 32)?;

    let sender = get_sender();
    require_role(&DEFAULT_ADMIN_ROLE, &sender)?;

    set_role_admin_internal(&role, &admin_role);

    Ok(())
}

// ============================================================================
// Pausable Operations
// ============================================================================

/// Pause contract
fn op_pause() -> Result<(), u64> {
    require_initialized()?;

    let sender = get_sender();
    require_role(&PAUSER_ROLE, &sender)?;

    storage_write_bool(KEY_PAUSED, true);

    Ok(())
}

/// Unpause contract
fn op_unpause() -> Result<(), u64> {
    require_initialized()?;

    let sender = get_sender();
    require_role(&PAUSER_ROLE, &sender)?;

    storage_write_bool(KEY_PAUSED, false);

    Ok(())
}

/// Check if paused
fn op_paused() -> Result<(), u64> {
    let paused = storage_read_bool(KEY_PAUSED);
    let result = if paused { 1u8 } else { 0u8 };
    return_value(&[result]);

    Ok(())
}

// ============================================================================
// Entry Point
// ============================================================================

#[no_mangle]
pub extern "C" fn entrypoint(instruction: u8, params_ptr: *const u8, params_len: usize) -> u64 {
    let params = if params_len > 0 {
        unsafe { core::slice::from_raw_parts(params_ptr, params_len) }
    } else {
        &[]
    };

    let result = match instruction {
        // ERC20 Operations
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

        // Vesting Operations
        OP_CREATE_VESTING_SCHEDULE => op_create_vesting_schedule(params),
        OP_RELEASE => op_release(params),
        OP_REVOKE_VESTING_SCHEDULE => op_revoke_vesting_schedule(params),
        OP_VESTED_AMOUNT => op_vested_amount(params),
        OP_RELEASABLE => op_releasable(params),
        OP_RELEASED => op_released(params),
        OP_GET_VESTING_SCHEDULE => op_get_vesting_schedule(params),
        OP_GET_ACTIVE_SCHEDULE_COUNT => op_get_active_schedule_count(),

        // Access Control Operations
        OP_GRANT_ROLE => op_grant_role(params),
        OP_REVOKE_ROLE => op_revoke_role(params),
        OP_RENOUNCE_ROLE => op_renounce_role(params),
        OP_HAS_ROLE => op_has_role(params),
        OP_GET_ROLE_ADMIN => op_get_role_admin(params),
        OP_SET_ROLE_ADMIN => op_set_role_admin(params),

        // Pausable Operations
        OP_PAUSE => op_pause(),
        OP_UNPAUSE => op_unpause(),
        OP_PAUSED => op_paused(),

        _ => Err(ERR_INVALID_INSTRUCTION),
    };

    match result {
        Ok(()) => 0,
        Err(code) => code,
    }
}

// ============================================================================
// Panic Handler
// ============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    abort(9999)
}
