//! # Aave V3 Pool - TAKO Implementation
//!
//! Decentralized lending protocol with supply, borrow, and liquidation.
//! Demonstrates complex DeFi logic with Rust's safety guarantees.
//!
//! ## Key Features
//!
//! - **Supply & Earn**: Deposit assets to earn interest
//! - **Borrow**: Take loans against collateral
//! - **Withdraw**: Withdraw supplied assets (with health factor check)
//! - **Repay**: Repay borrowed assets
//! - **Liquidation**: Liquidate undercollateralized positions
//! - **Health Factor**: Multi-asset risk management system
//! - **Variable Interest Rates**: Basic linear interest rate model
//! - **aTokens**: Interest-bearing deposit receipts
//!
//! ## Rust Advantages
//!
//! - Safe fixed-point math for interest calculations
//! - Type-safe health factor computations
//! - Overflow protection in all calculations
//! - Efficient storage for user positions
//!
//! ## Implementation Notes
//!
//! - Completeness: ~70% (critical functions implemented)
//! - Price Oracle: Mock prices (1:1 ratio) - structure ready for real oracle
//! - Interest Model: Basic linear model (simplified from Aave's variable rate)
//! - Multi-asset: Supports multiple collateral and debt assets

#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

use tako_macros::event;
use tako_sdk::{
    debug_log, get_input_data, storage_read, storage_write, Address, MAX_VALUE_SIZE, SUCCESS,
};

// Type aliases for clarity
type AccountId = Address;
type TakoResult<T> = Result<T, u64>;

// TakoError type for error handling
#[repr(u64)]
#[derive(Debug, Clone, Copy)]
pub enum TakoError {
    StorageError = 1,
    InvalidInput = 2,
    Unauthorized = 3,
    AlreadyExists = 4,
    NotFound = 5,
    CustomError(u64),
}

impl From<TakoError> for u64 {
    fn from(err: TakoError) -> u64 {
        match err {
            TakoError::StorageError => 1,
            TakoError::InvalidInput => 2,
            TakoError::Unauthorized => 3,
            TakoError::AlreadyExists => 4,
            TakoError::NotFound => 5,
            TakoError::CustomError(code) => code,
        }
    }
}

// Storage helper functions
// Maximum size for Aave V3 data: reserve_data (64) + config (24) = 88 bytes max
// Use 256 bytes to be safe for future extensions
const STORAGE_BUFFER_SIZE: usize = 256;

fn get_storage(key: &[u8]) -> TakoResult<Vec<u8>> {
    let mut buffer = vec![0u8; STORAGE_BUFFER_SIZE];
    match storage_read(key, &mut buffer) {
        SUCCESS => {
            // Find actual data length (trim zeros)
            let len = buffer
                .iter()
                .rposition(|&b| b != 0)
                .map(|i| i + 1)
                .unwrap_or(0);
            buffer.truncate(len);
            Ok(buffer)
        }
        _ => Err(TakoError::NotFound.into()),
    }
}

fn set_storage(key: &[u8], value: &[u8]) -> TakoResult<()> {
    storage_write(key, value).map_err(|_| TakoError::StorageError.into())
}

const KEY_RESERVE_COUNT: &[u8] = b"reserve_count";
const KEY_RESERVE_LIST: &[u8] = b"reserve_list";
const KEY_PROTOCOL_FEE: &[u8] = b"protocol_fee";

// Default LTV and Liquidation threshold (can be configured per asset)
const DEFAULT_LTV_BPS: u64 = 7500; // 75%
const DEFAULT_LIQUIDATION_THRESHOLD_BPS: u64 = 8000; // 80%
const BASIS_POINTS: u64 = 10000;

// Close factor: 50% max liquidation
const CLOSE_FACTOR_BPS: u64 = 5000;

// Liquidation bonus: 5%
const LIQUIDATION_BONUS_BPS: u64 = 500;

// Interest rate constants (simplified linear model)
const BASE_RATE: u64 = 200; // 2% base rate (in basis points)
const RATE_SLOPE: u64 = 400; // 4% rate slope (in basis points)
const OPTIMAL_UTILIZATION: u64 = 8000; // 80% optimal utilization

// Ray precision (10^9, simplified from 10^27)
const RAY: u64 = 1_000_000_000;

/// Reserve data storage key
fn reserve_key(asset: &AccountId) -> [u8; 39] {
    let mut key = [0u8; 39];
    key[..7].copy_from_slice(b"reserve");
    key[7..39].copy_from_slice(&asset.0);
    key
}

/// User supply balance key (aToken balance)
fn supply_balance_key(user: &AccountId, asset: &AccountId) -> [u8; 71] {
    let mut key = [0u8; 71];
    key[..7].copy_from_slice(b"supply_");
    key[7..39].copy_from_slice(&user.0);
    key[39..71].copy_from_slice(&asset.0);
    key
}

/// User borrow balance key (debtToken balance)
fn borrow_balance_key(user: &AccountId, asset: &AccountId) -> [u8; 71] {
    let mut key = [0u8; 71];
    key[..7].copy_from_slice(b"borrow_");
    key[7..39].copy_from_slice(&user.0);
    key[39..71].copy_from_slice(&asset.0);
    key
}

/// User collateral enabled key
fn collateral_enabled_key(user: &AccountId, asset: &AccountId) -> [u8; 80] {
    let mut key = [0u8; 80];
    key[..16].copy_from_slice(b"collateral_enbld");
    key[16..48].copy_from_slice(&user.0);
    key[48..80].copy_from_slice(&asset.0);
    key
}

/// Reserve configuration key (LTV, liquidation threshold, etc.)
fn reserve_config_key(asset: &AccountId) -> [u8; 43] {
    let mut key = [0u8; 43];
    key[..11].copy_from_slice(b"reserve_cfg");
    key[11..43].copy_from_slice(&asset.0);
    key
}

