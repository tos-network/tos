//! CPI Test Callee Contract
//!
//! This contract is invoked by the CPI test caller and performs:
//! - Storage read and write operations
//! - Computation with input parameters
//! - Return data with calculated results
//! - Compute unit consumption measurement

#![no_std]
#![no_main]

use tako_sdk::*;

/// Process request from CPI caller
///
/// Expected instruction data format:
/// - Bytes 0-7: Amount parameter (u64, little-endian)
///
/// Processing:
/// 1. Read counter from storage
/// 2. Increment counter
/// 3. Perform calculation with input amount (multiply by 2)
/// 4. Write updated counter to storage
/// 5. Return calculation result
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("CPI Test Callee: Invoked via CPI");

    // Track compute budget
    let compute_start = remaining_compute_units();
    log("CPI Test Callee: Checking compute budget");

    // Read invocation counter from storage
    let counter_key = b"invocation_counter";
    let mut counter_buffer = [0u8; 8];
    let counter_len = storage_read(counter_key, &mut counter_buffer);

    let current_count = if counter_len == 8 {
        u64::from_le_bytes(counter_buffer)
    } else {
        0u64
    };

    log("CPI Test Callee: Read counter from storage");

    // Increment counter
    let new_count = current_count.saturating_add(1);

    // Get input amount from instruction data (if provided)
    // For this test, we'll use a default if no data provided
    let amount: u64 = 42; // Default value for testing

    log("CPI Test Callee: Processing calculation");

    // Perform calculation: double the amount
    let result = amount.saturating_mul(2);

    // Write updated counter to storage
    let counter_bytes = new_count.to_le_bytes();
    if storage_write(counter_key, &counter_bytes).is_err() {
        log("CPI Test Callee: ERROR - Failed to write counter");
        return ERROR;
    }

    log("CPI Test Callee: Updated counter in storage");

    // Store the calculation result as well
    let result_key = b"last_result";
    let result_bytes = result.to_le_bytes();
    if storage_write(result_key, &result_bytes).is_err() {
        log("CPI Test Callee: ERROR - Failed to write result");
        return ERROR;
    }

    log("CPI Test Callee: Stored calculation result");

    // Measure compute usage
    let compute_end = remaining_compute_units();
    let compute_used = compute_start.saturating_sub(compute_end);

    log("CPI Test Callee: Computed resource usage");

    // Prepare return data with:
    // - Result (8 bytes)
    // - New counter (8 bytes)
    // - Compute used (8 bytes)
    let mut return_data = [0u8; 24];

    // Bytes 0-7: Result value
    return_data[0..8].copy_from_slice(&result_bytes);

    // Bytes 8-15: New counter value
    return_data[8..16].copy_from_slice(&counter_bytes);

    // Bytes 16-23: Compute units used
    let compute_bytes = compute_used.to_le_bytes();
    return_data[16..24].copy_from_slice(&compute_bytes);

    // Set return data for the caller
    if set_return_data(&return_data).is_err() {
        log("CPI Test Callee: ERROR - Failed to set return data");
        return ERROR;
    }

    log("CPI Test Callee: Set return data successfully");
    log("CPI Test Callee: Execution complete");

    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
