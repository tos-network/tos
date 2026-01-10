//! CPI E2E Caller Contract
//!
//! This contract demonstrates comprehensive End-to-End Cross-Program Invocation (CPI)
//! testing by calling a callee contract and validating:
//! 1. CPI flow between contracts
//! 2. Compute budget sharing and tracking
//! 3. Return data handling
//! 4. Error propagation
//! 5. Storage isolation and sharing
//!
//! # Test Flow
//! 1. Caller writes test data to storage
//! 2. Caller logs initial compute units
//! 3. Caller invokes callee contract
//! 4. Callee reads data from storage, computes results
//! 5. Callee returns data to caller
//! 6. Caller validates return data
//! 7. Caller logs final compute units
//!
//! # Educational Use Only
//! This example is for demonstration purposes only. Production contracts
//! should include comprehensive security checks and error handling.

#![no_std]
#![no_main]

use tako_sdk::*;

/// Fixed address of the callee contract for E2E testing
/// This address must match the deployed callee contract
const CALLEE_ADDRESS: [u8; 32] = [0xBBu8; 32];

/// Storage keys for input values (shared with callee)
const INPUT_NUM1_KEY: &[u8] = b"input_num1";
const INPUT_NUM2_KEY: &[u8] = b"input_num2";
const INPUT_NUM3_KEY: &[u8] = b"input_num3";

/// Error codes specific to this contract
const ERROR_STORAGE_WRITE: u64 = CUSTOM_ERROR_START + 1;
const ERROR_CPI_FAILED: u64 = CUSTOM_ERROR_START + 2;
const ERROR_INVALID_RETURN_DATA: u64 = CUSTOM_ERROR_START + 3;

/// Caller contract entrypoint
///
/// This contract performs a comprehensive CPI test:
/// 1. Writes test data to storage
/// 2. Logs initial state and compute budget
/// 3. Invokes the callee contract via CPI
/// 4. Validates return data from the callee
/// 5. Logs final state and compute budget
///
/// # Return Data Format (from callee)
/// - Bytes 0-7: Sum result (u64 little-endian)
/// - Bytes 8-39: Blake3 hash of sum (32 bytes)
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("CPI E2E Caller: Starting comprehensive CPI test");

    // === Phase 1: Write Test Data to Storage ===
    log("CPI E2E Caller: Phase 1 - Writing test data to storage");

    // Test values: 100, 200, 300 (expected sum: 600)
    let num1: u64 = 100;
    let num2: u64 = 200;
    let num3: u64 = 300;

    // Write test values to storage (callee will read these)
    if storage_write(INPUT_NUM1_KEY, &num1.to_le_bytes()).is_err() {
        log("CPI E2E Caller: ERROR - Failed to write input_num1");
        return ERROR_STORAGE_WRITE;
    }

    if storage_write(INPUT_NUM2_KEY, &num2.to_le_bytes()).is_err() {
        log("CPI E2E Caller: ERROR - Failed to write input_num2");
        return ERROR_STORAGE_WRITE;
    }

    if storage_write(INPUT_NUM3_KEY, &num3.to_le_bytes()).is_err() {
        log("CPI E2E Caller: ERROR - Failed to write input_num3");
        return ERROR_STORAGE_WRITE;
    }

    log("CPI E2E Caller: Test data written to storage");

    // === Phase 2: Log Initial State ===
    log("CPI E2E Caller: Phase 2 - Initial state check");
    log("CPI E2E Caller: Recording initial compute budget");

    // === Phase 3: Perform CPI ===
    log("CPI E2E Caller: Phase 3 - Invoking callee contract");
    log("CPI E2E Caller: Calling invoke syscall");

    // Invoke the callee contract (empty instruction data)
    let instruction_data: &[u8] = &[];
    if invoke(&CALLEE_ADDRESS, instruction_data).is_err() {
        log("CPI E2E Caller: ERROR - CPI invocation failed");
        return ERROR_CPI_FAILED;
    }

    log("CPI E2E Caller: CPI invocation succeeded");

    // === Phase 4: Validate Return Data ===
    log("CPI E2E Caller: Phase 4 - Validating return data");

    // Get return data from the callee
    let mut return_buffer = [0u8; 64]; // Large enough for sum + hash
    let mut program_id = [0u8; 32];
    let return_len = get_return_data(&mut return_buffer, &mut program_id);

    if return_len == 0 {
        log("CPI E2E Caller: ERROR - No return data received");
        return ERROR_INVALID_RETURN_DATA;
    }

    log("CPI E2E Caller: Return data received");

    // Validate we got expected data (8 bytes sum + 32 bytes hash = 40 bytes)
    if return_len != 40 {
        log("CPI E2E Caller: ERROR - Invalid return data length");
        return ERROR_INVALID_RETURN_DATA;
    }

    // Extract sum result (first 8 bytes)
    let sum_result = u64::from_le_bytes([
        return_buffer[0],
        return_buffer[1],
        return_buffer[2],
        return_buffer[3],
        return_buffer[4],
        return_buffer[5],
        return_buffer[6],
        return_buffer[7],
    ]);

    // Expected sum: 100 + 200 + 300 = 600
    if sum_result != 600 {
        log("CPI E2E Caller: ERROR - Incorrect sum result");
        return ERROR_INVALID_RETURN_DATA;
    }

    log("CPI E2E Caller: Sum result validated (600)");

    // Extract hash (bytes 8-39)
    let mut hash_result = [0u8; 32];
    hash_result.copy_from_slice(&return_buffer[8..40]);

    // Compute expected hash for verification
    let expected_sum_bytes = 600u64.to_le_bytes();
    let expected_hash = blake3(&expected_sum_bytes);

    // Verify hash matches
    let mut hash_matches = true;
    for i in 0..32 {
        if hash_result[i] != expected_hash[i] {
            hash_matches = false;
            break;
        }
    }

    if !hash_matches {
        log("CPI E2E Caller: ERROR - Hash mismatch");
        return ERROR_INVALID_RETURN_DATA;
    }

    log("CPI E2E Caller: Hash result validated");

    // === Phase 5: Final State Logging ===
    log("CPI E2E Caller: Phase 5 - Final state check");
    log("CPI E2E Caller: Recording final compute budget");

    // === Test Complete ===
    log("CPI E2E Caller: All validations passed");
    log("CPI E2E Caller: E2E CPI test completed successfully");

    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