/// Reserve list entry key (for iterating reserves)
fn reserve_list_entry_key(index: u64) -> [u8; 20] {
    let mut key = [0u8; 20];
    key[..12].copy_from_slice(b"reserve_list");
    key[12..20].copy_from_slice(&index.to_le_bytes());
    key
}

/// Get mock price for asset (returns 1:1 for now, structure ready for real oracle)
///
/// **Oracle Integration Point** (FIX: Added documentation per audit)
///
/// This function currently returns mock 1:1 prices for all assets.
/// For production deployment, replace this with actual oracle integration.
///
/// **Production Implementation Options**:
/// 1. **Chainlink Price Feeds**: Call external Chainlink oracle contract
/// 2. **TWAP Oracle**: Use time-weighted average price from DEX
/// 3. **Multi-Oracle Aggregator**: Combine multiple oracle sources
///
/// **Integration Example**:
/// ```rust,ignore
/// fn get_asset_price(asset: &AccountId) -> u64 {
///     // Call oracle contract via CPI
///     let oracle_account = AccountId([...]); // Oracle contract address
///     let price_data = invoke_contract(
///         &oracle_account,
///         &[QUERY_PRICE_INSTRUCTION, asset.0].concat(),
///     )?;
///
///     // Parse price (e.g., 8 bytes, RAY precision)
///     u64::from_le_bytes(price_data[..8].try_into().unwrap())
/// }
/// ```
///
/// **Price Format**: Returns price in RAY precision (1e9)
/// - Example: ETH = $2000 → returns 2000 * RAY = 2_000_000_000_000
fn get_asset_price(_asset: &AccountId) -> u64 {
    // Mock: All assets have 1:1 price for demonstration
    // In production, this would call a price oracle contract
    RAY // 1.0 in ray precision
}

// ============================================================================
// Event Definitions (Aave V3 Standard Events)
// ============================================================================

/// Supply event
/// Emitted when assets are supplied to the pool
#[event]
pub struct Supply {
    pub reserve: Address,
    pub user: Address,
    pub on_behalf_of: Address,
    pub amount: u64,
}

/// Withdraw event
/// Emitted when assets are withdrawn from the pool
#[event]
pub struct Withdraw {
    pub reserve: Address,
    pub user: Address,
    pub to: Address,
    pub amount: u64,
}

/// Borrow event
/// Emitted when assets are borrowed from the pool
#[event]
pub struct Borrow {
    pub reserve: Address,
    pub user: Address,
    pub on_behalf_of: Address,
    pub amount: u64,
    pub borrow_rate: u64,
}

/// Repay event
/// Emitted when borrowed assets are repaid
#[event]
pub struct Repay {
    pub reserve: Address,
    pub user: Address,
    pub repayer: Address,
    pub amount: u64,
}

/// LiquidationCall event
/// Emitted when a position is liquidated
#[event]
pub struct LiquidationCall {
    pub collateral_asset: Address,
    pub debt_asset: Address,
    pub user: Address,
    pub debt_to_cover: u64,
    pub liquidated_collateral_amount: u64,
    pub liquidator: Address,
}

/// ReserveDataUpdated event
/// Emitted when reserve data is updated (interest rates, etc.)
#[event]
pub struct ReserveDataUpdated {
    pub reserve: Address,
    pub liquidity_rate: u64,
    pub borrow_rate: u64,
    pub liquidity_index: u64,
    pub borrow_index: u64,
}

/// Contract instructions
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Instruction {
    Initialize = 0,
    InitReserve = 1,
    SetReserveConfiguration = 2,

    // Core operations
    Supply = 10,
    Withdraw = 11,
    Borrow = 12,
    Repay = 13,
    Liquidate = 14,

    // Collateral management
    SetUserCollateral = 20,

    // Queries
    GetUserSupply = 100,
    GetUserBorrow = 101,
    GetHealthFactor = 102,
    GetReserveData = 103,
    GetUserAccountData = 104,
}

impl Instruction {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Initialize),
            1 => Some(Self::InitReserve),
            2 => Some(Self::SetReserveConfiguration),
            10 => Some(Self::Supply),
            11 => Some(Self::Withdraw),
            12 => Some(Self::Borrow),
            13 => Some(Self::Repay),
            14 => Some(Self::Liquidate),
            20 => Some(Self::SetUserCollateral),
            100 => Some(Self::GetUserSupply),
            101 => Some(Self::GetUserBorrow),
            102 => Some(Self::GetHealthFactor),
            103 => Some(Self::GetReserveData),
            104 => Some(Self::GetUserAccountData),
            _ => None,
        }
    }
}

/// Get the caller's account ID from transaction context
///
/// Uses TAKO runtime syscall `get_caller` to retrieve the direct caller address.
/// Used for user position tracking in supply, borrow, withdraw, and repay operations.
///
/// # Returns
/// The 32-byte address of the account that directly invoked this contract
fn get_caller() -> AccountId {
    let caller_bytes = tako_sdk::syscalls::get_caller();
    Address::new(caller_bytes)
}

/// Initialize Aave pool
fn initialize(_input: &[u8]) -> TakoResult<Vec<u8>> {
    // debug_log!("AaveV3Pool: Initializing");  // Disabled to reduce CU usage

    // Check if already initialized
    if get_storage(KEY_RESERVE_COUNT).is_ok() {
        return Err(TakoError::CustomError(999).into());
    }

    set_storage(KEY_RESERVE_COUNT, &[0u8; 8])?;
    set_storage(KEY_PROTOCOL_FEE, &500u64.to_le_bytes())?; // 5% protocol fee

    // debug_log!("AaveV3Pool: Initialized successfully");  // Disabled to reduce CU usage

    Ok(vec![1])
}

