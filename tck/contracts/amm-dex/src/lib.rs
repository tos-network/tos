//! # Automated Market Maker (AMM) / DEX Example
//!
//! This example demonstrates a complete implementation of an Automated Market Maker,
//! similar to Uniswap V2. It uses the constant product formula (x * y = k) for pricing
//! and implements liquidity pools with LP token rewards.
//!
//! ## Key Features
//!
//! - **Constant Product AMM**: Uses x * y = k formula for automatic pricing
//! - **Liquidity Pools**: Add/remove liquidity with LP tokens
//! - **Token Swaps**: Swap between token pairs with automatic pricing
//! - **Fee Collection**: 0.3% trading fee (Uniswap standard)
//! - **LP Tokens**: Proportional ownership of pool liquidity
//! - **Slippage Protection**: Minimum output amounts prevent sandwich attacks

#![no_std]
#![no_main]

use tako_sdk::*;

/// Address type (32-byte hash)
pub type Address = [u8; 32];

/// Fee in basis points (30 = 0.3%)
pub const FEE_BASIS_POINTS: u64 = 30;
pub const BASIS_POINTS: u64 = 10000;

/// Minimum liquidity locked forever (prevents division by zero)
pub const MINIMUM_LIQUIDITY: u64 = 1000;

// Storage key prefixes
const KEY_TOKEN_A: &[u8] = b"token_a";
const KEY_TOKEN_B: &[u8] = b"token_b";
const KEY_RESERVE_A: &[u8] = b"reserve_a";
const KEY_RESERVE_B: &[u8] = b"reserve_b";
const KEY_TOTAL_LP: &[u8] = b"total_lp";
const KEY_FEE_A: &[u8] = b"fee_a";
const KEY_FEE_B: &[u8] = b"fee_b";
const KEY_LP_PREFIX: &[u8] = b"lp:";

// Error codes
define_errors! {
    SameTokens = 1001,
    ZeroLiquidity = 1002,
    InsufficientInitialLiquidity = 1003,
    InsufficientLpTokens = 1004,
    SlippageExceeded = 1005,
    ZeroSwapAmount = 1006,
    PoolNotInitialized = 1007,
    StorageError = 1008,
}

entrypoint!(process_instruction);

fn process_instruction(input: &[u8]) -> entrypoint::Result<()> {
    if input.is_empty() {
        return Ok(());
    }

    match input[0] {
        // Initialize pool: [0, token_a[32], token_b[32]]
        0 => {
            if input.len() < 65 {
                return Err(StorageError);
            }
            let mut token_a = [0u8; 32];
            let mut token_b = [0u8; 32];
            token_a.copy_from_slice(&input[1..33]);
            token_b.copy_from_slice(&input[33..65]);
            init_pool(&token_a, &token_b)
        }
        // Add liquidity: [1, provider[32], amount_a[8], amount_b[8], min_lp[8]]
        1 => {
            if input.len() < 57 {
                return Err(StorageError);
            }
            let mut provider = [0u8; 32];
            provider.copy_from_slice(&input[1..33]);
            let amount_a = u64::from_le_bytes(input[33..41].try_into().unwrap());
            let amount_b = u64::from_le_bytes(input[41..49].try_into().unwrap());
            let min_lp = u64::from_le_bytes(input[49..57].try_into().unwrap());
            add_liquidity(&provider, amount_a, amount_b, min_lp)
        }
        // Remove liquidity: [2, provider[32], lp_tokens[8], min_a[8], min_b[8]]
        2 => {
            if input.len() < 57 {
                return Err(StorageError);
            }
            let mut provider = [0u8; 32];
            provider.copy_from_slice(&input[1..33]);
            let lp_tokens = u64::from_le_bytes(input[33..41].try_into().unwrap());
            let min_a = u64::from_le_bytes(input[41..49].try_into().unwrap());
            let min_b = u64::from_le_bytes(input[49..57].try_into().unwrap());
            remove_liquidity(&provider, lp_tokens, min_a, min_b)
        }
        // Swap A for B: [3, trader[32], amount_in[8], min_out[8]]
        3 => {
            if input.len() < 49 {
                return Err(StorageError);
            }
            let amount_in = u64::from_le_bytes(input[33..41].try_into().unwrap());
            let min_out = u64::from_le_bytes(input[41..49].try_into().unwrap());
            swap_a_for_b(amount_in, min_out)
        }
        // Swap B for A: [4, trader[32], amount_in[8], min_out[8]]
        4 => {
            if input.len() < 49 {
                return Err(StorageError);
            }
            let amount_in = u64::from_le_bytes(input[33..41].try_into().unwrap());
            let min_out = u64::from_le_bytes(input[41..49].try_into().unwrap());
            swap_b_for_a(amount_in, min_out)
        }
        _ => Ok(()),
    }
}

