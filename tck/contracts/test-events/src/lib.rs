//! Event Emission Test Contract
//!
//! This contract tests EVM-compatible event logging (LOG0-LOG4 opcodes).
//! Events are essential for dApps to track on-chain activity and state changes.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{emit_event, log};

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// Test 1: Emit LOG0 (no topics, anonymous event)
#[no_mangle]
pub extern "C" fn test_log0_anonymous() -> u64 {
    log("Test 1: Emit LOG0 (anonymous)");

    let data: [u8; 32] = [0x01u8; 32];

    // Emit LOG0 (no topics)
    let topics: [[u8; 32]; 0] = [];
    if emit_event(&topics, &data).is_err() {
        log("LOG0 emission failed");
        return 1;
    }

    log("LOG0 emission: PASS");
    0
}

/// Test 2: Emit LOG1 (1 topic - event signature)
#[no_mangle]
pub extern "C" fn test_log1_single_topic() -> u64 {
    log("Test 2: Emit LOG1 (1 topic)");

    let topic1: [u8; 32] = [0x11u8; 32];
    let data: [u8; 64] = [0x02u8; 64];

    let topics = [topic1];
    if emit_event(&topics, &data).is_err() {
        log("LOG1 emission failed");
        return 1;
    }

    log("LOG1 emission: PASS");
    0
}

/// Test 3: Emit LOG2 (2 topics - signature + 1 indexed param)
#[no_mangle]
pub extern "C" fn test_log2_two_topics() -> u64 {
    log("Test 3: Emit LOG2 (2 topics)");

    let topics: [[u8; 32]; 2] = [
        [0x22u8; 32], // Event signature
        [0x33u8; 32], // Indexed parameter 1
    ];
    let data: [u8; 32] = [0x03u8; 32];

    if emit_event(&topics, &data).is_err() {
        log("LOG2 emission failed");
        return 1;
    }

    log("LOG2 emission: PASS");
    0
}

/// Test 4: Emit LOG3 (3 topics - signature + 2 indexed params)
#[no_mangle]
pub extern "C" fn test_log3_three_topics() -> u64 {
    log("Test 4: Emit LOG3 (3 topics)");

    let topics: [[u8; 32]; 3] = [
        [0x44u8; 32], // Event signature
        [0x55u8; 32], // Indexed parameter 1
        [0x66u8; 32], // Indexed parameter 2
    ];
    let data: [u8; 32] = [0x04u8; 32];

    if emit_event(&topics, &data).is_err() {
        log("LOG3 emission failed");
        return 1;
    }

    log("LOG3 emission: PASS");
    0
}

/// Test 5: Emit LOG4 (4 topics - max, signature + 3 indexed params)
#[no_mangle]
pub extern "C" fn test_log4_four_topics() -> u64 {
    log("Test 5: Emit LOG4 (4 topics, max)");

    let topics: [[u8; 32]; 4] = [
        [0x77u8; 32], // Event signature
        [0x88u8; 32], // Indexed parameter 1
        [0x99u8; 32], // Indexed parameter 2
        [0xAAu8; 32], // Indexed parameter 3
    ];
    let data: [u8; 32] = [0x05u8; 32];

    if emit_event(&topics, &data).is_err() {
        log("LOG4 emission failed");
        return 1;
    }

    log("LOG4 emission: PASS");
    0
}

/// Main entrypoint - runs all tests
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Event Emission Test Suite ===");

    let test1 = test_log0_anonymous();
    if test1 != 0 {
        return 100 + test1;
    }

    let test2 = test_log1_single_topic();
    if test2 != 0 {
        return 200 + test2;
    }

    let test3 = test_log2_two_topics();
    if test3 != 0 {
        return 300 + test3;
    }

    let test4 = test_log3_three_topics();
    if test4 != 0 {
        return 400 + test4;
    }

    let test5 = test_log4_four_topics();
    if test5 != 0 {
        return 500 + test5;
    }

    log("=== All Tests PASSED ===");
    0
}
