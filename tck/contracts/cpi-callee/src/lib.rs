//! CPI Callee Contract
//!
//! This contract is designed to be invoked by another contract via CPI.
//! It performs a simple operation and returns data to demonstrate CPI functionality.

#![no_std]
#![no_main]

use tako_sdk::*;

/// Callee contract entrypoint
///
/// This contract:
/// 1. Logs that it was invoked
/// 2. Reads a value from storage
/// 3. Increments it
/// 4. Writes it back
/// 5. Sets return data with the new value
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("CPI Callee: Invoked via CPI");

    // Read counter from storage
    let key = b"cpi_counter";
    let mut buffer = [0u8; 8];
    let len = storage_read(key, &mut buffer);

    let current = if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0u64
    };

    log("CPI Callee: Current counter value read");

    // Increment counter
    let new_value = current.saturating_add(1);

    // Write back to storage
    let value_bytes = new_value.to_le_bytes();
    if storage_write(key, &value_bytes).is_err() {
        log("CPI Callee: Failed to write counter");
        return ERROR;
    }

    log("CPI Callee: Counter incremented and written");

    // Set return data with the new value
    if set_return_data(&value_bytes).is_err() {
        log("CPI Callee: Failed to set return data");
        return ERROR;
    }

    log("CPI Callee: Return data set, execution complete");
    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