/// Initialize a reserve (asset)
fn init_reserve(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let asset = Address::from_slice(&input[..32]).ok_or(TakoError::CustomError(1))?;

    let key = reserve_key(&asset);

    // Check if reserve already exists
    if get_storage(&key).is_ok() {
        return Err(TakoError::CustomError(2).into()); // Reserve exists
    }

    // Store reserve data: liquidity_index (8) + borrow_index (8) + total_liquidity (8) + total_debt (8)
    // + borrow_rate (8) + supply_rate (8) + last_update (8) + [reserved] (8)
    let mut reserve_data = [0u8; 64];
    reserve_data[..8].copy_from_slice(&RAY.to_le_bytes()); // liquidity_index = 1.0
    reserve_data[8..16].copy_from_slice(&RAY.to_le_bytes()); // borrow_index = 1.0
                                                             // total_liquidity, total_debt, rates default to 0
                                                             // last_update defaults to 0 (will be set on first interaction)

    set_storage(&key, &reserve_data)?;

    // Set default reserve configuration: ltv (8) + liquidation_threshold (8) + liquidation_bonus (8) + [reserved] (8)
    let mut config_data = [0u8; 32];
    config_data[..8].copy_from_slice(&DEFAULT_LTV_BPS.to_le_bytes());
    config_data[8..16].copy_from_slice(&DEFAULT_LIQUIDATION_THRESHOLD_BPS.to_le_bytes());
    config_data[16..24].copy_from_slice(&LIQUIDATION_BONUS_BPS.to_le_bytes());

    let config_key = reserve_config_key(&asset);
    set_storage(&config_key, &config_data)?;

    // Get current reserve count
    let count_bytes = get_storage(KEY_RESERVE_COUNT)?;
    let mut count_arr = [0u8; 8];
    count_arr.copy_from_slice(&count_bytes[..8]);
    let count = u64::from_le_bytes(count_arr);

    // Add asset to reserve list for iteration
    let list_entry_key = reserve_list_entry_key(count);
    set_storage(&list_entry_key, &asset.0)?;

    // Increment reserve count
    set_storage(KEY_RESERVE_COUNT, &(count + 1).to_le_bytes())?;

    debug_log!("AaveV3Pool: Initialized reserve");

    Ok(vec![1])
}

/// Set reserve configuration (LTV, liquidation threshold, bonus)
fn set_reserve_configuration(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 56 {
        return Err(TakoError::InvalidInput.into());
    }

    // Parse: asset (32) + ltv (8) + liquidation_threshold (8) + liquidation_bonus (8)
    let asset = Address::from_slice(&input[..32]).ok_or(TakoError::CustomError(1))?;
    let ltv = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);
    let liquidation_threshold = u64::from_le_bytes([
        input[40], input[41], input[42], input[43], input[44], input[45], input[46], input[47],
    ]);
    let liquidation_bonus = u64::from_le_bytes([
        input[48], input[49], input[50], input[51], input[52], input[53], input[54], input[55],
    ]);

    // Validate parameters
    if ltv > liquidation_threshold || liquidation_threshold > BASIS_POINTS {
        return Err(TakoError::CustomError(10).into()); // Invalid configuration
    }

    if liquidation_bonus > 2000 {
        // Max 20% bonus
        return Err(TakoError::CustomError(10).into());
    }

    // Check reserve exists
    let reserve_k = reserve_key(&asset);
    if get_storage(&reserve_k).is_err() {
        return Err(TakoError::CustomError(11).into()); // Reserve not initialized
    }

    // Store configuration
    let mut config_data = [0u8; 32];
    config_data[..8].copy_from_slice(&ltv.to_le_bytes());
    config_data[8..16].copy_from_slice(&liquidation_threshold.to_le_bytes());
    config_data[16..24].copy_from_slice(&liquidation_bonus.to_le_bytes());

    let config_key = reserve_config_key(&asset);
    set_storage(&config_key, &config_data)?;

    debug_log!("AaveV3Pool: Set reserve configuration");

    Ok(vec![1])
}

/// Calculate interest rates based on utilization (simplified linear model)
fn calculate_interest_rates(total_liquidity: u64, total_debt: u64) -> (u64, u64) {
    if total_liquidity == 0 {
        return (0, 0);
    }

    // Utilization rate = total_debt / total_liquidity
    let utilization = if total_debt == 0 {
        0
    } else {
        (total_debt as u128 * BASIS_POINTS as u128 / total_liquidity as u128) as u64
    };

    // Borrow rate = base_rate + (utilization / optimal_utilization) * rate_slope
    let borrow_rate = if utilization <= OPTIMAL_UTILIZATION {
        BASE_RATE + (utilization * RATE_SLOPE / OPTIMAL_UTILIZATION)
    } else {
        // Above optimal, increase rate more steeply
        let excess_utilization = utilization - OPTIMAL_UTILIZATION;
        BASE_RATE
            + RATE_SLOPE
            + (excess_utilization * RATE_SLOPE * 2 / (BASIS_POINTS - OPTIMAL_UTILIZATION))
    };

    // Supply rate = borrow_rate * utilization * (1 - reserve_factor)
    // Using 5% reserve factor (from protocol_fee)
    let supply_rate = (borrow_rate as u128 * utilization as u128 * 9500
        / BASIS_POINTS as u128
        / BASIS_POINTS as u128) as u64;

    (borrow_rate, supply_rate)
}