/// Initialize a new AMM pool
fn init_pool(token_a: &Address, token_b: &Address) -> entrypoint::Result<()> {
    if token_a == token_b {
        return Err(SameTokens);
    }

    storage_write(KEY_TOKEN_A, token_a).map_err(|_| StorageError)?;
    storage_write(KEY_TOKEN_B, token_b).map_err(|_| StorageError)?;
    storage_write(KEY_RESERVE_A, &0u64.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_RESERVE_B, &0u64.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_TOTAL_LP, &0u64.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_FEE_A, &0u64.to_le_bytes()).map_err(|_| StorageError)?;
    storage_write(KEY_FEE_B, &0u64.to_le_bytes()).map_err(|_| StorageError)?;

    log("AMM Pool initialized");
    Ok(())
}

/// Add liquidity to the pool
fn add_liquidity(
    provider: &Address,
    amount_a: u64,
    amount_b: u64,
    min_lp_tokens: u64,
) -> entrypoint::Result<()> {
    if amount_a == 0 || amount_b == 0 {
        return Err(ZeroLiquidity);
    }

    let reserve_a = read_u64(KEY_RESERVE_A);
    let reserve_b = read_u64(KEY_RESERVE_B);
    let total_supply = read_u64(KEY_TOTAL_LP);

    let lp_tokens = if total_supply == 0 {
        // Initial liquidity: LP tokens = sqrt(amount_a * amount_b)
        let liquidity = sqrt((amount_a as u128) * (amount_b as u128)) as u64;
        if liquidity <= MINIMUM_LIQUIDITY {
            return Err(InsufficientInitialLiquidity);
        }
        liquidity - MINIMUM_LIQUIDITY
    } else {
        // Subsequent liquidity: proportional to existing reserves
        let lp_from_a = (amount_a as u128 * total_supply as u128) / reserve_a as u128;
        let lp_from_b = (amount_b as u128 * total_supply as u128) / reserve_b as u128;
        lp_from_a.min(lp_from_b) as u64
    };

    if lp_tokens < min_lp_tokens {
        return Err(SlippageExceeded);
    }

    // Update reserves
    storage_write(KEY_RESERVE_A, &(reserve_a + amount_a).to_le_bytes())
        .map_err(|_| StorageError)?;
    storage_write(KEY_RESERVE_B, &(reserve_b + amount_b).to_le_bytes())
        .map_err(|_| StorageError)?;

    // Mint LP tokens
    let new_supply = if total_supply == 0 {
        lp_tokens + MINIMUM_LIQUIDITY
    } else {
        total_supply + lp_tokens
    };
    storage_write(KEY_TOTAL_LP, &new_supply.to_le_bytes()).map_err(|_| StorageError)?;

    // Update provider's LP balance
    let provider_balance = read_lp_balance(provider);
    write_lp_balance(provider, provider_balance + lp_tokens)?;

    log("Liquidity added");
    Ok(())
}

/// Remove liquidity from the pool
fn remove_liquidity(
    provider: &Address,
    lp_tokens: u64,
    min_amount_a: u64,
    min_amount_b: u64,
) -> entrypoint::Result<()> {
    if lp_tokens == 0 {
        return Err(ZeroLiquidity);
    }

    let provider_balance = read_lp_balance(provider);
    if provider_balance < lp_tokens {
        return Err(InsufficientLpTokens);
    }

    let reserve_a = read_u64(KEY_RESERVE_A);
    let reserve_b = read_u64(KEY_RESERVE_B);
    let total_supply = read_u64(KEY_TOTAL_LP);

    // Calculate token amounts
    let amount_a = (lp_tokens as u128 * reserve_a as u128 / total_supply as u128) as u64;
    let amount_b = (lp_tokens as u128 * reserve_b as u128 / total_supply as u128) as u64;

    if amount_a < min_amount_a || amount_b < min_amount_b {
        return Err(SlippageExceeded);
    }

    // Update reserves
    storage_write(KEY_RESERVE_A, &(reserve_a - amount_a).to_le_bytes())
        .map_err(|_| StorageError)?;
    storage_write(KEY_RESERVE_B, &(reserve_b - amount_b).to_le_bytes())
        .map_err(|_| StorageError)?;

    // Burn LP tokens
    storage_write(KEY_TOTAL_LP, &(total_supply - lp_tokens).to_le_bytes())
        .map_err(|_| StorageError)?;
    write_lp_balance(provider, provider_balance - lp_tokens)?;

    log("Liquidity removed");
    Ok(())
}

