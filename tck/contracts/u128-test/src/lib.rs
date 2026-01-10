//! U128 Test Smart Contract for TAKO
//!
//! Simple test to verify u128 operations work with i128:128 alignment.

#![no_std]
#![no_main]

use tako_sdk::*;

/// Main contract entrypoint
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== U128 Simple Test ===");

    // Test 1: Basic u128 variable
    log("Test 1: Basic u128 value");
    let a: u128 = 12345;
    let b: u128 = 67890;
    let sum = a + b;

    log("Test 2: u128 arithmetic");
    let c: u128 = 1000;
    let d: u128 = 500;
    let diff = c - d;
    let prod = c * 2;

    log("Test 3: i128 signed values");
    let pos: i128 = 42;
    let neg: i128 = -24;
    let _result = pos + neg;

    log("=== All basic u128 tests completed ===");

    // Return success
    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