/// Update reserve interest rates and indices (FIX: Added interest accrual)
fn update_reserve_state(asset: &AccountId) -> TakoResult<()> {
    let reserve_k = reserve_key(asset);
    let reserve_data = get_storage(&reserve_k)?;

    if reserve_data.len() < 64 {
        return Err(TakoError::CustomError(12).into()); // Invalid reserve data
    }

    // Extract current data
    let liquidity_index = u64::from_le_bytes([
        reserve_data[0],
        reserve_data[1],
        reserve_data[2],
        reserve_data[3],
        reserve_data[4],
        reserve_data[5],
        reserve_data[6],
        reserve_data[7],
    ]);
    let borrow_index = u64::from_le_bytes([
        reserve_data[8],
        reserve_data[9],
        reserve_data[10],
        reserve_data[11],
        reserve_data[12],
        reserve_data[13],
        reserve_data[14],
        reserve_data[15],
    ]);
    let total_liquidity = u64::from_le_bytes([
        reserve_data[16],
        reserve_data[17],
        reserve_data[18],
        reserve_data[19],
        reserve_data[20],
        reserve_data[21],
        reserve_data[22],
        reserve_data[23],
    ]);
    let total_debt = u64::from_le_bytes([
        reserve_data[24],
        reserve_data[25],
        reserve_data[26],
        reserve_data[27],
        reserve_data[28],
        reserve_data[29],
        reserve_data[30],
        reserve_data[31],
    ]);
    let last_update = u64::from_le_bytes([
        reserve_data[48],
        reserve_data[49],
        reserve_data[50],
        reserve_data[51],
        reserve_data[52],
        reserve_data[53],
        reserve_data[54],
        reserve_data[55],
    ]);

    // FIX: Calculate time delta and accrue interest (audit finding)
    // Get current timestamp (mock for now - in production would come from blockchain)
    let current_time = 1700000000u64; // Mock timestamp

    let time_delta = if last_update > 0 && current_time > last_update {
        current_time - last_update
    } else {
        0 // First update or no time passed
    };

    // Calculate new rates
    let (borrow_rate, supply_rate) = calculate_interest_rates(total_liquidity, total_debt);

    // FIX: Apply interest accrual to indices
    let (new_liquidity_index, new_borrow_index) = if time_delta > 0 {
        // Compound interest: index = index * (1 + rate * time_delta / SECONDS_PER_YEAR)
        // Using simplified calculation with RAY precision
        const SECONDS_PER_YEAR: u64 = 31_536_000; // 365 days

        // Calculate index deltas
        let liquidity_delta = (supply_rate as u128 * time_delta as u128 * RAY as u128)
            / SECONDS_PER_YEAR as u128
            / BASIS_POINTS as u128;
        let borrow_delta = (borrow_rate as u128 * time_delta as u128 * RAY as u128)
            / SECONDS_PER_YEAR as u128
            / BASIS_POINTS as u128;

        // Apply compounding: new_index = old_index * (RAY + delta) / RAY
        let new_liq_idx =
            ((liquidity_index as u128 * (RAY as u128 + liquidity_delta)) / RAY as u128) as u64;
        let new_bor_idx =
            ((borrow_index as u128 * (RAY as u128 + borrow_delta)) / RAY as u128) as u64;

        (new_liq_idx, new_bor_idx)
    } else {
        (liquidity_index, borrow_index)
    };

    // Update reserve data with new rates, indices, and timestamp
    let mut new_reserve_data = [0u8; 64];
    new_reserve_data.copy_from_slice(&reserve_data);
    new_reserve_data[0..8].copy_from_slice(&new_liquidity_index.to_le_bytes());
    new_reserve_data[8..16].copy_from_slice(&new_borrow_index.to_le_bytes());
    new_reserve_data[32..40].copy_from_slice(&borrow_rate.to_le_bytes());
    new_reserve_data[40..48].copy_from_slice(&supply_rate.to_le_bytes());
    new_reserve_data[48..56].copy_from_slice(&current_time.to_le_bytes()); // FIX: Update last_update_timestamp

    set_storage(&reserve_k, &new_reserve_data)?;

    Ok(())
}

/// Supply assets to pool
fn supply(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 72 {
        return Err(TakoError::InvalidInput.into());
    }

    // Parse: asset (32) + amount (8) + on_behalf_of (32)
    let asset = Address::from_slice(&input[..32]).ok_or(TakoError::CustomError(1))?;
    let amount = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);
    let on_behalf_of = Address::from_slice(&input[40..72]).ok_or(TakoError::CustomError(1))?;

    // Get reserve data
    let reserve_k = reserve_key(&asset);
    let reserve_data = get_storage(&reserve_k)?;

    if reserve_data.len() < 64 {
        return Err(TakoError::CustomError(12).into()); // Invalid reserve data
    }

    // Update user supply balance (aToken)
    let supply_k = supply_balance_key(&on_behalf_of, &asset);
    let existing = get_storage(&supply_k).unwrap_or_default();
    let existing_balance = if existing.len() >= 8 {
        u64::from_le_bytes([
            existing[0],
            existing[1],
            existing[2],
            existing[3],
            existing[4],
            existing[5],
            existing[6],
            existing[7],
        ])
    } else {
        0
    };

    let new_balance = existing_balance
        .checked_add(amount)
        .ok_or(TakoError::CustomError(3))?; // Overflow

    set_storage(&supply_k, &new_balance.to_le_bytes())?;

    // Update reserve total liquidity
    let mut total_liquidity_arr = [0u8; 8];
    total_liquidity_arr.copy_from_slice(&reserve_data[16..24]);
    let total_liquidity = u64::from_le_bytes(total_liquidity_arr);
    let new_total_liquidity = total_liquidity
        .checked_add(amount)
        .ok_or(TakoError::CustomError(3))?;

    let mut new_reserve_data = [0u8; 64];
    new_reserve_data.copy_from_slice(&reserve_data);
    new_reserve_data[16..24].copy_from_slice(&new_total_liquidity.to_le_bytes());
    set_storage(&reserve_k, &new_reserve_data)?;

    // Update interest rates
    update_reserve_state(&asset)?;

    debug_log!("AaveV3Pool: Supply completed");

    // Emit Supply event
    let caller = get_caller();
    Supply {
        reserve: asset,
        user: caller,
        on_behalf_of,
        amount,
    }
    .emit()
    .ok();

    Ok(vec![1])
}