/// Swap token A for token B
fn swap_a_for_b(amount_in: u64, min_amount_out: u64) -> entrypoint::Result<()> {
    if amount_in == 0 {
        return Err(ZeroSwapAmount);
    }

    let reserve_a = read_u64(KEY_RESERVE_A);
    let reserve_b = read_u64(KEY_RESERVE_B);

    if reserve_a == 0 || reserve_b == 0 {
        return Err(PoolNotInitialized);
    }

    // Calculate output with fee
    let amount_in_with_fee =
        (amount_in as u128 * (BASIS_POINTS - FEE_BASIS_POINTS) as u128) / BASIS_POINTS as u128;
    let numerator = amount_in_with_fee * reserve_b as u128;
    let denominator = reserve_a as u128 + amount_in_with_fee;
    let amount_out = (numerator / denominator) as u64;

    if amount_out < min_amount_out {
        return Err(SlippageExceeded);
    }

    // Calculate fee
    let fee = amount_in
        - ((amount_in as u128 * (BASIS_POINTS - FEE_BASIS_POINTS) as u128) / BASIS_POINTS as u128)
            as u64;

    // Update reserves
    storage_write(KEY_RESERVE_A, &(reserve_a + amount_in).to_le_bytes())
        .map_err(|_| StorageError)?;
    storage_write(KEY_RESERVE_B, &(reserve_b - amount_out).to_le_bytes())
        .map_err(|_| StorageError)?;

    // Update fee reserves
    let fee_a = read_u64(KEY_FEE_A);
    storage_write(KEY_FEE_A, &(fee_a + fee).to_le_bytes()).map_err(|_| StorageError)?;

    log("Swap A->B completed");
    Ok(())
}

/// Swap token B for token A
fn swap_b_for_a(amount_in: u64, min_amount_out: u64) -> entrypoint::Result<()> {
    if amount_in == 0 {
        return Err(ZeroSwapAmount);
    }

    let reserve_a = read_u64(KEY_RESERVE_A);
    let reserve_b = read_u64(KEY_RESERVE_B);

    if reserve_a == 0 || reserve_b == 0 {
        return Err(PoolNotInitialized);
    }

    // Calculate output with fee
    let amount_in_with_fee =
        (amount_in as u128 * (BASIS_POINTS - FEE_BASIS_POINTS) as u128) / BASIS_POINTS as u128;
    let numerator = amount_in_with_fee * reserve_a as u128;
    let denominator = reserve_b as u128 + amount_in_with_fee;
    let amount_out = (numerator / denominator) as u64;

    if amount_out < min_amount_out {
        return Err(SlippageExceeded);
    }

    // Calculate fee
    let fee = amount_in
        - ((amount_in as u128 * (BASIS_POINTS - FEE_BASIS_POINTS) as u128) / BASIS_POINTS as u128)
            as u64;

    // Update reserves
    storage_write(KEY_RESERVE_B, &(reserve_b + amount_in).to_le_bytes())
        .map_err(|_| StorageError)?;
    storage_write(KEY_RESERVE_A, &(reserve_a - amount_out).to_le_bytes())
        .map_err(|_| StorageError)?;

    // Update fee reserves
    let fee_b = read_u64(KEY_FEE_B);
    storage_write(KEY_FEE_B, &(fee_b + fee).to_le_bytes()).map_err(|_| StorageError)?;

    log("Swap B->A completed");
    Ok(())
}

// Helper functions

fn read_u64(key: &[u8]) -> u64 {
    let mut buffer = [0u8; 8];
    let len = storage_read(key, &mut buffer);
    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

fn read_lp_balance(address: &Address) -> u64 {
    // Create key: "lp:" + address
    let mut key = [0u8; 35]; // 3 + 32
    key[0..3].copy_from_slice(KEY_LP_PREFIX);
    key[3..35].copy_from_slice(address);
    read_u64(&key)
}

fn write_lp_balance(address: &Address, balance: u64) -> entrypoint::Result<()> {
    let mut key = [0u8; 35];
    key[0..3].copy_from_slice(KEY_LP_PREFIX);
    key[3..35].copy_from_slice(address);
    storage_write(&key, &balance.to_le_bytes()).map_err(|_| StorageError)
}

/// Integer square root (Babylonian method)
fn sqrt(y: u128) -> u128 {
    if y == 0 {
        return 0;
    }
    let mut z = y;
    let mut x = y / 2 + 1;
    while x < z {
        z = x;
        x = (y / x + x) / 2;
    }
    z
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
