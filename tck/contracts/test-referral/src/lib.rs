//! Simple Referral Syscalls Test Contract
//!
//! Tests the 7 referral syscalls without complex business logic.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{
    get_caller, get_direct_referrals_count, get_referral_level, get_referrer, get_team_size,
    get_uplines, has_referrer, is_downline, log,
};

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Test 1: has_referrer
#[no_mangle]
pub extern "C" fn test_has_referrer() -> u64 {
    log("Test 1: has_referrer");
    let caller = get_caller();
    match has_referrer(&caller) {
        Ok(_) => {
            log("has_referrer: PASS");
            0
        }
        Err(code) => {
            log("has_referrer: FAIL");
            100 + code
        }
    }
}

/// Test 2: get_referrer
#[no_mangle]
pub extern "C" fn test_get_referrer() -> u64 {
    log("Test 2: get_referrer");
    let caller = get_caller();
    match get_referrer(&caller) {
        Ok(_) => {
            log("get_referrer: PASS");
            0
        }
        Err(code) => {
            log("get_referrer: FAIL");
            200 + code
        }
    }
}

/// Test 3: get_uplines
#[no_mangle]
pub extern "C" fn test_get_uplines() -> u64 {
    log("Test 3: get_uplines");
    let caller = get_caller();
    match get_uplines(&caller, 3) {
        Ok(_) => {
            log("get_uplines: PASS");
            0
        }
        Err(code) => {
            log("get_uplines: FAIL");
            300 + code
        }
    }
}

/// Test 4: get_direct_referrals_count
#[no_mangle]
pub extern "C" fn test_get_direct_referrals_count() -> u64 {
    log("Test 4: get_direct_referrals_count");
    let caller = get_caller();
    match get_direct_referrals_count(&caller) {
        Ok(_) => {
            log("get_direct_referrals_count: PASS");
            0
        }
        Err(code) => {
            log("get_direct_referrals_count: FAIL");
            400 + code
        }
    }
}

/// Test 5: get_team_size
#[no_mangle]
pub extern "C" fn test_get_team_size() -> u64 {
    log("Test 5: get_team_size");
    let caller = get_caller();
    match get_team_size(&caller) {
        Ok(_) => {
            log("get_team_size: PASS");
            0
        }
        Err(code) => {
            log("get_team_size: FAIL");
            500 + code
        }
    }
}

/// Test 6: get_referral_level
#[no_mangle]
pub extern "C" fn test_get_referral_level() -> u64 {
    log("Test 6: get_referral_level");
    let caller = get_caller();
    match get_referral_level(&caller) {
        Ok(_) => {
            log("get_referral_level: PASS");
            0
        }
        Err(code) => {
            log("get_referral_level: FAIL");
            600 + code
        }
    }
}

/// Test 7: is_downline
#[no_mangle]
pub extern "C" fn test_is_downline() -> u64 {
    log("Test 7: is_downline");
    let caller = get_caller();
    let other = [0x11u8; 32];
    match is_downline(&other, &caller, 10) {
        Ok(_) => {
            log("is_downline: PASS");
            0
        }
        Err(code) => {
            log("is_downline: FAIL");
            700 + code
        }
    }
}

/// Main entrypoint - runs all tests sequentially
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Referral Syscalls Test Suite ===");

    let test1 = test_has_referrer();
    if test1 != 0 {
        return test1;
    }

    let test2 = test_get_referrer();
    if test2 != 0 {
        return test2;
    }

    let test3 = test_get_uplines();
    if test3 != 0 {
        return test3;
    }

    let test4 = test_get_direct_referrals_count();
    if test4 != 0 {
        return test4;
    }

    let test5 = test_get_team_size();
    if test5 != 0 {
        return test5;
    }

    let test6 = test_get_referral_level();
    if test6 != 0 {
        return test6;
    }

    let test7 = test_is_downline();
    if test7 != 0 {
        return test7;
    }

    log("=== All Tests PASSED ===");
    0
}