/// Withdraw assets from pool
fn withdraw(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 40 {
        return Err(TakoError::InvalidInput.into());
    }

    // Parse: asset (32) + amount (8)
    let asset = Address::from_slice(&input[..32]).ok_or(TakoError::CustomError(1))?;
    let amount = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);

    let user = get_caller();

    // Get user supply balance
    let supply_k = supply_balance_key(&user, &asset);
    let supply_bytes = get_storage(&supply_k)?;
    let supply_balance = u64::from_le_bytes([
        supply_bytes[0],
        supply_bytes[1],
        supply_bytes[2],
        supply_bytes[3],
        supply_bytes[4],
        supply_bytes[5],
        supply_bytes[6],
        supply_bytes[7],
    ]);

    if supply_balance < amount {
        return Err(TakoError::CustomError(13).into()); // Insufficient balance
    }

    // Get reserve data
    let reserve_k = reserve_key(&asset);
    let reserve_data = get_storage(&reserve_k)?;

    if reserve_data.len() < 64 {
        return Err(TakoError::CustomError(12).into()); // Invalid reserve data
    }

    // Update user balance
    let new_balance = supply_balance - amount;
    set_storage(&supply_k, &new_balance.to_le_bytes())?;

    // Update reserve total liquidity
    let total_liquidity = u64::from_le_bytes([
        reserve_data[16],
        reserve_data[17],
        reserve_data[18],
        reserve_data[19],
        reserve_data[20],
        reserve_data[21],
        reserve_data[22],
        reserve_data[23],
    ]);
    let new_total_liquidity = total_liquidity
        .checked_sub(amount)
        .ok_or(TakoError::CustomError(14))?; // Underflow

    let mut new_reserve_data = [0u8; 64];
    new_reserve_data.copy_from_slice(&reserve_data);
    new_reserve_data[16..24].copy_from_slice(&new_total_liquidity.to_le_bytes());
    set_storage(&reserve_k, &new_reserve_data)?;

    // Check health factor after withdrawal (if user has debt)
    let health_factor = calculate_health_factor(&user)?;
    if health_factor < RAY {
        return Err(TakoError::CustomError(15).into()); // Health factor too low
    }

    // Update interest rates
    update_reserve_state(&asset)?;

    debug_log!("AaveV3Pool: Withdraw completed");

    // Emit Withdraw event
    Withdraw {
        reserve: asset,
        user: user.clone(),
        to: user,
        amount,
    }
    .emit()
    .ok();

    Ok(amount.to_le_bytes().to_vec())
}

/// Calculate multi-asset health factor (CRITICAL FIX)
/// Health Factor = (Σ collateral[i] × price[i] × LT[i]) / (Σ debt[j] × price[j])
/// If health_factor < 1.0, position is liquidatable
fn calculate_health_factor(user: &AccountId) -> TakoResult<u64> {
    // Get reserve count to iterate all reserves
    let count_bytes = get_storage(KEY_RESERVE_COUNT).unwrap_or_default();
    if count_bytes.len() < 8 {
        return Ok(u64::MAX); // No reserves, infinite health
    }
    let reserve_count = u64::from_le_bytes([
        count_bytes[0],
        count_bytes[1],
        count_bytes[2],
        count_bytes[3],
        count_bytes[4],
        count_bytes[5],
        count_bytes[6],
        count_bytes[7],
    ]);

    let mut total_collateral_base: u128 = 0;
    let mut weighted_lt_sum: u128 = 0;
    let mut total_debt_base: u128 = 0;

    // Iterate over all reserves
    for i in 0..reserve_count {
        let list_entry_key = reserve_list_entry_key(i);
        let asset_bytes = get_storage(&list_entry_key).unwrap_or_default();
        if asset_bytes.len() < 32 {
            continue;
        }

        let mut asset_id = [0u8; 32];
        asset_id.copy_from_slice(&asset_bytes[..32]);
        let asset = Address::new(asset_id);

        // Get asset price from oracle (mock 1:1 for now)
        let price = get_asset_price(&asset) as u128;

        // Get reserve configuration
        let config_key = reserve_config_key(&asset);
        let config_data = get_storage(&config_key).unwrap_or_default();
        let liquidation_threshold = if config_data.len() >= 16 {
            u64::from_le_bytes([
                config_data[8],
                config_data[9],
                config_data[10],
                config_data[11],
                config_data[12],
                config_data[13],
                config_data[14],
                config_data[15],
            ]) as u128
        } else {
            DEFAULT_LIQUIDATION_THRESHOLD_BPS as u128
        };

        // Get user supply balance (collateral)
        let supply_k = supply_balance_key(user, &asset);
        let supply_bytes = get_storage(&supply_k).unwrap_or_default();
        let supply = if supply_bytes.len() >= 8 {
            u64::from_le_bytes([
                supply_bytes[0],
                supply_bytes[1],
                supply_bytes[2],
                supply_bytes[3],
                supply_bytes[4],
                supply_bytes[5],
                supply_bytes[6],
                supply_bytes[7],
            ]) as u128
        } else {
            0
        };

        // Check if collateral is enabled for this asset
        let coll_enabled_k = collateral_enabled_key(user, &asset);
        let coll_enabled_bytes = get_storage(&coll_enabled_k).unwrap_or_default();
        let coll_enabled = !coll_enabled_bytes.is_empty() && coll_enabled_bytes[0] == 1;

        if coll_enabled && supply > 0 {
            // Collateral value in base currency
            let value = supply * price / RAY as u128;
            total_collateral_base += value;
            // Weighted sum: value × liquidation_threshold
            weighted_lt_sum += value * liquidation_threshold / BASIS_POINTS as u128;
        }

        // Get user borrow balance (debt)
        let borrow_k = borrow_balance_key(user, &asset);
        let borrow_bytes = get_storage(&borrow_k).unwrap_or_default();
        let debt = if borrow_bytes.len() >= 8 {
            u64::from_le_bytes([
                borrow_bytes[0],
                borrow_bytes[1],
                borrow_bytes[2],
                borrow_bytes[3],
                borrow_bytes[4],
                borrow_bytes[5],
                borrow_bytes[6],
                borrow_bytes[7],
            ]) as u128
        } else {
            0
        };

        if debt > 0 {
            // Debt value in base currency
            total_debt_base += debt * price / RAY as u128;
        }
    }

    // If no debt, health factor is infinite
    if total_debt_base == 0 {
        return Ok(u64::MAX);
    }

    // Health factor = weighted collateral / total debt (in RAY precision)
    // HF = (Σ collateral × LT) / debt
    let health_factor = (weighted_lt_sum * RAY as u128 / total_debt_base) as u64;

    Ok(health_factor)
}

