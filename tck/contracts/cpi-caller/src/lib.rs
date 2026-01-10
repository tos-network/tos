//! CPI Caller Contract
//!
//! This contract demonstrates Cross-Program Invocation (CPI) by calling
//! another contract and receiving return data.

#![no_std]
#![no_main]

use tako_sdk::*;

/// Fixed address of the callee contract for testing
/// In production, this would be passed as input data
const CALLEE_ADDRESS: [u8; 32] = [0xAAu8; 32];

/// Caller contract entrypoint
///
/// This contract:
/// 1. Logs that it's about to perform CPI
/// 2. Invokes the callee contract
/// 3. Retrieves and logs the return data
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("CPI Caller: Starting CPI test");

    // Perform cross-program invocation
    log("CPI Caller: Invoking callee contract");

    // Empty instruction data for now (callee doesn't use it)
    let instruction_data: &[u8] = &[];

    if invoke(&CALLEE_ADDRESS, instruction_data).is_err() {
        log("CPI Caller: CPI invocation failed");
        return ERROR;
    }

    log("CPI Caller: CPI invocation succeeded");

    // Get return data from the callee
    let mut return_buffer = [0u8; 32];
    let mut program_id = [0u8; 32];
    let return_len = get_return_data(&mut return_buffer, &mut program_id);

    if return_len > 0 {
        log("CPI Caller: Received return data from callee");

        // If we got 8 bytes, it's the counter value
        if return_len == 8 {
            let counter_value = u64::from_le_bytes([
                return_buffer[0],
                return_buffer[1],
                return_buffer[2],
                return_buffer[3],
                return_buffer[4],
                return_buffer[5],
                return_buffer[6],
                return_buffer[7],
            ]);

            log("CPI Caller: Counter value from callee");
            // In debug mode, this will show the actual value
        }
    } else {
        log("CPI Caller: No return data received");
    }

    log("CPI Caller: CPI test completed successfully");
    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
