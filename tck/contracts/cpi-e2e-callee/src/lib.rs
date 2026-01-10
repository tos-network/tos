//! CPI E2E Callee Contract
//!
//! This contract is designed to be invoked by the CPI E2E Caller contract
//! to test comprehensive Cross-Program Invocation (CPI) functionality.
//!
//! # Operations Performed
//! 1. Reads three numbers from storage (pre-set by caller)
//! 2. Computes the sum of the numbers
//! 3. Computes Blake3 hash of the sum
//! 4. Stores the result in contract storage
//! 5. Returns sum + hash to the caller
//!
//! # Storage Layout
//! Input (set by caller):
//! - `input_num1`: First number (u64)
//! - `input_num2`: Second number (u64)
//! - `input_num3`: Third number (u64)
//!
//! Output (set by callee):
//! - `result_sum`: Sum of three numbers (u64)
//! - `result_hash`: Blake3 hash of sum (32 bytes)
//!
//! # Return Data Format
//! - Bytes 0-7: Sum result (u64 little-endian)
//! - Bytes 8-39: Blake3 hash of sum (32 bytes)
//!
//! # Educational Use Only
//! This example is for demonstration purposes only. Production contracts
//! should include comprehensive security checks and error handling.

#![no_std]
#![no_main]

use tako_sdk::*;

/// Storage keys for input values (set by caller)
const INPUT_NUM1_KEY: &[u8] = b"input_num1";
const INPUT_NUM2_KEY: &[u8] = b"input_num2";
const INPUT_NUM3_KEY: &[u8] = b"input_num3";

/// Storage keys for output values (set by callee)
const RESULT_SUM_KEY: &[u8] = b"result_sum";
const RESULT_HASH_KEY: &[u8] = b"result_hash";

/// Error codes specific to this contract
const ERROR_INVALID_INPUT: u64 = CUSTOM_ERROR_START + 10;
const ERROR_STORAGE_WRITE: u64 = CUSTOM_ERROR_START + 11;
const ERROR_RETURN_DATA_FAILED: u64 = CUSTOM_ERROR_START + 13;

/// Callee contract entrypoint
///
/// This contract performs the following operations:
/// 1. Read three numbers from storage
/// 2. Compute sum of the numbers
/// 3. Compute Blake3 hash of the sum
/// 4. Store results in storage
/// 5. Return sum + hash to caller
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("CPI E2E Callee: Invoked via CPI");

    // === Phase 1: Read Input Data from Storage ===
    log("CPI E2E Callee: Phase 1 - Reading input from storage");

    // Read first number
    let mut buffer = [0u8; 8];
    let len = storage_read(INPUT_NUM1_KEY, &mut buffer);
    if len != 8 {
        log("CPI E2E Callee: ERROR - Invalid input_num1");
        return ERROR_INVALID_INPUT;
    }
    let num1 = u64::from_le_bytes(buffer);

    // Read second number
    let len = storage_read(INPUT_NUM2_KEY, &mut buffer);
    if len != 8 {
        log("CPI E2E Callee: ERROR - Invalid input_num2");
        return ERROR_INVALID_INPUT;
    }
    let num2 = u64::from_le_bytes(buffer);

    // Read third number
    let len = storage_read(INPUT_NUM3_KEY, &mut buffer);
    if len != 8 {
        log("CPI E2E Callee: ERROR - Invalid input_num3");
        return ERROR_INVALID_INPUT;
    }
    let num3 = u64::from_le_bytes(buffer);

    log("CPI E2E Callee: Input values read from storage");

    // === Phase 2: Compute Sum ===
    log("CPI E2E Callee: Phase 2 - Computing sum");

    // Compute sum with overflow protection
    let sum = num1.saturating_add(num2).saturating_add(num3);

    log("CPI E2E Callee: Sum computed");

    // === Phase 3: Compute Hash ===
    log("CPI E2E Callee: Phase 3 - Computing Blake3 hash");

    // Compute Blake3 hash of the sum bytes
    let sum_bytes = sum.to_le_bytes();
    let hash_result = blake3(&sum_bytes);

    log("CPI E2E Callee: Blake3 hash computed");

    // === Phase 4: Store Results ===
    log("CPI E2E Callee: Phase 4 - Writing results to storage");

    // Write sum to storage
    if storage_write(RESULT_SUM_KEY, &sum_bytes).is_err() {
        log("CPI E2E Callee: ERROR - Failed to write result_sum");
        return ERROR_STORAGE_WRITE;
    }

    // Write hash to storage
    if storage_write(RESULT_HASH_KEY, &hash_result).is_err() {
        log("CPI E2E Callee: ERROR - Failed to write result_hash");
        return ERROR_STORAGE_WRITE;
    }

    log("CPI E2E Callee: Results written to storage");

    // === Phase 5: Prepare Return Data ===
    log("CPI E2E Callee: Phase 5 - Preparing return data");

    // Build return data: sum (8 bytes) + hash (32 bytes) = 40 bytes
    let mut return_data = [0u8; 40];
    return_data[0..8].copy_from_slice(&sum_bytes);
    return_data[8..40].copy_from_slice(&hash_result);

    // Set return data for caller
    if set_return_data(&return_data).is_err() {
        log("CPI E2E Callee: ERROR - Failed to set return data");
        return ERROR_RETURN_DATA_FAILED;
    }

    log("CPI E2E Callee: Return data set (40 bytes)");

    // === Execution Complete ===
    log("CPI E2E Callee: Execution completed successfully");

    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