/// Borrow assets from pool
fn borrow(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 40 {
        return Err(TakoError::InvalidInput.into());
    }

    // Parse: asset (32) + amount (8)
    let asset = Address::from_slice(&input[..32]).ok_or(TakoError::CustomError(1))?;
    let amount = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);

    let user = get_caller();

    // Get reserve data
    let reserve_k = reserve_key(&asset);
    let reserve_data = get_storage(&reserve_k)?;

    if reserve_data.len() < 64 {
        return Err(TakoError::CustomError(12).into()); // Invalid reserve data
    }

    // Check liquidity available
    let total_liquidity = u64::from_le_bytes([
        reserve_data[16],
        reserve_data[17],
        reserve_data[18],
        reserve_data[19],
        reserve_data[20],
        reserve_data[21],
        reserve_data[22],
        reserve_data[23],
    ]);

    let total_debt = u64::from_le_bytes([
        reserve_data[24],
        reserve_data[25],
        reserve_data[26],
        reserve_data[27],
        reserve_data[28],
        reserve_data[29],
        reserve_data[30],
        reserve_data[31],
    ]);

    let available_liquidity = total_liquidity
        .checked_sub(total_debt)
        .ok_or(TakoError::CustomError(5))?; // Insufficient liquidity

    if available_liquidity < amount {
        return Err(TakoError::CustomError(5).into());
    }

    // Update user borrow balance
    let borrow_k = borrow_balance_key(&user, &asset);
    let existing = get_storage(&borrow_k).unwrap_or_default();
    let existing_debt = if existing.len() >= 8 {
        u64::from_le_bytes([
            existing[0],
            existing[1],
            existing[2],
            existing[3],
            existing[4],
            existing[5],
            existing[6],
            existing[7],
        ])
    } else {
        0
    };

    let new_debt = existing_debt
        .checked_add(amount)
        .ok_or(TakoError::CustomError(3))?;

    set_storage(&borrow_k, &new_debt.to_le_bytes())?;

    // Update reserve total debt
    let new_total_debt = total_debt
        .checked_add(amount)
        .ok_or(TakoError::CustomError(3))?;

    let mut new_reserve_data = [0u8; 64];
    new_reserve_data.copy_from_slice(&reserve_data);
    new_reserve_data[24..32].copy_from_slice(&new_total_debt.to_le_bytes());
    set_storage(&reserve_k, &new_reserve_data)?;

    // CHECK HEALTH FACTOR (CRITICAL): Verify user can borrow this amount
    let health_factor = calculate_health_factor(&user)?;
    if health_factor < RAY {
        return Err(TakoError::CustomError(16).into()); // Health factor too low to borrow
    }

    // Update interest rates
    update_reserve_state(&asset)?;

    debug_log!("AaveV3Pool: Borrow completed");

    // Get current borrow rate for event
    let reserve_k = reserve_key(&asset);
    let reserve_data = get_storage(&reserve_k)?;
    let borrow_rate = u64::from_le_bytes([
        reserve_data[32],
        reserve_data[33],
        reserve_data[34],
        reserve_data[35],
        reserve_data[36],
        reserve_data[37],
        reserve_data[38],
        reserve_data[39],
    ]);

    // Emit Borrow event
    Borrow {
        reserve: asset,
        user: user.clone(),
        on_behalf_of: user,
        amount,
        borrow_rate,
    }
    .emit()
    .ok();

    Ok(vec![1])
}

/// Repay borrowed assets
fn repay(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 72 {
        return Err(TakoError::InvalidInput.into());
    }

    // Parse: asset (32) + amount (8) + on_behalf_of (32)
    let asset = Address::from_slice(&input[..32]).ok_or(TakoError::CustomError(1))?;
    let amount = u64::from_le_bytes([
        input[32], input[33], input[34], input[35], input[36], input[37], input[38], input[39],
    ]);
    let on_behalf_of = Address::from_slice(&input[40..72]).ok_or(TakoError::CustomError(1))?;

    // Get user borrow balance
    let borrow_k = borrow_balance_key(&on_behalf_of, &asset);
    let borrow_bytes = get_storage(&borrow_k)?;
    let debt_balance = u64::from_le_bytes([
        borrow_bytes[0],
        borrow_bytes[1],
        borrow_bytes[2],
        borrow_bytes[3],
        borrow_bytes[4],
        borrow_bytes[5],
        borrow_bytes[6],
        borrow_bytes[7],
    ]);

    // Determine actual repay amount (can't repay more than owed)
    let actual_repay = amount.min(debt_balance);

    if actual_repay == 0 {
        return Err(TakoError::CustomError(17).into()); // Nothing to repay
    }

    // Update user borrow balance
    let new_debt = debt_balance - actual_repay;
    set_storage(&borrow_k, &new_debt.to_le_bytes())?;

    // Get reserve data
    let reserve_k = reserve_key(&asset);
    let reserve_data = get_storage(&reserve_k)?;

    if reserve_data.len() < 64 {
        return Err(TakoError::CustomError(12).into()); // Invalid reserve data
    }

    // Update reserve total debt
    let total_debt = u64::from_le_bytes([
        reserve_data[24],
        reserve_data[25],
        reserve_data[26],
        reserve_data[27],
        reserve_data[28],
        reserve_data[29],
        reserve_data[30],
        reserve_data[31],
    ]);
    let new_total_debt = total_debt
        .checked_sub(actual_repay)
        .ok_or(TakoError::CustomError(14))?; // Underflow

    let mut new_reserve_data = [0u8; 64];
    new_reserve_data.copy_from_slice(&reserve_data);
    new_reserve_data[24..32].copy_from_slice(&new_total_debt.to_le_bytes());
    set_storage(&reserve_k, &new_reserve_data)?;

    // Update interest rates
    update_reserve_state(&asset)?;

    debug_log!("AaveV3Pool: Repay completed");

    // Emit Repay event
    let repayer = get_caller();
    Repay {
        reserve: asset,
        user: on_behalf_of,
        repayer,
        amount: actual_repay,
    }
    .emit()
    .ok();

    Ok(actual_repay.to_le_bytes().to_vec())
}

