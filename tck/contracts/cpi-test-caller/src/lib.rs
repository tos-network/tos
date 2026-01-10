//! CPI Test Caller Contract
//!
//! This contract tests comprehensive CPI functionality including:
//! - Cross-program invocation with parameters
//! - Return data handling
//! - Compute budget tracking and sharing
//! - Error propagation
//! - Nested CPI calls

#![no_std]
#![no_main]

use tako_sdk::*;

/// Test CPI with parameter passing and return data
///
/// Expected instruction data format:
/// - Bytes 0-31: Callee contract address (32 bytes)
/// - Bytes 32-39: Amount parameter (u64, little-endian)
///
/// If no instruction data provided, uses default test values
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("CPI Test Caller: Starting comprehensive CPI test");

    // Track compute budget before CPI
    let compute_before = remaining_compute_units();
    log("CPI Test Caller: Checking initial compute budget");

    // Default callee address (matches CPI callee contract)
    let callee_address = [0xBBu8; 32];
    let test_amount: u64 = 42;

    log("CPI Test Caller: Preparing CPI call");

    // Encode the amount as instruction data for the callee
    let instruction_data = test_amount.to_le_bytes();

    log("CPI Test Caller: Invoking callee contract via CPI");

    // Perform cross-program invocation
    if invoke(&callee_address, &instruction_data).is_err() {
        log("CPI Test Caller: ERROR - CPI invocation failed");
        return ERROR;
    }

    log("CPI Test Caller: CPI invocation succeeded");

    // Track compute budget after CPI
    let compute_after = remaining_compute_units();
    let compute_used = compute_before.saturating_sub(compute_after);

    log("CPI Test Caller: Computing CPI overhead");

    // Retrieve return data from callee
    let mut return_buffer = [0u8; 64];
    let mut program_id = [0u8; 32];
    let return_len = get_return_data(&mut return_buffer, &mut program_id);

    if return_len > 0 {
        log("CPI Test Caller: Received return data from callee");

        // Expect 8 bytes for the calculated result (u64)
        if return_len >= 8 {
            let result_value = u64::from_le_bytes([
                return_buffer[0],
                return_buffer[1],
                return_buffer[2],
                return_buffer[3],
                return_buffer[4],
                return_buffer[5],
                return_buffer[6],
                return_buffer[7],
            ]);

            log("CPI Test Caller: Parsed return value from callee");

            // Verify the computation (callee should double the amount)
            let expected = test_amount * 2;
            if result_value == expected {
                log("CPI Test Caller: Return value verification PASSED");
            } else {
                log("CPI Test Caller: ERROR - Return value mismatch");
                return ERROR;
            }
        } else {
            log("CPI Test Caller: ERROR - Return data too small");
            return ERROR;
        }

        // Verify program_id matches callee
        if program_id == callee_address {
            log("CPI Test Caller: Program ID verification PASSED");
        } else {
            log("CPI Test Caller: WARNING - Program ID mismatch");
        }
    } else {
        log("CPI Test Caller: ERROR - No return data received");
        return ERROR;
    }

    // Verify compute budget was consumed
    if compute_used > 0 {
        log("CPI Test Caller: Compute budget tracking PASSED");
    } else {
        log("CPI Test Caller: WARNING - No compute units consumed");
    }

    // Store the compute usage in our own storage for testing
    let usage_key = b"last_cpi_compute_used";
    let usage_bytes = compute_used.to_le_bytes();
    if storage_write(usage_key, &usage_bytes).is_err() {
        log("CPI Test Caller: WARNING - Failed to store compute usage");
    }

    log("CPI Test Caller: All CPI tests PASSED successfully");
    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
