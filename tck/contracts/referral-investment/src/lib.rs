//! Referral Investment Contract
//!
//! A 3-level profit sharing contract that demonstrates the use of TAKO's
//! native referral syscalls. When a user invests, rewards are automatically
//! distributed to up to 3 levels of uplines.
//!
//! # Reward Structure (3 levels max for legal compliance)
//!
//! | Level | Ratio | Description |
//! |-------|-------|-------------|
//! | 1 | 10% | Direct referrer |
//! | 2 | 5% | Referrer's referrer |
//! | 3 | 3% | Third level |
//!
//! # Entry Points
//!
//! - `invest` - User invests with profit sharing to uplines
//! - `check_referrer` - Check if user has a referrer
//! - `get_upline_info` - Get user's upline chain
//! - `get_team_stats` - Get team statistics

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{
    get_call_value, get_caller, get_contract_hash, get_direct_referrals_count, get_referral_level,
    get_referrer, get_team_size, get_uplines, has_referrer, is_downline, log, set_return_data,
    transfer,
};

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

// ============================================================================
// Constants
// ============================================================================

/// Maximum referral levels for profit sharing (legal compliance: 3 levels)
const MAX_PROFIT_LEVELS: u8 = 3;

/// Reward ratios in basis points (1/10000)
/// Level 1: 10%, Level 2: 5%, Level 3: 3%
const REWARD_RATIOS: [u64; 3] = [1000, 500, 300];

/// Basis points denominator
const BASIS_POINTS: u64 = 10000;

// ============================================================================
// Main Entry Points
// ============================================================================

/// Main investment entry point
///
/// When called with value, distributes rewards to up to 3 levels of uplines.
/// Returns the number of uplines rewarded.
#[no_mangle]
pub extern "C" fn invest() -> u64 {
    log("=== Referral Investment: invest() ===");

    // Get caller and investment amount
    let caller = get_caller();
    let amount = get_call_value();

    if amount == 0 {
        log("Error: Investment amount is zero");
        return 1;
    }

    log("Processing investment with profit sharing...");

    // Get up to 3 levels of uplines
    let uplines_result = match get_uplines(&caller, MAX_PROFIT_LEVELS) {
        Ok(result) => result,
        Err(code) => {
            log("Error: Failed to get uplines");
            return 100 + code;
        }
    };

    let levels_found = uplines_result.levels_returned as usize;
    log("Found upline levels");

    // Distribute rewards to each level
    let mut total_distributed: u64 = 0;
    let mut rewards_sent: u8 = 0;

    for i in 0..levels_found {
        if i >= REWARD_RATIOS.len() {
            break;
        }

        let upline = &uplines_result.as_slice()[i];
        let reward = calculate_reward(amount, REWARD_RATIOS[i]);

        if reward > 0 {
            match transfer(upline, reward) {
                Ok(_) => {
                    total_distributed = total_distributed.saturating_add(reward);
                    rewards_sent = rewards_sent.saturating_add(1);
                    log("Reward transferred to upline");
                }
                Err(_) => {
                    log("Warning: Transfer failed for upline");
                }
            }
        }
    }

    log("Investment processed successfully");

    // Return number of uplines rewarded
    rewards_sent as u64
}

/// Check if caller has a referrer
///
/// Returns: 1 if has referrer, 0 if not, error code on failure
#[no_mangle]
pub extern "C" fn check_referrer() -> u64 {
    log("=== Referral Investment: check_referrer() ===");

    let caller = get_caller();

    match has_referrer(&caller) {
        Ok(has_ref) => {
            if has_ref {
                log("Caller has a referrer");
                1
            } else {
                log("Caller has no referrer");
                0
            }
        }
        Err(code) => {
            log("Error: Failed to check referrer");
            100 + code
        }
    }
}

/// Get detailed upline information for caller
///
/// Writes upline data to return data and returns levels found
#[no_mangle]
pub extern "C" fn get_upline_info() -> u64 {
    log("=== Referral Investment: get_upline_info() ===");

    let caller = get_caller();

    // Get caller's level in the tree
    let level = match get_referral_level(&caller) {
        Ok(l) => l,
        Err(_) => 0,
    };

    // Get uplines
    let uplines_result = match get_uplines(&caller, MAX_PROFIT_LEVELS) {
        Ok(result) => result,
        Err(code) => {
            log("Error: Failed to get uplines");
            return 100 + code;
        }
    };

    log("Upline query successful");

    // Build return data: [level: 1 byte][count: 1 byte][uplines: 32 bytes each]
    let count = uplines_result.levels_returned;
    let mut return_data = [0u8; 98]; // 1 + 1 + 32*3 = 98 bytes max
    return_data[0] = level;
    return_data[1] = count;

    for i in 0..(count as usize).min(3) {
        let offset = 2 + i * 32;
        return_data[offset..offset + 32].copy_from_slice(&uplines_result.as_slice()[i]);
    }

    let data_len = 2 + (count as usize).min(3) * 32;
    let _ = set_return_data(&return_data[..data_len]);

    count as u64
}