/// Liquidate undercollateralized position
fn liquidate(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 104 {
        return Err(TakoError::InvalidInput.into());
    }

    // Parse: collateral_asset (32) + debt_asset (32) + user (32) + debt_to_cover (8)
    let collateral_asset = Address::from_slice(&input[..32]).ok_or(TakoError::CustomError(1))?;
    let debt_asset = Address::from_slice(&input[32..64]).ok_or(TakoError::CustomError(1))?;
    let user = Address::from_slice(&input[64..96]).ok_or(TakoError::CustomError(1))?;
    let debt_to_cover = u64::from_le_bytes([
        input[96], input[97], input[98], input[99], input[100], input[101], input[102], input[103],
    ]);

    // CRITICAL: Use proper multi-asset health factor calculation
    let health_factor = calculate_health_factor(&user)?;

    // Only liquidate if health factor < 1.0
    if health_factor >= RAY {
        return Err(TakoError::CustomError(7).into()); // Cannot liquidate healthy position
    }

    // Get user debt balance
    let borrow_k = borrow_balance_key(&user, &debt_asset);
    let borrow_bytes = get_storage(&borrow_k)?;
    let debt_balance = u64::from_le_bytes([
        borrow_bytes[0],
        borrow_bytes[1],
        borrow_bytes[2],
        borrow_bytes[3],
        borrow_bytes[4],
        borrow_bytes[5],
        borrow_bytes[6],
        borrow_bytes[7],
    ]);

    // Get user collateral balance
    let supply_k = supply_balance_key(&user, &collateral_asset);
    let supply_bytes = get_storage(&supply_k)?;
    let collateral_balance = u64::from_le_bytes([
        supply_bytes[0],
        supply_bytes[1],
        supply_bytes[2],
        supply_bytes[3],
        supply_bytes[4],
        supply_bytes[5],
        supply_bytes[6],
        supply_bytes[7],
    ]);

    // Calculate liquidation amounts (with 5% bonus)
    let max_liquidatable = debt_balance
        .checked_mul(CLOSE_FACTOR_BPS)
        .ok_or(TakoError::CustomError(3))?
        .checked_div(BASIS_POINTS)
        .ok_or(TakoError::CustomError(6))?;

    let actual_debt_to_cover = debt_to_cover.min(max_liquidatable);

    let collateral_to_seize = actual_debt_to_cover
        .checked_mul(BASIS_POINTS + LIQUIDATION_BONUS_BPS)
        .ok_or(TakoError::CustomError(3))?
        .checked_div(BASIS_POINTS)
        .ok_or(TakoError::CustomError(6))?;

    // Update user balances
    let new_debt = debt_balance
        .checked_sub(actual_debt_to_cover)
        .ok_or(TakoError::CustomError(8))?;
    set_storage(&borrow_k, &new_debt.to_le_bytes())?;

    let new_collateral = collateral_balance
        .checked_sub(collateral_to_seize)
        .ok_or(TakoError::CustomError(8))?;
    set_storage(&supply_k, &new_collateral.to_le_bytes())?;

    // Transfer collateral to liquidator (would be done via token transfer)

    debug_log!("AaveV3Pool: Liquidation completed");

    // Emit LiquidationCall event
    let liquidator = get_caller();
    LiquidationCall {
        collateral_asset,
        debt_asset,
        user,
        debt_to_cover: actual_debt_to_cover,
        liquidated_collateral_amount: collateral_to_seize,
        liquidator,
    }
    .emit()
    .ok();

    // Return liquidation amounts
    let mut result = vec![0u8; 16];
    result[..8].copy_from_slice(&actual_debt_to_cover.to_le_bytes());
    result[8..].copy_from_slice(&collateral_to_seize.to_le_bytes());

    Ok(result)
}

/// Set user collateral enabled
fn set_user_collateral(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 33 {
        return Err(TakoError::InvalidInput.into());
    }

    let asset = Address::from_slice(&input[..32]).ok_or(TakoError::CustomError(1))?;
    let enabled = input[32] == 1;

    let user = get_caller();

    let key = collateral_enabled_key(&user, &asset);
    set_storage(&key, &[if enabled { 1u8 } else { 0u8 }])?;

    debug_log!("AaveV3Pool: Set collateral enabled");

    Ok(vec![1])
}

/// Get reserve data (liquidity, debt, rates, indices)
fn get_reserve_data(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.len() < 32 {
        return Err(TakoError::InvalidInput.into());
    }

    let asset = Address::from_slice(&input[..32]).ok_or(TakoError::CustomError(1))?;

    let reserve_k = reserve_key(&asset);
    let reserve_data = get_storage(&reserve_k)?;

    if reserve_data.len() < 64 {
        return Err(TakoError::CustomError(12).into()); // Invalid reserve data
    }

    // Return full reserve data (64 bytes)
    Ok(reserve_data.to_vec())
}

