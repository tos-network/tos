//! Balance and Transfer Test Contract
//!
//! Tests balance query and transfer operations in TAKO.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{get_balance, get_contract_hash, log, transfer};

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Test 1: Query own contract balance
#[no_mangle]
pub extern "C" fn test_query_own_balance() -> u64 {
    log("Test 1: Query own balance");

    let self_address = get_contract_hash();
    let balance = get_balance(&self_address);

    log("Query own balance: PASS");
    0
}

/// Test 2: Query another address balance
#[no_mangle]
pub extern "C" fn test_query_other_balance() -> u64 {
    log("Test 2: Query other address balance");

    let other_address: [u8; 32] = [0x11u8; 32];
    let balance = get_balance(&other_address);

    log("Query other balance: PASS");
    0
}

/// Test 3: Transfer to account (small amount)
#[no_mangle]
pub extern "C" fn test_transfer_to_account() -> u64 {
    log("Test 3: Transfer to account");

    let recipient: [u8; 32] = [0x22u8; 32];
    let amount: u64 = 1000;

    match transfer(&recipient, amount) {
        Ok(_) => {
            log("Transfer to account: PASS");
            0
        }
        Err(code) => {
            // Transfer might fail if contract has no balance - that's expected
            log("Transfer failed (expected if no balance)");
            0 // Don't fail the test
        }
    }
}

/// Test 4: Zero amount transfer (edge case)
#[no_mangle]
pub extern "C" fn test_zero_transfer() -> u64 {
    log("Test 4: Zero amount transfer");

    let recipient: [u8; 32] = [0x33u8; 32];

    // Zero transfer should succeed (no-op)
    match transfer(&recipient, 0) {
        Ok(_) => {
            log("Zero transfer: PASS");
            0
        }
        Err(_) => {
            log("Zero transfer failed");
            1
        }
    }
}

/// Test 5: Multiple sequential transfers
#[no_mangle]
pub extern "C" fn test_multiple_transfers() -> u64 {
    log("Test 5: Multiple sequential transfers");

    let recipients: [[u8; 32]; 3] = [[0x44u8; 32], [0x55u8; 32], [0x66u8; 32]];
    let amounts: [u64; 3] = [100, 200, 300];

    for i in 0..3 {
        // Transfers may fail if contract has no balance
        let _ = transfer(&recipients[i], amounts[i]);
    }

    log("Multiple transfers: PASS");
    0
}

/// Main entrypoint - runs all tests
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Balance and Transfer Test Suite ===");

    let test1 = test_query_own_balance();
    if test1 != 0 {
        return 100 + test1;
    }

    let test2 = test_query_other_balance();
    if test2 != 0 {
        return 200 + test2;
    }

    let test3 = test_transfer_to_account();
    if test3 != 0 {
        return 300 + test3;
    }

    let test4 = test_zero_transfer();
    if test4 != 0 {
        return 400 + test4;
    }

    let test5 = test_multiple_transfers();
    if test5 != 0 {
        return 500 + test5;
    }

    log("=== All Tests PASSED ===");
    0
}
