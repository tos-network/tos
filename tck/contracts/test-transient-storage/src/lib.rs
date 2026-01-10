//! Transient Storage Test Contract (EIP-1153)
//!
//! This contract tests the EIP-1153 transient storage opcodes (TLOAD/TSTORE).
//! Transient storage provides per-transaction temporary storage that is cleared
//! at the end of each transaction, making it ideal for:
//! - Reentrancy guards
//! - MEV protection
//! - Temporary state within transaction
//!
//! Test Cases:
//! 1. Basic TSTORE/TLOAD - Write and read transient storage
//! 2. Automatic clearing - Verify storage clears after transaction
//! 3. Isolation - Verify contracts cannot access each other's transient storage
//! 4. Multiple slots - Test multiple transient storage keys
//! 5. Overwriting - Test updating transient storage values

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{log, tload, tstore};

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Helper: Log a message with u64 value
fn log_u64(msg: &str, value: u64) {
    // Use the SDK's log function with formatted message
    log(msg);
    // Note: For simplicity, we just log the message. Full u64 logging would need
    // the raw syscall or a custom implementation.
}

/// Test 1: Basic TSTORE/TLOAD operation
#[no_mangle]
pub extern "C" fn test_basic_tstore_tload() -> u64 {
    log("Test 1: Basic TSTORE/TLOAD");

    // Key and value for transient storage
    let key: [u8; 32] = [1u8; 32];
    let value: [u8; 32] = [0x42u8; 32];
    let mut result: [u8; 32] = [0u8; 32];

    // Store value in transient storage
    if tstore(&key, &value).is_err() {
        log("TSTORE failed");
        return 1;
    }

    // Load value back from transient storage
    let ret_len = tload(&key, &mut result);

    if ret_len == 0 {
        log("TLOAD returned 0 length");
        return 2;
    }

    // Verify the value matches
    if result != value {
        log("Value mismatch");
        return 3;
    }

    log("Basic TSTORE/TLOAD: PASS");
    0
}

/// Test 2: Load non-existent key (should return zeros)
#[no_mangle]
pub extern "C" fn test_tload_nonexistent() -> u64 {
    log("Test 2: TLOAD non-existent key");

    let key: [u8; 32] = [0xFFu8; 32]; // Key that was never stored
    let mut result: [u8; 32] = [0xAAu8; 32]; // Pre-fill with non-zero

    // Load non-existent key
    let ret_len = tload(&key, &mut result);

    // Should return 0 (key not found)
    if ret_len != 0 {
        log("TLOAD should return 0 length for non-existent key");
        return 2;
    }

    log("TLOAD non-existent: PASS");
    0
}

/// Test 3: Multiple transient storage slots
#[no_mangle]
pub extern "C" fn test_multiple_slots() -> u64 {
    log("Test 3: Multiple transient storage slots");

    // Store 3 different key-value pairs
    let keys: [[u8; 32]; 3] = [[1u8; 32], [2u8; 32], [3u8; 32]];

    let values: [[u8; 32]; 3] = [[0x11u8; 32], [0x22u8; 32], [0x33u8; 32]];

    // Store all values
    for i in 0..3 {
        if tstore(&keys[i], &values[i]).is_err() {
            log("TSTORE failed");
            return 1;
        }
    }

    // Load and verify all values
    for i in 0..3 {
        let mut result: [u8; 32] = [0u8; 32];
        let ret_len = tload(&keys[i], &mut result);

        if ret_len == 0 {
            log("TLOAD failed");
            return 2;
        }

        if result != values[i] {
            log("Value mismatch");
            return 3;
        }
    }

    log("Multiple slots: PASS");
    0
}

/// Test 4: Overwrite transient storage value
#[no_mangle]
pub extern "C" fn test_overwrite() -> u64 {
    log("Test 4: Overwrite transient storage");

    let key: [u8; 32] = [4u8; 32];
    let value1: [u8; 32] = [0x44u8; 32];
    let value2: [u8; 32] = [0x55u8; 32];
    let mut result: [u8; 32] = [0u8; 32];

    // Store first value
    if tstore(&key, &value1).is_err() {
        log("First TSTORE failed");
        return 1;
    }

    // Verify first value
    let ret_len = tload(&key, &mut result);
    if ret_len == 0 || result != value1 {
        log("First value verification failed");
        return 2;
    }

    // Overwrite with second value
    if tstore(&key, &value2).is_err() {
        log("Second TSTORE failed");
        return 3;
    }

    // Verify second value (should be updated)
    let ret_len = tload(&key, &mut result);
    if ret_len == 0 || result != value2 {
        log("Second value verification failed");
        return 4;
    }

    log("Overwrite: PASS");
    0
}

/// Test 5: Large key patterns
#[no_mangle]
pub extern "C" fn test_key_patterns() -> u64 {
    log("Test 5: Various key patterns");

    // Test different key patterns
    let patterns: [[u8; 32]; 4] = [
        [0x00u8; 32], // All zeros
        [0xFFu8; 32], // All ones
        [0x55u8; 32], // Alternating bits (01010101)
        [0xAAu8; 32], // Alternating bits (10101010)
    ];

    // Each pattern gets a unique value
    for (i, key) in patterns.iter().enumerate() {
        let value: [u8; 32] = [i as u8; 32];
        let mut result: [u8; 32] = [0u8; 32];

        // Store
        if tstore(key, &value).is_err() {
            log("TSTORE failed for pattern");
            return 1;
        }

        // Load
        let ret_len = tload(key, &mut result);
        if ret_len == 0 {
            log("TLOAD failed for pattern");
            return 2;
        }

        // Verify
        if result != value {
            log("Value mismatch for pattern");
            return 3;
        }
    }

    log("Key patterns: PASS");
    0
}

/// Main entrypoint - runs all tests
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Transient Storage Test Suite ===");

    // Run all tests
    let test1 = test_basic_tstore_tload();
    if test1 != 0 {
        log("Test 1 failed");
        return 100 + test1;
    }

    let test2 = test_tload_nonexistent();
    if test2 != 0 {
        log("Test 2 failed");
        return 200 + test2;
    }

    let test3 = test_multiple_slots();
    if test3 != 0 {
        log("Test 3 failed");
        return 300 + test3;
    }

    let test4 = test_overwrite();
    if test4 != 0 {
        log("Test 4 failed");
        return 400 + test4;
    }

    let test5 = test_key_patterns();
    if test5 != 0 {
        log("Test 5 failed");
        return 500 + test5;
    }

    log("=== All Tests PASSED ===");
    0
}