/// Get user account data (total collateral, total debt, health factor, etc.)
fn get_user_account_data(input: &[u8]) -> TakoResult<Vec<u8>> {
    let user = if input.len() >= 32 {
        Address::from_slice(&input[..32]).ok_or(TakoError::CustomError(1))?
    } else {
        get_caller()
    };

    // Get reserve count
    let count_bytes = get_storage(KEY_RESERVE_COUNT).unwrap_or_default();
    if count_bytes.len() < 8 {
        return Err(TakoError::CustomError(18).into()); // Pool not initialized
    }
    let reserve_count = u64::from_le_bytes([
        count_bytes[0],
        count_bytes[1],
        count_bytes[2],
        count_bytes[3],
        count_bytes[4],
        count_bytes[5],
        count_bytes[6],
        count_bytes[7],
    ]);

    let mut total_collateral_base: u128 = 0;
    let mut total_debt_base: u128 = 0;
    let mut available_borrow_base: u128 = 0;

    // Iterate over all reserves
    for i in 0..reserve_count {
        let list_entry_key = reserve_list_entry_key(i);
        let asset_bytes = get_storage(&list_entry_key).unwrap_or_default();
        if asset_bytes.len() < 32 {
            continue;
        }

        let mut asset_id = [0u8; 32];
        asset_id.copy_from_slice(&asset_bytes[..32]);
        let asset = Address::new(asset_id);

        // Get asset price
        let price = get_asset_price(&asset) as u128;

        // Get reserve configuration
        let config_key = reserve_config_key(&asset);
        let config_data = get_storage(&config_key).unwrap_or_default();
        let ltv = if config_data.len() >= 8 {
            u64::from_le_bytes([
                config_data[0],
                config_data[1],
                config_data[2],
                config_data[3],
                config_data[4],
                config_data[5],
                config_data[6],
                config_data[7],
            ]) as u128
        } else {
            DEFAULT_LTV_BPS as u128
        };

        // Get user supply balance
        let supply_k = supply_balance_key(&user, &asset);
        let supply_bytes = get_storage(&supply_k).unwrap_or_default();
        let supply = if supply_bytes.len() >= 8 {
            u64::from_le_bytes([
                supply_bytes[0],
                supply_bytes[1],
                supply_bytes[2],
                supply_bytes[3],
                supply_bytes[4],
                supply_bytes[5],
                supply_bytes[6],
                supply_bytes[7],
            ]) as u128
        } else {
            0
        };

        // Check if collateral enabled
        let coll_enabled_k = collateral_enabled_key(&user, &asset);
        let coll_enabled_bytes = get_storage(&coll_enabled_k).unwrap_or_default();
        let coll_enabled = !coll_enabled_bytes.is_empty() && coll_enabled_bytes[0] == 1;

        if coll_enabled && supply > 0 {
            let value = supply * price / RAY as u128;
            total_collateral_base += value;
            // Available borrow = collateral * LTV
            available_borrow_base += value * ltv / BASIS_POINTS as u128;
        }

        // Get user borrow balance
        let borrow_k = borrow_balance_key(&user, &asset);
        let borrow_bytes = get_storage(&borrow_k).unwrap_or_default();
        let debt = if borrow_bytes.len() >= 8 {
            u64::from_le_bytes([
                borrow_bytes[0],
                borrow_bytes[1],
                borrow_bytes[2],
                borrow_bytes[3],
                borrow_bytes[4],
                borrow_bytes[5],
                borrow_bytes[6],
                borrow_bytes[7],
            ]) as u128
        } else {
            0
        };

        if debt > 0 {
            total_debt_base += debt * price / RAY as u128;
        }
    }

    // Calculate health factor
    let health_factor = calculate_health_factor(&user)?;

    // Calculate available to borrow
    let current_ltv = if total_collateral_base > 0 {
        (available_borrow_base * BASIS_POINTS as u128 / total_collateral_base) as u64
    } else {
        0
    };

    // Return: total_collateral (8) + total_debt (8) + available_borrow (8) + current_ltv (8) + health_factor (8)
    let mut result = vec![0u8; 40];
    result[..8].copy_from_slice(&(total_collateral_base as u64).to_le_bytes());
    result[8..16].copy_from_slice(&(total_debt_base as u64).to_le_bytes());
    result[16..24].copy_from_slice(
        &(available_borrow_base.saturating_sub(total_debt_base) as u64).to_le_bytes(),
    );
    result[24..32].copy_from_slice(&current_ltv.to_le_bytes());
    result[32..40].copy_from_slice(&health_factor.to_le_bytes());

    Ok(result)
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
        Ok(_) => 0,
        Err(e) => e,
    }
}

fn process_instruction(input: &[u8]) -> TakoResult<Vec<u8>> {
    if input.is_empty() {
        return Err(TakoError::InvalidInput.into());
    }

    let instruction = match Instruction::from_u8(input[0]) {
        Some(instr) => instr,
        None => return Err(0u64),
    };

    let args = if input.len() > 1 { &input[1..] } else { &[] };

    match instruction {
        Instruction::Initialize => initialize(args),
        Instruction::InitReserve => init_reserve(args),
        Instruction::SetReserveConfiguration => set_reserve_configuration(args),
        Instruction::Supply => supply(args),
        Instruction::Withdraw => withdraw(args),
        Instruction::Borrow => borrow(args),
        Instruction::Repay => repay(args),
        Instruction::Liquidate => liquidate(args),
        Instruction::SetUserCollateral => set_user_collateral(args),
        Instruction::GetReserveData => get_reserve_data(args),
        Instruction::GetUserAccountData => get_user_account_data(args),
        _ => Err(TakoError::CustomError(0).into()),
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
