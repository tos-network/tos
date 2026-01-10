//! Environment Information Test Contract
//!
//! This contract tests access to execution environment information.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{get_caller, log, storage_read, storage_write};

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Test 1: Get caller address
#[no_mangle]
pub extern "C" fn test_get_caller() -> u64 {
    log("Test 1: Get caller address");
    let caller = get_caller();

    // Log first byte for debugging
    if caller[0] == 0 && caller[1] == 0 && caller[2] == 0 && caller[3] == 0 {
        log("Warning: Caller starts with zeros");
    }

    log("Get caller: PASS");
    0
}

/// Test 2: Verify caller is non-zero
#[no_mangle]
pub extern "C" fn test_caller_nonzero() -> u64 {
    log("Test 2: Verify caller is non-zero");
    let caller = get_caller();

    let mut is_zero = true;
    for &byte in &caller {
        if byte != 0 {
            is_zero = false;
            break;
        }
    }

    if is_zero {
        log("Warning: Caller is zero address");
    } else {
        log("Caller is non-zero");
    }

    log("Caller non-zero check: PASS");
    0
}

/// Test 3: Store and retrieve caller
#[no_mangle]
pub extern "C" fn test_store_caller() -> u64 {
    log("Test 3: Store and retrieve caller");

    let caller = get_caller();
    let storage_key: [u8; 32] = [0x01u8; 32];

    // Store caller in contract storage
    if storage_write(&storage_key, &caller).is_err() {
        log("Failed to store caller");
        return 1;
    }

    // Retrieve caller from storage
    let mut retrieved_caller: [u8; 32] = [0u8; 32];
    let bytes_read = storage_read(&storage_key, &mut retrieved_caller);

    if bytes_read == 0 {
        log("Storage key not found");
        return 2;
    }

    // Verify they match
    if caller != retrieved_caller {
        log("Stored caller doesn't match retrieved caller");
        return 3;
    }

    log("Store caller: PASS");
    0
}

/// Test 4: Access control pattern (owner check)
#[no_mangle]
pub extern "C" fn test_access_control() -> u64 {
    log("Test 4: Access control simulation");

    let caller = get_caller();
    let owner_key: [u8; 32] = [0x02u8; 32];

    // Store caller as owner
    if storage_write(&owner_key, &caller).is_err() {
        log("Failed to store owner");
        return 1;
    }

    // Check if caller is owner
    let mut stored_owner: [u8; 32] = [0u8; 32];
    let bytes_read = storage_read(&owner_key, &mut stored_owner);

    if bytes_read == 0 {
        log("Owner not found in storage");
        return 2;
    }

    if caller == stored_owner {
        log("Access granted: Caller is owner");
    } else {
        log("Access denied: Caller is not owner");
        return 3;
    }

    log("Access control: PASS");
    0
}

/// Test 5: Multiple caller queries (consistency)
#[no_mangle]
pub extern "C" fn test_caller_consistency() -> u64 {
    log("Test 5: Caller consistency");

    let caller1 = get_caller();
    let caller2 = get_caller();
    let caller3 = get_caller();

    if caller1 != caller2 || caller2 != caller3 {
        log("Caller changed between calls");
        return 1;
    }

    log("Caller is consistent across calls");
    log("Caller consistency: PASS");
    0
}

/// Main entrypoint - runs all tests
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Environment Information Test Suite ===");

    let test1 = test_get_caller();
    if test1 != 0 {
        return 100 + test1;
    }

    let test2 = test_caller_nonzero();
    if test2 != 0 {
        return 200 + test2;
    }

    let test3 = test_store_caller();
    if test3 != 0 {
        return 300 + test3;
    }

    let test4 = test_access_control();
    if test4 != 0 {
        return 400 + test4;
    }

    let test5 = test_caller_consistency();
    if test5 != 0 {
        return 500 + test5;
    }

    log("=== All Tests PASSED ===");
    0
}