/// Get team statistics for caller
///
/// Returns team size in return data
#[no_mangle]
pub extern "C" fn get_team_stats() -> u64 {
    log("=== Referral Investment: get_team_stats() ===");

    let caller = get_caller();

    // Get direct referrals count
    let direct_count = match get_direct_referrals_count(&caller) {
        Ok(count) => count,
        Err(_) => 0,
    };

    // Get total team size
    let team_size = match get_team_size(&caller) {
        Ok(size) => size,
        Err(_) => 0,
    };

    // Get referral level
    let level = match get_referral_level(&caller) {
        Ok(l) => l,
        Err(_) => 0,
    };

    log("Team stats query successful");

    // Build return data: [level: 1][direct_count: 4][team_size: 8]
    let mut return_data = [0u8; 13];
    return_data[0] = level;
    return_data[1..5].copy_from_slice(&direct_count.to_le_bytes());
    return_data[5..13].copy_from_slice(&team_size.to_le_bytes());

    let _ = set_return_data(&return_data);

    team_size
}

/// Check if one address is a downline of another
///
/// Input: [ancestor: 32][descendant: 32]
/// Returns: 1 if is downline, 0 if not
#[no_mangle]
pub extern "C" fn check_downline() -> u64 {
    log("=== Referral Investment: check_downline() ===");

    // For this example, we'll check if caller is downline of contract owner
    let caller = get_caller();
    let contract = get_contract_hash();

    match is_downline(&contract, &caller, MAX_PROFIT_LEVELS) {
        Ok(is_down) => {
            if is_down {
                log("Caller is a downline of contract");
                1
            } else {
                log("Caller is NOT a downline of contract");
                0
            }
        }
        Err(code) => {
            log("Error: is_downline check failed");
            100 + code
        }
    }
}

/// Test all referral syscalls
///
/// Comprehensive test entry point for integration testing
#[no_mangle]
pub extern "C" fn test_all_syscalls() -> u64 {
    log("=== Referral Investment: test_all_syscalls() ===");

    let caller = get_caller();

    // Test 1: has_referrer
    log("Test 1: has_referrer");
    let test1 = match has_referrer(&caller) {
        Ok(_) => {
            log("has_referrer: PASS");
            0
        }
        Err(code) => {
            log("has_referrer: FAIL");
            100 + code
        }
    };
    if test1 != 0 {
        return test1;
    }

    // Test 2: get_referrer
    log("Test 2: get_referrer");
    let test2 = match get_referrer(&caller) {
        Ok(_) => {
            log("get_referrer: PASS");
            0
        }
        Err(code) => {
            log("get_referrer: FAIL");
            200 + code
        }
    };
    if test2 != 0 {
        return test2;
    }

    // Test 3: get_uplines
    log("Test 3: get_uplines");
    let test3 = match get_uplines(&caller, 3) {
        Ok(_result) => {
            log("get_uplines: PASS");
            0
        }
        Err(code) => {
            log("get_uplines: FAIL");
            300 + code
        }
    };
    if test3 != 0 {
        return test3;
    }

    // Test 4: get_direct_referrals_count
    log("Test 4: get_direct_referrals_count");
    let test4 = match get_direct_referrals_count(&caller) {
        Ok(_) => {
            log("get_direct_referrals_count: PASS");
            0
        }
        Err(code) => {
            log("get_direct_referrals_count: FAIL");
            400 + code
        }
    };
    if test4 != 0 {
        return test4;
    }

    // Test 5: get_team_size
    log("Test 5: get_team_size");
    let test5 = match get_team_size(&caller) {
        Ok(_) => {
            log("get_team_size: PASS");
            0
        }
        Err(code) => {
            log("get_team_size: FAIL");
            500 + code
        }
    };
    if test5 != 0 {
        return test5;
    }

    // Test 6: get_referral_level
    log("Test 6: get_referral_level");
    let test6 = match get_referral_level(&caller) {
        Ok(_) => {
            log("get_referral_level: PASS");
            0
        }
        Err(code) => {
            log("get_referral_level: FAIL");
            600 + code
        }
    };
    if test6 != 0 {
        return test6;
    }

    // Test 7: is_downline
    log("Test 7: is_downline");
    let contract = get_contract_hash();
    let test7 = match is_downline(&contract, &caller, 10) {
        Ok(_) => {
            log("is_downline: PASS");
            0
        }
        Err(code) => {
            log("is_downline: FAIL");
            700 + code
        }
    };
    if test7 != 0 {
        return test7;
    }

    log("=== All Referral Syscall Tests PASSED ===");
    0
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Calculate reward amount from basis points
#[inline]
fn calculate_reward(amount: u64, basis_points: u64) -> u64 {
    // Safe calculation: amount * basis_points / 10000
    // Use checked arithmetic to prevent overflow
    amount
        .checked_mul(basis_points)
        .map(|v| v / BASIS_POINTS)
        .unwrap_or(0)
}

// ============================================================================
// Default Entry Point
// ============================================================================

/// Default entrypoint - runs syscall tests
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    test_all_syscalls()
}
