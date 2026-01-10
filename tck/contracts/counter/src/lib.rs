//! Counter Example Contract
//!
//! A simple counter that demonstrates storage operations.
//!
//! This version uses a single entry point and increments on every call.

#![no_std]
#![no_main]

use tako_sdk::*;

/// Storage key for the counter value
const COUNTER_KEY: &[u8] = b"count";

/// Main contract entrypoint
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("Counter: Increment");

    // Read current value
    let mut buffer = [0u8; 8];
    let len = storage_read(COUNTER_KEY, &mut buffer);

    let current = if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        // First time, initialize to 0
        log("Counter: First call, initializing");
        0u64
    };

    // Increment
    let new_value = current.saturating_add(1);

    // Write back
    let value_bytes = new_value.to_le_bytes();
    match storage_write(COUNTER_KEY, &value_bytes) {
        Ok(_) => {
            log("Counter incremented successfully");
            SUCCESS
        }
        Err(_) => {
            log("Counter: Storage write failed");
            ERROR
        }
    }
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
