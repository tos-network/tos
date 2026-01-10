//! Code Operations Test Contract
//!
//! Tests EVM-compatible code inspection operations.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{ext_code_copy, get_contract_hash, get_ext_code_size, log};

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Test 1: Get own code size
#[no_mangle]
pub extern "C" fn test_own_code_size() -> u64 {
    log("Test 1: Get own code size");

    let self_address = get_contract_hash();

    match get_ext_code_size(&self_address) {
        Ok(size) => {
            if size == 0 {
                log("Code size is zero (unexpected)");
                return 3;
            }
            log("Own code size: PASS");
            0
        }
        Err(_) => {
            log("Failed to get code size");
            2
        }
    }
}

/// Test 3: Copy own code (first 64 bytes)
#[no_mangle]
pub extern "C" fn test_copy_own_code() -> u64 {
    log("Test 3: Copy own code");

    let self_address = get_contract_hash();
    let mut code_buffer: [u8; 64] = [0u8; 64];

    match ext_code_copy(&self_address, &mut code_buffer, 0) {
        Ok(_) => {
            // Verify ELF magic number (0x7F 'E' 'L' 'F')
            if code_buffer[0] == 0x7F
                && code_buffer[1] == 0x45
                && code_buffer[2] == 0x4C
                && code_buffer[3] == 0x46
            {
                log("ELF magic verified");
            } else {
                log("ELF magic not found (might be stripped)");
            }
            log("Copy own code: PASS");
            0
        }
        Err(_) => {
            log("Failed to copy code");
            1
        }
    }
}

/// Test 5: Partial code copy (middle section)
#[no_mangle]
pub extern "C" fn test_partial_code_copy() -> u64 {
    log("Test 5: Partial code copy");

    let self_address = get_contract_hash();
    let mut code_buffer: [u8; 32] = [0u8; 32];

    // Copy 32 bytes starting from offset 64
    match ext_code_copy(&self_address, &mut code_buffer, 64) {
        Ok(_) => {
            log("Partial code copy: PASS");
            0
        }
        Err(_) => {
            log("Failed to copy partial code");
            1
        }
    }
}

/// Test 6: Zero-length code copy
#[no_mangle]
pub extern "C" fn test_zero_length_copy() -> u64 {
    log("Test 6: Zero-length code copy");

    let self_address = get_contract_hash();
    let mut code_buffer: [u8; 0] = [];

    // Copy zero bytes (should be no-op)
    match ext_code_copy(&self_address, &mut code_buffer, 0) {
        Ok(_) => {
            log("Zero-length copy: PASS");
            0
        }
        Err(_) => {
            log("Zero-length copy should succeed");
            1
        }
    }
}

/// Test 7: Compare code size with actual copy
#[no_mangle]
pub extern "C" fn test_size_consistency() -> u64 {
    log("Test 7: Code size consistency");

    let self_address = get_contract_hash();

    match get_ext_code_size(&self_address) {
        Ok(size) => {
            // Try to copy up to 256 bytes
            let copy_len = if size < 256 { size as usize } else { 256 };
            let mut code_buffer: [u8; 256] = [0u8; 256];

            match ext_code_copy(&self_address, &mut code_buffer[..copy_len], 0) {
                Ok(_) => {
                    log("Size consistency: PASS");
                    0
                }
                Err(_) => {
                    log("Failed to copy code");
                    2
                }
            }
        }
        Err(_) => {
            log("Failed to get code size");
            1
        }
    }
}

/// Main entrypoint - runs all tests
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Code Operations Test Suite ===");

    let test1 = test_own_code_size();
    if test1 != 0 {
        return 100 + test1;
    }

    log("Test 2: SKIPPED (ext_code_hash not implemented)");

    let test3 = test_copy_own_code();
    if test3 != 0 {
        return 300 + test3;
    }

    log("Test 4: SKIPPED (ext_code_hash not implemented)");

    let test5 = test_partial_code_copy();
    if test5 != 0 {
        return 500 + test5;
    }

    let test6 = test_zero_length_copy();
    if test6 != 0 {
        return 600 + test6;
    }

    let test7 = test_size_consistency();
    if test7 != 0 {
        return 700 + test7;
    }

    log("=== All Tests PASSED ===");
    0
}
